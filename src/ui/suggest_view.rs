//! Плашка подсказок `[[`: скруглённая, с мягкой тенью. Рисуется без ОС (tiny-skia); окно,
//! в которое её кладёт Windows, — в `platform::win32::ui::suggest`.

use super::settings_form::Fonts;
use crate::render::painter::{blend, Painter};
use crate::render::palette::Ui;

pub const ROW: f32 = 34.0;
const PAD: f32 = 6.0;
pub const WIDTH: f32 = 330.0;
pub const MAX_ROWS: usize = 7;
/// Поля вокруг плашки под тень.
pub const SHADOW: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Размер всей картинки (плашка + тень), px.
    pub w: i32,
    pub h: i32,
    /// Плашка внутри неё: x, y, ширина, высота.
    pub panel: (f32, f32, f32, f32),
}

pub fn visible_rows(count: usize) -> usize {
    count.min(MAX_ROWS)
}

pub fn geometry(count: usize, s: f32) -> Geometry {
    let (pw, ph) = (WIDTH * s, (visible_rows(count) as f32 * ROW + 2.0 * PAD) * s);
    let m = SHADOW * s;
    Geometry {
        w: (pw + 2.0 * m).ceil() as i32,
        h: (ph + 2.0 * m).ceil() as i32,
        panel: (m, m * 0.8, pw, ph),
    }
}

/// Какая строка под точкой (`y` — в координатах картинки); `top` — первая показанная.
pub fn row_at(g: &Geometry, s: f32, y: f32, top: usize, count: usize) -> Option<usize> {
    let inside = y - g.panel.1 - PAD * s;
    if inside < 0.0 {
        return None;
    }
    let row = top + (inside / (ROW * s)) as usize;
    (row < count.min(top + MAX_ROWS)).then_some(row)
}

pub fn render(
    items: &[String],
    sel: usize,
    hover: Option<usize>,
    top: usize,
    s: f32,
    pal: &Ui,
    fonts: &Fonts,
) -> Option<Painter> {
    let g = geometry(items.len(), s);
    let mut p = Painter::new(g.w as u32, g.h as u32)?;
    let (x, y, w, h) = g.panel;
    let r = 14.0 * s;

    // тень: слои, чем дальше от края — тем прозрачнее
    let steps = (SHADOW * s) as i32;
    for i in (0..steps).rev() {
        let d = i as f32;
        p.fill_rrect(x - d, y - d + 3.0 * s, w + 2.0 * d, h + 2.0 * d, r + d, 0x000000, 0.012);
    }
    let body = blend(pal.bg2, pal.fg, 0.07);
    p.fill_rrect(x, y, w, h, r, body, 1.0);
    p.stroke_rrect(x, y, w, h, r, pal.edge, 0.35, 1.0_f32.max(s));

    let first = top.min(items.len().saturating_sub(1));
    let scroll = items.len() > MAX_ROWS;
    let right = if scroll { 12.0 * s } else { 0.0 };
    for (i, text) in items.iter().enumerate().skip(first).take(MAX_ROWS) {
        let ry = y + (PAD + (i - first) as f32 * ROW) * s;
        let (rx, rw, rh) = (x + PAD * s, w - 2.0 * PAD * s - right, ROW * s);
        let selected = i == sel;
        if selected {
            p.fill_rrect(rx, ry, rw, rh - 2.0 * s, 9.0 * s, pal.acc, 1.0);
        } else if hover == Some(i) {
            p.fill_rrect(rx, ry, rw, rh - 2.0 * s, 9.0 * s, pal.fg, 0.09);
        }
        let color = if selected { pal.on_acc } else { pal.fg };
        p.text_clipped(&fonts.regular, text, rx + 12.0 * s, ry + (rh - 2.0 * s) / 2.0, 14.0 * s, color, rw - 24.0 * s);
    }
    if scroll {
        // ползунок: где сейчас окно показа среди всех подсказок
        let track = h - 2.0 * PAD * s - 4.0 * s;
        let thumb = (track * MAX_ROWS as f32 / items.len() as f32).max(18.0 * s);
        let span = (items.len() - MAX_ROWS).max(1) as f32;
        let ty = y + PAD * s + 2.0 * s + (track - thumb) * first as f32 / span;
        p.fill_rrect(x + w - 9.0 * s, ty, 3.0 * s, thumb, 1.5 * s, pal.dim, 0.7);
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Theme;
    use crate::render::palette;

    #[test]
    fn rows_map_back_from_pixels() {
        let g = geometry(10, 1.0);
        let y0 = g.panel.1 + PAD;
        assert_eq!(row_at(&g, 1.0, y0 + 1.0, 0, 10), Some(0));
        assert_eq!(row_at(&g, 1.0, y0 + ROW * 2.0 + 5.0, 0, 10), Some(2));
        assert_eq!(row_at(&g, 1.0, y0 + ROW * 2.0 + 5.0, 3, 10), Some(5)); // прокручено
        assert_eq!(row_at(&g, 1.0, y0 - 2.0, 0, 10), None); // над списком
        assert_eq!(row_at(&geometry(2, 1.0), 1.0, y0 + ROW * 3.0, 0, 2), None); // ниже последней
        assert_eq!(visible_rows(100), MAX_ROWS);
    }

    #[test]
    fn renders_in_both_themes() {
        let Some(fonts) = Fonts::load() else { return };
        for t in [Theme::Dark, Theme::Light] {
            let items: Vec<String> = (0..12).map(|i| format!("Заметка {i}")).collect();
            let p = render(&items, 2, Some(4), 1, 1.5, &palette::ui(t), &fonts).unwrap();
            let g = geometry(items.len(), 1.5);
            assert_eq!((p.width() as i32, p.height() as i32), (g.w, g.h));
        }
    }
}
