//! Windows integration helpers: theme, autostart, DPAPI, monitors, misc.
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
pub fn taskbar_is_light() -> bool {
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
pub fn enable_dark_menus() {
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

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut r) };
    r
}

/// Work area (screen minus taskbar) of the monitor containing the point.
pub fn work_area(x: i32, y: i32) -> RECT {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}

pub fn small_icon_size() -> i32 {
    unsafe { GetSystemMetrics(SM_CXSMICON).max(16) }
}
