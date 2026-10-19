//! Размеры меню в пикселях. Меню — концентрические кольца: в первом основные кнопки-сектора,
//! в каждом следующем веер кнопки внутреннего кольца. Все размеры заданы в «макетных» px
//! (100 %), затем умножаются на масштаб (DPI, настройки) и вписываются в экран.
//!
//! Макет (от центра): кружок 16, пустое поле, кольцо 124–176 (основные), зазор 32, кольцо
//! 208–260 (веер), зазор 32, кольцо 292–344 (вложенный веер). Толщина кольца 52, зазор между
//! соседними кнопками в кольце 16.

use crate::model::{Menu, Settings};

pub const HYST: f32 = 25.0; // гистерезис: с подпункта курсор уходит на столько px глубже
pub const HOVER_GROW: f32 = 0.08; // на сколько кнопка вырастает при наведении
pub const APPEAR_T: f64 = 0.24; // сек: появление кнопки
pub const FADE_IN: f64 = 0.14; // сек: проявление окна
pub const FADE_OUT: f64 = 0.12; // сек: закрытие
pub const COLOR_T: f64 = 0.07; // сек: сглаживание цвета
pub const STUCK_T: f64 = 20.0; // сек: предохранитель «меню зависло открытым»
pub const TOAST_T: f64 = 0.9; // сек: подпись громкости

const H: f32 = 26.0; // половина толщины кольца
const GAP: f32 = 16.0; // зазор между соседними кнопками одного кольца
const RING_GAP: f32 = 32.0; // зазор между кольцами
const R1_MIN: f32 = 150.0; // радиус оси первого кольца (не меньше)
const HUB_R: f32 = 16.0; // радиус центрального кружка
const DEAD: f32 = 56.0; // мёртвая зона в центре
const SENS: f32 = 0.5; // чувствительность мыши внутри меню
const MARGIN: f32 = 24.0;
const RMAX_PAD: f32 = 12.0; // указатель уходит за край раскрытого кольца не дальше, чем на столько

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Половина толщины кольца.
    pub h: f32,
    /// Зазор между соседними кнопками кольца.
    pub gap: f32,
    /// Зазор между кольцами.
    pub ring_gap: f32,
    /// Радиус оси первого кольца.
    pub r1: f32,
    /// Колец в меню (1 — только основные).
    pub levels: usize,
    pub dead: f32,
    pub hub_r: f32,
    pub sens: f32,
    /// Задержка раскрытия веера, сек.
    pub dwell: f64,
}

/// Угол (°), на который смещены центры двух соседних кнопок кольца радиуса `r`, чтобы между
/// ними остался зазор `gap` (кнопки — полосы толщины 2h с круглыми концами).
pub fn pitch_deg(r: f32, h: f32, gap: f32) -> f32 {
    (2.0 * ((2.0 * h + gap) / (2.0 * r)).clamp(0.0, 1.0).asin()).to_degrees()
}

impl Metrics {
    /// `ui_scale` — масштаб DPI (1.0 = 96 dpi), `screen` — размер экрана, куда надо вписаться.
    pub fn new(s: &Settings, ui_scale: f32, screen: (i32, i32), menu: &Menu) -> Metrics {
        let k0 = ui_scale * s.size_percent as f32 / 100.0;
        let top = menu.top().len().max(1);
        // кольцо раздвигается, если основных кнопок много: между центрами соседних — хотя бы 2h+gap
        let r1 = if top > 1 {
            R1_MIN.max((2.0 * H + GAP) / (2.0 * (std::f32::consts::PI / top as f32).sin()))
        } else {
            R1_MIN
        };
        let mut m = Metrics {
            h: H * k0,
            gap: GAP * k0,
            ring_gap: RING_GAP * k0,
            r1: r1 * k0,
            levels: menu.depth().max(1),
            dead: DEAD * k0,
            hub_r: HUB_R * k0,
            sens: SENS * ui_scale * s.sens_percent as f32 / 100.0,
            dwell: s.dwell_ms as f64 / 1000.0,
        };
        let small = screen.0.min(screen.1) as f32;
        let k = ((small / 2.0 - MARGIN) / m.extent()).min(1.0);
        if k > 0.0 {
            for v in [&mut m.h, &mut m.gap, &mut m.ring_gap, &mut m.r1, &mut m.dead, &mut m.hub_r] {
                *v *= k;
            }
        }
        m
    }

