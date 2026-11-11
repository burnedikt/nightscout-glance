#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod nightscout;
mod platform;
mod render;
mod settings;

use nightscout::{Entry, MMOL_FACTOR};
use platform::{FlyoutHost, Tray, TrayEvent, TrayState};
use render::{zone_color, zone_of, Rgb, Zone};
use settings::Settings;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

slint::include_modules!();

const STALE_MINUTES: i64 = 15;
const RANGES: [u32; 4] = [3, 6, 12, 24];

enum WorkerMsg {
    Refresh,
    Config(Settings),
    Quit,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn ago_text(minutes: i64) -> String {
    match minutes {
        m if m < 1 => "just now".into(),
        m if m < 60 => format!("{m} min ago"),
        m => format!("{} h {} min ago", m / 60, m % 60),
    }
}

fn to_image(pm: &tiny_skia::Pixmap) -> slint::Image {
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pm.data(), pm.width(), pm.height());
    slint::Image::from_rgba8_premultiplied(buf)
}

fn color(c: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(c.0, c.1, c.2)
}

struct App {
    settings: Settings,
    entries: Vec<Entry>,
    error: Option<String>,
    demo: bool,
    force_dark: Option<bool>,
    /// Development aid (`--debug-flyout`): keep the flyout open regardless of the pointer.
    pinned: bool,

    flyout: Flyout,
    host: FlyoutHost,
    flyout_visible: bool,
    left_at: Option<Instant>,

    tray: Tray,

    last_zone: Option<Zone>,
    stale_notified: bool,

