//! Everything that touches the operating system lives behind this module, so the rest of
//! the app (data, settings, drawing, UI logic) is the same on every platform.
use crate::render::Zone;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

pub const IS_MAC: bool = cfg!(target_os = "macos");

pub enum TrayEvent {
    /// Pointer rests on the tray item.
    Hover,
    Click,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    DoubleClick,
    MenuOpening,
    Command(u32),
}

pub const CMD_OPEN_SITE: u32 = 1;
pub const CMD_REFRESH: u32 = 2;
pub const CMD_SETTINGS: u32 = 3;
pub const CMD_QUIT: u32 = 4;

/// What the tray item should currently show.
pub struct TrayState<'a> {
    /// The formatted value, or "--".
    pub text: &'a str,
    /// Trend arrow characters, possibly empty (shown next to the value on macOS).
    #[cfg_attr(windows, allow(dead_code))]
    pub arrow: &'a str,
    /// `None` when there is no reading at all.
    pub zone: Option<Zone>,
    pub stale: bool,
    /// Tooltip text (Windows; on macOS the flyout opens on hover instead).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub tip: &'a str,
}
