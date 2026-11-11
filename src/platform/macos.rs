//! macOS implementation of the platform layer: a menu-bar item that shows the value as
//! text, a borderless flyout window below it, Keychain, LaunchAgent autostart.
use super::{TrayEvent, TrayState, CMD_OPEN_SITE, CMD_QUIT, CMD_REFRESH, CMD_SETTINGS};
use crate::render::Zone;
use crate::Flyout;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSFloatingWindowLevel, NSMenu, NSMenuItem, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::{NSObject, NSRect, NSString};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::OnceLock;
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const KEYCHAIN_SERVICE: &str = "Nightscout Glance";
const KEYCHAIN_ACCOUNT: &str = "nightscout-api-secret";
const KEYCHAIN_MARKER: &str = "keychain";
const LAUNCH_AGENT: &str = "com.burnedikt.nightscout-glance";

pub fn init() {
    use slint::winit_030::winit::event_loop::EventLoop;
    use slint::winit_030::winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

    // A menu-bar app: no Dock icon, even when run outside the bundle, and no focus grab at
    // launch. winit would otherwise activate the app as soon as it first shows a window,
    // which is the first hover over the menu-bar item, and keep the keyboard focus there.
    let mut builder = EventLoop::with_user_event();
    builder.with_activation_policy(ActivationPolicy::Accessory).with_activate_ignoring_other_apps(false);
    let _ = slint::BackendSelector::new().backend_name("winit".into()).with_winit_event_loop_builder(builder).select();
}

pub fn single_instance() -> bool {
    // Launch Services already keeps a bundled app to one instance.
    true
}

pub fn system_is_dark() -> bool {
    Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Dark"))
        .unwrap_or(false)
}

/// The menu bar restyles its own text, and the flyout checks the theme each time it opens,
/// so there is nothing to watch.
pub fn watch_theme(_cb: impl Fn() + Send + 'static) {}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into())
}

pub fn config_dir() -> PathBuf {
    home().join("Library/Application Support/Nightscout Glance")
}

fn launch_agent_path() -> PathBuf {
    home().join("Library/LaunchAgents").join(format!("{LAUNCH_AGENT}.plist"))
}

pub fn autostart_enabled() -> bool {
    launch_agent_path().exists()
}

pub fn set_autostart(enabled: bool) {
    let path = launch_agent_path();
    if !enabled {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LAUNCH_AGENT}</string>
    <key>ProgramArguments</key><array><string>{exe}</string></array>
    <key>RunAtLoad</key><true/>
</dict>
</plist>
"#
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, plist);
}

/// Stores the secret in the login Keychain; the settings file only keeps a marker.
pub fn protect(secret: &str) -> String {
    match security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, secret.as_bytes()) {
        Ok(()) => KEYCHAIN_MARKER.to_string(),
        Err(_) => String::new(),
    }
}

pub fn unprotect(marker: &str) -> Option<String> {
    if marker != KEYCHAIN_MARKER {
        return None;
    }
    let bytes = security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn open_url(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
}

pub fn utc_offset_minutes() -> i64 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64 / 60
    }
}

pub fn focus_window(_window: &slint::Window) {
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// Hands the keyboard focus back to the app the user was in once none of our windows needs
/// it. Clicking into the flyout or opening the settings activates this app, and an accessory
/// app without windows would otherwise keep the focus to itself.
pub fn release_focus() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    // The menu-bar item lives in a window above the floating level; it doesn't count.
    let window_open = app.windows().iter().any(|w| w.isVisible() && w.level() < NSFloatingWindowLevel);
    if app.isActive() && !window_open {
        app.hide(None);
    }
}

// ---------------------------------------------------------------------------------------
// Menu-bar item

thread_local! {
    static HANDLER: RefCell<Option<Rc<dyn Fn(TrayEvent)>>> = const { RefCell::new(None) };
    static HOVERING: Cell<bool> = const { Cell::new(false) };
}

fn emit(ev: TrayEvent) {
    let handler = HANDLER.with(|h| h.borrow().clone());
    if let Some(h) = handler {
        h(ev);
    }
}