    tx: mpsc::Sender<WorkerMsg>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static SETTINGS_WIN: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
    static HOVER_TIMER: Timer = Timer::default();
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

impl App {
    fn on_data(&mut self, result: Result<Vec<Entry>, String>) {
        match result {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) if e.is_empty() => {
                self.entries.clear();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.refresh_ui();
    }

    /// Updates tray icon, tooltip, notifications and (if visible) the flyout.
    fn refresh_ui(&mut self) {
        let now = now_ms();
        let s = &self.settings;
        let (text, arrow, zone, stale, tip) = match self.entries.last() {
            Some(e) => {
                let age = (now - e.ts) / 60_000;
                let arrow = render::arrow_char(&e.direction);
                let tip = format!("{} {} {}  \u{b7}  {}", s.fmt(e.sgv), s.unit(), arrow, ago_text(age));
                (s.fmt(e.sgv), arrow, Some(zone_of(e.sgv, s)), age >= STALE_MINUTES, tip)
            }
            None => {
                let tip = if s.url.is_empty() && !self.demo {
                    "Nightscout Glance: not set up yet".to_string()
                } else {
                    self.error.clone().unwrap_or_else(|| "Nightscout Glance: connecting\u{2026}".into())
                };
                ("--".to_string(), "", None, true, tip)
            }
        };
        self.tray.update(&TrayState { text: &text, arrow, zone, stale, tip: &tip });

        self.check_alerts(now);
        if self.flyout_visible {
            self.update_flyout();
        }
    }

    fn check_alerts(&mut self, now: i64) {
        if !self.settings.alerts || self.demo {
            return;
        }
        let Some(e) = self.entries.last() else { return };
        let s = &self.settings;
        let age = (now - e.ts) / 60_000;
        if age >= STALE_MINUTES {
            if !self.stale_notified {
                self.stale_notified = true;
                self.tray.notify("No recent readings", &format!("The last glucose reading arrived {}.", ago_text(age)));
            }
            return;
        }
        self.stale_notified = false;
        let zone = zone_of(e.sgv, s);
        let prev = self.last_zone.replace(zone);
        if prev == Some(zone) || zone == Zone::InRange {
            return;
        }
        // Recovering from urgent to plain low/high is good news, not worth an alert.
        if matches!((prev, zone), (Some(Zone::UrgentLow), Zone::Low) | (Some(Zone::UrgentHigh), Zone::High)) {
            return;
        }
        let title = match zone {
            Zone::UrgentLow => "Urgent low glucose",
            Zone::Low => "Low glucose",
            Zone::High => "High glucose",
            _ => "Urgent high glucose",
        };
        let body = format!("{} {} {}", s.fmt(e.sgv), s.unit(), render::arrow_char(&e.direction));
        self.tray.notify(title, body.trim());
    }

    fn update_flyout(&mut self) {
        let f = &self.flyout;
        let s = &self.settings;
        let now = now_ms();
        let dark = self.force_dark.unwrap_or_else(platform::system_is_dark);
        let scale = f.window().scale_factor();
        let fg: Rgb = if dark { (255, 255, 255) } else { (0x1a, 0x1a, 0x1a) };
        let tz = platform::utc_offset_minutes();

        f.set_dark(dark);
        f.set_range_index(RANGES.iter().position(|r| *r == s.range_hours).unwrap_or(0) as i32);

        match self.entries.last() {
            Some(e) => {
                let age = (now - e.ts) / 60_000;
                let prev = self.entries.iter().rev().find(|p| e.ts - p.ts >= 210_000 && e.ts - p.ts <= 660_000);
                f.set_value(s.fmt(e.sgv).into());
                f.set_accent(color(zone_color(zone_of(e.sgv, s), dark, fg)));
                f.set_stale(age >= STALE_MINUTES);
                f.set_delta(match prev {
                    Some(p) => format!("{} {}", s.fmt_delta(e.sgv - p.sgv), s.unit()).into(),
                    None => s.unit().into(),
                });
                f.set_ago(ago_text(age).into());
                let arrow_px = (30.0 * scale).round() as u32;
                let arrow_rgb = if age >= STALE_MINUTES { (128, 128, 128) } else { fg };
                f.set_arrow(to_image(&render::render_arrow(arrow_px, &e.direction, arrow_rgb)));
                f.set_message("".into());
                let local = (e.ts / 60_000 + tz).rem_euclid(1440);
                match &self.error {
                    Some(err) => {
                        f.set_footer(err.as_str().into());
                        f.set_footer_warning(true);
                    }
                    None => {
                        f.set_footer(format!("Last reading {:02}:{:02}", local / 60, local % 60).into());
                        f.set_footer_warning(false);
                    }
                }
            }
            None => {
                f.set_value("--".into());
                f.set_accent(color(fg));
                f.set_stale(true);
                f.set_delta("".into());
                f.set_ago("".into());
                f.set_arrow(slint::Image::default());
                f.set_footer("".into());
                f.set_message(
                    if s.url.is_empty() && !self.demo {
                        "Add your Nightscout address in Settings to get started.".into()
                    } else if let Some(err) = &self.error {
                        err.as_str().into()
                    } else {
                        "Loading\u{2026}".into()
                    },
                );
            }
        }

        let graph = render::render_graph(&render::GraphInput {
            width: (f.get_graph_w() * scale).round() as u32,
            height: (f.get_graph_h() * scale).round() as u32,
            scale,
            entries: &self.entries,
            now_ms: now,
            hours: s.range_hours,
            settings: s,
            dark,
            utc_offset_min: tz,
        });
        f.set_graph(to_image(&graph.pixmap));
        let labels = |v: Vec<(f32, String)>| {
            ModelRc::new(VecModel::from(
                v.into_iter().map(|(p, t)| AxisLabel { pos: p / scale, text: t.into() }).collect::<Vec<_>>(),
            ))
        };
        f.set_x_labels(labels(graph.x_labels));
        f.set_y_labels(labels(graph.y_labels));
    }
}

// ---------------------------------------------------------------------------------------
// Flyout: shown while the pointer is over the tray item or the flyout itself. How the
// native window is shown, placed and hidden is up to the platform's FlyoutHost.

/// Runs once the event loop is up: draws the tray item and hooks up the flyout window.
fn startup() {
    with_app(|app| app.refresh_ui());
    attach_flyout();
}

fn attach_flyout() {
    let done = with_app(|app| app.host.attach(&app.flyout)).unwrap_or(true);
    if !done {
        Timer::single_shot(Duration::from_millis(40), attach_flyout);
    }
}

fn show_flyout() {
    let started = with_app(|app| {
        if !app.host.ready() {
            return false;
        }
        app.left_at = None;
        if app.flyout_visible {
            return false;
        }
        app.flyout_visible = true;
        app.update_flyout();
        app.host.show(&app.flyout, &app.tray);
        true
    });
    if started == Some(true) {
        HOVER_TIMER.with(|t| t.start(TimerMode::Repeated, Duration::from_millis(120), hover_tick));
    }
}

fn hide_flyout() {
    with_app(|app| {
        if app.flyout_visible {
            app.flyout_visible = false;
            app.host.hide(&app.flyout);
        }
    });
}

fn hover_tick() {
    let stop = with_app(|app| {
        if !app.flyout_visible {
            return true;
        }
        if app.pinned || app.host.pointer_inside(&app.flyout, &app.tray) {
            app.left_at = None;
            return false;
        }
        if app.left_at.get_or_insert_with(Instant::now).elapsed() < Duration::from_millis(450) {
            return false;
        }
        app.flyout_visible = false;
        app.host.hide(&app.flyout);
        true
    })
    .unwrap_or(false);
    if stop {
        HOVER_TIMER.with(|t| t.stop());
        platform::release_focus();
    }
}

// ---------------------------------------------------------------------------------------
// Settings window (created on demand, dropped when closed)

fn close_settings() {
    if let Some(win) = SETTINGS_WIN.with(|s| s.borrow_mut().take()) {
        let _ = win.hide();
        platform::release_focus();
        // Drop outside of the window's own callback.
        Timer::single_shot(Duration::from_millis(50), move || drop(win));
    }
}

fn fill_thresholds(win: &SettingsWindow, s: &Settings) {
    win.set_urgent_low(s.fmt(s.urgent_low).into());
    win.set_low(s.fmt(s.low).into());
    win.set_high(s.fmt(s.high).into());
    win.set_urgent_high(s.fmt(s.urgent_high).into());
}

fn read_thresholds(win: &SettingsWindow, mmol: bool) -> Option<[f32; 4]> {
    let parse = |t: slint::SharedString| {
        t.replace(',', ".").trim().parse::<f32>().ok().map(|v| if mmol { v * MMOL_FACTOR } else { v })
    };
    Some([parse(win.get_urgent_low())?, parse(win.get_low())?, parse(win.get_high())?, parse(win.get_urgent_high())?])
}

fn build_settings_window(s: &Settings) -> SettingsWindow {
    let win = SettingsWindow::new().unwrap();
    win.set_url(s.url.as_str().into());
    win.set_secret(s.secret.as_str().into());
    win.set_unit_index(s.mmol as i32);
    fill_thresholds(&win, s);
    win.set_alerts(s.alerts);
    win.set_autostart(platform::autostart_enabled());
    win.set_mac(platform::IS_MAC);
    win.set_version(format!("Version {}  ·  Made with Slint", env!("CARGO_PKG_VERSION")).into());

    win.on_cancel(close_settings);
    win.window().on_close_requested(|| {
        close_settings();
        slint::CloseRequestResponse::HideWindow
    });

    // Convert the threshold fields when the unit changes.
    let weak = win.as_weak();
    let shown_mmol = Rc::new(Cell::new(s.mmol));
    win.on_units_changed(move |idx| {
        let Some(win) = weak.upgrade() else { return };
        let mmol = idx == 1;
        if mmol == shown_mmol.get() {
            return;
        }
        if let Some([ul, l, h, uh]) = read_thresholds(&win, shown_mmol.get()) {
            let tmp = Settings { mmol, urgent_low: ul, low: l, high: h, urgent_high: uh, ..Settings::default() };
            fill_thresholds(&win, &tmp);
        }
        shown_mmol.set(mmol);
    });

    let weak = win.as_weak();
    win.on_test(move || {
        let Some(win) = weak.upgrade() else { return };
        win.set_test_state(1);
        win.set_test_message("".into());
        let (url, secret, mmol) = (win.get_url().to_string(), win.get_secret().to_string(), win.get_unit_index() == 1);
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = nightscout::fetch(&url, &secret, now_ms() - 6 * 3_600_000);
            let unit = Settings { mmol, ..Settings::default() };
            let (state, msg) = match result {
                Ok(entries) => match entries.last() {
                    Some(e) => (
                        2,
                        format!(
                            "Connected. Latest reading: {} {}, {}.",
                            unit.fmt(e.sgv),
                            unit.unit(),
                            ago_text((now_ms() - e.ts) / 60_000)
                        ),
                    ),
                    None => (2, "Connected, but there are no readings from the last 6 hours.".to_string()),
                },
                Err(e) => (3, e),
            };
            let _ = weak.upgrade_in_event_loop(move |win| {
                win.set_test_state(state);
                win.set_test_message(msg.into());
            });
        });
    });

