//! Nightscout REST client (portable).
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::time::Duration;

pub const MMOL_FACTOR: f32 = 18.018;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Unix time in milliseconds.
    pub ts: i64,
    /// Glucose in mg/dL.
    pub sgv: f32,
    pub direction: String,
}

#[derive(Deserialize)]
struct RawEntry {
    sgv: Option<f64>,
    date: Option<f64>,
    direction: Option<String>,
}

fn looks_like_token(s: &str) -> bool {
    // Nightscout access tokens look like "name-0123456789abcdef".
    match s.rsplit_once('-') {
        Some((name, hex)) => !name.is_empty() && hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => false,
    }
}

pub fn normalize_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.is_empty() || u.starts_with("http://") || u.starts_with("https://") {
        u.to_string()
    } else {
        format!("https://{u}")
    }
}

/// Fetches all SGV entries newer than `since_ms`, oldest first.
pub fn fetch(base_url: &str, secret: &str, since_ms: i64) -> Result<Vec<Entry>, String> {
    let base = normalize_url(base_url);
    if base.is_empty() {
        return Err("Not configured".into());
    }
    let secret = secret.trim();
    let mut url = format!("{base}/api/v1/entries/sgv.json?find%5Bdate%5D%5B%24gte%5D={since_ms}&count=2000");
    let token = looks_like_token(secret);
    if token {
        url.push_str("&token=");
        url.push_str(secret);
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        )
        .build()
        .into();

    let mut req = agent.get(&url).header("Accept", "application/json");
    if !secret.is_empty() && !token {
        let hash: String = Sha1::digest(secret.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        req = req.header("api-secret", &hash);
    }

    let mut resp = req.call().map_err(|e| match e {
        ureq::Error::StatusCode(401) | ureq::Error::StatusCode(403) => {
            "Access denied. Check the API secret or token.".to_string()
        }
        ureq::Error::StatusCode(404) => "Nightscout API not found at this address.".to_string(),
        ureq::Error::StatusCode(c) => format!("Server responded with HTTP {c}."),
        ureq::Error::Timeout(_) => "Connection timed out.".to_string(),
        ureq::Error::HostNotFound => "Server not found. Check the address.".to_string(),
        _ => "Could not reach the server.".to_string(),
    })?;
    let body = resp.body_mut().read_to_string().map_err(|_| "Could not read the server response.".to_string())?;
    let raw: Vec<RawEntry> =
        serde_json::from_str(&body).map_err(|_| "Unexpected response. Is this a Nightscout site?".to_string())?;

    let mut entries: Vec<Entry> = raw
        .into_iter()
        .filter_map(|r| {
            let sgv = r.sgv? as f32;
            let ts = r.date? as i64;
            (sgv > 0.0).then(|| Entry { ts, sgv, direction: r.direction.unwrap_or_default() })
        })
        .collect();
    entries.sort_by_key(|e| e.ts);
    entries.dedup_by_key(|e| e.ts);
    Ok(entries)
}

/// Synthetic 24 h of readings for `--demo` and screenshots.
pub fn demo(now_ms: i64) -> Vec<Entry> {
    let step = 5 * 60_000i64;
    let last = now_ms - 90_000;
    let value = |ts: i64| {
        let m = ts as f64 / 60_000.0;
        (128.0 + 46.0 * (m / 97.0).sin() + 27.0 * (m / 41.0 + 1.3).sin() + 9.0 * (m / 17.0).sin()) as f32
    };
    (0..288)
        .rev()
        .map(|i| {
            let ts = last - i * step;
            let (v, prev) = (value(ts), value(ts - step));
            let d = v - prev;
            let direction = match d {
                d if d > 15.0 => "DoubleUp",
                d if d > 10.0 => "SingleUp",
                d if d > 5.0 => "FortyFiveUp",
                d if d < -15.0 => "DoubleDown",
                d if d < -10.0 => "SingleDown",
                d if d < -5.0 => "FortyFiveDown",
                _ => "Flat",
            };
            Entry { ts, sgv: v.round(), direction: direction.into() }
        })
        .collect()
}
