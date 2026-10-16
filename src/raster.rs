//! Предварительная отрисовка: «стеклянная» кнопка, иконки, точка-указатель, плашки с подписями.
//! Всё строится один раз при запуске; во время работы кадр собирается из готовых масок
//! (см. `compose`), поэтому в простое меню не тратит ни процессор, ни память на графику.

use std::collections::HashMap;

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::assets::{self, IconSrc};

/// Сторона спрайта кнопки и диаметр круга в нём (макет: круг 64 в квадрате 80).
pub const BOX: f32 = 80.0;
pub const DIAM: f32 = 64.0;
/// Сторона иконки в спрайте.
pub const ICON_BOX: f32 = 24.0;
/// Сторона точки-указателя в центре.
pub const DOT_BOX: f32 = 16.0;

/// Текстура кнопки без цвета: три канала покрытия (тень снаружи круга, круг, обводка).
pub struct ButtonTex {
    pub n: usize,
    pub data: Vec<[u8; 3]>,
}

/// Одноканальная маска (иконка).
pub struct Mask {
    pub n: usize,
    pub data: Vec<u8>,
}

/// Картинка RGBA с предумноженной альфой.
pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PillKind {
    /// Подпись кнопки.
    Label,
    /// Подпись громкости под центром.
    Toast,
}

pub struct Bake {
    /// Во сколько раз текстуры подробнее макета (для чёткости при масштабировании).
    pub t: usize,
    pub button: ButtonTex,
    pub icons: HashMap<&'static str, Mask>,
    pub dot: Rgba,
    pills: HashMap<(String, PillKind), Rgba>,
}

impl Bake {
    pub fn pill(&self, text: &str, kind: PillKind) -> Option<&Rgba> {
        self.pills.get(&(text.to_string(), kind))
    }

    /// `scale` — общий масштаб интерфейса; от него зависит детальность текстур.
    pub fn build(
        icon_keys: &[&'static str],
        texts: &[(String, PillKind)],
        scale: f32,
        font: Option<&fontdue::Font>,
    ) -> Bake {
        let t = ((2.0 * scale).ceil() as usize).clamp(2, 6);
        let icons = icon_keys
            .iter()
            .filter_map(|&k| icon_mask(k, (ICON_BOX as usize) * t).map(|m| (k, m)))
            .collect();
        let mut pills = HashMap::new();
        if let Some(font) = font {
            for (text, kind) in texts {
                let px = match kind {
                    PillKind::Label => 13.0,
                    PillKind::Toast => 14.0,
                } * scale;
                pills.insert((text.clone(), *kind), pill(font, text, px, scale));
            }
        }
        Bake {
            t,
            button: button_tex(t),
            icons,
            dot: dot(t),
            pills,
        }
    }
}

// ---------------------------------------------------------------- кнопка

fn circle_coverage(n: usize, r: f32) -> Vec<f32> {
    let mut pm = Pixmap::new(n as u32, n as u32).expect("размер текстуры");
    let c = n as f32 / 2.0;
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;
    if let Some(path) = PathBuilder::from_circle(c, c, r) {
        pm.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    pm.data()
        .chunks_exact(4)
        .map(|p| p[3] as f32 / 255.0)
        .collect()
}

/// Гауссово размытие (разделяемое), за краем — ноль.
fn gaussian_blur(src: &[f32], n: usize, sigma: f32) -> Vec<f32> {
    let rad = (sigma * 3.0).ceil() as i32;
    let mut kernel: Vec<f32> = (-rad..=rad)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|k| *k /= sum);
    let pass = |inp: &[f32], horizontal: bool| {
        let mut out = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                let mut acc = 0.0;
                for (j, k) in kernel.iter().enumerate() {
                    let d = j as i32 - rad;
                    let (sx, sy) = if horizontal {
                        (x as i32 + d, y as i32)
                    } else {
                        (x as i32, y as i32 + d)
                    };
                    if sx >= 0 && sy >= 0 && (sx as usize) < n && (sy as usize) < n {
                        acc += inp[sy as usize * n + sx as usize] * k;
                    }
                }
                out[y * n + x] = acc;
            }
        }
        out
    };
    pass(&pass(src, true), false)
}

fn button_tex(t: usize) -> ButtonTex {
    let n = BOX as usize * t;
    let tf = t as f32;
    let circle = circle_coverage(n, DIAM / 2.0 * tf);
    let inner = circle_coverage(n, (DIAM / 2.0 - 1.0) * tf); // обводка 1px внутри края
                                                             // мягкая тень: blur σ=4, чёрная 30%, только снаружи круга
    let blurred = gaussian_blur(&circle, n, 4.0 * tf);
    let data = (0..n * n)
        .map(|i| {
            let shadow = blurred[i] * 0.30 * (1.0 - circle[i]);
            let ring = (circle[i] - inner[i]).max(0.0);
            [
                (shadow * 255.0).round() as u8,
                (circle[i] * 255.0).round() as u8,
                (ring * 255.0).round() as u8,
            ]
        })
        .collect();
    ButtonTex { n, data }
}

