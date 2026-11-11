//! Portable drawing: history graph and trend arrow (tiny-skia), plus glucose zones.
use crate::nightscout::Entry;
use crate::settings::Settings;
use tiny_skia::*;

pub type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    UrgentLow,
    Low,
    InRange,
    High,
    UrgentHigh,
}

pub fn zone_of(v: f32, s: &Settings) -> Zone {
    if v < s.urgent_low {
        Zone::UrgentLow
    } else if v < s.low {
        Zone::Low
    } else if v > s.urgent_high {
        Zone::UrgentHigh
    } else if v > s.high {
        Zone::High
    } else {
        Zone::InRange
    }
}

/// Colour for a zone on a dark or light surface. `neutral` is used for in-range values.
pub fn zone_color(z: Zone, dark: bool, neutral: Rgb) -> Rgb {
    match (z, dark) {
        (Zone::InRange, _) => neutral,
        (Zone::Low | Zone::UrgentLow, true) => (0xFF, 0x6B, 0x6B),
        (Zone::Low | Zone::UrgentLow, false) => (0xC4, 0x2B, 0x1C),
        (Zone::High, true) => (0xFF, 0xC2, 0x47),
        (Zone::High, false) => (0x9A, 0x60, 0x00),
        (Zone::UrgentHigh, true) => (0xFF, 0x9A, 0x4D),
        (Zone::UrgentHigh, false) => (0xB8, 0x45, 0x0B),
    }
}

fn green(dark: bool) -> Rgb {
    if dark { (0x4C, 0xD0, 0x80) } else { (0x17, 0x91, 0x4A) }
}

// ---------------------------------------------------------------------------------------
// Graph

pub struct Graph {
    pub pixmap: Pixmap,
    /// (x in px, label)
    pub x_labels: Vec<(f32, String)>,
    /// (y in px, label)
    pub y_labels: Vec<(f32, String)>,
}

pub struct GraphInput<'a> {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub entries: &'a [Entry],
    pub now_ms: i64,
    pub hours: u32,
    pub settings: &'a Settings,
    pub dark: bool,
    pub utc_offset_min: i64,
}

fn paint(rgb: Rgb, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(rgb.0, rgb.1, rgb.2, a);
    p.anti_alias = true;
    p
}

