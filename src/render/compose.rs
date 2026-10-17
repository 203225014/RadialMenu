//! Программный композитор: собирает кадр в буфер BGRA с предумноженной альфой
//! (именно такой формат ждёт UpdateLayeredWindow), без промежуточных картинок.

use super::palette::Look;
use super::raster::{ButtonTex, Icon, Rgba, BOX, ICON_BOX};

/// Прямоугольник в пикселях, правая/нижняя границы не включены.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn union(self, o: Rect) -> Rect {
        if self.is_empty() {
            o
        } else if o.is_empty() {
            self
        } else {
            Rect {
                x0: self.x0.min(o.x0),
                y0: self.y0.min(o.y0),
                x1: self.x1.max(o.x1),
                y1: self.y1.max(o.y1),
            }
        }
    }

    pub fn clip(self, w: usize, h: usize) -> Rect {
        Rect {
            x0: self.x0.max(0),
            y0: self.y0.max(0),
            x1: self.x1.min(w as i32),
            y1: self.y1.min(h as i32),
        }
    }
}

pub struct Canvas<'a> {
    pub w: usize,
    pub h: usize,
    /// BGRA, предумноженная альфа, `w*h*4` байт.
    pub px: &'a mut [u8],
}

impl<'a> Canvas<'a> {
    pub fn new(w: usize, h: usize, px: &'a mut [u8]) -> Canvas<'a> {
        assert!(px.len() >= w * h * 4, "буфер меньше холста");
        Canvas { w, h, px }
    }

    pub fn bounds(&self) -> Rect {
        Rect {
            x0: 0,
            y0: 0,
            x1: self.w as i32,
            y1: self.h as i32,
        }
    }

    pub fn clear(&mut self, r: Rect) {
        let r = r.clip(self.w, self.h);
        if r.is_empty() {
            return;
        }
        for y in r.y0..r.y1 {
            let o = (y as usize * self.w + r.x0 as usize) * 4;
            self.px[o..o + (r.x1 - r.x0) as usize * 4].fill(0);
        }
    }

    /// Положить пиксель поверх: `col` — предумноженный RGB (0..1), `a` — альфа (0..1).
    #[inline]
    fn over(&mut self, x: i32, y: i32, col: [f32; 3], a: f32) {
        let o = (y as usize * self.w + x as usize) * 4;
        let inv = 1.0 - a;
        let d = &mut self.px[o..o + 4];
        d[0] = (col[2] * 255.0 + d[0] as f32 * inv + 0.5).min(255.0) as u8;
        d[1] = (col[1] * 255.0 + d[1] as f32 * inv + 0.5).min(255.0) as u8;
        d[2] = (col[0] * 255.0 + d[2] as f32 * inv + 0.5).min(255.0) as u8;
        d[3] = (a * 255.0 + d[3] as f32 * inv + 0.5).min(255.0) as u8;
    }

    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    /// Кнопка-«стекло» с иконкой. `size` — сторона спрайта в px (круг внутри — 64/80 от неё).
    pub fn button(
        &mut self,
        tex: &ButtonTex,
        icon: Option<&Icon>,
        cx: f32,
        cy: f32,
        size: f32,
        look: &Look,
        alpha: f32,
    ) -> Rect {
        if size < 6.0 || alpha <= 0.0 {
            return Rect::EMPTY;
        }
        let half = size / 2.0;
        let (left, top) = (cx - half, cy - half);
        let r = Rect {
            x0: left.floor() as i32,
            y0: top.floor() as i32,
            x1: (cx + half).ceil() as i32,
            y1: (cy + half).ceil() as i32,
        }
        .clip(self.w, self.h);
        let isize = size * ICON_BOX / BOX;
        let (ileft, itop) = (cx - isize / 2.0, cy - isize / 2.0);
        let (n, scale) = (tex.n, tex.n as f32 / size);
        for y in r.y0..r.y1 {
            let v = (y as f32 + 0.5 - top) * scale - 0.5;
            for x in r.x0..r.x1 {
                let u = (x as f32 + 0.5 - left) * scale - 0.5;
                let [shadow, circle, ring] = sample3(&tex.data, n, u, v);
                // тень → заливка → обводка → иконка, каждый слой поверх предыдущего
                let mut a = shadow;
                let mut col = [0.0f32; 3];
                let fa = circle * look.fill_a;
                for c in 0..3 {
                    col[c] = look.fill[c] * fa + col[c] * (1.0 - fa);
                }
                a = fa + a * (1.0 - fa);
                for c in 0..3 {
                    col[c] = look.stroke[c] * ring + col[c] * (1.0 - ring);
                }
                a = ring + a * (1.0 - ring);
                match icon {
                    // одноцветная — цветом состояния
                    Some(Icon::Mask(ic)) => {
                        let iscale = ic.n as f32 / isize;
                        let iu = (x as f32 + 0.5 - ileft) * iscale - 0.5;
                        let iv = (y as f32 + 0.5 - itop) * iscale - 0.5;
                        if iu > -1.0 && iv > -1.0 && iu < ic.n as f32 && iv < ic.n as f32 {
                            let ia = sample1(&ic.data, ic.n, iu, iv);
                            for c in 0..3 {
                                col[c] = look.icon[c] * ia + col[c] * (1.0 - ia);
                            }
                            a = ia + a * (1.0 - ia);
                        }
                    }
                    // цветная — как есть (предумноженная)
                    Some(Icon::Color(img)) => {
                        let iscale = img.w as f32 / isize;
                        let iu = (x as f32 + 0.5 - ileft) * iscale - 0.5;
                        let iv = (y as f32 + 0.5 - itop) * iscale - 0.5;
                        if iu > -1.0 && iv > -1.0 && iu < img.w as f32 && iv < img.h as f32 {
                            let p = sample4(&img.data, img.w, img.h, iu, iv);
                            for c in 0..3 {
                                col[c] = p[c] + col[c] * (1.0 - p[3]);
                            }
                            a = p[3] + a * (1.0 - p[3]);
                        }
                    }
                    None => {}
                }
                if a > 0.002 {
                    self.over(
                        x,
                        y,
                        [col[0] * alpha, col[1] * alpha, col[2] * alpha],
                        a * alpha,
                    );
                }
            }
        }
        r
    }

    /// Квадратная картинка RGBA (предумноженная), масштабируется до `size`, центр в (cx, cy).
    pub fn image_scaled(&mut self, img: &Rgba, cx: f32, cy: f32, size: f32, alpha: f32) -> Rect {
        if size < 3.0 || alpha <= 0.0 {
            return Rect::EMPTY;
        }
        let half = size / 2.0;
        let (left, top) = (cx - half, cy - half);
        let r = Rect {
            x0: left.floor() as i32,
            y0: top.floor() as i32,
            x1: (cx + half).ceil() as i32,
            y1: (cy + half).ceil() as i32,
        }
        .clip(self.w, self.h);
        let scale = img.w as f32 / size;
        for y in r.y0..r.y1 {
            let v = (y as f32 + 0.5 - top) * scale - 0.5;
            for x in r.x0..r.x1 {
                let u = (x as f32 + 0.5 - left) * scale - 0.5;
                let p = sample4(&img.data, img.w, img.h, u, v);
                if p[3] > 0.002 {
                    self.over(
                        x,
                        y,
                        [p[0] * alpha, p[1] * alpha, p[2] * alpha],
                        p[3] * alpha,
                    );
                }
            }
        }
        r
    }

    /// Картинка 1:1 в целых координатах (плашки).
    pub fn image(&mut self, img: &Rgba, x: i32, y: i32, alpha: f32) -> Rect {
        let r = Rect {
            x0: x,
            y0: y,
            x1: x + img.w as i32,
            y1: y + img.h as i32,
        }
        .clip(self.w, self.h);
        for py in r.y0..r.y1 {
            for px in r.x0..r.x1 {
                let o = ((py - y) as usize * img.w + (px - x) as usize) * 4;
                let s = &img.data[o..o + 4];
                if s[3] == 0 {
                    continue;
                }
                let k = alpha / 255.0;
                self.over(
                    px,
                    py,
                    [s[0] as f32 * k, s[1] as f32 * k, s[2] as f32 * k],
                    s[3] as f32 * k,
                );
            }
        }
        r
    }
}

#[inline]
fn bilinear<const C: usize>(data: &[[u8; C]], w: usize, h: usize, u: f32, v: f32) -> [f32; C] {
    let (x0, y0) = (u.floor(), v.floor());
    let (tx, ty) = (u - x0, v - y0);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let mut out = [0.0f32; C];
    for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
        for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
            let (x, y) = (x0 + dx, y0 + dy);
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                continue; // за краем — прозрачно
            }
            let px = &data[y as usize * w + x as usize];
            let k = wx * wy / 255.0;
            for c in 0..C {
                out[c] += px[c] as f32 * k;
            }
        }
    }
    out
}