    /// Расстояние между осями соседних колец.
    pub fn step(&self) -> f32 {
        2.0 * self.h + self.ring_gap
    }

    /// Радиус оси кольца номер `level` (0 — основные).
    pub fn ring(&self, level: usize) -> f32 {
        self.r1 + self.step() * level as f32
    }

    /// Радиус, с которого выбираются кнопки кольца `level` (≥ 1): середина зазора перед ним.
    pub fn enter(&self, level: usize) -> f32 {
        self.ring(level - 1) + self.h + self.ring_gap / 2.0
    }

    /// Дальше этого радиуса невидимый указатель не уходит; `open` — кольцо раскрытого веера.
    pub fn limit(&self, open: Option<usize>) -> f32 {
        match open {
            None => self.enter(1) + RMAX_PAD,
            Some(level) => self.ring(level) + self.h + RMAX_PAD,
        }
    }

    /// Внешний край кольца веера плеера (первый веер; без вееров — основного кольца): ниже него
    /// карточка трека ничему не мешает и стоит на месте.
    pub fn card_edge(&self) -> f32 {
        self.ring(1.min(self.levels - 1)) + self.h
    }

    /// От центра до внешнего края самого дальнего кольца (с запасом на рост при наведении).
    pub fn reach(&self) -> f32 {
        self.ring(self.levels - 1) + self.h * (1.0 + HOVER_GROW)
    }

    /// Дальний край того, что рисуется: кольца или (внизу) карточка трека — отступ как между кольцами и высота 54 при h = 26.
    fn extent(&self) -> f32 {
        self.reach().max(self.card_edge() + self.ring_gap + self.h * (54.0 / H))
    }

    /// Половина стороны окна меню (окно — квадрат вокруг центра).
    pub fn half(&self) -> i32 {
        (self.extent() + MARGIN) as i32
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
    fn rings_follow_the_layout_sketch() {
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &default_menu());
        // кольца 124–176 и 208–260 (макет), толщина 52, зазор 32
        assert!((m.ring(0) - m.h - 124.0).abs() < 1e-3 && (m.ring(0) + m.h - 176.0).abs() < 1e-3);
        assert!((m.ring(1) - m.h - 208.0).abs() < 1e-3 && (m.ring(1) + m.h - 260.0).abs() < 1e-3);
        assert!((m.enter(1) - 192.0).abs() < 1e-3); // середина зазора между 176 и 208
        assert!((m.sens - 0.5).abs() < 1e-6);
        assert_eq!(m.levels, 2);
        assert_eq!(m.half(), (260.0 + 32.0 + 54.0 + 24.0) as i32); // с местом под карточку трека
    }

    #[test]
    fn pitch_keeps_the_gap() {
        // центры двух кнопок, разведённые на pitch, отстоят на 2h + gap — между краями ровно gap
        let (r, h, gap) = (150.0f32, 26.0, 16.0);
        let a = pitch_deg(r, h, gap).to_radians();
        let chord = 2.0 * r * (a / 2.0).sin();
        assert!((chord - (2.0 * h + gap)).abs() < 1e-3);
    }

    #[test]
    fn scales_with_ui_and_fits_small_screens() {
        let big = Metrics::new(&Settings::default(), 2.0, (3840, 2160), &default_menu());
        let one = Metrics::new(&Settings::default(), 1.0, (3840, 2160), &default_menu());
        assert!((big.h - 2.0 * one.h).abs() < 1e-3 && (big.ring(1) - 2.0 * one.ring(1)).abs() < 1e-3);
        let tiny = Metrics::new(&Settings::default(), 1.0, (500, 400), &default_menu());
        assert!(tiny.half() as f32 <= 200.0 + 1.0, "{}", tiny.half());
    }

    #[test]
    fn many_buttons_spread_the_ring() {
        let few = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu(4, 1));
        let many = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu(12, 1));
        assert!(many.r1 >= few.r1);
        // у 12 кнопок между осями соседних хотя бы 2h + gap
        let chord = 2.0 * many.r1 * (std::f32::consts::PI / 12.0).sin();
        assert!(chord >= 2.0 * many.h + many.gap - 1e-3);
    }

    #[test]
    fn limits_grow_with_open_rings() {
        let m = Metrics::new(&Settings::default(), 1.0, (1920, 1080), &menu(4, 2));
        assert!(m.limit(None) > m.enter(1) && m.limit(Some(1)) > m.limit(None));
    }
}
