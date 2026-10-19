//! Предварительная отрисовка: «стеклянная» кнопка, иконки, точка-указатель, плашки с подписями.
//! Всё строится один раз при запуске; во время работы кадр собирается из готовых масок
//! (см. `compose`), поэтому в простое меню не тратит ни процессор, ни память на графику.

use std::collections::HashMap;
use std::path::Path as FsPath;

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use super::assets;
use super::palette::{self, PillColors};
use crate::model::{IconRef, Theme};

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

/// Иконка кнопки: маска (красится под тему и состояние) или картинка в своих цветах.
pub enum Icon {
    Mask(Mask),
    Color(Rgba),
}

/// Как достать иконку программы (`IconRef::Program`): путь и сторона в px → квадрат RGBA
/// с предумноженной альфой. Это умеет только платформа; без неё — `no_program_icons`.
pub type ProgramIcons<'a> = &'a dyn Fn(&FsPath, u32) -> Option<Rgba>;

pub fn no_program_icons(_: &FsPath, _: u32) -> Option<Rgba> {
    None
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
    pub icons: HashMap<IconRef, Icon>,
    pub dot: Rgba,
    pills: HashMap<(String, PillKind, Theme), Rgba>,
}

impl Bake {
    pub fn pill(&self, text: &str, kind: PillKind, theme: Theme) -> Option<&Rgba> {
        self.pills.get(&(text.to_string(), kind, theme))
    }

    pub fn icon(&self, r: &IconRef) -> Option<&Icon> {
        self.icons.get(r)
    }

    /// Сторона иконки в px текстуры.
    pub fn icon_size(&self) -> usize {
        (ICON_BOX as usize) * self.t
    }

