//! Анимация: появление, наведение и плавная смена цвета каждой кнопки.

use crate::menu::{Context, Menu, NodeId, ROOT};
use crate::metrics::{APPEAR_T, COLOR_T};
use crate::style::{self, Weights};

#[derive(Clone, Copy, Debug, Default)]
pub struct NodeAnim {
    /// Появление 0..1 (отрицательное = задержка перед началом).
    pub p: f32,
    /// Наведение 0..1.
    pub hv: f32,
    /// Веса состояний; `None` — ещё не инициализированы.
    pub w: Option<Weights>,
}

#[derive(Debug)]
pub struct Anim {
    pub nodes: Vec<NodeAnim>,
    pub hub_p: f32,
    pub hub_w: Option<Weights>,
}

/// Что аниматору нужно знать о происходящем в кадре.
pub struct Scene<'a> {
    pub menu: &'a Menu,
    pub ctx: &'a Context,
    pub active: bool,
    pub open: Option<NodeId>,
    pub hot: Option<NodeId>,
}

fn approach(w: &mut Weights, target: usize, k: f32) {
    for (i, v) in w.iter_mut().enumerate() {
        *v += (if i == target { 1.0 } else { 0.0 } - *v) * k;
    }
}

impl Anim {
    pub fn new(menu: &Menu) -> Anim {
        let mut a = Anim {
            nodes: vec![NodeAnim::default(); menu.nodes.len()],
            hub_p: 0.0,
            hub_w: None,
        };
        a.reset(menu);
        a
    }

    /// Начало показа: основные кнопки появляются по очереди.
    pub fn reset(&mut self, menu: &Menu) {
        for n in &mut self.nodes {
            *n = NodeAnim::default();
        }
        for (i, &id) in menu.top().iter().enumerate() {
            self.nodes[id].p = -0.10 * i as f32;
        }
        self.hub_p = 0.0;
        self.hub_w = None;
    }

    pub fn step(&mut self, dt: f64, sc: &Scene) {
        let k = (1.0 - (-dt / COLOR_T).exp()) as f32;
        let appear = (dt / APPEAR_T) as f32;
        let vanish = (dt / (APPEAR_T * 0.6)) as f32;
        for id in sc.menu.ids() {
            let visible = sc.active && is_visible(sc.menu, id, sc.open);
            let n = &mut self.nodes[id];
            n.p = if visible {
                (n.p + appear).min(1.0)
            } else if n.p > 0.0 {
                (n.p - vanish).max(0.0)
            } else {
                0.0
            };
            let node = sc.menu.node(id);
            let hot = sc.active
                && sc.hot == Some(id)
                && (node.children.is_empty() || node.action.is_some());
            let target = if hot {
                style::HOVER
            } else if sc.open == Some(id) {
                style::OPEN
            } else if sc.menu.is_state_on(id, sc.ctx) {
                style::ON
            } else {
                style::DEFAULT
            };
            match &mut n.w {
                Some(w) => approach(w, target, k),
                None => n.w = Some(style::one_hot(target)),
            }
            n.hv += ((if hot { 1.0 } else { 0.0 }) - n.hv) * k;
        }
        self.hub_p = if sc.active {
            (self.hub_p + appear).min(1.0)
        } else {
            (self.hub_p - vanish).max(0.0)
        };
        let target = if sc.hot.is_none() && sc.active {
            style::OPEN
        } else {
            style::DEFAULT
        };
        match &mut self.hub_w {
            Some(w) => approach(w, target, k),
            None => self.hub_w = Some(style::one_hot(target)),
        }
    }
}

/// Кнопка видна: основная, либо пункт раскрытого подменю.
pub fn is_visible(menu: &Menu, id: NodeId, open: Option<NodeId>) -> bool {
    match menu.node(id).parent {
        Some(ROOT) => true,
        Some(p) => open == Some(p),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::default_menu;

    #[test]
    fn top_buttons_appear_in_turn_and_fully() {
        let menu = default_menu();
        let ctx = Context::default();
        let mut a = Anim::new(&menu);
        let sc = Scene {
            menu: &menu,
            ctx: &ctx,
            active: true,
            open: None,
            hot: None,
        };
        a.step(0.016, &sc);
        let t = menu.top();
        assert!(a.nodes[t[0]].p > a.nodes[t[1]].p);
        for _ in 0..60 {
            a.step(0.016, &sc);
        }
        assert!(t.iter().all(|&i| (a.nodes[i].p - 1.0).abs() < 1e-6));
        // подпункты закрытого подменю не видны
        assert!(menu.children(t[0]).iter().all(|&i| a.nodes[i].p == 0.0));
    }

    #[test]
    fn hover_ramps_up_and_colour_follows() {
        let menu = default_menu();
        let ctx = Context::default();
        let mut a = Anim::new(&menu);
        let hot = menu.top()[1]; // «Вставить» с действием
        let sc = Scene {
            menu: &menu,
            ctx: &ctx,
            active: true,
            open: None,
            hot: Some(hot),
        };
        for _ in 0..60 {
            a.step(0.016, &sc);
        }
        assert!(a.nodes[hot].hv > 0.95);
        assert!(a.nodes[hot].w.unwrap()[style::HOVER] > 0.95);
    }

    #[test]
    fn media_state_turns_green() {
        let menu = default_menu();
        let ctx = Context {
            media_playing: true,
        };
        let mut a = Anim::new(&menu);
        let sc = Scene {
            menu: &menu,
            ctx: &ctx,
            active: true,
            open: None,
            hot: None,
        };
        a.step(0.016, &sc);
        assert_eq!(a.nodes[menu.top()[2]].w.unwrap()[style::ON], 1.0);
    }
}