define_class!(
    /// Receives the clicks in the menu-bar item's menu; each item's tag is its command.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NightscoutGlanceMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(itemClicked:))]
        fn item_clicked(&self, item: &NSMenuItem) {
            let cmd = item.tag() as u32;
            // Runs inside the menu's tracking loop: hop onto the event loop.
            let _ = slint::invoke_from_event_loop(move || emit(TrayEvent::Command(cmd)));
        }
    }
);

/// The menu is a plain `NSMenu` rather than a muda one: muda delivers all clicks through one
/// process-wide handler, which Slint already claims for its own native menus (e.g. the
/// Cut/Copy/Paste menu of the text fields in the settings window).
struct NativeMenu(Retained<NSMenu>);

impl tray_icon::menu::ContextMenu for NativeMenu {
    unsafe fn show_context_menu_for_nsview(
        &self,
        _view: *const c_void,
        _position: Option<tray_icon::dpi::Position>,
    ) -> bool {
        false
    }

    fn ns_menu(&self) -> *mut c_void {
        Retained::as_ptr(&self.0) as *mut c_void
    }
}

fn build_menu(mtm: MainThreadMarker, target: &MenuTarget) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    let add = |cmd: u32, title: &str| {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(sel!(itemClicked:)),
                &NSString::new(),
            )
        };
        unsafe { item.setTarget(Some(target)) };
        item.setTag(cmd as isize);
        menu.addItem(&item);
    };
    add(CMD_OPEN_SITE, "Open Nightscout");
    add(CMD_REFRESH, "Refresh Now");
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add(CMD_SETTINGS, "Settings\u{2026}");
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add(CMD_QUIT, "Quit Nightscout Glance");
    menu
}

pub struct Tray {
    icon: RefCell<Option<TrayIcon>>,
    shown: RefCell<String>,
    /// Menu items only hold a weak reference to their target.
    menu_target: RefCell<Option<Retained<MenuTarget>>>,
}

impl Tray {
    /// The status item itself is created on the first `update`, because macOS wants that
    /// to happen once the event loop is running.
    pub fn new(handler: Rc<dyn Fn(TrayEvent)>) -> Tray {
        HANDLER.with(|h| *h.borrow_mut() = Some(handler));
        Tray { icon: RefCell::new(None), shown: RefCell::new(String::new()), menu_target: RefCell::new(None) }
    }

    fn build(&self) -> Option<TrayIcon> {
        let mtm = MainThreadMarker::new()?;
        let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::alloc(mtm), init] };
        let menu = build_menu(mtm, &target);
        *self.menu_target.borrow_mut() = Some(target);

        // May fire outside of our event-loop callbacks: hop onto it.
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
            let _ = slint::invoke_from_event_loop(move || match e {
                TrayIconEvent::Enter { .. } => {
                    HOVERING.with(|h| h.set(true));
                    emit(TrayEvent::Hover);
                }
                TrayIconEvent::Leave { .. } => HOVERING.with(|h| h.set(false)),
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => {
                    emit(TrayEvent::Click)
                }
                TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Down, .. } => {
                    emit(TrayEvent::MenuOpening)
                }
                _ => {}
            });
        }));

        // No tooltip: the flyout opens on hover anyway, and the tooltip would be drawn on
        // top of it.
        TrayIconBuilder::new()
            .with_title("--")
            .with_menu(Box::new(NativeMenu(menu)))
            .with_menu_on_left_click(false)
            .build()
            .ok()
    }

    pub fn update(&self, st: &TrayState) {
        if self.icon.borrow().is_none() {
            let icon = self.build();
            *self.icon.borrow_mut() = icon;
        }
        let icon = self.icon.borrow();
        let Some(icon) = icon.as_ref() else { return };

        let title = if st.arrow.is_empty() { st.text.to_string() } else { format!("{} {}", st.text, st.arrow) };
        let key = format!("{title}|{:?}|{}", st.zone, st.stale);
        if key == *self.shown.borrow() {
            return;
        }
        *self.shown.borrow_mut() = key;

        icon.set_title(Some(&title));
        // In-range values keep the menu bar's own text colour (which follows light/dark);
        // everything else gets a system colour on top.
        let tint = match (st.zone, st.stale) {
            (None, _) | (_, true) => Some(Tint::Dim),
            (Some(Zone::Low | Zone::UrgentLow), _) => Some(Tint::Red),
            (Some(Zone::High), _) => Some(Tint::Yellow),
            (Some(Zone::UrgentHigh), _) => Some(Tint::Orange),
            (Some(Zone::InRange), _) => None,
        };
        if let Some(tint) = tint {
            set_colored_title(icon, &title, tint);
        }
    }

    pub fn notify(&self, title: &str, body: &str) {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"{}\"", esc(body), esc(title));
        let _ = Command::new("osascript").args(["-e", &script]).spawn();
    }

    /// Where the flyout's top-left corner goes, in global display points with the origin at
    /// the top left: centred under the menu-bar item and kept on the item's screen.
    fn flyout_origin(&self, width: f64) -> Option<(f64, f64)> {
        let mtm = MainThreadMarker::new()?;
        let icon = self.icon.borrow();
        let window = icon.as_ref()?.ns_status_item()?.button(mtm)?.window()?;
        let item = window.frame();
        let screen = window.screen()?.frame();
        // AppKit's global coordinates grow upwards from the bottom of the first screen.
        let first_screen_height = NSScreen::screens(mtm).firstObject()?.frame().size.height;
        let x = (item.origin.x + item.size.width / 2.0 - width / 2.0)
            .min(screen.origin.x + screen.size.width - width - 8.0)
            .max(screen.origin.x + 8.0);
        let y = first_screen_height - item.origin.y + 6.0;
        Some((x, y))
    }
}

