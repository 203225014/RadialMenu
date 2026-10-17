//! Размеры меню в пикселях: базовые значения из макета × масштаб (DPI, настройки),
//! раздвинутые под число кнопок и вписанные в экран, если он маленький.

use crate::model::{Menu, Settings};

pub const HYST: f32 = 25.0; // гистерезис: с подпункта курсор уходит на столько px глубже
pub const HOVER_GROW: f32 = 0.08; // на сколько кнопка вырастает при наведении
pub const APPEAR_T: f64 = 0.24; // сек: появление кнопки
pub const FADE_IN: f64 = 0.14; // сек: проявление окна
pub const FADE_OUT: f64 = 0.12; // сек: закрытие
pub const COLOR_T: f64 = 0.07; // сек: сглаживание цвета
pub const STUCK_T: f64 = 20.0; // сек: предохранитель «меню зависло открытым»
pub const TOAST_T: f64 = 0.9; // сек: подпись громкости

const MAIN_D: f32 = 117.0; // от центра до основных кнопок (не меньше)
const FAN_D: f32 = 116.0; // от основной кнопки до кнопок её подменю (не меньше)
const R_OUT: f32 = 39.0; // дальше main_d + R_OUT выбираются кнопки подменю
const RMAX: f32 = 51.0; // указатель не уходит дальше main_d + RMAX (подменю закрыто)
const RMAX_SUB: f32 = 13.0; // … и дальше main_d + fan_d + RMAX_SUB, когда открыто
const DEAD: f32 = 34.0; // мёртвая зона в центре
const CIRCLE_R: f32 = 32.0; // радиус кнопки
const HUB_R: f32 = 17.0; // радиус центральной кнопки
const SENS: f32 = 0.5; // чувствительность мыши внутри меню
const MARGIN: f32 = 24.0;
const GAP: f32 = 10.0; // зазор между соседними кнопками
/// Самый широкий веер, ° (шире — пункты заходят за соседние основные кнопки).
const FAN_MAX_SPAN: f32 = 150.0;
/// … и вложенного (шире — пункты залезают на соседей по вееру).
const SUB_MAX_SPAN: f32 = 60.0;

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub main_d: f32,
    pub fan_d: f32,
    /// От пункта веера до пунктов его вложенного веера (0, если вложенных нет).
    pub sub_d: f32,
    pub r_out: f32,
    pub rmax: f32,
    pub rmax_sub: f32,
    /// … а когда раскрыт вложенный веер.
    pub rmax_sub2: f32,
    pub dead: f32,
    pub circle_r: f32,
    pub hub_r: f32,
    pub sens: f32,
    /// Задержка раскрытия подменю, сек.
    pub dwell: f64,
    /// Угол между пунктами веера, ° (уже с учётом их числа).
    pub fan_spread: f32,
    /// То же для вложенных вееров.
    pub sub_spread: f32,
}

/// Расстояние до центров, при котором `n` кнопок, стоящих через угол `step`°, не касаются.
fn spaced(n_step_deg: f32) -> f32 {
    let chord = 2.0 * CIRCLE_R * (1.0 + HOVER_GROW) + GAP;
    chord / (2.0 * (n_step_deg.to_radians() / 2.0).sin())
}

