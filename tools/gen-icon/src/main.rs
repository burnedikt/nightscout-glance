//! Draws the placeholder app logo and writes assets/icon.png + assets/icon.ico.
//! Run from the repo root: cargo run --manifest-path tools/gen-icon/Cargo.toml
use tiny_skia::*;

fn draw(size: u32) -> Pixmap {
    let mut pm = Pixmap::new(size, size).unwrap();
    let ts = Transform::from_scale(size as f32 / 256.0, size as f32 / 256.0);

    // Rounded-square background
    let bg = {
        let (r, s) = (60.0, 256.0);
        let k = 0.5523 * r;
        let mut p = PathBuilder::new();
        p.move_to(r, 0.0);
        p.line_to(s - r, 0.0);
        p.cubic_to(s - r + k, 0.0, s, r - k, s, r);
        p.line_to(s, s - r);
        p.cubic_to(s, s - r + k, s - r + k, s, s - r, s);
        p.line_to(r, s);
        p.cubic_to(r - k, s, 0.0, s - r + k, 0.0, s - r);
        p.line_to(0.0, r);
        p.cubic_to(0.0, r - k, r - k, 0.0, r, 0.0);
        p.close();
        p.finish().unwrap()
    };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(256.0, 256.0),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0x24, 0x34, 0x5C, 255)),
            GradientStop::new(1.0, Color::from_rgba8(0x0E, 0x14, 0x26, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&bg, &paint, FillRule::Winding, ts, None);

    // Target band
    let mut band = Paint::default();
    band.set_color_rgba8(255, 255, 255, 20);
    pm.fill_rect(Rect::from_ltrb(0.0, 96.0, 256.0, 168.0).unwrap(), &band, ts, Some(&mask(&bg, size, ts)));

    // Glucose trace
    let mut p = PathBuilder::new();
    p.move_to(40.0, 172.0);
    p.cubic_to(78.0, 172.0, 84.0, 112.0, 118.0, 124.0);
    p.cubic_to(146.0, 134.0, 152.0, 98.0, 190.0, 92.0);
    let trace = p.finish().unwrap();
    let mut line = Paint::default();
    line.anti_alias = true;
    line.set_color_rgba8(0x4A, 0xDE, 0x80, 255);
    let stroke = Stroke { width: 22.0, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    pm.stroke_path(&trace, &line, &stroke, ts, None);

    // Current-value dot
    let mut white = Paint::default();
    white.anti_alias = true;
    white.set_color_rgba8(255, 255, 255, 255);
    pm.fill_path(&PathBuilder::from_circle(194.0, 91.0, 27.0).unwrap(), &white, FillRule::Winding, ts, None);
    pm.fill_path(&PathBuilder::from_circle(194.0, 91.0, 13.0).unwrap(), &line, FillRule::Winding, ts, None);
    pm
}

fn mask(path: &Path, size: u32, ts: Transform) -> Mask {
    let mut m = Mask::new(size, size).unwrap();
    m.fill_path(path, FillRule::Winding, true, ts);
    m
}

fn main() {
    std::fs::create_dir_all("assets").unwrap();
    draw(256).save_png("assets/icon.png").unwrap();
    draw(512).save_png("site/icon.png").ok();

    let sizes = [16u32, 20, 24, 32, 48, 64, 256];
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|&s| draw(s).encode_png().unwrap()).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, png) in sizes.iter().zip(&pngs) {
        let b = if *s >= 256 { 0u8 } else { *s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    std::fs::write("assets/icon.ico", ico).unwrap();
    println!("wrote assets/icon.png, assets/icon.ico");
}
