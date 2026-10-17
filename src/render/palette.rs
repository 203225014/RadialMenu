//! Цвета: стили состояний кнопки меню, плашки подписей и палитра окон — для каждой темы.
//!
//! Тёмная тема (как в макетах):
//!   default  заливка #CCCCCC 70%, обводка белая,   иконка #101010
//!   open     заливка #4D5E80 75%, обводка #B3CCFF, иконка белая  (блок с открытым подменю)
//!   hover    заливка #699BFF 80%, обводка #B3CCFF, иконка белая
//!   on       заливка #4D805E 75%, обводка #B3FFCC, иконка белая  («Медиа» играет)
//! Светлая — те же роли на белом «стекле» с тёмными иконками.

use crate::model::Theme;

pub const DEFAULT: usize = 0;
pub const OPEN: usize = 1;
pub const HOVER: usize = 2;
pub const ON: usize = 3;

/// Веса состояний (для плавного перетекания цвета).
pub type Weights = [f32; 4];

/// Результирующие цвета кнопки (каналы 0..1).
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub fill: [f32; 3],
    pub fill_a: f32,
    pub stroke: [f32; 3],
    pub icon: [f32; 3],
}

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

const fn look(fill: [f32; 3], fill_a: f32, stroke: [f32; 3], icon: [f32; 3]) -> Look {
    Look {
        fill,
        fill_a,
        stroke,
        icon,
    }
}

const DARK: [Look; 4] = [
    look(rgb(204, 204, 204), 0.70, rgb(255, 255, 255), rgb(16, 16, 16)),
    look(rgb(77, 94, 128), 0.75, rgb(179, 204, 255), rgb(255, 255, 255)),
    look(rgb(105, 155, 255), 0.80, rgb(179, 204, 255), rgb(255, 255, 255)),
    look(rgb(77, 128, 94), 0.75, rgb(179, 255, 204), rgb(255, 255, 255)),
];

const LIGHT: [Look; 4] = [
    look(rgb(255, 255, 255), 0.86, rgb(196, 205, 222), rgb(30, 34, 46)),
    look(rgb(214, 226, 250), 0.92, rgb(105, 155, 255), rgb(43, 76, 140)),
    look(rgb(105, 155, 255), 0.92, rgb(255, 255, 255), rgb(255, 255, 255)),
    look(rgb(208, 238, 219), 0.92, rgb(77, 160, 110), rgb(28, 98, 58)),
];

fn looks(theme: Theme) -> &'static [Look; 4] {
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

pub fn one_hot(state: usize) -> Weights {
    let mut w = [0.0; 4];
    w[state] = 1.0;
    w
}

/// Подсветка своим цветом (`color:` у кнопки): заливка — этот цвет, иконка — белая
/// или тёмная, смотря что контрастнее.
fn accent_look(hex: u32) -> Look {
    let c = [
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
    ];
    let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let lighter = c.map(|v| v + (1.0 - v) * 0.55);
    let icon = if luma > 0.6 { rgb(24, 28, 38) } else { rgb(255, 255, 255) };
    look(c, 0.88, lighter, icon)
}

/// Смесь стилей по весам; пренебрежимо малые веса отбрасываются.
/// `accent` — свой цвет кнопки: заменяет синий цвет наведения.
pub fn mix(w: &Weights, theme: Theme, accent: Option<u32>) -> Look {
    let mut table = *looks(theme);
    if let Some(a) = accent {
        table[HOVER] = accent_look(a);
    }
    let used: Vec<(usize, f32)> = w
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, v)| v > 0.01)
        .collect();
    let total: f32 = used.iter().map(|&(_, v)| v).sum();
    if used.is_empty() || total <= 0.0 {
        return table[DEFAULT];
    }
    let mut out = look([0.0; 3], 0.0, [0.0; 3], [0.0; 3]);
    for (i, v) in used {
        let k = v / total;
        let l = &table[i];
        for c in 0..3 {
            out.fill[c] += l.fill[c] * k;
            out.stroke[c] += l.stroke[c] * k;
            out.icon[c] += l.icon[c] * k;
        }
        out.fill_a += l.fill_a * k;
    }
    out
}