    let weak = win.as_weak();
    win.on_save(move || {
        let Some(win) = weak.upgrade() else { return };
        let mmol = win.get_unit_index() == 1;
        let Some([ul, l, h, uh]) = read_thresholds(&win, mmol).filter(|t| t[0] > 0.0 && t.windows(2).all(|p| p[0] < p[1]))
        else {
            win.set_error("Thresholds must be ascending: urgent low < low < high < urgent high.".into());
            return;
        };
        let url = nightscout::normalize_url(&win.get_url());
        let secret = win.get_secret().trim().to_string();
        let alerts = win.get_alerts();
        platform::set_autostart(win.get_autostart());
        with_app(|app| {
            let source_changed = app.settings.url != url || app.settings.secret != secret;
            app.settings = Settings { url, secret, mmol, urgent_low: ul, low: l, high: h, urgent_high: uh, alerts, ..app.settings.clone() };
            app.settings.save();
            if source_changed {
                app.entries.clear();
                app.error = None;
            }
            app.last_zone = None;
            let _ = app.tx.send(WorkerMsg::Config(app.settings.clone()));
            app.refresh_ui();
        });
        close_settings();
    });
    win
}

fn open_settings() {
    hide_flyout();
    let existing = SETTINGS_WIN.with(|s| s.borrow().as_ref().map(|w| w.clone_strong()));
    let win = match existing {
        Some(win) => win,
        None => {
            let Some(settings) = with_app(|a| a.settings.clone()) else { return };
            let win = build_settings_window(&settings);
            SETTINGS_WIN.with(|s| *s.borrow_mut() = Some(win.clone_strong()));
            win
        }
    };
    let _ = win.show();
    platform::focus_window(win.window());
}

