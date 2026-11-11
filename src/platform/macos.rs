//! macOS implementation of the platform layer: a menu-bar item that shows the value as
//! text, a borderless flyout window below it, Keychain, LaunchAgent autostart.
use super::{TrayEvent, TrayState, CMD_OPEN_SITE, CMD_QUIT, CMD_REFRESH, CMD_SETTINGS};
use crate::render::Zone;
use crate::Flyout;
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const KEYCHAIN_SERVICE: &str = "Nightscout Glance";
const KEYCHAIN_ACCOUNT: &str = "nightscout-api-secret";
const KEYCHAIN_MARKER: &str = "keychain";
const LAUNCH_AGENT: &str = "com.burnedikt.nightscout-glance";

pub fn init() {}

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
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
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

pub struct Tray {
    icon: RefCell<Option<TrayIcon>>,
    shown: RefCell<String>,
}

impl Tray {
    /// The status item itself is created on the first `update`, because macOS wants that
    /// to happen once the event loop is running.
    pub fn new(handler: Rc<dyn Fn(TrayEvent)>) -> Tray {
        HANDLER.with(|h| *h.borrow_mut() = Some(handler));
        Tray { icon: RefCell::new(None), shown: RefCell::new(String::new()) }
    }

    fn build() -> Option<TrayIcon> {
        let menu = Menu::new();
        let item = |id: u32, text: &str| MenuItem::with_id(id.to_string(), text, true, None);
        menu.append_items(&[
            &item(CMD_OPEN_SITE, "Open Nightscout"),
            &item(CMD_REFRESH, "Refresh Now"),
            &PredefinedMenuItem::separator(),
            &item(CMD_SETTINGS, "Settings\u{2026}"),
            &PredefinedMenuItem::separator(),
            &item(CMD_QUIT, "Quit Nightscout Glance"),
        ])
        .ok()?;

        // Both handlers may fire outside of our event-loop callbacks: hop onto it.
        MenuEvent::set_event_handler(Some(|e: MenuEvent| {
            let _ = slint::invoke_from_event_loop(move || {
                if let Ok(cmd) = e.id.0.parse::<u32>() {
                    emit(TrayEvent::Command(cmd));
                }
            });
        }));
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

        TrayIconBuilder::new()
            .with_title("--")
            .with_tooltip("Nightscout Glance")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .ok()
    }

    pub fn update(&self, st: &TrayState) {
        if self.icon.borrow().is_none() {
            *self.icon.borrow_mut() = Self::build();
        }
        let icon = self.icon.borrow();
        let Some(icon) = icon.as_ref() else { return };

        let title = if st.arrow.is_empty() { st.text.to_string() } else { format!("{} {}", st.text, st.arrow) };
        let key = format!("{title}|{:?}|{}|{}", st.zone, st.stale, st.tip);
        if key == *self.shown.borrow() {
            return;
        }
        *self.shown.borrow_mut() = key;

        icon.set_title(Some(&title));
        let _ = icon.set_tooltip(Some(st.tip));
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

    fn rect(&self) -> Option<tray_icon::Rect> {
        self.icon.borrow().as_ref().and_then(|i| i.rect())
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

pub struct FlyoutHost;

impl FlyoutHost {
    pub fn new() -> Self {
        FlyoutHost
    }

    pub fn prepare(&self, _flyout: &Flyout) {}

    pub fn attach(&mut self, _flyout: &Flyout) -> bool {
        true
    }

    pub fn ready(&self) -> bool {
        true
    }

    pub fn show(&mut self, flyout: &Flyout, tray: &Tray) {
        let scale = flyout.window().scale_factor() as f64;
        let width = 388.0 * scale;
        if let Some(r) = tray.rect() {
            let x = (r.position.x + r.size.width as f64 / 2.0 - width / 2.0).max(8.0 * scale);
            let y = r.position.y + r.size.height as f64 + 6.0 * scale;
            flyout.window().set_position(slint::PhysicalPosition::new(x as i32, y as i32));
        }
        let _ = flyout.show();
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
