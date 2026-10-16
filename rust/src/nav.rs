//! Навигация: виртуальный указатель внутри меню и выбор кнопки по направлению от центра.

use crate::geom::{ang_diff, polar};
use crate::layout::Layout;
use crate::menu::{Menu, NodeId};
use crate::metrics::{Metrics, HYST};

#[derive(Debug, Default)]
pub struct Nav {
    /// Виртуальный указатель относительно центра меню.
    pub px: f32,
    pub py: f32,
    /// Выбранная кнопка (основная или пункт подменю).
    pub hot: Option<NodeId>,
    /// Основная кнопка с раскрытым подменю.
    pub open: Option<NodeId>,
    dwell_root: Option<NodeId>,
    dwell_t0: f64,
}

impl Nav {
    pub fn reset(&mut self) {
        *self = Nav::default();
    }

    /// Сдвинуть указатель на `delta` (сырые единицы мыши) и пересчитать выбор.
    pub fn update(&mut self, menu: &Menu, lay: &Layout, m: &Metrics, delta: (f32, f32), now: f64) {
        self.px += delta.0 * m.sens;
        self.py += delta.1 * m.sens;
        let (mut r, phi) = polar(self.px, self.py);
        let rmax = if self.open.is_some() {
            m.rmax_sub
        } else {
            m.rmax
        };
        if r > rmax {
            // указатель (невидимый) не уходит дальше края меню
            let k = rmax / r;
            self.px *= k;
            self.py *= k;
            r = rmax;
        }
        let mid = |id: NodeId| lay.slot(id).sector.mid();

        let mut hot = None;
        let mut root = None;
        if r >= m.dead {
            // блок выбирается по направлению от центра
            let top = menu
                .top()
                .iter()
                .copied()
                .min_by(|&a, &b| ang_diff(phi, mid(a)).total_cmp(&ang_diff(phi, mid(b))))
                .expect("в меню нет кнопок");
            root = Some(top);
            hot = root;
            let node = menu.node(top);
            // подпункты выбираются, только когда раскрыты
            if !node.children.is_empty() && (!node.dwell || self.open == Some(top)) {
                // если уже стоим на подпункте этого блока — порог выхода из подменю ниже
                let keep = self.hot.is_some_and(|h| menu.node(h).parent == Some(top));
                if r > m.r_out - if keep { HYST } else { 0.0 } {
                    // ближайший по углу подпункт; зазоры между секторами не считаются «пустотой»
                    let k = node
                        .children
                        .iter()
                        .copied()
                        .min_by(|&a, &b| ang_diff(phi, mid(a)).total_cmp(&ang_diff(phi, mid(b))))
                        .expect("пустой список проверен выше");
                    if ang_diff(phi, mid(k)) <= lay.slot(k).sector.half() + 1.5 {
                        hot = Some(k);
                    }
                }
            }
        }
        if r < m.dead && self.open.is_none() {
            self.dwell_root = None; // вернулись в центр, ничего не открыто: отсчёт задержки заново
        }
        if let Some(top) = root {
            // в центре подменю не трогаем: оно закрывается только переходом на другой блок
            if root != self.dwell_root {
                self.dwell_root = root;
                self.dwell_t0 = now;
            }
            let node = menu.node(top);
            let ready = !node.dwell || self.open == root || now - self.dwell_t0 >= m.dwell;
            self.open = if !node.children.is_empty() && ready {
                root
            } else {
                None
            };
        }
        self.hot = hot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::menu::default_menu;

    struct Rig {
        menu: Menu,
        lay: Layout,
        m: Metrics,
        nav: Nav,
    }

    fn rig() -> Rig {
        let menu = default_menu();
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080));
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
            self.nav.update(&self.menu, &self.lay, &self.m, d, now);
        }
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
        let rr = r.m.r_out + 20.0;
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
            (r.m.r_out + 10.0) * ang.cos(),
            (r.m.r_out + 10.0) * ang.sin(),
            0.1,
        );
        assert_eq!(r.nav.hot, Some(kid));
        let inner = r.m.r_out - HYST / 2.0; // внутри порога входа, но в зоне гистерезиса
        r.put(inner * ang.cos(), inner * ang.sin(), 0.2);
        assert_eq!(r.nav.hot, Some(kid));
        let out = r.m.r_out - HYST - 5.0;
        r.put(out * ang.cos(), out * ang.sin(), 0.3);
        assert_eq!(r.nav.hot, Some(top));
    }

    #[test]
    fn pointer_is_clamped() {
        let mut r = rig();
        r.nav.update(&r.menu, &r.lay, &r.m, (10_000.0, 0.0), 0.0);
        assert!((r.nav.px - r.m.rmax).abs() < 1e-3);
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
}