impl Metrics {
    /// `ui_scale` — масштаб DPI (1.0 = 96 dpi), `screen` — размер экрана, куда надо вписаться.
    pub fn new(s: &Settings, ui_scale: f32, screen: (i32, i32), menu: &Menu) -> Metrics {
        let k0 = ui_scale * s.size_percent as f32 / 100.0;
        let top = menu.top().len().max(1);
        let kids = menu.max_fan();
        // круг раздвигается, если основных кнопок много
        let main = if top > 1 { MAIN_D.max(spaced(360.0 / top as f32)) } else { MAIN_D };
        // веер сжимается до FAN_MAX_SPAN и отодвигается, чтобы пункты не слипались
        let spread = if kids > 1 {
            menu.shape.fan_spread.min(FAN_MAX_SPAN / (kids - 1) as f32)
        } else {
            menu.shape.fan_spread
        };
        let fan = if kids > 1 { FAN_D.max(spaced(spread)) } else { FAN_D };
        // вложенный веер не должен задевать соседей по вееру: он узкий (≤ ±30°) и отодвинут
        let subs = menu.max_sub();
        let sub_spread = if subs > 1 {
            menu.shape.fan_spread.min(SUB_MAX_SPAN / (subs - 1) as f32)
        } else {
            menu.shape.fan_spread
        };
        let sub = if subs == 0 { 0.0 } else if subs > 1 { FAN_D.max(spaced(sub_spread)) } else { FAN_D };
        let mut m = Metrics {
            main_d: main * k0,
            fan_d: fan * k0,
            sub_d: sub * k0,
            r_out: (main + R_OUT) * k0,
            rmax: (main + RMAX) * k0,
            rmax_sub: (main + fan + RMAX_SUB) * k0,
            rmax_sub2: (main + fan + sub + RMAX_SUB) * k0,
            dead: DEAD * k0,
            circle_r: CIRCLE_R * k0,
            hub_r: HUB_R * k0,
            sens: SENS * ui_scale * s.sens_percent as f32 / 100.0,
            dwell: s.dwell_ms as f64 / 1000.0,
            fan_spread: spread,
            sub_spread,
        };
        let small = screen.0.min(screen.1) as f32;
        let k =
            ((small / 2.0 - MARGIN) / (m.reach() + m.circle_r * 1.2 + MARGIN)).min(1.0);
        if k > 0.0 {
            for v in [
                &mut m.main_d,
                &mut m.fan_d,
                &mut m.sub_d,
                &mut m.r_out,
                &mut m.rmax,
                &mut m.rmax_sub,
                &mut m.rmax_sub2,
                &mut m.dead,
                &mut m.circle_r,
                &mut m.hub_r,
            ] {
                *v *= k;
            }
        }
        m
    }

    /// От центра до самой дальней кнопки самого глубокого веера.
    pub fn reach(&self) -> f32 {
        self.main_d + self.fan_d + self.sub_d
    }

    /// Половина стороны окна меню (окно — квадрат вокруг центра).
    pub fn half(&self) -> i32 {
        (self.reach() + self.circle_r * (1.0 + HOVER_GROW) * 1.2 + MARGIN) as i32
    }

    /// Размер окна, достаточный для меню без вписывания в экран.
    pub fn window_half(s: &Settings, ui_scale: f32, menu: &Menu) -> i32 {
        Metrics::new(s, ui_scale, (i32::MAX / 2, i32::MAX / 2), menu).half()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{default_menu, Action, Item};

    fn menu(top: usize, kids: usize) -> Menu {
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
    fn matches_original_at_100() {
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &default_menu());
        assert_eq!(m.half(), (117.0 + 116.0 + 32.0 * 1.08 * 1.2 + 24.0) as i32);
        assert!((m.sens - 0.5).abs() < 1e-6);
        assert!((m.r_out - 156.0).abs() < 1e-3 && (m.rmax_sub - 246.0).abs() < 1e-3);
    }

    #[test]
    fn shrinks_on_small_screen_and_window_fits() {
        let s = Settings::default();
        let small = Metrics::new(&s, 1.0, (500, 400), &default_menu());
        let big = Metrics::new(&s, 1.0, (1920, 1080), &default_menu());
        assert!(small.main_d < big.main_d);
        assert!(small.half() <= Metrics::window_half(&s, 1.0, &default_menu()));
    }

    #[test]
    fn ring_and_fan_grow_with_count() {
        let s = Settings::default();
        let few = Metrics::new(&s, 1.0, (4000, 4000), &menu(4, 3));
        let many = Metrics::new(&s, 1.0, (4000, 4000), &menu(12, 6));
        assert!(many.main_d > few.main_d && many.fan_d > few.fan_d);
        assert!(many.fan_spread * 5.0 <= FAN_MAX_SPAN + 1e-3);
        // соседние основные кнопки не касаются
        let chord = 2.0 * many.main_d * (std::f32::consts::PI / 12.0).sin();
        assert!(chord >= 2.0 * many.circle_r * (1.0 + HOVER_GROW));
    }
}
