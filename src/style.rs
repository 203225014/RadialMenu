//! Стили состояний кнопки (из макетов default / intermediate / hover + своё «on»).
//!
//!   default  заливка #CCCCCC 70%, обводка белая,   иконка #101010
//!   open     заливка #4D5E80 75%, обводка #B3CCFF, иконка белая  (блок с открытым подменю)
//!   hover    заливка #699BFF 80%, обводка #B3CCFF, иконка белая
//!   on       заливка #4D805E 75%, обводка #B3FFCC, иконка белая  («Медиа» играет)

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

const LOOKS: [Look; 4] = [
    Look {
        fill: rgb(204, 204, 204),
        fill_a: 0.70,
        stroke: rgb(255, 255, 255),
        icon: rgb(16, 16, 16),
    },
    Look {
        fill: rgb(77, 94, 128),
        fill_a: 0.75,
        stroke: rgb(179, 204, 255),
        icon: rgb(255, 255, 255),
    },
    Look {
        fill: rgb(105, 155, 255),
        fill_a: 0.80,
        stroke: rgb(179, 204, 255),
        icon: rgb(255, 255, 255),
    },
    Look {
        fill: rgb(77, 128, 94),
        fill_a: 0.75,
        stroke: rgb(179, 255, 204),
        icon: rgb(255, 255, 255),
    },
];

pub fn one_hot(state: usize) -> Weights {
    let mut w = [0.0; 4];
    w[state] = 1.0;
    w
}

/// Смесь стилей по весам; пренебрежимо малые веса отбрасываются.
pub fn mix(w: &Weights) -> Look {
    let used: Vec<(usize, f32)> = w
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, v)| v > 0.01)
        .collect();
    let total: f32 = used.iter().map(|&(_, v)| v).sum();
    if used.is_empty() || total <= 0.0 {
        return LOOKS[DEFAULT];
    }
    let mut out = Look {
        fill: [0.0; 3],
        fill_a: 0.0,
        stroke: [0.0; 3],
        icon: [0.0; 3],
    };
    for (i, v) in used {
        let k = v / total;
        let l = &LOOKS[i];
        for c in 0..3 {
            out.fill[c] += l.fill[c] * k;
            out.stroke[c] += l.stroke[c] * k;
            out.icon[c] += l.icon[c] * k;
        }
        out.fill_a += l.fill_a * k;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hot_is_exact() {
        let l = mix(&one_hot(HOVER));
        assert!((l.fill_a - 0.80).abs() < 1e-6);
        assert!((l.fill[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn blend_is_between() {
        let l = mix(&[0.5, 0.0, 0.5, 0.0]);
        assert!((l.fill_a - 0.75).abs() < 1e-6);
    }

    #[test]
    fn empty_weights_fall_back_to_default() {
        assert!((mix(&[0.0; 4]).fill_a - 0.70).abs() < 1e-6);
    }
}