    /// `scale` — общий масштаб интерфейса; от него зависит детальность текстур.
    /// Плашки рисуются сразу для обеих тем: смена темы не требует шрифта.
    /// Иконки, которые не удалось загрузить, просто отсутствуют (см. `icon`).
    pub fn build(
        icons: &[IconRef],
        texts: &[(String, PillKind)],
        scale: f32,
        font: Option<&fontdue::Font>,
        programs: ProgramIcons,
    ) -> Bake {
        let t = ((2.0 * scale).ceil() as usize).clamp(2, 6);
        let size = (ICON_BOX as usize) * t;
        let icons = icons
            .iter()
            .filter_map(|r| icon(r, size, font, programs).map(|i| (r.clone(), i)))
            .collect();
        let mut pills = HashMap::new();
        if let Some(font) = font {
            for theme in Theme::ALL {
                let colors = palette::pill(theme);
                for (text, kind) in texts {
                    let px = match kind {
                        PillKind::Label => 13.0,
                        PillKind::Toast => 14.0,
                    } * scale;
                    pills.insert(
                        (text.clone(), *kind, theme),
                        pill(font, text, px, scale, &colors),
                    );
                }
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

    /// Дорисовать иконки, которых ещё нет (например, буквы вместо не найденных программ).
    pub fn add_icons(&mut self, icons: &[IconRef], font: Option<&fontdue::Font>) {
        let size = self.icon_size();
        for r in icons {
            if !self.icons.contains_key(r) {
                if let Some(i) = icon(r, size, font, &no_program_icons) {
                    self.icons.insert(r.clone(), i);
                }
            }
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

/// Иконка нужного размера (`size` — сторона в px текстуры).
pub fn icon(r: &IconRef, size: usize, font: Option<&fontdue::Font>, programs: ProgramIcons) -> Option<Icon> {
    match r {
        IconRef::None => None,
        IconRef::Builtin(name) => svg(assets::icon(name)?.as_bytes(), size, false),
        IconRef::File { path, color } => {
            let bytes = std::fs::read(path).ok()?;
            let is_svg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
            if is_svg {
                svg(&bytes, size, *color)
            } else {
                png(&bytes, size, *color)
            }
        }
        IconRef::Program(path) => programs(path, size as u32).map(|img| Icon::Color(fit(img, size))),
        IconRef::Letter(ch) => letter(font?, *ch, size).map(Icon::Mask),
    }
}

/// Маска из альфы картинки (одноцветная иконка красится при композитинге).
fn alpha_of(img: &Rgba) -> Mask {
    Mask {
        n: img.w,
        data: img.data.chunks_exact(4).map(|p| p[3]).collect(),
    }
}

/// SVG любой сложности (resvg): вписывается в квадрат с сохранением пропорций.
fn svg(bytes: &[u8], size: usize, color: bool) -> Option<Icon> {
    use resvg::{tiny_skia as ts, usvg};
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let mut pm = ts::Pixmap::new(size as u32, size as u32)?;
    let k = size as f32 / w.max(h);
    let (dx, dy) = ((size as f32 - w * k) / 2.0, (size as f32 - h * k) / 2.0);
    resvg::render(&tree, ts::Transform::from_row(k, 0.0, 0.0, k, dx, dy), &mut pm.as_mut());
    let img = Rgba {
        w: size,
        h: size,
        data: pm.take(),
    };
    Some(if color { Icon::Color(img) } else { Icon::Mask(alpha_of(&img)) })
}

/// PNG любого вида (RGB, RGBA, серый, палитра): в квадрат `size` с сохранением пропорций.
fn png(bytes: &[u8], size: usize, color: bool) -> Option<Icon> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px: Vec<[u8; 4]> = match info.color_type {
        png::ColorType::Rgba => buf.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect(),
        png::ColorType::Rgb => buf.chunks_exact(3).map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None, // EXPAND превращает палитру в RGB(A)
    };
    // к предумноженной альфе
    let data: Vec<u8> = px
        .iter()
        .take(w * h)
        .flat_map(|&[r, g, b, a]| {
            let m = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
            [m(r), m(g), m(b), a]
        })
        .collect();
    let img = fit(Rgba { w, h, data }, size);
    Some(if color { Icon::Color(img) } else { Icon::Mask(alpha_of(&img)) })
}

/// Вписать картинку в квадрат `size` (билинейно, по центру, с сохранением пропорций).
pub fn fit(img: Rgba, size: usize) -> Rgba {
    if img.w == size && img.h == size {
        return img;
    }
    let k = size as f32 / img.w.max(img.h).max(1) as f32;
    let (sw, sh) = (img.w as f32 * k, img.h as f32 * k);
    let (ox, oy) = ((size as f32 - sw) / 2.0, (size as f32 - sh) / 2.0);
    let at = |x: i32, y: i32, c: usize| -> f32 {
        // за краем — ближайший пиксель края (иначе края картинки темнеют)
        let x = x.clamp(0, img.w as i32 - 1) as usize;
        let y = y.clamp(0, img.h as i32 - 1) as usize;
        img.data[(y * img.w + x) * 4 + c] as f32
    };
    let mut data = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            if cx < ox || cy < oy || cx > ox + sw || cy > oy + sh {
                continue; // поля вокруг вписанной картинки — прозрачные
            }
            let fx = (cx - ox) / k - 0.5;
            let fy = (cy - oy) / k - 0.5;
            let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x0 + 1, y0, c) * tx;
                let bot = at(x0, y0 + 1, c) * (1.0 - tx) + at(x0 + 1, y0 + 1, c) * tx;
                data[(y * size + x) * 4 + c] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
    Rgba {
        w: size,
        h: size,
        data,
    }
}

/// Буква вместо иконки: для кнопок без своей картинки.
fn letter(font: &fontdue::Font, ch: char, size: usize) -> Option<Mask> {
    let px = size as f32 * 0.8;
    let (m, bmp) = font.rasterize(ch, px);
    if m.width == 0 || m.height == 0 {
        return None;
    }
    let mut data = vec![0u8; size * size];
    // по центру квадрата — по габаритам самого знака
    let ox = (size as i32 - m.width as i32) / 2;
    let oy = (size as i32 - m.height as i32) / 2;
    for row in 0..m.height {
        for col in 0..m.width {
            let (x, y) = (ox + col as i32, oy + row as i32);
            if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
                data[y as usize * size + x as usize] = bmp[row * m.width + col];
            }
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
    rrect(w, h, h / 2.0)
}

/// Прямоугольник со скруглёнными углами радиуса `r` (не больше половины меньшей стороны).
fn rrect(w: f32, h: f32, r: f32) -> Option<Path> {
    const K: f32 = 0.552_284_8;
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    pb.line_to(w - r, 0.0);
    pb.cubic_to(w - r + K * r, 0.0, w, r - K * r, w, r);
    pb.line_to(w, h - r);
    pb.cubic_to(w, h - r + K * r, w - r + K * r, h, w - r, h);
    pb.line_to(r, h);
    pb.cubic_to(r - K * r, h, 0.0, h - r + K * r, 0.0, h - r);
    pb.line_to(0.0, r);
    pb.cubic_to(0.0, r - K * r, r - K * r, 0.0, r, 0.0);
    pb.close();
    pb.finish()
}

/// Плашка с подписью: полупрозрачная «таблетка» с текстом, цвета — по теме.
fn pill(font: &fontdue::Font, text: &str, px: f32, scale: f32, colors: &PillColors) -> Rgba {
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
        let [r, g, b, a] = colors.bg;
        paint.set_color_rgba8(r, g, b, a);
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    if let Some(p) = stadium(w as f32 - 1.0, h as f32 - 1.0)
        .and_then(|p| p.transform(Transform::from_translate(0.5, 0.5)))
    {
        let [r, g, b, a] = colors.border;
        paint.set_color_rgba8(r, g, b, a);
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
                // цвет текста с покрытием cov поверх предумноженного фона (RGBA)
                let t = colors.text;
                let src = [t[0] as u32, t[1] as u32, t[2] as u32, 255];
                for c in 0..4 {
                    let d = data[o + c] as u32;
                    data[o + c] = (src[c] * cov / 255 + d * (255 - cov) / 255).min(255) as u8;
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

/// Что сейчас играет.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub paused: bool,
}

/// Размеры карточки трека (при масштабе 1): ширина постоянная, чтобы карточка не «дышала» от названия.
pub const CARD_W: f32 = 264.0;
pub const CARD_H: f32 = 54.0;
const CARD_ICON: f32 = 36.0;
const CARD_PAD: f32 = 9.0;
/// Поля справа и слева от текста: значок с отступами слева, отступ справа.
pub const CARD_PAD_L: f32 = CARD_PAD + CARD_ICON + 12.0;
pub const CARD_PAD_R: f32 = 16.0;

/// Положить текст в пиксели (предумноженный RGBA): `cy` — середина строки по вертикали.
#[allow(clippy::too_many_arguments)]
fn blit_text(data: &mut [u8], w: usize, h: usize, font: &fontdue::Font, text: &str, px: f32, x: f32, cy: f32, color: u32) {
    let lm = font.horizontal_line_metrics(px);
    let (ascent, descent) = lm.map(|l| (l.ascent, l.descent)).unwrap_or((px * 0.95, -px * 0.25));
    let baseline = cy - (ascent - descent) / 2.0 + ascent;
    let src = [(color >> 16) & 255, (color >> 8) & 255, color & 255, 255];
    let chars: Vec<char> = text.chars().collect();
    let mut pen = x;
    for (i, &ch) in chars.iter().enumerate() {
        let (m, bmp) = font.rasterize(ch, px);
        let left = (pen + m.xmin as f32).round() as i32;
        let top = (baseline - (m.ymin + m.height as i32) as f32).round() as i32;
        for row in 0..m.height {
            for col in 0..m.width {
                let cov = bmp[row * m.width + col] as u32;
                let (gx, gy) = (left + col as i32, top + row as i32);
                if cov == 0 || gx < 0 || gy < 0 || gx as usize >= w || gy as usize >= h {
                    continue;
                }
                let o = (gy as usize * w + gx as usize) * 4;
                for c in 0..4 {
                    let d = data[o + c] as u32;
                    data[o + c] = (src[c] * cov / 255 + d * (255 - cov) / 255).min(255) as u8;
                }
            }
        }
        pen += m.advance_width;
        if let Some(&next) = chars.get(i + 1) {
            pen += font.horizontal_kern(ch, next, px).unwrap_or(0.0);
        }
    }
}

/// Карточка «что играет»: значок в цвете акцента, название и исполнитель. Строки приходят уже
/// подогнанными по ширине (`Engine::set_track`).
pub fn track_card(font: &fontdue::Font, t: &Track, theme: Theme, scale: f32) -> Rgba {
    let (w, h) = ((CARD_W * scale).round() as usize, (CARD_H * scale).round() as usize);
    let (ui, pc) = (palette::ui(theme), palette::pill(theme));
    let mut pm = Pixmap::new(w as u32, h as u32).expect("размер карточки");
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    let rgb = |c: u32| ((c >> 16) as u8, (c >> 8) as u8, c as u8);

    if let Some(p) = rrect(w as f32, h as f32, 18.0 * scale) {
        let [r, g, b, a] = pc.bg;
        paint.set_color_rgba8(r, g, b, a);
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    if let Some(p) = rrect(w as f32 - 1.0, h as f32 - 1.0, 18.0 * scale - 0.5)
        .and_then(|p| p.transform(Transform::from_translate(0.5, 0.5)))
    {
        let [r, g, b, a] = pc.border;
        paint.set_color_rgba8(r, g, b, a);
        pm.stroke_path(&p, &paint, &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
    }

    // значок: плитка акцента, на ней — «эквалайзер» или «пауза»
    let side = CARD_ICON * scale;
    let (ix, iy) = (CARD_PAD * scale, (h as f32 - side) / 2.0);
    if let Some(p) = rrect(side, side, 12.0 * scale).and_then(|p| p.transform(Transform::from_translate(ix, iy))) {
        let (r, g, b) = rgb(ui.acc);
        paint.set_color_rgba8(r, g, b, 255);
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
    let (r, g, b) = rgb(ui.on_acc);
    paint.set_color_rgba8(r, g, b, 255);
    let k = 16.0 * scale / 14.0; // рисунок задан в клетке 14×14
    let (ox, oy) = (ix + (side - 16.0 * scale) / 2.0, iy + (side - 16.0 * scale) / 2.0);
    let bars: &[(f32, f32, f32, f32)] = if t.paused {
        &[(3.0, 2.0, 3.0, 10.0), (8.0, 2.0, 3.0, 10.0)]
    } else {
        &[(1.0, 5.0, 3.0, 8.0), (5.5, 1.0, 3.0, 12.0), (10.0, 7.0, 3.0, 6.0)]
    };
    for &(bx, by, bw, bh) in bars {
        if let Some(p) = rrect(bw * k, bh * k, 1.5 * k).and_then(|p| p.transform(Transform::from_translate(ox + bx * k, oy + by * k))) {
            pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    let tx = ix + side + 12.0 * scale;
    let data = pm.data_mut();
    blit_text(data, w, h, font, &t.title, 13.5 * scale, tx, h as f32 / 2.0 - 8.0 * scale, ui.fg);
    blit_text(data, w, h, font, &t.artist, 12.0 * scale, tx, h as f32 / 2.0 + 9.0 * scale, ui.dim);
    Rgba { w, h, data: pm.take() }
}

/// Системный шрифт для подписей (жирный Segoe UI → Arial; на других ОС — DejaVu для отладки).
pub fn load_font() -> Option<fontdue::Font> {
    find_font(
        "RADIAL_FONT",
        &[
            r"C:\Windows\Fonts\segoeuib.ttf",
            r"C:\Windows\Fonts\arialbd.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        ],
    )
}

/// Обычный (не жирный) шрифт для текста окон.
pub fn load_font_regular() -> Option<fontdue::Font> {
    find_font(
        "RADIAL_FONT_REGULAR",
        &[
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ],
    )
}

fn find_font(env: &str, candidates: &[&str]) -> Option<fontdue::Font> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var(env) {
        paths.push(p);
    }
    paths.extend(candidates.iter().map(|p| p.to_string()));
    paths.iter().find_map(|p| {
        let bytes = std::fs::read(p).ok()?;
        let settings = fontdue::FontSettings { keep: Some(wanted_char), load_substitutions: false, ..Default::default() };
        fontdue::Font::from_bytes(bytes, settings).ok()
    })
}

/// Какие символы грузить из шрифта: латиница, греческий, кириллица и знаки. Весь Segoe UI — это
/// десятки тысяч глифов и ~20 МБ памяти на шрифт; для подписей и заметок хватает этого.
fn wanted_char(c: u32) -> bool {
    matches!(c, 0x20..=0x24F | 0x370..=0x52F | 0x2000..=0x20CF | 0x2190..=0x23FF | 0x25A0..=0x26FF | 0xFFFD)
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

    fn mask(r: &IconRef, size: usize) -> Mask {
        match icon(r, size, None, &no_program_icons) {
            Some(Icon::Mask(m)) => m,
            _ => panic!("нет маски {r:?}"),
        }
    }

    #[test]
    fn all_menu_icons_rasterise() {
        let menu = crate::model::default_menu();
        for r in menu.icons() {
            let m = mask(&r, 48);
            let ink: usize = m.data.iter().filter(|&&v| v > 128).count();
            assert!(ink > 40, "{r:?}: пустая маска ({ink})");
        }
    }

    #[test]
    fn evenodd_scissors_has_holes() {
        // кольцо ножниц (evenodd) не должно быть залито целиком
        let m = mask(&IconRef::builtin("scissors"), 96);
        let filled = m.data.iter().filter(|&&v| v > 128).count();
        assert!(filled > 200 && filled < 96 * 96 / 2);
    }

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("rm_icon_{}_{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn user_svg_with_shapes_and_colour() {
        // круг и прямоугольник без <path> — прежний разбор такое не умел
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10">
            <circle cx="5" cy="5" r="4" fill="#ff0000"/><rect x="11" y="1" width="8" height="8" fill="#0000ff"/></svg>"##;
        let p = tmp("a.svg", svg);
        let colour = IconRef::File { path: p.clone(), color: true };
        let Some(Icon::Color(img)) = icon(&colour, 40, None, &no_program_icons) else {
            panic!("цветная svg")
        };
        let at = |x: usize, y: usize| img.data[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].to_vec();
        // 20×10 вписывается в 40×40 по центру: строка y=20 — середина
        assert!(at(10, 20)[0] > 200 && at(10, 20)[2] < 30, "красный круг слева");
        assert!(at(30, 20)[2] > 200, "синий квадрат справа");
        assert_eq!(at(20, 2)[3], 0, "сверху пусто (пропорции сохранены)");
        let mono = IconRef::File { path: p.clone(), color: false };
        let Some(Icon::Mask(m)) = icon(&mono, 40, None, &no_program_icons) else {
            panic!("маска")
        };
        assert!(m.data[20 * 40 + 10] > 200);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn user_png_rgb_and_missing_files() {
        // PNG 2×1 без альфы: красный и зелёный
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 2, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[255, 0, 0, 0, 255, 0])
                .unwrap();
        }
        let p = tmp("b.png", &bytes);
        let r = IconRef::File { path: p.clone(), color: true };
        let Some(Icon::Color(img)) = icon(&r, 8, None, &no_program_icons) else {
            panic!()
        };
        assert_eq!((img.w, img.h), (8, 8));
        // 2×1 → 8×4 по центру: строки 2..6
        let px = |x: usize, y: usize| img.data[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].to_vec();
        assert!(px(0, 4)[0] > 200 && px(7, 4)[1] > 200, "{:?} {:?}", px(0, 4), px(7, 4));
        assert_eq!(px(4, 0)[3], 0);
        let _ = std::fs::remove_file(p);
        let missing = IconRef::File { path: "Z:/нет/такого.png".into(), color: true };
        assert!(icon(&missing, 8, None, &no_program_icons).is_none());
    }

    #[test]
    fn program_icons_come_from_platform_and_letters_from_font() {
        let red = |_: &FsPath, n: u32| {
            Some(Rgba {
                w: n as usize,
                h: n as usize,
                data: [255, 0, 0, 255].repeat((n * n) as usize),
            })
        };
        let r = IconRef::Program("app.exe".into());
        assert!(matches!(icon(&r, 16, None, &red), Some(Icon::Color(_))));
        assert!(icon(&r, 16, None, &no_program_icons).is_none());
        let Some(font) = load_font() else { return };
        let Some(Icon::Mask(m)) = icon(&IconRef::Letter('Ж'), 48, Some(&font), &no_program_icons) else {
            panic!("буква")
        };
        assert!(m.data.iter().filter(|&&v| v > 128).count() > 100);
    }

    #[test]
    fn pills_render_when_font_present() {
        let Some(font) = load_font() else { return };
        let p = pill(&font, "Копировать", 13.0, 1.0, &palette::pill(Theme::Dark));
        assert!(p.w > 60 && p.h > 14);
        assert!(p.data.chunks_exact(4).any(|px| px[0] > 200 && px[3] > 200)); // белый текст
        let l = pill(&font, "Копировать", 13.0, 1.0, &palette::pill(Theme::Light));
        // светлая: фон светлый, текст тёмный
        assert!(l.data.chunks_exact(4).any(|px| px[3] > 200 && px[0] < 80));
        assert!(l.data.chunks_exact(4).any(|px| px[3] > 200 && px[0] > 200));
    }

    #[test]
    fn track_card_is_fixed_size_and_themed() {
        let Some(font) = load_font() else { return };
        let t = Track { title: "Portorose in July".into(), artist: "Gramatik".into(), paused: false };
        let (d, l) = (track_card(&font, &t, Theme::Dark, 1.0), track_card(&font, &t, Theme::Light, 1.0));
        assert_eq!((d.w, d.h), (264, 54));
        assert_eq!((l.w, l.h), (264, 54));
        assert_ne!(d.data, l.data);
        // на паузе рисунок значка другой, размер тот же
        let p = track_card(&font, &Track { paused: true, ..t.clone() }, Theme::Dark, 1.0);
        assert_eq!((p.w, p.h), (264, 54));
        assert_ne!(p.data, d.data);
        let big = track_card(&font, &t, Theme::Dark, 1.5);
        assert_eq!((big.w, big.h), (396, 81));
    }
}
