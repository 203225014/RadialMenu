//! Лёгкое размытие картинки (для «стекла» под кнопками): прогоны скользящего среднего по
//! строкам и столбцам; два прогона дают гладкое, почти гауссово размытие.

/// Размыть BGRA/RGBA-буфер `w`×`h` (все четыре канала) с радиусом `r` px, `passes` раз.
pub fn box_blur(px: &mut [u8], w: usize, h: usize, r: usize, passes: usize) {
    if r == 0 || w == 0 || h == 0 || px.len() < w * h * 4 {
        return;
    }
    let mut tmp = vec![0u8; w * h * 4];
    for _ in 0..passes {
        for y in 0..h {
            line(px, &mut tmp, y * w * 4, 4, w, r);
        }
        for x in 0..w {
            line(&tmp, px, x * 4, w * 4, h, r);
        }
    }
}

/// Одна линия из `len` пикселей: начало `base`, шаг между пикселями `stride` байт.
fn line(src: &[u8], dst: &mut [u8], base: usize, stride: usize, len: usize, r: usize) {
    let last = len as isize - 1;
    let r = r as isize;
    let div = (2 * r + 1) as u32;
    for c in 0..4 {
        let at = |i: isize| src[base + i.clamp(0, last) as usize * stride + c] as u32;
        let mut sum: u32 = (-r..=r).map(&at).sum();
        for i in 0..len as isize {
            dst[base + i as usize * stride + c] = ((sum + div / 2) / div) as u8;
            sum = sum + at(i + r + 1) - at(i - r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_stays_flat_and_a_dot_spreads() {
        let (w, h) = (40, 30);
        let mut flat = vec![120u8; w * h * 4];
        box_blur(&mut flat, w, h, 4, 2);
        assert!(flat.iter().all(|&v| v == 120));

        let mut dot = vec![0u8; w * h * 4];
        for c in 0..4 {
            dot[(15 * w + 20) * 4 + c] = 255;
        }
        box_blur(&mut dot, w, h, 3, 2);
        let at = |x: usize, y: usize| dot[(y * w + x) * 4];
        assert!(at(20, 15) < 255 && at(20, 15) > 0, "центр размыт");
        assert!(at(23, 15) > 0 && at(27, 15) == 0, "и только в пределах радиуса");
        assert_eq!(at(21, 15), at(19, 15), "симметрично");
    }

    #[test]
    fn zero_radius_and_empty_are_noops() {
        let mut px = vec![1u8, 2, 3, 4];
        box_blur(&mut px, 1, 1, 0, 2);
        assert_eq!(px, [1, 2, 3, 4]);
        box_blur(&mut [], 0, 0, 3, 1);
    }
}
