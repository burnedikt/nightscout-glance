//! Windows implementation of the platform layer: tray icon, flyout window hosting,
//! theme, autostart, DPAPI and friends.
mod icon;
mod shell_tray;

use super::{TrayEvent, TrayState};
use crate::render::{zone_color, Rgb};
use crate::Flyout;
use shell_tray::ShellTray;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Security::Cryptography::*;
use windows_sys::Win32::System::LibraryLoader::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::SystemInformation::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::Time::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "NightscoutGlance";

/// True when the taskbar ("Windows mode") uses the light theme.
fn taskbar_is_light() -> bool {
    let mut data: u32 = 0;
    let mut size = 4u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w(PERSONALIZE).as_ptr(),
            w("SystemUsesLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    rc == ERROR_SUCCESS && data != 0
}

/// Calls `cb` (on a background thread) whenever the Windows theme settings change.
pub fn watch_theme(cb: impl Fn() + Send + 'static) {
    std::thread::spawn(move || unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, w(PERSONALIZE).as_ptr(), 0, KEY_NOTIFY, &mut key) != ERROR_SUCCESS {
            return;
        }
        while RegNotifyChangeKeyValue(key, 0, REG_NOTIFY_CHANGE_LAST_SET, null_mut(), 0) == ERROR_SUCCESS {
            cb();
        }
    });
}

/// Opts the process into dark context menus when the app theme is dark (undocumented uxtheme
/// API, also used by Explorer; fails harmlessly on systems without it).
pub fn init() {
    unsafe {
        let ux = LoadLibraryW(w("uxtheme.dll").as_ptr());
        if ux.is_null() {
            return;
        }
        if let Some(f) = GetProcAddress(ux, 135 as *const u8) {
            let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_preferred_app_mode(1); // AllowDark
        }
        if let Some(f) = GetProcAddress(ux, 136 as *const u8) {
            let flush_menu_themes: extern "system" fn() = std::mem::transmute(f);
            flush_menu_themes();
        }
    }
}

pub fn autostart_enabled() -> bool {
    let mut size = 0u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w(RUN_KEY).as_ptr(),
            w(RUN_VALUE).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut size,
        ) == ERROR_SUCCESS
    }
}

pub fn set_autostart(enabled: bool) {
    unsafe {
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return };
            let value = w(&format!("\"{}\"", exe.display()));
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                w(RUN_KEY).as_ptr(),
                w(RUN_VALUE).as_ptr(),
                REG_SZ,
                value.as_ptr() as *const _,
                (value.len() * 2) as u32,
            );
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, w(RUN_KEY).as_ptr(), w(RUN_VALUE).as_ptr());
        }
    }
}

/// Encrypts a string for the current Windows user. Returns hex.
pub fn protect(s: &str) -> String {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: s.len() as u32, pbData: s.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        if CryptProtectData(&input, null(), null(), null(), null(), 0, &mut out) == 0 {
            return String::new();
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize);
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
        LocalFree(out.pbData as _);
        hex
    }
}

pub fn unprotect(hex: &str) -> Option<String> {
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        if CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut out) == 0 {
            return None;
        }
        let s = String::from_utf8(std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec()).ok();
        LocalFree(out.pbData as _);
        s
    }
}

pub fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(null_mut(), w("open").as_ptr(), w(url).as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

/// Returns false if another instance is already running.
pub fn single_instance() -> bool {
    unsafe {
        let h = CreateMutexW(null(), 0, w("Local\\NightscoutGlance.SingleInstance").as_ptr());
        !(h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
    }
}

/// Local time offset from UTC, in minutes.
pub fn utc_offset_minutes() -> i64 {
    unsafe {
        let (mut utc, mut local) = (std::mem::zeroed(), std::mem::zeroed());
        GetSystemTime(&mut utc);
        if SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) == 0 {
            return 0;
        }
        let (mut fu, mut fl): (FILETIME, FILETIME) = (std::mem::zeroed(), std::mem::zeroed());
        SystemTimeToFileTime(&utc, &mut fu);
        SystemTimeToFileTime(&local, &mut fl);
        let q = |f: FILETIME| ((f.dwHighDateTime as i64) << 32) | f.dwLowDateTime as i64;
        ((q(fl) - q(fu)) as f64 / 600_000_000.0).round() as i64
    }
}

fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut r) };
    r
}

/// Work area (screen minus taskbar) of the monitor containing the point.
fn work_area(x: i32, y: i32) -> RECT {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}

fn small_icon_size() -> i32 {
    unsafe { GetSystemMetrics(SM_CXSMICON).max(16) }
}

pub fn system_is_dark() -> bool {
    !taskbar_is_light()
}

pub fn config_dir() -> std::path::PathBuf {
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into());
    base.join("Nightscout Glance")
}

