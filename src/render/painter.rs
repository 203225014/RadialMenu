//! Р РёСЃРѕРІР°РЅРёРµ РѕРєРѕРЅ (РЅР°СЃС‚СЂРѕР№РєРё): СЃРіР»Р°Р¶РµРЅРЅС‹Рµ С„РёРіСѓСЂС‹ Рё С‚РµРєСЃС‚ РІ Р±СѓС„РµСЂ. РџР»Р°С‚С„РѕСЂРјР° С‚РѕР»СЊРєРѕ
//! РєРѕРїРёСЂСѓРµС‚ РіРѕС‚РѕРІС‹Р№ Р±СѓС„РµСЂ РЅР° СЌРєСЂР°РЅ, РїРѕСЌС‚РѕРјСѓ РѕРєРЅРѕ РІС‹РіР»СЏРґРёС‚ РѕРґРёРЅР°РєРѕРІРѕ Рё РїСЂРѕРІРµСЂСЏРµС‚СЃСЏ Р±РµР· РћРЎ.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Р¦РІРµС‚ 0xRRGGBB Рё РїСЂРѕР·СЂР°С‡РЅРѕСЃС‚СЊ 0..1.
fn color(rgb: u32, a: f32) -> Color {
    Color::from_rgba8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn paint(rgb: u32, a: f32) -> Paint<'static> {
    let mut p = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    p.set_color(color(rgb, a));
    p
}

