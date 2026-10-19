//! Раскладка кнопок: основные — секторами первого кольца, веер кнопки — секторами следующего
//! кольца прямо над ней. У всех колец один способ расчёта: кнопка — дуга радиуса кольца с
//! круглыми концами (полоса толщины 2h); соседние кнопки разведены так, что зазор между их
//! краями везде равен `gap`; веер занимает не меньше угла своей родительской кнопки.

use super::geom::Sector;
use super::metrics::{pitch_deg, Metrics};
use crate::model::{Menu, NodeId, ROOT};

/// Дуга кнопки: ось — окружность радиуса `r`, центр кнопки на угле `theta`, концы оси
/// отстоят от центра на `span` (градусы); с учётом круглых концов кнопка занимает `extent`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Arc {
    pub r: f32,
    pub theta: f32,
    pub span: f32,
    pub extent: f32,
}

impl Arc {
    pub fn point(&self, t: f32) -> (f32, f32) {
        let a = t.to_radians();
        (self.r * a.cos(), self.r * a.sin())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Slot {
    /// Смещение центра кнопки от центра меню, px (сюда кладётся иконка и рядом — подпись).
    pub off: (f32, f32),
    /// Угловой сектор, в котором кнопка выбирается.
    pub sector: Sector,
    pub arc: Arc,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub slots: Vec<Slot>,
}

/// На сколько градусов закруглённый конец полосы толщины 2h на радиусе `r` выступает за ось.
fn cap_deg(h: f32, r: f32) -> f32 {
    (h / r).clamp(0.0, 1.0).asin().to_degrees()
}

impl Layout {
    pub fn new(menu: &Menu, m: &Metrics) -> Layout {
        let mut lay = Layout {
            slots: vec![Slot::default(); menu.nodes.len()],
        };
        lay.root(menu, m);
        lay.fans(menu, m, ROOT, 1);
        lay
    }

    pub fn slot(&self, id: NodeId) -> &Slot {
        &self.slots[id]
    }

    fn put(&mut self, id: NodeId, arc: Arc, half_sector: f32) {
        let (x, y) = arc.point(arc.theta);
        self.slots[id] = Slot {
            off: (x, y),
            sector: Sector {
                a1: arc.theta - half_sector,
                a2: arc.theta + half_sector,
            },
            arc,
        };
    }

    /// Основные кнопки: с начального угла, по или против часовой, поровну по кругу.
    fn root(&mut self, menu: &Menu, m: &Metrics) {
        let top = menu.children(ROOT);
        let n = top.len() as f32;
        let dir = if menu.shape.clockwise { 1.0 } else { -1.0 };
        let r = m.ring(0);
        let pitch = 360.0 / n; // угол между центрами соседних
        let span = (pitch / 2.0 - pitch_deg(r, m.h, m.gap) / 2.0).max(0.0);
        for (i, &id) in top.iter().enumerate() {
            let mid = (menu.shape.start_angle + dir * i as f32 * pitch).rem_euclid(360.0);
            let arc = Arc {
                r,
                theta: mid,
                span,
                extent: span + cap_deg(m.h, r),
            };
            self.put(id, arc, pitch / 2.0);
        }
    }

    /// Веера всех кнопок ниже `parent`; `level` — кольцо их детей.
    fn fans(&mut self, menu: &Menu, m: &Metrics, parent: NodeId, level: usize) {
        for &id in menu.children(parent) {
            if !menu.children(id).is_empty() {
                self.fan(menu, m, id, level);
                self.fans(menu, m, id, level + 1);
            }
        }
    }

    /// Веер кнопки `parent` кольцом `level`: тот же угол, что у родителя (шире — если иначе кнопки
    /// слиплись бы), поровну на всех, зазор между кнопками `gap`.
    fn fan(&mut self, menu: &Menu, m: &Metrics, parent: NodeId, level: usize) {
        let kids = menu.children(parent);
        let k = kids.len() as f32;
        let r = m.ring(level);
        let cap = cap_deg(m.h, r);
        let step = pitch_deg(r, m.h, m.gap); // минимальный шаг между центрами соседних (при span = 0)
        let pa = self.slots[parent].arc;
        // полуширина веера: как у родителя, но не меньше, чем нужно кругляшам с зазором
        let ext = pa.extent.max((k - 1.0) / 2.0 * step + cap);
        let centers_half = ext - cap; // от центра веера до центра крайнего конца дуги
        let span = ((2.0 * centers_half - (k - 1.0) * step) / (2.0 * k)).max(0.0);
        let pitch = 2.0 * span + step;
        // порядок кнопок: слева направо (или сверху вниз, если веер у боковой кнопки)
        let (sin, cos) = pa.theta.to_radians().sin_cos();
        let vertical = sin.abs() > cos.abs();
        let pick = |t: f32| {
            let p = Arc { r, theta: t, span: 0.0, extent: 0.0 }.point(t);
            if vertical { p.0 } else { p.1 }
        };
        let sign = if pick(pa.theta + 1.0) > pick(pa.theta - 1.0) { 1.0 } else { -1.0 };
        for (j, &id) in kids.iter().enumerate() {
            let theta = pa.theta + sign * (j as f32 - (k - 1.0) / 2.0) * pitch;
            let arc = Arc {
                r,
                theta,
                span,
                extent: span + cap,
            };
            self.put(id, arc, if k > 1.0 { pitch / 2.0 } else { ext });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{default_menu, Action, Item, Settings, Shape};
    use crate::engine::geom::ang_diff;
    use crate::render::compose::SectorShape;

    fn setup(menu: Menu) -> (Menu, Layout, Metrics) {
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu);
        let lay = Layout::new(&menu, &m);
        (menu, lay, m)
    }

    fn big(top: usize, kids: usize) -> Menu {
        Menu::from_items(
            (0..top)
                .map(|_| {
                    Item::new("gear", "X").children(
                        (0..kids).map(|_| Item::new("gear", "Y").action(Action::Note)).collect(),
                    )
                })
                .collect(),
        )
    }

    /// Меню из трёх уровней: `top` кнопок, у каждой `kids` пунктов, у каждого пункта `subs` вложенных.
    fn nested(top: usize, kids: usize, subs: usize) -> Menu {
        Menu::from_items(
            (0..top)
                .map(|_| {
                    Item::new("gear", "X").children(
                        (0..kids)
                            .map(|_| {
                                Item::new("gear", "Y").children(
                                    (0..subs).map(|_| Item::new("gear", "Z").action(Action::Note)).collect(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    fn shape(a: &Arc) -> SectorShape {
        SectorShape {
            cx: 0.0,
            cy: 0.0,
            r: a.r,
            a0: (a.theta - a.span).to_radians(),
            a1: (a.theta + a.span).to_radians(),
            h: 0.0,
        }
    }

    /// Зазор между краями двух кнопок: расстояние между их осями минус две половины толщины.
    fn gap(a: &Arc, b: &Arc, h: f32) -> f32 {
        let sb = shape(b);
        let n = 400;
        (0..=n)
            .map(|i| {
                let (x, y) = a.point(a.theta - a.span + 2.0 * a.span * i as f32 / n as f32);
                sb.dist(x, y)
            })
            .fold(f32::MAX, f32::min)
            - 2.0 * h
    }

    #[test]
    fn top_buttons_go_clockwise_from_top() {
        let (menu, lay, m) = setup(default_menu());
        let t = menu.top();
        let (x, y) = lay.slot(t[0]).off;
        assert!(x.abs() < 1e-3 && (y + m.ring(0)).abs() < 1e-3); // сверху
        let (x, y) = lay.slot(t[1]).off;
        assert!((x - m.ring(0)).abs() < 1e-3 && y.abs() < 1e-3); // справа
        let (x, y) = lay.slot(t[2]).off;
        assert!(x.abs() < 1e-3 && (y - m.ring(0)).abs() < 1e-3); // снизу
    }

    #[test]
    fn start_angle_and_direction_are_respected() {
        let shape = Shape {
            start_angle: 0.0,
            clockwise: false,
            ..Shape::default()
        };
        let (menu, lay, m) = setup(default_menu().with_shape(shape));
        let t = menu.top();
        let (x, y) = lay.slot(t[0]).off;
        assert!((x - m.ring(0)).abs() < 1e-3 && y.abs() < 1e-3); // справа
        let (x, y) = lay.slot(t[1]).off;
        assert!(x.abs() < 1e-3 && (y + m.ring(0)).abs() < 1e-3); // дальше — против часовой: сверху
    }

    #[test]
    fn fan_is_left_to_right_for_top_button() {
        let (menu, lay, _) = setup(default_menu());
        let kids = menu.children(menu.top()[0]);
        let xs: Vec<f32> = kids.iter().map(|&k| lay.slot(k).off.0).collect();
        assert!(xs[0] < xs[1] && xs[1] < xs[2], "{xs:?}");
    }

    #[test]
    fn gaps_are_equal_everywhere() {
        // и между соседними основными, и между соседними пунктами веера — ровно gap
        for menu in [default_menu(), big(7, 5), big(12, 6), big(3, 2), nested(4, 3, 3)] {
            let (menu, lay, m) = setup(menu);
            let check = |ids: &[NodeId]| {
                for w in ids.windows(2) {
                    let (a, b) = (lay.slot(w[0]).arc, lay.slot(w[1]).arc);
                    let g = gap(&a, &b, m.h);
                    assert!(g > m.gap - 0.6, "зазор {g} меньше {}", m.gap);
                    assert!(g < m.gap + 0.6, "зазор {g} больше {}", m.gap);
                }
            };
            check(menu.top());
            for &t in menu.top() {
                check(menu.children(t));
                for &k in menu.children(t) {
                    check(menu.children(k));
                }
            }
        }
    }

    #[test]
    fn rings_are_separated_by_the_ring_gap() {
        let (menu, lay, m) = setup(nested(4, 3, 3));
        for &t in menu.top() {
            let top = lay.slot(t).arc;
            for &k in menu.children(t) {
                let kid = lay.slot(k).arc;
                assert!((kid.r - top.r - m.step()).abs() < 1e-3);
                for &s in menu.children(k) {
                    assert!((lay.slot(s).arc.r - kid.r - m.step()).abs() < 1e-3);
                }
            }
        }
    }

    #[test]
    fn fan_sits_over_its_parent() {
        let (menu, lay, _) = setup(default_menu());
        for &t in menu.top() {
            let p = lay.slot(t).arc;
            let kids = menu.children(t);
            if kids.is_empty() {
                continue;
            }
            let first = lay.slot(kids[0]).arc;
            let last = lay.slot(*kids.last().unwrap()).arc;
            let (lo, hi) = (first.theta.min(last.theta) - first.extent, first.theta.max(last.theta) + first.extent);
            // веер не уже родителя и симметричен ему
            assert!(hi - lo >= 2.0 * p.extent - 0.5, "{lo} {hi} {}", p.extent);
            assert!(((lo + hi) / 2.0 - p.theta).abs() < 0.5);
        }
    }

    #[test]
    fn fan_sectors_do_not_overlap() {
        let (menu, lay, _) = setup(big(6, 6));
        for &t in menu.top() {
            let kids = menu.children(t);
            for w in kids.windows(2) {
                let (a, b) = (lay.slot(w[0]).sector, lay.slot(w[1]).sector);
                let gap = ang_diff(a.mid(), b.mid());
                assert!(a.half() + b.half() <= gap + 1e-3);
            }
        }
    }

    #[test]
    fn everything_fits_the_window() {
        for menu in [default_menu(), big(12, 6), nested(12, 6, 4), nested(2, 1, 1)] {
            let (menu, lay, m) = setup(menu);
            for id in menu.ids() {
                let a = lay.slot(id).arc;
                assert!(a.r + m.h < m.half() as f32, "кнопка {id} вылезла из окна");
            }
        }
    }
}
