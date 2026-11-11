//! Persistent settings (JSON in the user's config dir; secret encrypted by the OS).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub url: String,
    /// API secret / access token, in memory only.
    #[serde(skip)]
    pub secret: String,
    /// The secret as protected by the OS (Windows: DPAPI blob, macOS: a Keychain marker).
    pub secret_enc: String,
    pub mmol: bool,
    // Thresholds are always stored in mg/dL.
    pub urgent_low: f32,
    pub low: f32,
    pub high: f32,
    pub urgent_high: f32,
    pub range_hours: u32,
    pub alerts: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            url: String::new(),
            secret: String::new(),
            secret_enc: String::new(),
            mmol: false,
            urgent_low: 55.0,
            low: 70.0,
            high: 180.0,
            urgent_high: 250.0,
            range_hours: 3,
            alerts: true,
        }
    }
}

fn path() -> PathBuf {
    crate::platform::config_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        let mut s: Settings = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.secret = crate::platform::unprotect(&s.secret_enc).unwrap_or_default();
        s
    }

    pub fn save(&mut self) {
        self.secret_enc = if self.secret.is_empty() { String::new() } else { crate::platform::protect(&self.secret) };
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }

    pub fn unit(&self) -> &'static str {
        if self.mmol { "mmol/L" } else { "mg/dL" }
    }

    /// Formats a mg/dL value in the user's unit.
    pub fn fmt(&self, mgdl: f32) -> String {
        if self.mmol {
            format!("{:.1}", mgdl / crate::nightscout::MMOL_FACTOR)
        } else {
            format!("{:.0}", mgdl)
        }
    }

    pub fn fmt_delta(&self, mgdl: f32) -> String {
        let s = self.fmt(mgdl.abs());
        let zero = s.chars().all(|c| c == '0' || c == '.');
        format!("{}{}", if mgdl < 0.0 && !zero { "\u{2212}" } else { "+" }, s)
    }
}
