//! Раскладка кнопок: основные — по кругу, подменю — веером наружу от основной.

use super::geom::{ang_diff, Sector};
use super::metrics::Metrics;
use crate::model::{Menu, NodeId, ROOT};

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
        lay.fans(menu, m, ROOT, 0);
        lay
    }

    pub fn slot(&self, id: NodeId) -> &Slot {
        &self.slots[id]
    }

    /// Основные кнопки на расстоянии `main_d`: с начального угла, по или против часовой.
    fn root(&mut self, menu: &Menu, m: &Metrics) {
        let top = menu.children(ROOT);
        let span = 360.0 / top.len() as f32;
        let dir = if menu.shape.clockwise { 1.0 } else { -1.0 };
        for (i, &id) in top.iter().enumerate() {
            let mid = (menu.shape.start_angle + dir * i as f32 * span).rem_euclid(360.0);
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

    /// Веера всех кнопок ниже `parent`: сначала веер кнопки, затем веера его пунктов.
    fn fans(&mut self, menu: &Menu, m: &Metrics, parent: NodeId, level: usize) {
        for &id in menu.children(parent) {
            if !menu.children(id).is_empty() {
                self.fan(menu, m, id, level);
                self.fans(menu, m, id, level + 1);
            }
        }
    }

    /// Подменю веером наружу от основной кнопки: `fan_d` от неё, шаг `fan_spread`°.
    fn fan(&mut self, menu: &Menu, m: &Metrics, parent: NodeId, level: usize) {
        let (dist, step) = if level == 0 { (m.fan_d, m.fan_spread) } else { (m.sub_d, m.sub_spread) };
        let Slot {
            off: (ox, oy),
            sector,
        } = self.slots[parent];
        let mid = sector.mid();
        let kids = menu.children(parent);
        let n = kids.len() as f32;
        let spot = |deg: f32| {
            let a = (mid + deg).to_radians();
            (ox + dist * a.cos(), oy + dist * a.sin())
        };
        // порядок кнопок: слева направо (или сверху вниз, если веер горизонтальный)
        let vertical = mid.to_radians().sin().abs() > mid.to_radians().cos().abs();
        let pick = |p: (f32, f32)| if vertical { p.0 } else { p.1 };
        let sign = if pick(spot(step)) > pick(spot(-step)) { 1.0 } else { -1.0 };
        let mut angles = Vec::with_capacity(kids.len());
        for (j, &id) in kids.iter().enumerate() {
            let off = spot((j as f32 - (n - 1.0) / 2.0) * step * sign);
            let ang = off.1.atan2(off.0).to_degrees().rem_euclid(360.0); // куда смотреть из центра
            self.slots[id].off = off;
            angles.push(ang);
        }
        // сектор пункта — до середины между ним и соседями (крайние — симметрично)
        for (j, &id) in kids.iter().enumerate() {
            let gap = |k: Option<usize>| k.and_then(|k| angles.get(k)).map(|&b| ang_diff(angles[j], b));
            let left = gap(j.checked_sub(1));
            let right = gap(Some(j + 1));
            let g = match (left, right) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => 60.0,
            };
            let half = (g / 2.0 - 1.0).max(4.0);
            self.slots[id].sector = Sector {
                a1: angles[j] - half,
                a2: angles[j] + half,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{default_menu, Action, Item, Settings, Shape};

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

    #[test]
    fn top_buttons_go_clockwise_from_top() {
        let (menu, lay, m) = setup(default_menu());
        let t = menu.top();
        let (x, y) = lay.slot(t[0]).off;
        assert!(x.abs() < 1e-3 && (y + m.main_d).abs() < 1e-3); // сверху
        let (x, y) = lay.slot(t[1]).off;
        assert!((x - m.main_d).abs() < 1e-3 && y.abs() < 1e-3); // справа
        let (x, y) = lay.slot(t[2]).off;
        assert!(x.abs() < 1e-3 && (y - m.main_d).abs() < 1e-3); // снизу
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
        assert!((x - m.main_d).abs() < 1e-3 && y.abs() < 1e-3); // справа
        let (x, y) = lay.slot(t[1]).off;
        assert!(x.abs() < 1e-3 && (y + m.main_d).abs() < 1e-3); // дальше — против часовой: сверху
    }

    #[test]
    fn fan_is_left_to_right_for_top_button() {
        let (menu, lay, _) = setup(default_menu());
        let kids = menu.children(menu.top()[0]);
        let xs: Vec<f32> = kids.iter().map(|&k| lay.slot(k).off.0).collect();
        assert!(xs[0] < xs[1] && xs[1] < xs[2], "{xs:?}");
    }

    #[test]
    fn nothing_collides_even_when_full() {
        for menu in [default_menu(), big(12, 6), big(7, 5), big(2, 1)] {
            let (menu, lay, m) = setup(menu);
            let min = 2.0 * m.circle_r;
            let top = menu.top();
            for (i, &a) in top.iter().enumerate() {
                for &b in &top[i + 1..] {
                    let (pa, pb) = (lay.slot(a).off, lay.slot(b).off);
                    assert!((pa.0 - pb.0).hypot(pa.1 - pb.1) > min, "основные {i}");
                }
                let kids = menu.children(a);
                for (j, &k) in kids.iter().enumerate() {
                    for &l in &kids[j + 1..] {
                        let (pa, pb) = (lay.slot(k).off, lay.slot(l).off);
                        assert!((pa.0 - pb.0).hypot(pa.1 - pb.1) > min, "веер {i}");
                    }
                    // пункт веера не залезает в центр меню
                    let p = lay.slot(k).off;
                    assert!(p.0.hypot(p.1) > m.main_d, "веер {i} уходит внутрь");
                }
            }
        }
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

    #[test]
    fn nested_fans_never_collide_with_what_is_shown_with_them() {
        for menu in [nested(4, 3, 3), nested(12, 6, 4), nested(6, 2, 2), nested(3, 6, 1), nested(8, 4, 4)] {
            let (menu, lay, m) = setup(menu);
            let min = 2.0 * m.circle_r;
            for open in menu.ids().filter(|&i| !menu.children(i).is_empty()) {
                // видно: основные, а также пункты каждого раскрытого веера на пути к `open`
                let shown: Vec<_> = menu
                    .ids()
                    .filter(|&i| menu.node(i).parent == Some(ROOT) || menu.on_path(menu.node(i).parent.unwrap(), Some(open)))
                    .collect();
                for (n, &a) in shown.iter().enumerate() {
                    for &b in &shown[n + 1..] {
                        let (pa, pb) = (lay.slot(a).off, lay.slot(b).off);
                        assert!((pa.0 - pb.0).hypot(pa.1 - pb.1) > min, "{a} и {b} при открытом {open}");
                    }
                    let p = lay.slot(a).off;
                    assert!(p.0.hypot(p.1) > m.main_d - 1.0, "{a} внутри круга");
                    assert!(p.0.hypot(p.1) + m.circle_r < m.half() as f32, "{a} вылез из окна");
                }
            }
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
}
