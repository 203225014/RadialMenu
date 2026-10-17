//! Собирает assets/RadialMenu.ico (и logo.svg) из векторного описания логотипа.
//! Запуск: cargo run --release --example make_icon -- assets
//! Для 16/24/32 px — упрощённые варианты (толще обводка, меньше деталей), чтобы значок читался в трее.

use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

// сектора кольца (r=78, центр 128,128): верх, право, низ, лево
const ARCS: [&str; 4] = [
    "M89.47 60.18A78 78 0 0 1 166.53 60.18",
    "M195.82 89.47A78 78 0 0 1 195.82 166.53",
    "M166.53 195.82A78 78 0 0 1 89.47 195.82",
    "M60.18 166.53A78 78 0 0 1 60.18 89.47",
];

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a * 255.0).round() as u8)
}

fn path(d: &str) -> tiny_skia::Path {
    use svgtypes::{SimplePathSegment as S, SimplifyingPathParser};
    let mut pb = PathBuilder::new();
    for seg in SimplifyingPathParser::from(d) {
        match seg.unwrap() {
            S::MoveTo { x, y } => pb.move_to(x as f32, y as f32),
            S::LineTo { x, y } => pb.line_to(x as f32, y as f32),
            S::Quadratic { x1, y1, x, y } => pb.quad_to(x1 as f32, y1 as f32, x as f32, y as f32),
            S::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => pb.cubic_to(
                x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
            ),
            S::ClosePath => pb.close(),
        }
    }
    pb.finish().unwrap()
}

fn fill(pm: &mut Pixmap, p: &tiny_skia::Path, c: Color, tf: Transform) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(c);
    pm.fill_path(p, &paint, FillRule::Winding, tf, None);
}

fn stroke(pm: &mut Pixmap, p: &tiny_skia::Path, c: Color, w: f32, round: bool, tf: Transform) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(c);
    let s = Stroke {
        width: w,
        line_cap: if round { LineCap::Round } else { LineCap::Butt },
        ..Stroke::default()
    };
    pm.stroke_path(p, &paint, &s, tf, None);
}

fn circle(cx: f32, cy: f32, r: f32) -> tiny_skia::Path {
    PathBuilder::from_circle(cx, cy, r).unwrap()
}

fn tile(rect: f32, rx: f32, off: f32) -> tiny_skia::Path {
    let mut pb = PathBuilder::new();
    let (x0, y0, x1, y1) = (off, off, off + rect, off + rect);
    let k = 0.552_284_8 * rx;
    pb.move_to(x0 + rx, y0);
    pb.line_to(x1 - rx, y0);
    pb.cubic_to(x1 - rx + k, y0, x1, y0 + rx - k, x1, y0 + rx);
    pb.line_to(x1, y1 - rx);
    pb.cubic_to(x1, y1 - rx + k, x1 - rx + k, y1, x1 - rx, y1);
    pb.line_to(x0 + rx, y1);
    pb.cubic_to(x0 + rx - k, y1, x0, y1 - rx + k, x0, y1 - rx);
    pb.line_to(x0, y0 + rx);
    pb.cubic_to(x0, y0 + rx - k, x0 + rx - k, y0, x0 + rx, y0);
    pb.close();
    pb.finish().unwrap()
}

/// Вид значка: с тёмной плиткой (exe, Проводник) или без неё — для трея, под цвет панели задач.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Plate,
    /// Без плитки, светлый рисунок — для тёмной панели задач.
    TrayDark,
    /// Без плитки, тёмный рисунок — для светлой панели задач.
    TrayLight,
}

