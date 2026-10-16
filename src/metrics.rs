//! Размеры меню в пикселях: базовые значения из макета × масштаб (DPI, настройки),
//! вписанные в экран, если он маленький.

use crate::config::Settings;

pub const FAN_SPREAD: f32 = 60.0; // угол между кнопками подменю в веере, °
pub const HYST: f32 = 25.0; // гистерезис: с подпункта курсор уходит на столько px глубже
pub const HOVER_GROW: f32 = 0.08; // на сколько кнопка вырастает при наведении
pub const APPEAR_T: f64 = 0.24; // сек: появление кнопки
pub const FADE_IN: f64 = 0.14; // сек: проявление окна
pub const FADE_OUT: f64 = 0.12; // сек: закрытие
pub const COLOR_T: f64 = 0.07; // сек: сглаживание цвета
pub const STUCK_T: f64 = 20.0; // сек: предохранитель «меню зависло открытым»
pub const TOAST_T: f64 = 0.9; // сек: подпись громкости

const MAIN_D: f32 = 117.0; // от центра до основных кнопок
const FAN_D: f32 = 116.0; // от основной кнопки до кнопок её подменю
const R_OUT: f32 = 156.0; // дальше этого радиуса выбираются кнопки подменю
const RMAX: f32 = 168.0; // указатель не уходит дальше (подменю закрыто)
const RMAX_SUB: f32 = 246.0; // то же, когда подменю открыто
const DEAD: f32 = 34.0; // мёртвая зона в центре
const CIRCLE_R: f32 = 32.0; // радиус кнопки
const HUB_R: f32 = 17.0; // радиус центральной кнопки
const SENS: f32 = 0.5; // чувствительность мыши внутри меню
const MARGIN: f32 = 24.0;

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub main_d: f32,
    pub fan_d: f32,
    pub r_out: f32,
    pub rmax: f32,
    pub rmax_sub: f32,
    pub dead: f32,
    pub circle_r: f32,
    pub hub_r: f32,
    pub sens: f32,
    /// Задержка раскрытия подменю, сек.
    pub dwell: f64,
}

impl Metrics {
    /// `ui_scale` — масштаб DPI (1.0 = 96 dpi), `screen` — размер экрана, куда надо вписаться.
    pub fn new(s: &Settings, ui_scale: f32, screen: (i32, i32)) -> Metrics {
        let k0 = ui_scale * s.size_percent as f32 / 100.0;
        let mut m = Metrics {
            main_d: MAIN_D * k0,
            fan_d: FAN_D * k0,
            r_out: R_OUT * k0,
            rmax: RMAX * k0,
            rmax_sub: RMAX_SUB * k0,
            dead: DEAD * k0,
            circle_r: CIRCLE_R * k0,
            hub_r: HUB_R * k0,
            sens: SENS * ui_scale * s.sens_percent as f32 / 100.0,
            dwell: s.dwell_ms as f64 / 1000.0,
        };
        let small = screen.0.min(screen.1) as f32;
        let k =
            ((small / 2.0 - MARGIN) / (m.main_d + m.fan_d + m.circle_r * 1.2 + MARGIN)).min(1.0);
        if k > 0.0 {
            for v in [
                &mut m.main_d,
                &mut m.fan_d,
                &mut m.r_out,
                &mut m.rmax,
                &mut m.rmax_sub,
                &mut m.dead,
                &mut m.circle_r,
                &mut m.hub_r,
            ] {
                *v *= k;
            }
        }
        m
    }

    /// Половина стороны окна меню (окно — квадрат вокруг центра).
    pub fn half(&self) -> i32 {
        (self.main_d + self.fan_d + self.circle_r * (1.0 + HOVER_GROW) * 1.2 + MARGIN) as i32
    }

    /// Размер окна, достаточный для меню без вписывания в экран.
    pub fn window_half(s: &Settings, ui_scale: f32) -> i32 {
        Metrics::new(s, ui_scale, (i32::MAX / 2, i32::MAX / 2)).half()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_original_at_100() {
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080));
        assert_eq!(m.half(), (117.0 + 116.0 + 32.0 * 1.08 * 1.2 + 24.0) as i32);
        assert!((m.sens - 0.5).abs() < 1e-6);
    }

    #[test]
    fn shrinks_on_small_screen_and_window_fits() {
        let s = Settings::default();
        let small = Metrics::new(&s, 1.0, (500, 400));
        let big = Metrics::new(&s, 1.0, (1920, 1080));
        assert!(small.main_d < big.main_d);
        assert!(small.half() <= Metrics::window_half(&s, 1.0));
    }
}