// ---------------------------------------------------------------- иконки

fn icon_mask(key: &str, size: usize) -> Option<Mask> {
    match assets::icon(key)? {
        IconSrc::Svg(text) => svg_mask(text, size),
        IconSrc::Png(bytes) => png_mask(bytes, size),
    }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!(" {name}=\"");
    let start = tag.find(&pat)? + pat.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// Маска из SVG (Gravity UI): только `<path>` с заливкой, правила nonzero/evenodd.
fn svg_mask(text: &str, size: usize) -> Option<Mask> {
    let vb = attr(text, "viewBox")
        .and_then(|v| {
            v.split_whitespace()
                .nth(2)
                .and_then(|w| w.parse::<f32>().ok())
        })
        .unwrap_or(16.0);
    let k = size as f32 / vb;
    let mut pm = Pixmap::new(size as u32, size as u32)?;
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(0, 0, 0, 255);
    for part in text.split("<path").skip(1) {
        let tag = part.split('>').next().unwrap_or("");
        let Some(d) = attr(tag, "d") else { continue };
        let rule = if tag.contains("evenodd") {
            FillRule::EvenOdd
        } else {
            FillRule::Winding
        };
        if let Some(path) = svg_path(d) {
            if let Some(path) = path.transform(Transform::from_scale(k, k)) {
                pm.fill_path(&path, &paint, rule, Transform::identity(), None);
            }
        }
    }
    Some(Mask {
        n: size,
        data: pm.data().chunks_exact(4).map(|p| p[3]).collect(),
    })
}

fn svg_path(d: &str) -> Option<Path> {
    use svgtypes::{SimplePathSegment as S, SimplifyingPathParser};
    let mut pb = PathBuilder::new();
    for seg in SimplifyingPathParser::from(d) {
        match seg.ok()? {
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
    pb.finish()
}

/// Маска из альфа-канала PNG, билинейно увеличенная до `size`.
fn png_mask(bytes: &[u8], size: usize) -> Option<Mask> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let alpha: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf.chunks_exact(4).map(|p| p[3]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).map(|p| p[1]).collect(),
        _ => return None,
    };
    let at = |x: i32, y: i32| {
        alpha[y.clamp(0, h as i32 - 1) as usize * w + x.clamp(0, w as i32 - 1) as usize] as f32
    };
    let mut data = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let fx = (x as f32 + 0.5) * w as f32 / size as f32 - 0.5;
            let fy = (y as f32 + 0.5) * h as f32 / size as f32 - 0.5;
            let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
            let bot = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
            data.push((top * (1.0 - ty) + bot * ty).round() as u8);
        }
    }
    Some(Mask { n: size, data })
}

// ---------------------------------------------------------------- точка и плашки

