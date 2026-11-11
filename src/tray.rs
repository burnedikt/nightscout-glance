//! Notification-area (tray) icon on raw Win32.
//!
//! Uses NOTIFYICON_VERSION_4 so Windows tells us when the pointer rests on the icon
//! (NIN_POPUPOPEN) instead of showing its plain-text tooltip. That is what drives the
//! rich hover flyout.
use crate::win::w;
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::rc::Rc;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum TrayEvent {
    /// Pointer rests on the icon.
    Hover,
    Click,
    DoubleClick,
    MenuOpening,
    Command(u32),
}

pub const CMD_OPEN_SITE: u32 = 1;
pub const CMD_REFRESH: u32 = 2;
pub const CMD_SETTINGS: u32 = 3;
pub const CMD_QUIT: u32 = 4;

const WM_TRAY: u32 = WM_APP + 1;
const EV_SELECT: u32 = 0x400; // NIN_SELECT
const EV_KEYSELECT: u32 = 0x401; // NIN_KEYSELECT
const EV_POPUPOPEN: u32 = 0x406; // NIN_POPUPOPEN

type Handler = Rc<dyn Fn(TrayEvent)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
    static NID: RefCell<Option<NOTIFYICONDATAW>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

fn emit(ev: TrayEvent) {
    let handler = HANDLER.with(|h| h.borrow().clone());
    if let Some(h) = handler {
        h(ev);
    }
}

fn copy_str(dst: &mut [u16], s: &str) {
    let n = dst.len() - 1;
    let mut i = 0;
    for u in s.encode_utf16().take(n) {
        dst[i] = u;
        i += 1;
    }
    dst[i] = 0;
}

pub struct Tray {
    hwnd: HWND,
    owns_icon: Cell<bool>,
}

impl Tray {
    pub fn new(handler: Handler) -> Tray {
        HANDLER.with(|h| *h.borrow_mut() = Some(handler));
        unsafe {
            let hinst = GetModuleHandleW(null());
            let class = w("NightscoutGlanceTray");
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinst;
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), hinst, null());
            TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w("TaskbarCreated").as_ptr())));

            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY;
            nid.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            copy_str(&mut nid.szTip, "Nightscout Glance");
            add(&nid);
            NID.with(|n| *n.borrow_mut() = Some(nid));
            Tray { hwnd, owns_icon: Cell::new(false) }
        }
    }

    /// Takes ownership of `icon`.
    pub fn set_icon(&self, icon: HICON, tip: &str) {
        NID.with(|n| {
            if let Some(nid) = n.borrow_mut().as_mut() {
                let old = nid.hIcon;
                nid.hIcon = icon;
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                copy_str(&mut nid.szTip, tip);
                unsafe {
                    Shell_NotifyIconW(NIM_MODIFY, nid);
                    if self.owns_icon.replace(true) {
                        DestroyIcon(old);
                    }
                }
            }
        });
    }

    pub fn set_tip(&self, tip: &str) {
        NID.with(|n| {
            if let Some(nid) = n.borrow_mut().as_mut() {
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                copy_str(&mut nid.szTip, tip);
                unsafe { Shell_NotifyIconW(NIM_MODIFY, nid) };
            }
        });
    }

    /// Shows a Windows notification attached to the tray icon.
    pub fn notify(&self, title: &str, body: &str) {
        NID.with(|n| {
            if let Some(nid) = n.borrow().as_ref() {
                let mut m = *nid;
                m.uFlags = NIF_INFO;
                m.dwInfoFlags = NIIF_WARNING;
                copy_str(&mut m.szInfoTitle, title);
                copy_str(&mut m.szInfo, body);
                unsafe { Shell_NotifyIconW(NIM_MODIFY, &m) };
            }
        });
    }

    /// Screen rectangle of the icon, if it is currently visible.
    pub fn icon_rect(&self) -> Option<RECT> {
        unsafe {
            let mut id: NOTIFYICONIDENTIFIER = std::mem::zeroed();
            id.cbSize = std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32;
            id.hWnd = self.hwnd;
            id.uID = 1;
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            (Shell_NotifyIconGetRect(&id, &mut r) == 0).then_some(r)
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        NID.with(|n| {
            if let Some(nid) = n.borrow_mut().take() {
                unsafe {
                    Shell_NotifyIconW(NIM_DELETE, &nid);
                    if self.owns_icon.get() {
                        DestroyIcon(nid.hIcon);
                    }
                    DestroyWindow(self.hwnd);
                }
            }
        });
    }
}

unsafe fn add(nid: &NOTIFYICONDATAW) {
    Shell_NotifyIconW(NIM_ADD, nid);
    Shell_NotifyIconW(NIM_SETVERSION, nid);
}

unsafe fn show_menu(hwnd: HWND) {
    emit(TrayEvent::MenuOpening);
    let menu = CreatePopupMenu();
    AppendMenuW(menu, MF_STRING, CMD_OPEN_SITE as usize, w("Open Nightscout").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_REFRESH as usize, w("Refresh now").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, CMD_SETTINGS as usize, w("Settings\u{2026}").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, CMD_QUIT as usize, w("Quit").as_ptr());
    let mut p = POINT { x: 0, y: 0 };
    GetCursorPos(&mut p);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, p.x, p.y, 0, hwnd, null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
    if cmd != 0 {
        emit(TrayEvent::Command(cmd as u32));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_TRAY {
        match (lparam as u32) & 0xFFFF {
            EV_POPUPOPEN => emit(TrayEvent::Hover),
            EV_SELECT | EV_KEYSELECT => emit(TrayEvent::Click),
            WM_LBUTTONDBLCLK => emit(TrayEvent::DoubleClick),
            WM_CONTEXTMENU => show_menu(hwnd),
            _ => {}
        }
        return 0;
    }
    if msg != 0 && msg == TASKBAR_CREATED.with(|t| t.get()) {
        // Explorer restarted: put the icon back.
        NID.with(|n| {
            if let Some(nid) = n.borrow().as_ref() {
                add(nid);
            }
        });
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