enum Tint {
    Red,
    Yellow,
    Orange,
    Dim,
}

fn set_colored_title(icon: &TrayIcon, title: &str, tint: Tint) {
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSColor, NSForegroundColorAttributeName};
    use objc2_foundation::{NSAttributedString, NSAttributedStringKey, NSDictionary, NSString};

    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(item) = icon.ns_status_item() else { return };
    let Some(button) = item.button(mtm) else { return };
    let color = match tint {
        Tint::Red => NSColor::systemRedColor(),
        Tint::Yellow => NSColor::systemYellowColor(),
        Tint::Orange => NSColor::systemOrangeColor(),
        Tint::Dim => NSColor::tertiaryLabelColor(),
    };
    unsafe {
        let key: &NSAttributedStringKey = NSForegroundColorAttributeName;
        let value: &AnyObject = &color;
        let attrs = NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(&[key], &[value]);
        let text = NSAttributedString::new_with_attributes(&NSString::from_str(title), &attrs);
        button.setAttributedTitle(&text);
    }
}

// ---------------------------------------------------------------------------------------
// Flyout hosting: a plain borderless Slint window placed under the menu-bar item.

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const core::ffi::c_void) -> *mut core::ffi::c_void;
    fn CGEventGetLocation(event: *mut core::ffi::c_void) -> CGPoint;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const core::ffi::c_void);
}

/// Pointer position in global display points, origin at the top left.
fn pointer() -> Option<(f64, f64)> {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return None;
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev);
        Some((p.x, p.y))
    }
}

fn ns_window(window: &slint::Window) -> Option<Retained<NSWindow>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    let RawWindowHandle::AppKit(handle) = handle.window_handle().ok()?.as_raw() else { return None };
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

static NSWINDOW_CONSTRAIN: OnceLock<Imp> = OnceLock::new();

