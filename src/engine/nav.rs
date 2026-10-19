//! Навигация: виртуальный указатель внутри меню и выбор кнопки по направлению от центра.

use super::geom::{ang_diff, polar};
use super::layout::Layout;
use super::metrics::{Metrics, HYST};
use crate::model::{Context, Menu, NodeId, ROOT};

#[derive(Debug, Default)]
pub struct Nav {
    /// Виртуальный указатель относительно центра меню.
    pub px: f32,
    pub py: f32,
    /// Выбранная кнопка (основная или пункт подменю).
    pub hot: Option<NodeId>,
    /// Основная кнопка с раскрытым подменю.
    pub open: Option<NodeId>,
    dwell_node: Option<NodeId>,
    dwell_t0: f64,
}

impl Nav {
    pub fn reset(&mut self) {
        *self = Nav::default();
    }

    /// Сдвинуть указатель на `delta` (сырые единицы мыши) и пересчитать выбор.
    pub fn update(&mut self, menu: &Menu, ctx: &Context, lay: &Layout, m: &Metrics, delta: (f32, f32), now: f64) {
        self.px += delta.0 * m.sens;
        self.py += delta.1 * m.sens;
        let (mut r, phi) = polar(self.px, self.py);
        let rmax = m.limit(self.open.map(|o| menu.level(o) + 1));
        if r > rmax {
            // указатель (невидимый) не уходит дальше края меню
            let k = rmax / r;
            self.px *= k;
            self.py *= k;
            r = rmax;
        }

        let mid = |id: NodeId| lay.slot(id).sector.mid();
        let nearest = |ids: &[NodeId]| {
            ids.iter()
                .copied()
                .min_by(|&a, &b| ang_diff(phi, mid(a)).total_cmp(&ang_diff(phi, mid(b))))
        };
        let mut hot = None;
        if r >= m.dead {
            // блок выбирается по направлению от центра
            let mut cur = nearest(menu.top()).expect("в меню нет кнопок");
            hot = Some(cur);
            // дальше вглубь, пока веер раскрыт, а указатель зашёл за кнопку
            for level in 0.. {
                let node = menu.node(cur);
                if node.children.is_empty() || (node.dwell && !menu.on_path(cur, self.open)) {
                    break;
                }
                // если уже стоим на пункте этого веера — порог выхода из него ниже
                let keep = self.hot.is_some_and(|h| menu.node(h).parent == Some(cur));
                if r <= m.enter(level + 1) - if keep { HYST } else { 0.0 } {
                    break;
                }
                // ближайший по углу пункт; зазоры между секторами не считаются «пустотой»
                let Some(k) = nearest(&node.children) else { break };
                if ang_diff(phi, mid(k)) > lay.slot(k).sector.half() + 1.5 {
                    break;
                }
                hot = Some(k);
                cur = k;
            }
        }

        // недоступная кнопка не выбирается (её веер, если он раскрыт, остаётся как есть)
        if hot.is_some_and(|h| menu.is_disabled(h, ctx)) {
            hot = None;
        }

        if r < m.dead && self.open.is_none() {
            self.dwell_node = None; // вернулись в центр, ничего не открыто: отсчёт задержки заново
        }
        // в центре веера не трогаем: они закрываются только переходом на другую кнопку
        if let Some(h) = hot {
            if hot != self.dwell_node {
                self.dwell_node = hot;
                self.dwell_t0 = now;
            }
            // раскрыт самый глубокий веер на пути к выбранной кнопке, который уже можно показывать
            let waited = now - self.dwell_t0 >= m.dwell;
            let mut path = Vec::new();
            let mut cur = Some(h);
            while let Some(c) = cur {
                path.push(c);
                cur = menu.node(c).parent.filter(|&p| p != ROOT);
            }
            self.open = path.into_iter().find(|&c| {
                let n = menu.node(c);
                !n.children.is_empty() && (!n.dwell || menu.on_path(c, self.open) || (c == h && waited))
            });
        }
        self.hot = hot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{default_menu, Settings};

    struct Rig {
        menu: Menu,
        lay: Layout,
        m: Metrics,
        nav: Nav,
    }

    fn rig() -> Rig {
        let menu = default_menu();
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu);
        let lay = Layout::new(&menu, &m);
        Rig {
            menu,
            lay,
            m,
            nav: Nav::default(),
        }
    }

    impl Rig {
        /// Поставить указатель точно в точку (в единицах экрана) и пересчитать.
        fn put(&mut self, x: f32, y: f32, now: f64) {
            let d = (
                (x - self.nav.px) / self.m.sens,
                (y - self.nav.py) / self.m.sens,
            );
            self.nav.update(&self.menu, &Context::default(), &self.lay, &self.m, d, now);
        }
    }