/// Цвета плашки с подписью (RGBA, 0..255; альфа не предумножена).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PillColors {
    pub bg: [u8; 4],
    pub border: [u8; 4],
    pub text: [u8; 3],
}

pub fn pill(theme: Theme) -> PillColors {
    match theme {
        Theme::Dark => PillColors {
            bg: [16, 16, 16, 185],
            border: [255, 255, 255, 70],
            text: [255, 255, 255],
        },
        Theme::Light => PillColors {
            bg: [250, 251, 253, 228],
            border: [20, 30, 60, 46],
            text: [24, 28, 38],
        },
    }
}

/// Палитра окон (заметка, настройки), цвета 0xRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ui {
    /// Фон окна.
    pub bg: u32,
    /// Фон полей, списков, неактивных кнопок.
    pub bg2: u32,
    /// Рамка окна и полей.
    pub edge: u32,
    /// Основной текст.
    pub fg: u32,
    /// Вторичный текст.
    pub dim: u32,
    /// Акцент (выбранное, ползунки).
    pub acc: u32,
    /// Текст поверх акцента.
    pub on_acc: u32,
    pub ok: u32,
    pub bad: u32,
}

pub fn ui(theme: Theme) -> Ui {
    match theme {
        Theme::Dark => Ui {
            bg: 0x1b1f2b,
            bg2: 0x262c3d,
            edge: 0x8fa6d6,
            fg: 0xffffff,
            dim: 0x8791a8,
            acc: 0x699bff,
            on_acc: 0xffffff,
            ok: 0x7fe0a3,
            bad: 0xff8a8a,
        },
        Theme::Light => Ui {
            bg: 0xf6f7fb,
            bg2: 0xe7ebf3,
            edge: 0xb3c0dc,
            fg: 0x1b1f2b,
            dim: 0x687189,
            acc: 0x4d7fff,
            on_acc: 0xffffff,
            ok: 0x1f9d55,
            bad: 0xd64545,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hot_is_exact() {
        let l = mix(&one_hot(HOVER), Theme::Dark, None);
        assert!((l.fill_a - 0.80).abs() < 1e-6);
        assert!((l.fill[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn blend_is_between() {
        let l = mix(&[0.5, 0.0, 0.5, 0.0], Theme::Dark, None);
        assert!((l.fill_a - 0.75).abs() < 1e-6);
    }

    #[test]
    fn empty_weights_fall_back_to_default() {
        assert!((mix(&[0.0; 4], Theme::Dark, None).fill_a - 0.70).abs() < 1e-6);
    }

    /// Контраст иконки с заливкой — по яркости, в обеих темах и во всех состояниях.
    #[test]
    fn icons_stand_out_in_both_themes() {
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for theme in Theme::ALL {
            for state in [DEFAULT, OPEN, HOVER, ON] {
                let l = mix(&one_hot(state), theme, None);
                let d = (luma(l.icon) - luma(l.fill)).abs();
                assert!(d > 0.3, "{theme:?}/{state}: {d}");
            }
            let p = pill(theme);
            let bg = [p.bg[0], p.bg[1], p.bg[2]].map(|v| v as f32 / 255.0);
            let fg = p.text.map(|v| v as f32 / 255.0);
            assert!((luma(fg) - luma(bg)).abs() > 0.6, "{theme:?}: плашка");
        }
    }

    #[test]
    fn custom_colour_replaces_hover_only() {
        let red = mix(&one_hot(HOVER), Theme::Dark, Some(0xE5484D));
        assert!(red.fill[0] > 0.85 && red.fill[2] < 0.35);
        assert_eq!(red.icon, rgb(255, 255, 255));
        let yellow = mix(&one_hot(HOVER), Theme::Light, Some(0xFFE066));
        assert_eq!(yellow.icon, rgb(24, 28, 38), "на светлом цвете иконка тёмная");
        let idle = mix(&one_hot(DEFAULT), Theme::Dark, Some(0xE5484D));
        assert!((idle.fill_a - 0.70).abs() < 1e-6, "в покое кнопка обычная");
    }

    #[test]
    fn themes_differ() {
        assert_ne!(ui(Theme::Light), ui(Theme::Dark));
        assert_ne!(pill(Theme::Light), pill(Theme::Dark));
    }
}