fn sample3(data: &[[u8; 3]], n: usize, u: f32, v: f32) -> [f32; 3] {
    bilinear(data, n, n, u, v)
}

fn sample1(data: &[u8], n: usize, u: f32, v: f32) -> f32 {
    let (x0, y0) = (u.floor(), v.floor());
    let (tx, ty) = (u - x0, v - y0);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let mut out = 0.0;
    for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
        for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
            let (x, y) = (x0 + dx, y0 + dy);
            if x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n {
                out += data[y as usize * n + x as usize] as f32 * wx * wy;
            }
        }
    }
    out / 255.0
}

fn sample4(data: &[u8], w: usize, h: usize, u: f32, v: f32) -> [f32; 4] {
    let (x0, y0) = (u.floor(), v.floor());
    let (tx, ty) = (u - x0, v - y0);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let mut out = [0.0f32; 4];
    for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
        for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
            let (x, y) = (x0 + dx, y0 + dy);
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                continue;
            }
            let o = (y as usize * w + x as usize) * 4;
            for c in 0..4 {
                out[c] += data[o + c] as f32 * wx * wy / 255.0;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IconRef, Theme};
    use crate::render::palette::{self, DEFAULT};
    use crate::render::raster::{self, no_program_icons, Bake, PillKind};

    #[test]
    fn rect_algebra() {
        let a = Rect {
            x0: 0,
            y0: 0,
            x1: 10,
            y1: 10,
        };
        let b = Rect {
            x0: 5,
            y0: 5,
            x1: 20,
            y1: 8,
        };
        assert_eq!(
            a.union(b),
            Rect {
                x0: 0,
                y0: 0,
                x1: 20,
                y1: 10
            }
        );
        assert_eq!(Rect::EMPTY.union(a), a);
        assert!(a.clip(4, 4).x1 == 4);
    }

    #[test]
    fn button_draws_inside_its_box_only() {
        let bake = Bake::build(&[IconRef::builtin("gear")], &[("x".into(), PillKind::Label)], 1.0, None, &no_program_icons);
        let (w, h) = (200usize, 200usize);
        let mut buf = vec![0u8; w * h * 4];
        let mut cv = Canvas::new(w, h, &mut buf);
        let r = cv.button(
            &bake.button,
            bake.icon(&IconRef::builtin("gear")),
            100.0,
            100.0,
            80.0,
            &palette::mix(&palette::one_hot(DEFAULT), Theme::Dark, None),
            1.0,
        );
        assert_eq!(
            r,
            Rect {
                x0: 60,
                y0: 60,
                x1: 140,
                y1: 140
            }
        );
        let px = |x: usize, y: usize| &buf[(y * w + x) * 4..(y * w + x) * 4 + 4];
        assert!(px(75, 100)[3] > 100); // тело кнопки
        assert_eq!(px(10, 10), &[0, 0, 0, 0]); // вне кнопки — пусто
        let darkest = (85..115)
            .flat_map(|y| (85..115).map(move |x| (x, y)))
            .map(|(x, y)| px(x, y)[0])
            .min()
            .unwrap();
        assert!(
            darkest < 80,
            "тёмная иконка поверх серой заливки: {darkest}"
        );
    }

    #[test]
    fn alpha_scales_and_clear_works() {
        let bake = Bake::build(&[], &[], 1.0, None, &no_program_icons);
        let (w, h) = (120usize, 120usize);
        let mut a = vec![0u8; w * h * 4];
        let mut b = vec![0u8; w * h * 4];
        let look = palette::mix(&palette::one_hot(DEFAULT), Theme::Dark, None);
        let r = Canvas::new(w, h, &mut a).button(&bake.button, None, 60.0, 60.0, 80.0, &look, 1.0);
        Canvas::new(w, h, &mut b).button(&bake.button, None, 60.0, 60.0, 80.0, &look, 0.5);
        let i = (60 * w + 60) * 4 + 3;
        assert!((a[i] as i32 - 2 * b[i] as i32).abs() <= 2);
        Canvas::new(w, h, &mut a).clear(r);
        assert!(a.iter().all(|&v| v == 0));
    }

    #[test]
    fn dot_and_image_blit() {
        let bake = Bake::build(&[], &[], 1.0, None, &no_program_icons);
        let mut buf = vec![0u8; 64 * 64 * 4];
        let mut cv = Canvas::new(64, 64, &mut buf);
        let r = cv.image_scaled(&bake.dot, 32.0, 32.0, raster::DOT_BOX, 1.0);
        assert!(!r.is_empty());
        assert!(buf[(32 * 64 + 32) * 4 + 3] > 200);
    }
}