    #[test]
    fn disabled_button_is_not_selected() {
        use crate::model::{Flag, Item};
        let menu = Menu::from_items(vec![
            Item::new("copy", "A").action(crate::model::Action::Note).enabled_if(Flag::DesktopPrev),
            Item::new("copy", "B").action(crate::model::Action::Note),
        ]);
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu);
        let lay = Layout::new(&menu, &m);
        let mut nav = Nav::default();
        // тянемся к первой кнопке (сверху, угол -90°): она выключена — выбора нет
        let off = Context::default();
        nav.update(&menu, &off, &lay, &m, (0.0, -10_000.0), 0.0);
        assert_eq!(nav.hot, None);
        let mut nav = Nav::default();
        nav.update(&menu, &off.with(Flag::DesktopPrev, true), &lay, &m, (0.0, -10_000.0), 0.0);
        assert!(nav.hot.is_some());
    }

    #[test]
    fn center_selects_nothing() {
        let mut r = rig();
        r.put(5.0, 5.0, 0.0);
        assert_eq!(r.nav.hot, None);
        assert_eq!(r.nav.open, None);
    }

    #[test]
    fn direction_picks_block_and_opens_submenu() {
        let mut r = rig();
        r.put(0.0, -80.0, 0.0); // вверх
        let top = r.menu.top()[0];
        assert_eq!(r.nav.hot, Some(top));
        assert_eq!(r.nav.open, Some(top)); // без dwell открывается сразу
    }

    #[test]
    fn dwell_block_opens_only_after_delay() {
        let mut r = rig();
        let paste = r.menu.top()[1]; // справа, dwell
        r.put(80.0, 0.0, 0.0);
        assert_eq!((r.nav.hot, r.nav.open), (Some(paste), None));
        r.put(80.0, 0.0, 0.2);
        assert_eq!(r.nav.open, None);
        r.put(80.0, 0.0, 0.5);
        assert_eq!(r.nav.open, Some(paste));
    }

    #[test]
    fn submenu_item_selected_beyond_r_out() {
        let mut r = rig();
        let top = r.menu.top()[0];
        r.put(0.0, -80.0, 0.0);
        let kid = r.menu.children(top)[0]; // самый левый
        let (ox, oy) = r.lay.slot(kid).off;
        let ang = oy.atan2(ox);
        let rr = r.m.enter(1) + 20.0;
        r.put(rr * ang.cos(), rr * ang.sin(), 0.1);
        assert_eq!(r.nav.hot, Some(kid));
    }

    #[test]
    fn hysteresis_keeps_submenu_item() {
        let mut r = rig();
        let top = r.menu.top()[0];
        r.put(0.0, -80.0, 0.0);
        let kid = r.menu.children(top)[1];
        let (ox, oy) = r.lay.slot(kid).off;
        let ang = oy.atan2(ox);
        r.put(
            (r.m.enter(1) + 10.0) * ang.cos(),
            (r.m.enter(1) + 10.0) * ang.sin(),
            0.1,
        );
        assert_eq!(r.nav.hot, Some(kid));
        let inner = r.m.enter(1) - HYST / 2.0; // внутри порога входа, но в зоне гистерезиса
        r.put(inner * ang.cos(), inner * ang.sin(), 0.2);
        assert_eq!(r.nav.hot, Some(kid));
        let out = r.m.enter(1) - HYST - 5.0;
        r.put(out * ang.cos(), out * ang.sin(), 0.3);
        assert_eq!(r.nav.hot, Some(top));
    }

    #[test]
    fn pointer_is_clamped() {
        let mut r = rig();
        r.nav.update(&r.menu, &Context::default(), &r.lay, &r.m, (10_000.0, 0.0), 0.0);
        assert!((r.nav.px - r.m.limit(None)).abs() < 1e-3);
    }

    #[test]
    fn center_keeps_open_submenu() {
        let mut r = rig();
        r.put(0.0, -80.0, 0.0);
        let top = r.menu.top()[0];
        r.put(3.0, 3.0, 0.1);
        assert_eq!(r.nav.open, Some(top));
        assert_eq!(r.nav.hot, None);
    }

    #[test]
    fn nested_fan_opens_step_by_step() {
        use crate::model::{Action, Item};
        let leaf = |n: &str| Item::new("gear", n).action(Action::Note);
        let menu = Menu::from_items(vec![Item::new("gear", "A").action(Action::Note).children(vec![
            Item::new("gear", "B").children(vec![leaf("C1"), leaf("C2")]),
        ])]);
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu);
        let lay = Layout::new(&menu, &m);
        let mut r = Rig { menu, lay, m, nav: Nav::default() };
        let (a, b, c1) = (r.menu.top()[0], 2, 3);
        let at = |r: &Rig, id| r.lay.slot(id).off;
        let p = at(&r, a);
        r.put(p.0, p.1, 0.0);
        assert_eq!((r.nav.hot, r.nav.open), (Some(a), Some(a)));
        let p = at(&r, b);
        r.put(p.0, p.1, 0.1);
        assert_eq!((r.nav.hot, r.nav.open), (Some(b), Some(b)));
        let p = at(&r, c1);
        r.put(p.0, p.1, 0.2);
        assert_eq!(r.nav.hot, Some(c1));
        assert_eq!(r.nav.open, Some(b)); // веер B остаётся раскрытым
        // назад на A: вложенный веер закрывается
        let p = at(&r, a);
        r.put(p.0, p.1, 0.3);
        assert_eq!((r.nav.hot, r.nav.open), (Some(a), Some(a)));
    }
}