// ---------------------------------------------------------------------------------------

fn open_site() {
    match with_app(|a| a.settings.url.clone()) {
        Some(url) if !url.is_empty() => platform::open_url(&nightscout::normalize_url(&url)),
        _ => open_settings(),
    }
}

fn on_tray_event(ev: TrayEvent) {
    match ev {
        TrayEvent::Hover | TrayEvent::Click => show_flyout(),
        TrayEvent::DoubleClick => open_site(),
        TrayEvent::MenuOpening => hide_flyout(),
        TrayEvent::Command(platform::CMD_OPEN_SITE) => open_site(),
        TrayEvent::Command(platform::CMD_REFRESH) => {
            with_app(|a| a.tx.send(WorkerMsg::Refresh).ok());
        }
        TrayEvent::Command(platform::CMD_SETTINGS) => open_settings(),
        TrayEvent::Command(platform::CMD_QUIT) => {
            with_app(|a| a.tx.send(WorkerMsg::Quit).ok());
            let _ = slint::quit_event_loop();
        }
        TrayEvent::Command(_) => {}
    }
}

/// Background polling. Wakes up shortly after the next reading is expected.
fn worker(rx: mpsc::Receiver<WorkerMsg>, mut cfg: Settings, demo: bool) {
    loop {
        let now = now_ms();
        let result = if demo {
            Ok(nightscout::demo(now))
        } else if cfg.url.trim().is_empty() {
            Err(String::new())
        } else {
            nightscout::fetch(&cfg.url, &cfg.secret, now - 24 * 3_600_000 - 600_000)
        };
        let wait_ms = match &result {
            Ok(v) if v.len() >= 2 => {
                let last = v[v.len() - 1].ts;
                let interval = (last - v[v.len() - 2].ts).clamp(60_000, 300_000);
                (last + interval + 15_000 - now_ms()).clamp(20_000, 300_000)
            }
            Ok(_) => 60_000,
            Err(_) => 30_000,
        };
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| a.on_data(result));
        });
        loop {
            match rx.recv_timeout(Duration::from_millis(wait_ms as u64)) {
                Ok(WorkerMsg::Config(s)) => cfg = s,
                Ok(WorkerMsg::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(WorkerMsg::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            break;
        }
    }
}

fn save_snapshot(window: &slint::Window, path: &std::path::Path) {
    if let Ok(buf) = window.take_snapshot() {
        if let Some(size) = tiny_skia::IntSize::from_wh(buf.width(), buf.height()) {
            if let Some(pm) = tiny_skia::Pixmap::from_vec(buf.as_bytes().to_vec(), size) {
                let _ = pm.save_png(path);
            }
        }
    }
}

/// `--screenshots <dir>`: renders the UI with demo data into PNGs and exits.
fn take_screenshots(dir: std::path::PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    for (name, dark) in [("flyout-dark", true), ("flyout-light", false)] {
        with_app(|a| {
            a.force_dark = Some(dark);
            a.update_flyout();
            save_snapshot(a.flyout.window(), &dir.join(format!("{name}.png")));
        });
    }
    for (name, dark) in [("settings-light", false), ("settings-dark", true)] {
        let demo = Settings { url: "https://my-nightscout.example.com".into(), secret: "glance-0123456789abcdef".into(), ..Settings::default() };
        let win = build_settings_window(&demo);
        win.invoke_set_dark(dark);
        let _ = win.show();
        save_snapshot(win.window(), &dir.join(format!("{name}.png")));
        let _ = win.hide();
    }
    let _ = slint::quit_event_loop();
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    let shots = args.iter().position(|a| a == "--screenshots").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let demo = shots.is_some() || args.iter().any(|a| a == "--demo");

    if shots.is_none() && !platform::single_instance() {
        return Ok(());
    }
    platform::init();

    let settings = Settings::load();
    let flyout = Flyout::new()?;
    flyout.set_mac(platform::IS_MAC);
    flyout.on_range_selected(|i| {
        with_app(|app| {
            app.settings.range_hours = RANGES[(i as usize).min(RANGES.len() - 1)];
            if !app.demo {
                app.settings.save();
            }
            app.update_flyout();
        });
    });
    flyout.on_open_settings(open_settings);
    flyout.on_open_site(|| {
        hide_flyout();
        open_site();
    });

    let (tx, rx) = mpsc::channel();
    let tray = Tray::new(Rc::new(on_tray_event));
    let not_configured = settings.url.is_empty() && !demo;

    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            settings: settings.clone(),
            entries: Vec::new(),
            error: None,
            demo,
            force_dark: None,
            pinned: false,
            flyout: flyout.clone_strong(),
            host: FlyoutHost::new(),
            flyout_visible: false,
            left_at: None,
            tray,
            last_zone: None,
            stale_notified: false,
            tx,
        })
    });
    std::thread::spawn(move || worker(rx, settings, demo));
    platform::watch_theme(|| {
        let _ = slint::invoke_from_event_loop(|| {
            with_app(|a| a.refresh_ui());
        });
    });

    // Keeps "x min ago" and the stale state current between fetches.
    let tick = Timer::default();
    tick.start(TimerMode::Repeated, Duration::from_secs(15), || {
        with_app(|a| a.refresh_ui());
    });

    with_app(|a| a.host.prepare(&a.flyout));
    Timer::single_shot(Duration::from_millis(0), startup);

    if let Some(dir) = shots {
        Timer::single_shot(Duration::from_millis(1500), move || take_screenshots(dir));
    } else if args.iter().any(|a| a == "--debug-flyout") {
        // Development aid: open the flyout (twice, to exercise re-showing), keep it open
        // and report where it is.
        Timer::single_shot(Duration::from_millis(1000), || {
            show_flyout();
            with_app(|a| a.pinned = true);
        });
        Timer::single_shot(Duration::from_millis(1800), hide_flyout);
        Timer::single_shot(Duration::from_millis(2600), || {
            with_app(|a| a.pinned = true);
            show_flyout();
            with_app(|a| {
                let (x, y, w, h) = a.host.debug_rect(&a.flyout);
                let _ = std::fs::write(std::env::temp_dir().join("ng-flyout-rect.txt"), format!("{x} {y} {w} {h}"));
            });
        });
    } else if not_configured {
        Timer::single_shot(Duration::from_millis(300), open_settings);
    }

    slint::run_event_loop_until_quit()?;
    APP.with(|a| a.borrow_mut().take()); // removes the tray item
    Ok(())
}