/// Рисует логотип в квадрат `px`×`px` (координаты макета — 256×256); `target` — размер, в котором
/// значок увидят (мелкие рисуются в 4× и сжимаются, а упрощение зависит от конечного размера).
fn draw(px: u32, target: u32, style: Style) -> Pixmap {
    let mut pm = Pixmap::new(px, px).unwrap();
    let tf = Transform::from_scale(px as f32 / 256.0, px as f32 / 256.0);
    let (ring_w, dot_y, detail) = match target {
        0..=16 => (34.0, 124.0, 0),
        17..=24 => (32.0, 124.0, 1),
        25..=32 => (30.0, 118.0, 2),
        _ => (30.0, 118.0, 3),
    };
    // в трее значок крошечный: рисунок увеличиваем, чтобы кольцо занимало всю клетку, как у соседей
    let plate = style == Style::Plate;
    let k = if !plate { 1.3 } else if target <= 40 { 1.24 } else { 1.0 };
    let tfg = tf.pre_translate(128.0, 128.0).pre_scale(k, k).pre_translate(-128.0, -128.0);
    if plate {
        fill(&mut pm, &tile(256.0, 58.0, 0.0), rgba(0x1b, 0x1f, 0x2b, 1.0), tf);
    }
    if plate && detail == 3 {
        stroke(
            &mut pm,
            &tile(254.0, 57.0, 1.0),
            rgba(0x8f, 0xa6, 0xd6, 0.35),
            2.0,
            false,
            tf,
        );
    }
    let (seg, accent, hub) = match style {
        Style::TrayLight => (rgba(0x2b, 0x33, 0x4d, 1.0), rgba(0x3b, 0x6c, 0xf0, 1.0), rgba(0x5b, 0x65, 0x80, 0.85)),
        _ if detail == 3 => (rgba(0xdd, 0xe3, 0xf0, 0.92), rgba(0x69, 0x9b, 0xff, 1.0), rgba(0xcc, 0xcc, 0xcc, 0.75)),
        _ => (rgba(0xdd, 0xe3, 0xf0, 1.0), rgba(0x69, 0x9b, 0xff, 1.0), rgba(0xcc, 0xcc, 0xcc, 0.75)),
    };
    for (i, d) in ARCS.iter().enumerate() {
        let c = if i == 0 {
            accent
        } else {
            seg
        };
        stroke(&mut pm, &path(d), c, ring_w, true, tfg);
    }
    match detail {
        0 => fill(
            &mut pm,
            &circle(128.0, dot_y, 20.0),
            rgba(0x4c, 0x7d, 0xff, 1.0),
            tfg,
        ),
        1 => {
            fill(
                &mut pm,
                &circle(128.0, dot_y, 20.0),
                rgba(0xb3, 0xcc, 0xff, 1.0),
                tfg,
            );
            fill(
                &mut pm,
                &circle(128.0, dot_y, 16.0),
                rgba(0x4c, 0x7d, 0xff, 1.0),
                tfg,
            );
        }
        _ => {
            fill(
                &mut pm,
                &circle(128.0, 128.0, 22.0),
                hub,
                tfg,
            );
            if detail == 3 {
                stroke(
                    &mut pm,
                    &circle(128.0, 128.0, 22.0),
                    rgba(0xff, 0xff, 0xff, 0.9),
                    2.0,
                    false,
                    tfg,
                );
            }
            fill(
                &mut pm,
                &circle(128.0, dot_y, 10.25),
                rgba(0xb3, 0xcc, 0xff, 1.0),
                tfg,
            );
            fill(
                &mut pm,
                &circle(128.0, dot_y, 7.75),
                rgba(0x4c, 0x7d, 0xff, 1.0),
                tfg,
            );
        }
    }
    pm
}

/// Мелкие размеры рисуем в 4× и усредняем — края чище, чем у прямой растеризации.
fn render(px: u32, style: Style) -> Pixmap {
    if px >= 64 {
        return draw(px, px, style);
    }
    let big = draw(px * 4, px, style);
    let mut out = Pixmap::new(px, px).unwrap();
    let (src, dst) = (big.data(), out.data_mut());
    for y in 0..px as usize {
        for x in 0..px as usize {
            for c in 0..4 {
                let mut acc = 0u32;
                for (dy, dx) in (0..4).flat_map(|a| (0..4).map(move |b| (a, b))) {
                    acc += src[((y * 4 + dy) * px as usize * 4 + x * 4 + dx) * 4 + c] as u32;
                }
                dst[(y * px as usize + x) * 4 + c] = (acc / 16) as u8;
            }
        }
    }
    out
}

fn png_bytes(pm: &Pixmap) -> Vec<u8> {
    // tiny-skia хранит предумноженную альфу, PNG ждёт обычную
    let mut rgba = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, pm.width(), pm.height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    out
}

/// ICO из PNG-кадров заданных размеров.
fn build_ico(sizes: &[u32], style: Style) -> (Vec<u8>, Vec<Vec<u8>>) {
    let frames: Vec<Vec<u8>> = sizes.iter().map(|&s| png_bytes(&render(s, style))).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (s, f) in sizes.iter().zip(&frames) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(f.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += f.len();
    }
    frames.iter().for_each(|f| ico.extend_from_slice(f));
    (ico, frames)
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "assets".into()));
    let sizes = [16u32, 20, 24, 28, 32, 40, 48, 64, 128, 256];
    let (ico, frames) = build_ico(&sizes, Style::Plate);
    std::fs::write(dir.join("RadialMenu.ico"), &ico).unwrap();
    std::fs::write(dir.join("logo-256.png"), frames.last().unwrap()).unwrap();
    // трей: без плитки, под тёмную и светлую панель задач (какую — решает программа при запуске)
    let tray = [16u32, 20, 24, 28, 32, 40, 48, 64];
    std::fs::write(dir.join("tray-dark.ico"), build_ico(&tray, Style::TrayDark).0).unwrap();
    std::fs::write(dir.join("tray-light.ico"), build_ico(&tray, Style::TrayLight).0).unwrap();
    let _ = Rect::from_xywh(0.0, 0.0, 1.0, 1.0);
    eprintln!("RadialMenu.ico: {} байт, размеры {:?}; tray-dark/light.ico: {:?}", ico.len(), sizes, tray);
}