pub fn render_graph(g: &GraphInput) -> Graph {
    let (w, h, sc) = (g.width as f32, g.height as f32, g.scale);
    let mut pm = Pixmap::new(g.width.max(1), g.height.max(1)).unwrap();
    let s = g.settings;
    let span = g.hours as i64 * 3_600_000;
    let t0 = g.now_ms - span;
    let visible: Vec<&Entry> = g.entries.iter().filter(|e| e.ts >= t0 - 600_000).collect();

    let pad_r = 7.0 * sc;
    let pad_y = 6.0 * sc;
    let max_v = visible.iter().map(|e| e.sgv).fold(0.0f32, f32::max);
    let y_min = 40.0f32;
    let y_max = (max_v + 20.0).max(s.high + 50.0).min(420.0);
    let x_of = |ts: i64| (ts - t0) as f32 / span as f32 * (w - pad_r);
    let y_of = |v: f32| h - pad_y - (v.clamp(y_min, y_max) - y_min) / (y_max - y_min) * (h - 2.0 * pad_y);

    let neutral: Rgb = if g.dark { (255, 255, 255) } else { (0, 0, 0) };
    let id = Transform::identity();

    // Target band
    if let Some(r) = Rect::from_ltrb(0.0, y_of(s.high), w, y_of(s.low)) {
        pm.fill_rect(r, &paint(green(g.dark), if g.dark { 30 } else { 26 }), id, None);
    }

    // Time grid + labels, aligned to local clock hours
    let step_h: i64 = match g.hours {
        0..=3 => 1,
        4..=6 => 2,
        7..=12 => 3,
        _ => 6,
    };
    let step = step_h * 3_600_000;
    let off = g.utc_offset_min * 60_000;
    let mut t = (t0 + off).div_euclid(step) * step + step - off;
    let mut x_labels = Vec::new();
    let hair = Stroke { width: sc.max(1.0), ..Default::default() };
    while t < g.now_ms {
        let x = x_of(t).round() + 0.5;
        let mut pb = PathBuilder::new();
        pb.move_to(x, 0.0);
        pb.line_to(x, h);
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &paint(neutral, if g.dark { 20 } else { 18 }), &hair, id, None);
        }
        let local_min = (t + off).div_euclid(60_000).rem_euclid(1440);
        x_labels.push((x, format!("{:02}:{:02}", local_min / 60, local_min % 60)));
        t += step;
    }

    // Threshold lines
    let dash = Stroke {
        width: sc.max(1.0),
        dash: StrokeDash::new(vec![3.0 * sc, 3.0 * sc], 0.0),
        ..Default::default()
    };
    let mut y_labels = Vec::new();
    for (v, z) in [(s.low, Zone::Low), (s.high, Zone::High)] {
        let y = y_of(v).round() + 0.5;
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, y);
        pb.line_to(w, y);
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &paint(zone_color(z, g.dark, neutral), 150), &dash, id, None);
        }
        y_labels.push((y, s.fmt(v)));
    }

    // Connecting line (broken at gaps)
    let mut pb = PathBuilder::new();
    let mut prev_ts = 0i64;
    for e in &visible {
        let (x, y) = (x_of(e.ts), y_of(e.sgv));
        if e.ts - prev_ts > 16 * 60_000 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
        prev_ts = e.ts;
    }
    if let Some(p) = pb.finish() {
        let stroke = Stroke { width: 1.5 * sc, line_join: LineJoin::Round, line_cap: LineCap::Round, ..Default::default() };
        pm.stroke_path(&p, &paint(neutral, if g.dark { 70 } else { 60 }), &stroke, id, None);
    }

    // Readings
    let r = sc * match g.hours {
        0..=3 => 2.6,
        4..=6 => 2.2,
        7..=12 => 1.8,
        _ => 1.5,
    };
    for e in &visible {
        let c = zone_color(zone_of(e.sgv, s), g.dark, green(g.dark));
        if let Some(p) = PathBuilder::from_circle(x_of(e.ts), y_of(e.sgv), r) {
            pm.fill_path(&p, &paint(c, 255), FillRule::Winding, id, None);
        }
    }
    if let Some(e) = visible.last() {
        let c = zone_color(zone_of(e.sgv, s), g.dark, green(g.dark));
        let (x, y) = (x_of(e.ts), y_of(e.sgv));
        if let Some(p) = PathBuilder::from_circle(x, y, 6.5 * sc) {
            pm.fill_path(&p, &paint(c, 60), FillRule::Winding, id, None);
        }
        if let Some(p) = PathBuilder::from_circle(x, y, 3.6 * sc) {
            pm.fill_path(&p, &paint(c, 255), FillRule::Winding, id, None);
        }
    }

    Graph { pixmap: pm, x_labels, y_labels }
}

// ---------------------------------------------------------------------------------------
// Trend arrow

pub fn arrow_char(direction: &str) -> &'static str {
    match direction {
        "Flat" => "\u{2192}",
        "FortyFiveUp" => "\u{2197}",
        "SingleUp" => "\u{2191}",
        "DoubleUp" => "\u{2191}\u{2191}",
        "FortyFiveDown" => "\u{2198}",
        "SingleDown" => "\u{2193}",
        "DoubleDown" => "\u{2193}\u{2193}",
        _ => "",
    }
}

pub fn render_arrow(size: u32, direction: &str, rgb: Rgb) -> Pixmap {
    let mut pm = Pixmap::new(size.max(1), size.max(1)).unwrap();
    let (angle, double) = match direction {
        "Flat" => (0.0, false),
        "FortyFiveUp" => (-45.0, false),
        "SingleUp" => (-90.0, false),
        "DoubleUp" => (-90.0, true),
        "FortyFiveDown" => (45.0, false),
        "SingleDown" => (90.0, false),
        "DoubleDown" => (90.0, true),
        _ => return pm,
    };
    let mut pb = PathBuilder::new();
    pb.move_to(-0.34, 0.0);
    pb.line_to(0.34, 0.0);
    pb.move_to(0.08, -0.25);
    pb.line_to(0.34, 0.0);
    pb.line_to(0.08, 0.25);
    let path = pb.finish().unwrap();
    let stroke = Stroke { width: 0.1, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    let s = size as f32;
    let offsets: &[f32] = if double { &[-0.2, 0.2] } else { &[0.0] };
    for off in offsets {
        let k = if double { 0.86 } else { 1.0 };
        let ts = Transform::from_translate(0.0, *off)
            .post_scale(k, k)
            .post_rotate(angle)
            .post_scale(s, s)
            .post_translate(s / 2.0, s / 2.0);
        pm.stroke_path(&path, &paint(rgb, 255), &stroke, ts, None);
    }
    pm
}
