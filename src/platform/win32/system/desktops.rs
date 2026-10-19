//! Рабочие столы Windows: какой по счёту текущий и сколько их всего. Публичного API для этого нет,
//! но Проводник держит оба сведения в реестре: список столов (`VirtualDesktopIDs` — подряд идущие
//! 16-байтные идентификаторы) и текущий (`CurrentVirtualDesktop`).

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY};

const KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops");

fn read(name: PCWSTR) -> Option<Vec<u8>> {
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, KEY, name, RRF_RT_REG_BINARY, None, None, Some(&mut size)).ok().ok()?;
        let mut buf = vec![0u8; size as usize];
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        buf.truncate(size as usize);
        Some(buf)
    }
}

/// Номер текущего стола (с нуля) и число столов по содержимому двух значений реестра.
pub fn position(ids: &[u8], current: &[u8]) -> Option<(usize, usize)> {
    if ids.is_empty() || !ids.len().is_multiple_of(16) || current.len() < 16 {
        return None;
    }
    let index = ids.chunks_exact(16).position(|id| id == &current[..16])?;
    Some((index, ids.len() / 16))
}

/// Есть ли стол слева и справа от текущего. Если узнать не удалось — считаем, что есть оба
/// (лучше нажать впустую, чем не пустить).
pub fn neighbours() -> (bool, bool) {
    match read(w!("VirtualDesktopIDs")).zip(read(w!("CurrentVirtualDesktop"))) {
        Some((ids, cur)) => match position(&ids, &cur) {
            Some((i, n)) => (i > 0, i + 1 < n),
            None => (true, true),
        },
        None => (true, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guid(n: u8) -> [u8; 16] {
        [n; 16]
    }

    #[test]
    fn position_is_found_by_id() {
        let ids: Vec<u8> = (1..=3).flat_map(guid).collect();
        assert_eq!(position(&ids, &guid(1)), Some((0, 3)));
        assert_eq!(position(&ids, &guid(3)), Some((2, 3)));
        assert_eq!(position(&ids, &guid(9)), None);
        assert_eq!(position(&[], &guid(1)), None);
        assert_eq!(position(&ids[..20], &guid(1)), None, "обрезанные данные");
    }

    /// Живое чтение: `cargo test live_desktops -- --ignored --nocapture`.
    #[test]
    #[ignore = "нужен Проводник"]
    fn live_desktops() {
        eprintln!("столы слева/справа: {:?}", neighbours());
    }
}
