//! Геометрия: полярные координаты и угловые сектора (углы в градусах, ось Y вниз).

/// Наименьшая разница углов, 0..=180.
pub fn ang_diff(a: f32, b: f32) -> f32 {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs()
}

/// (радиус, угол 0..360) точки относительно центра.
pub fn polar(x: f32, y: f32) -> (f32, f32) {
    (x.hypot(y), y.atan2(x).to_degrees().rem_euclid(360.0))
}

/// Угловой сектор, по которому выбирается кнопка.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sector {
    pub a1: f32,
    pub a2: f32,
}

impl Sector {
    pub fn mid(&self) -> f32 {
        (self.a1 + self.a2) * 0.5
    }

    pub fn half(&self) -> f32 {
        (self.a2 - self.a1) * 0.5
    }

    /// На сколько градусов угол `phi` выходит за сектор (0 — внутри).
    pub fn overshoot(&self, phi: f32) -> f32 {
        (ang_diff(phi, self.mid()) - self.half()).max(0.0)
    }
}

/// Плавный выход с лёгким «пружинным» перелётом.
pub fn ease_back(p: f32) -> f32 {
    const C1: f32 = 1.1;
    const C3: f32 = C1 + 1.0;
    let p = p.clamp(0.0, 1.0) - 1.0;
    1.0 + C3 * p * p * p + C1 * p * p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_wraps() {
        assert!((ang_diff(350.0, 10.0) - 20.0).abs() < 1e-4);
        assert!((ang_diff(0.0, 180.0) - 180.0).abs() < 1e-4);
    }

    #[test]
    fn polar_axes() {
        let (r, a) = polar(0.0, -10.0);
        assert!((r - 10.0).abs() < 1e-4 && (a - 270.0).abs() < 1e-3);
        let (_, a) = polar(5.0, 0.0);
        assert!(a.abs() < 1e-4);
    }

    #[test]
    fn ease_ends() {
        assert!(ease_back(0.0).abs() < 1e-6);
        assert!((ease_back(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_back(0.8) > 1.0); // перелёт
    }
}