fn hwnd_of(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    let handle = handle.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

pub fn focus_window(window: &slint::Window) {
    if let Some(hwnd) = hwnd_of(window) {
        unsafe { SetForegroundWindow(hwnd) };
    }
}

// ---------------------------------------------------------------------------------------
// Tray: the value is drawn into the icon itself.

pub struct Tray {
    shell: ShellTray,
    icon_key: RefCell<String>,
    tip: RefCell<String>,
}

impl Tray {
    pub fn new(handler: Rc<dyn Fn(TrayEvent)>) -> Tray {
        Tray { shell: ShellTray::new(handler), icon_key: RefCell::new(String::new()), tip: RefCell::new(String::new()) }
    }

    pub fn update(&self, st: &TrayState) {
        let light = taskbar_is_light();
        let neutral: Rgb = if light { (0x1a, 0x1a, 0x1a) } else { (255, 255, 255) };
        let rgb = st.zone.map_or(neutral, |z| zone_color(z, !light, neutral));
        let dim = st.stale || st.zone.is_none();
        let size = small_icon_size();
        let key = format!("{}|{rgb:?}|{dim}|{size}", st.text);
        if key != *self.icon_key.borrow() {
            let pixels = icon::icon_pixels(st.text, rgb, size, dim);
            self.shell.set_icon(icon::icon_from_pixels(&pixels, size), st.tip);
            *self.icon_key.borrow_mut() = key;
            *self.tip.borrow_mut() = st.tip.to_string();
        } else if st.tip != *self.tip.borrow() {
            self.shell.set_tip(st.tip);
            *self.tip.borrow_mut() = st.tip.to_string();
        }
    }

    pub fn notify(&self, title: &str, body: &str) {
        self.shell.notify(title, body);
    }
}

// ---------------------------------------------------------------------------------------
// Flyout hosting
//
// Slint considers the flyout permanently shown; we show, hide and move the native window
// ourselves so it never takes focus or appears in the taskbar.

pub struct FlyoutHost {
    hwnd: HWND,
}

impl FlyoutHost {
    pub fn new() -> Self {
        FlyoutHost { hwnd: null_mut() }
    }

    /// Called once before the event loop starts.
    pub fn prepare(&self, flyout: &Flyout) {
        flyout.window().set_position(slint::PhysicalPosition::new(-32000, -32000));
        let _ = flyout.show();
    }

    /// Takes over the native window once it exists. Returns false if it is not there yet.
    pub fn attach(&mut self, flyout: &Flyout) -> bool {
        let Some(hwnd) = hwnd_of(flyout.window()) else { return false };
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let ex = (ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST) & !WS_EX_APPWINDOW;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
            let round: u32 = 2; // DWMWCP_ROUND
            DwmSetWindowAttribute(hwnd, 33, &round as *const u32 as *const _, 4);
        }
        self.hwnd = hwnd;
        true
    }

    pub fn ready(&self) -> bool {
        !self.hwnd.is_null()
    }

    pub fn show(&mut self, flyout: &Flyout, tray: &Tray) {
        let scale = flyout.window().scale_factor();
        let wr = window_rect(self.hwnd);
        let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
        let (cx, cy) = cursor_pos();
        let (ax, top, bottom) = match tray.shell.icon_rect() {
            Some(r) => ((r.left + r.right) / 2, r.top, r.bottom),
            None => (cx, cy, cy),
        };
        let wa = work_area(ax, top);
        let gap = (10.0 * scale) as i32;
        let x = (ax - w / 2).clamp(wa.left + gap, (wa.right - w - gap).max(wa.left + gap));
        let y = if top >= wa.bottom - 4 {
            wa.bottom - h - gap // taskbar at the bottom
        } else if bottom <= wa.top + 4 {
            wa.top + gap // taskbar at the top
        } else if top - h - gap >= wa.top {
            top - h - gap // icon lives in the overflow popup
        } else {
            bottom + gap
        };
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        // Whatever was drawn while the window was hidden is gone: repaint everything.
        flyout.set_repaint_tick(!flyout.get_repaint_tick());
        flyout.window().request_redraw();
    }

    pub fn hide(&mut self, _flyout: &Flyout) {
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    /// True while the pointer is over the tray icon or the flyout.
    pub fn pointer_inside(&self, _flyout: &Flyout, tray: &Tray) -> bool {
        let (x, y) = cursor_pos();
        let within = |r: RECT, m: i32| x >= r.left - m && x < r.right + m && y >= r.top - m && y < r.bottom + m;
        within(window_rect(self.hwnd), 14) || tray.shell.icon_rect().is_some_and(|r| within(r, 3))
    }

    /// Screen rectangle of the flyout (development aid).
    pub fn debug_rect(&self, _flyout: &Flyout) -> (i32, i32, i32, i32) {
        let r = window_rect(self.hwnd);
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    }
}