/// РЎРјРµС€Р°С‚СЊ РґРІР° С†РІРµС‚Р° 0xRRGGBB: `t` = 0 в†’ `a`, 1 в†’ `b`.
pub fn blend(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

pub struct Painter {
    pm: Pixmap,
}

/// Р’С‹СЂР°РІРЅРёРІР°РЅРёРµ С‚РµРєСЃС‚Р° РїРѕ РіРѕСЂРёР·РѕРЅС‚Р°Р»Рё РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Painter {
    pub fn new(w: u32, h: u32) -> Option<Painter> {
        Some(Painter {
            pm: Pixmap::new(w.max(1), h.max(1))?,
        })
    }

    pub fn width(&self) -> u32 {
        self.pm.width()
    }

    pub fn height(&self) -> u32 {
        self.pm.height()
    }

    pub fn clear(&mut self, rgb: u32) {
        self.pm.fill(color(rgb, 1.0));
    }

    fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        if r <= 0.0 {
            return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?));
        }
        const K: f32 = 0.552_284_8;
        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.cubic_to(x + w - r + K * r, y, x + w, y + r - K * r, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.cubic_to(x + w, y + h - r + K * r, x + w - r + K * r, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.cubic_to(x + r - K * r, y + h, x, y + h - r + K * r, x, y + h - r);
        pb.line_to(x, y + r);
        pb.cubic_to(x, y + r - K * r, x + r - K * r, y, x + r, y);
        pb.close();
        pb.finish()
    }

    /// РЎРєСЂСѓРіР»С‘РЅРЅС‹Р№ РїСЂСЏРјРѕСѓРіРѕР»СЊРЅРёРє СЃ Р·Р°Р»РёРІРєРѕР№.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, rgb: u32, a: f32) {
        if let Some(p) = Self::rrect_path(x, y, w, h, r) {
            self.pm
                .fill_path(&p, &paint(rgb, a), FillRule::Winding, Transform::identity(), None);
        }
    }

    /// РЎРєСЂСѓРіР»С‘РЅРЅР°СЏ СЂР°РјРєР° С‚РѕР»С‰РёРЅРѕР№ `width` (РІРЅСѓС‚СЂРё РїСЂСЏРјРѕСѓРіРѕР»СЊРЅРёРєР°).
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, rgb: u32, a: f32, width: f32) {
        let half = width / 2.0;
        if let Some(p) = Self::rrect_path(x + half, y + half, w - width, h - width, (r - half).max(0.0)) {
            let stroke = Stroke {
                width,
                ..Stroke::default()
            };
            self.pm
                .stroke_path(&p, &paint(rgb, a), &stroke, Transform::identity(), None);
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, rgb: u32, a: f32) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.pm
                .fill_path(&p, &paint(rgb, a), FillRule::Winding, Transform::identity(), None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, rgb: u32, a: f32, width: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(p) = pb.finish() {
            let stroke = Stroke {
                width,
                line_cap: tiny_skia::LineCap::Round,
                ..Stroke::default()
            };
            self.pm
                .stroke_path(&p, &paint(rgb, a), &stroke, Transform::identity(), None);
        }
    }

    /// РЁРёСЂРёРЅР° СЃС‚СЂРѕРєРё РІ px.
    pub fn measure(font: &fontdue::Font, text: &str, px: f32) -> f32 {
        let chars: Vec<char> = text.chars().collect();
        let mut x = 0.0;
        for (i, &ch) in chars.iter().enumerate() {
            x += font.metrics(ch, px).advance_width;
            if let Some(&next) = chars.get(i + 1) {
                x += font.horizontal_kern(ch, next, px).unwrap_or(0.0);
            }
        }
        x
    }

    /// РЎС‚СЂРѕРєР° С‚РµРєСЃС‚Р°; `cy` вЂ” СЃРµСЂРµРґРёРЅР° СЃС‚СЂРѕРєРё РїРѕ РІРµСЂС‚РёРєР°Р»Рё. Р’РѕР·РІСЂР°С‰Р°РµС‚ С€РёСЂРёРЅСѓ.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        font: &fontdue::Font,
        text: &str,
        x: f32,
        cy: f32,
        px: f32,
        rgb: u32,
        align: Align,
    ) -> f32 {
        let width = Self::measure(font, text, px);
        let left = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        let (ascent, descent) = font
            .horizontal_line_metrics(px)
            .map(|l| (l.ascent, l.descent))
            .unwrap_or((px * 0.95, -px * 0.25));
        let baseline = cy + (ascent + descent) / 2.0;
        let (w, h) = (self.pm.width() as i32, self.pm.height() as i32);
        let (r, g, b) = (((rgb >> 16) & 0xFF), ((rgb >> 8) & 0xFF), (rgb & 0xFF));
        let chars: Vec<char> = text.chars().collect();
        let mut pen = left;
        let data = self.pm.data_mut();
        for (i, &ch) in chars.iter().enumerate() {
            let (m, bmp) = font.rasterize(ch, px);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - (m.ymin + m.height as i32) as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let cov = bmp[row * m.width + col] as u32;
                    let (px_, py_) = (gx + col as i32, gy + row as i32);
                    if cov == 0 || px_ < 0 || py_ < 0 || px_ >= w || py_ >= h {
                        continue;
                    }
                    let o = (py_ * w + px_) as usize * 4;
                    let d = &mut data[o..o + 4];
                    let inv = 255 - cov;
                    d[0] = ((r * cov + d[0] as u32 * inv) / 255) as u8;
                    d[1] = ((g * cov + d[1] as u32 * inv) / 255) as u8;
                    d[2] = ((b * cov + d[2] as u32 * inv) / 255) as u8;
                    d[3] = (cov + d[3] as u32 * inv / 255).min(255) as u8;
                }
            }
            pen += m.advance_width;
            if let Some(&next) = chars.get(i + 1) {
                pen += font.horizontal_kern(ch, next, px).unwrap_or(0.0);
            }
        }
        width
    }

    /// РЎС‚СЂРѕРєР°, РѕР±СЂРµР·Р°РЅРЅР°СЏ РјРЅРѕРіРѕС‚РѕС‡РёРµРј РїРѕ С€РёСЂРёРЅРµ `max_w`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_clipped(
        &mut self,
        font: &fontdue::Font,
        text: &str,
        x: f32,
        cy: f32,
        px: f32,
        rgb: u32,
        max_w: f32,
    ) {
        if Self::measure(font, text, px) <= max_w {
            self.text(font, text, x, cy, px, rgb, Align::Left);
            return;
        }
        let chars: Vec<char> = text.chars().collect();
        // СЂРµР¶РµРј СЃРµСЂРµРґРёРЅСѓ: Сѓ РїСѓС‚РµР№ РІР°Р¶РЅС‹ Рё РЅР°С‡Р°Р»Рѕ (РґРёСЃРє), Рё РєРѕРЅРµС† (РїР°РїРєР°)
        let mut keep = chars.len();
        while keep > 2 {
            keep -= 1;
            let head: String = chars[..keep / 2].iter().collect();
            let tail: String = chars[chars.len() - (keep - keep / 2)..].iter().collect();
            let s = format!("{head}вЂ¦{tail}");
            if Self::measure(font, &s, px) <= max_w {
                self.text(font, &s, x, cy, px, rgb, Align::Left);
                return;
            }
        }
        self.text(font, "вЂ¦", x, cy, px, rgb, Align::Left);
    }

    /// РџРёРєСЃРµР»Рё RGBA (РїСЂРµРґСѓРјРЅРѕР¶РµРЅРЅС‹Рµ) вЂ” РґР»СЏ СЃРѕС…СЂР°РЅРµРЅРёСЏ РІ PNG.
    pub fn rgba(&self) -> &[u8] {
        self.pm.data()
    }

    /// РЎРєРѕРїРёСЂРѕРІР°С‚СЊ РІ Р±СѓС„РµСЂ BGRA (С„РѕСЂРјР°С‚ DIB Windows), СЃС‚СЂРѕРєРё СЃРІРµСЂС…Сѓ РІРЅРёР·.
    pub fn to_bgra(&self, out: &mut [u8]) {
        for (d, s) in out.chunks_exact_mut(4).zip(self.pm.data().chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(p: &Painter, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * p.width() + x) * 4) as usize;
        p.rgba()[o..o + 4].try_into().unwrap()
    }

    #[test]
    fn shapes_land_where_asked() {
        let mut p = Painter::new(100, 60).unwrap();
        p.clear(0x000000);
        p.fill_rrect(10.0, 10.0, 40.0, 20.0, 6.0, 0xFF0000, 1.0);
        assert_eq!(px(&p, 30, 20), [255, 0, 0, 255]);
        assert_eq!(px(&p, 10, 10)[0], 0); // СЃРєСЂСѓРіР»С‘РЅРЅС‹Р№ СѓРіРѕР» РЅРµ Р·Р°Р»РёС‚
        p.fill_circle(80.0, 30.0, 10.0, 0x00FF00, 1.0);
        assert_eq!(px(&p, 80, 30), [0, 255, 0, 255]);
        let mut bgra = vec![0u8; 100 * 60 * 4];
        p.to_bgra(&mut bgra);
        let o = (20 * 100 + 30) * 4;
        assert_eq!(&bgra[o..o + 4], &[0, 0, 255, 255]);
    }

    #[test]
    fn blend_mixes_channels() {
        assert_eq!(blend(0x000000, 0xFFFFFF, 0.0), 0x000000);
        assert_eq!(blend(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(blend(0x000000, 0x2040FF, 0.5), 0x0010_2080);
    }

    #[test]
    fn text_draws_and_measures() {
        let Some(font) = crate::render::raster::load_font_regular() else { return };
        let mut p = Painter::new(200, 40).unwrap();
        p.clear(0x000000);
        let w = p.text(&font, "РќР°СЃС‚СЂРѕР№РєРё", 10.0, 20.0, 16.0, 0xFFFFFF, Align::Left);
        assert!(w > 50.0 && (w - Painter::measure(&font, "РќР°СЃС‚СЂРѕР№РєРё", 16.0)).abs() < 1e-3);
        assert!(p.rgba().chunks_exact(4).any(|c| c[0] > 200));
        // РѕР±СЂРµР·РєР°: РґР»РёРЅРЅС‹Р№ РїСѓС‚СЊ РЅРµ РІС‹Р»РµР·Р°РµС‚ Р·Р° РіСЂР°РЅРёС†Сѓ
        let mut q = Painter::new(120, 30).unwrap();
        q.clear(0x000000);
        q.text_clipped(&font, "C:/РѕС‡РµРЅСЊ/РґР»РёРЅРЅС‹Р№/РїСѓС‚СЊ/Рє/С…СЂР°РЅРёР»РёС‰Сѓ/Р·Р°РјРµС‚РѕРє", 0.0, 15.0, 13.0, 0xFFFFFF, 80.0);
        for y in 0..30 {
            for x in 82..120 {
                assert_eq!(px(&q, x, y)[0], 0, "({x},{y})");
            }
        }
    }
}