/// Lets winit's floating windows (the flyout is the only one) sit right under the menu bar.
///
/// Slint asks winit whether a window is maximised on every occlusion change, and for a
/// borderless window winit finds out by giving it a title bar for a moment. AppKit then
/// constrains that titled frame to stay below the menu bar, which pushes a flyout placed just
/// under the menu bar down by the height of the title bar. Popover-like windows usually opt
/// out of the constraint by overriding `constrainFrameRect:toScreen:`; winit owns the class,
/// so add the override at runtime. Normal windows, like the settings, keep AppKit's behaviour.
fn keep_floating_windows_in_place(window: &NSWindow) {
    extern "C-unwind" fn constrain(this: &NSWindow, cmd: Sel, frame: NSRect, screen: *mut AnyObject) -> NSRect {
        if this.level() >= NSFloatingWindowLevel {
            return frame;
        }
        let inherited: extern "C-unwind" fn(&NSWindow, Sel, NSRect, *mut AnyObject) -> NSRect =
            unsafe { std::mem::transmute(*NSWINDOW_CONSTRAIN.get().unwrap()) };
        inherited(this, cmd, frame, screen)
    }

    let sel = sel!(constrainFrameRect:toScreen:);
    let Some(inherited) = AnyClass::get(c"NSWindow").and_then(|c| c.instance_method(sel)) else { return };
    if NSWINDOW_CONSTRAIN.set(inherited.implementation()).is_err() {
        return; // Already installed.
    }
    // The flyout's class may be a KVO subclass of winit's; add the override to winit's own.
    let mut cls = window.class();
    while !cls.name().to_bytes().starts_with(b"Winit") {
        let Some(sup) = cls.superclass() else { return };
        cls = sup;
    }
    unsafe {
        let imp: Imp = std::mem::transmute(
            constrain as extern "C-unwind" fn(&NSWindow, Sel, NSRect, *mut AnyObject) -> NSRect,
        );
        let types = objc2::ffi::method_getTypeEncoding(inherited);
        objc2::ffi::class_addMethod(cls as *const AnyClass as *mut AnyClass, sel, imp, types);
    }
}

pub struct FlyoutHost {
    window: Option<Retained<NSWindow>>,
}

impl FlyoutHost {
    pub fn new() -> Self {
        FlyoutHost { window: None }
    }

    pub fn prepare(&self, _flyout: &Flyout) {}

    /// Adjusts the flyout's native window, once Slint has created it.
    pub fn attach(&mut self, flyout: &Flyout) -> bool {
        if self.window.is_some() {
            return true;
        }
        let Some(window) = ns_window(flyout.window()) else { return false };
        // Open on whichever Space is active, also over full-screen apps, and stay out of
        // Mission Control and window cycling.
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        keep_floating_windows_in_place(&window);
        self.window = Some(window);
        true
    }

    pub fn ready(&self) -> bool {
        true
    }

    pub fn show(&mut self, flyout: &Flyout, tray: &Tray) {
        // `release_focus` may have hidden the app; bring its windows back without taking focus.
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            if app.isHidden() {
                app.unhideWithoutActivation();
            }
        }
        self.attach(flyout);
        let scale = flyout.window().scale_factor() as f64;
        let width = match flyout.window().size().width {
            0 => 388.0,
            w => w as f64 / scale,
        };
        if let Some((x, y)) = tray.flyout_origin(width) {
            flyout.window().set_position(slint::LogicalPosition::new(x as f32, y as f32));
        }
        let _ = flyout.show();
        // In case the native window only came into being just now.
        self.attach(flyout);
    }

    pub fn hide(&mut self, flyout: &Flyout) {
        let _ = flyout.hide();
    }

    pub fn pointer_inside(&self, flyout: &Flyout, _tray: &Tray) -> bool {
        if HOVERING.with(|h| h.get()) {
            return true;
        }
        let Some((px, py)) = pointer() else { return true };
        let scale = flyout.window().scale_factor() as f64;
        let pos = flyout.window().position();
        let size = flyout.window().size();
        let (left, top) = (pos.x as f64 / scale, pos.y as f64 / scale);
        let (w, h) = (size.width as f64 / scale, size.height as f64 / scale);
        // The margin above bridges the gap between the menu bar and the flyout.
        px >= left - 12.0 && px < left + w + 12.0 && py >= top - 30.0 && py < top + h + 12.0
    }

    pub fn debug_rect(&self, flyout: &Flyout) -> (i32, i32, i32, i32) {
        let (pos, size) = (flyout.window().position(), flyout.window().size());
        (pos.x, pos.y, size.width as i32, size.height as i32)
    }
}
