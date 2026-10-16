//! Раскладка кнопок: основные — по кругу, подменю — веером наружу от основной.

use crate::geom::Sector;
use crate::menu::{Menu, NodeId, ROOT};
use crate::metrics::{Metrics, FAN_SPREAD};

#[derive(Clone, Copy, Debug, Default)]
pub struct Slot {
    /// Смещение центра кнопки от центра меню, px.
    pub off: (f32, f32),
    /// Угловой сектор, в котором кнопка выбирается.
    pub sector: Sector,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub slots: Vec<Slot>,
}

impl Layout {
    pub fn new(menu: &Menu, m: &Metrics) -> Layout {
        let mut lay = Layout {
            slots: vec![Slot::default(); menu.nodes.len()],
        };
        lay.root(menu, m);
        for &t in menu.top() {
            if !menu.children(t).is_empty() {
                lay.fan(menu, m, t);
            }
        }
        lay
    }

    pub fn slot(&self, id: NodeId) -> &Slot {
        &self.slots[id]
    }

    /// Основные кнопки на расстоянии `main_d`: сверху и далее по часовой.
    fn root(&mut self, menu: &Menu, m: &Metrics) {
        let top = menu.children(ROOT);
        let span = 360.0 / top.len() as f32;
        for (i, &id) in top.iter().enumerate() {
            let mid = 270.0 + i as f32 * span;
            let a = mid.to_radians();
            self.slots[id] = Slot {
                off: (m.main_d * a.cos(), m.main_d * a.sin()),
                sector: Sector {
                    a1: mid - span / 2.0 + 1.5,
                    a2: mid + span / 2.0 - 1.5,
                },
            };
        }
    }

    /// Подменю веером наружу от основной кнопки: `fan_d` от неё, шаг `FAN_SPREAD`°.
    fn fan(&mut self, menu: &Menu, m: &Metrics, parent: NodeId) {
        let Slot {
            off: (ox, oy),
            sector,
        } = self.slots[parent];
        let mid = sector.mid();
        let kids = menu.children(parent);
        let n = kids.len() as f32;
        let spot = |deg: f32| {
            let a = (mid + deg).to_radians();
            (ox + m.fan_d * a.cos(), oy + m.fan_d * a.sin())
        };
        // порядок кнопок: слева направо (или сверху вниз, если веер горизонтальный)
        let vertical = mid.to_radians().sin().abs() > mid.to_radians().cos().abs();
        let pick = |p: (f32, f32)| if vertical { p.0 } else { p.1 };
        let sign = if pick(spot(FAN_SPREAD)) > pick(spot(-FAN_SPREAD)) {
            1.0
        } else {
            -1.0
        };
        for (j, &id) in kids.iter().enumerate() {
            let off = spot((j as f32 - (n - 1.0) / 2.0) * FAN_SPREAD * sign);
            let ang = off.1.atan2(off.0).to_degrees().rem_euclid(360.0); // куда смотреть из центра
            let half = FAN_SPREAD.min(90.0 / n) / 2.0;
            self.slots[id] = Slot {
                off,
                sector: Sector {
                    a1: ang - half + 1.0,
                    a2: ang + half - 1.0,
                },
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::menu::default_menu;

    fn setup() -> (Menu, Layout, Metrics) {
        let menu = default_menu();
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080));
        let lay = Layout::new(&menu, &m);
        (menu, lay, m)
    }

    #[test]
    fn top_buttons_go_clockwise_from_top() {
        let (menu, lay, m) = setup();
        let t = menu.top();
        let (x, y) = lay.slot(t[0]).off;
        assert!(x.abs() < 1e-3 && (y + m.main_d).abs() < 1e-3); // сверху
        let (x, y) = lay.slot(t[1]).off;
        assert!((x - m.main_d).abs() < 1e-3 && y.abs() < 1e-3); // справа
        let (x, y) = lay.slot(t[2]).off;
        assert!(x.abs() < 1e-3 && (y - m.main_d).abs() < 1e-3); // снизу
    }

    #[test]
    fn fan_is_left_to_right_for_top_button() {
        let (menu, lay, _) = setup();
        let kids = menu.children(menu.top()[0]);
        let xs: Vec<f32> = kids.iter().map(|&k| lay.slot(k).off.0).collect();
        assert!(xs[0] < xs[1] && xs[1] < xs[2], "{xs:?}");
    }

    #[test]
    fn fan_buttons_do_not_collide() {
        let (menu, lay, m) = setup();
        for &t in menu.top() {
            let kids = menu.children(t);
            for (i, &a) in kids.iter().enumerate() {
                for &b in &kids[i + 1..] {
                    let (pa, pb) = (lay.slot(a).off, lay.slot(b).off);
                    assert!((pa.0 - pb.0).hypot(pa.1 - pb.1) > m.circle_r * 2.0);
                }
            }
        }
    }
}