fn dot(t: usize) -> Rgba {
    let n = DOT_BOX as usize * t;
    let c = n as f32 / 2.0;
    let r = (DOT_BOX / 2.0 - 2.0) * t as f32;
    let mut pm = Pixmap::new(n as u32, n as u32).expect("размер точки");
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(179, 204, 255, 255);
    if let Some(p) = PathBuilder::from_circle(c, c, r) {
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    paint.set_color_rgba8(76, 125, 255, 255);
    if let Some(p) = PathBuilder::from_circle(c, c, r - 0.8 * t as f32) {
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    Rgba {
        w: n,
        h: n,
        data: pm.take(),
    }
}

fn stadium(w: f32, h: f32) -> Option<Path> {
    const K: f32 = 0.552_284_8;
    let r = h / 2.0;
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    pb.line_to(w - r, 0.0);
    pb.cubic_to(w - r + K * r, 0.0, w, r - K * r, w, r);
    pb.cubic_to(w, r + K * r, w - r + K * r, h, w - r, h);
    pb.line_to(r, h);
    pb.cubic_to(r - K * r, h, 0.0, r + K * r, 0.0, r);
    pb.cubic_to(0.0, r - K * r, r - K * r, 0.0, r, 0.0);
    pb.close();
    pb.finish()
}

/// Плашка с подписью: тёмная полупрозрачная «таблетка», белый текст.
fn pill(font: &fontdue::Font, text: &str, px: f32, scale: f32) -> Rgba {
    let lm = font.horizontal_line_metrics(px);
    let (ascent, descent) = lm
        .map(|l| (l.ascent, l.descent))
        .unwrap_or((px * 0.95, -px * 0.25));
    let chars: Vec<char> = text.chars().collect();
    let mut x = 0.0f32;
    let mut glyphs = Vec::with_capacity(chars.len());
    for (i, &ch) in chars.iter().enumerate() {
        let (m, bmp) = font.rasterize(ch, px);
        glyphs.push((x + m.xmin as f32, m, bmp));
        x += m.advance_width;
        if let Some(&next) = chars.get(i + 1) {
            x += font.horizontal_kern(ch, next, px).unwrap_or(0.0);
        }
    }
    let (pad_x, pad_y) = (9.0 * scale, 5.0 * scale);
    let w = (x + 2.0 * pad_x).ceil().max(8.0) as usize;
    let h = (ascent - descent + 2.0 * pad_y).ceil().max(8.0) as usize;
    let mut pm = Pixmap::new(w as u32, h as u32).expect("размер плашки");
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    if let Some(p) = stadium(w as f32, h as f32) {
        paint.set_color_rgba8(16, 16, 16, 185);
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    if let Some(p) = stadium(w as f32 - 1.0, h as f32 - 1.0)
        .and_then(|p| p.transform(Transform::from_translate(0.5, 0.5)))
    {
        paint.set_color_rgba8(255, 255, 255, 70);
        pm.stroke_path(
            &p,
            &paint,
            &Stroke {
                width: 1.0,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
    let baseline = pad_y + ascent;
    let data = pm.data_mut();
    for (gx, m, bmp) in &glyphs {
        let left = (pad_x + gx).round() as i32;
        let top = (baseline - (m.ymin + m.height as i32) as f32).round() as i32;
        for row in 0..m.height {
            for col in 0..m.width {
                let cov = bmp[row * m.width + col] as u32;
                let (px_, py_) = (left + col as i32, top + row as i32);
                if cov == 0 || px_ < 0 || py_ < 0 || px_ as usize >= w || py_ as usize >= h {
                    continue;
                }
                let o = (py_ as usize * w + px_ as usize) * 4;
                for c in 0..4 {
                    // белый с покрытием cov поверх предумноженного фона
                    let d = data[o + c] as u32;
                    data[o + c] = (cov + d * (255 - cov) / 255).min(255) as u8;
                }
            }
        }
    }
    Rgba {
        w,
        h,
        data: pm.take(),
    }
}

/// Системный шрифт для подписей (жирный Segoe UI → Arial; на других ОС — DejaVu для отладки).
pub fn load_font() -> Option<fontdue::Font> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("RADIAL_FONT") {
        paths.push(p);
    }
    for p in [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    ] {
        paths.push(p.to_string());
    }
    paths.iter().find_map(|p| {
        let bytes = std::fs::read(p).ok()?;
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_tex_geometry() {
        let t = button_tex(2);
        let n = t.n;
        let at = |x: usize, y: usize| t.data[y * n + x];
        let c = at(n / 2, n / 2);
        assert_eq!(c[1], 255); // центр — круг
        assert_eq!(c[0], 0); // тени внутри круга нет
        assert_eq!(c[2], 0);
        // на краю круга — обводка
        let edge = at(n / 2 + (DIAM as usize / 2) * 2 - 1, n / 2);
        assert!(edge[2] > 100, "{edge:?}");
        // тень снаружи есть, вдали — нет
        let near = at(n / 2 + (DIAM as usize / 2) * 2 + 4, n / 2);
        assert!(near[0] > 10 && near[1] == 0, "{near:?}");
        assert_eq!(at(0, 0), [0, 0, 0]);
    }

    #[test]
    fn all_menu_icons_rasterise() {
        let menu = crate::menu::default_menu();
        for key in menu.icon_keys() {
            let m = icon_mask(key, 48).unwrap_or_else(|| panic!("нет иконки {key}"));
            let ink: usize = m.data.iter().filter(|&&v| v > 128).count();
            assert!(ink > 40, "{key}: пустая маска ({ink})");
        }
    }

    #[test]
    fn evenodd_scissors_has_holes() {
        // кольцо ножниц (evenodd) не должно быть залито целиком
        let m = icon_mask("scissors", 96).unwrap();
        let filled = m.data.iter().filter(|&&v| v > 128).count();
        assert!(filled < 96 * 96 / 2);
    }

    #[test]
    fn pills_render_when_font_present() {
        let Some(font) = load_font() else { return };
        let p = pill(&font, "Копировать", 13.0, 1.0);
        assert!(p.w > 60 && p.h > 14);
        assert!(p.data.chunks_exact(4).any(|px| px[0] > 200 && px[3] > 200)); // белый текст
    }
}
