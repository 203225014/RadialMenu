//! Журнал для разбора проблем: `%APPDATA%\RadialMenu\RadialMenu.log` (нет APPDATA — во временной
//! папке). Пишутся запуск, ошибки и аварии (паники с трассировкой). Файл не растёт бесконечно:
//! когда он больше `MAX_BYTES`, прежний уезжает в `RadialMenu.old.log`.
//! Без ОС-вызовов, поэтому проверяется тестами на любой платформе.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 256 * 1024;

pub fn log_path() -> PathBuf {
    let dir = std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("RadialMenu"))
        .filter(|d| fs::create_dir_all(d).is_ok())
        .unwrap_or_else(std::env::temp_dir);
    dir.join("RadialMenu.log")
}

/// Дата и время по Гринвичу: `2026-10-18 12:34:56`.
pub fn fmt_utc(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // гражданская дата из числа дней от 1970-01-01 (алгоритм Говарда Хиннанта)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn rotate_if_big(path: &PathBuf) {
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(path, path.with_file_name("RadialMenu.old.log"));
    }
}

/// Строка в журнал (ошибки записи молча игнорируются: журналу некуда жаловаться).
pub fn log(text: &str) {
    let path = log_path();
    rotate_if_big(&path);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        if f.metadata().is_ok_and(|m| m.len() == 0) {
            let _ = f.write_all(b"ï»¿"); // BOM: Блокнот и PowerShell сразу читают русский текст
        }
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let _ = writeln!(f, "[{} UTC] {text}", fmt_utc(secs));
    }
}

/// Первая строка сессии: версия и откуда запущено.
pub fn log_start() {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    log(&format!("запуск: версия {}, {exe}", env!("CARGO_PKG_VERSION")));
}

/// Паника пишется в журнал с трассировкой, затем `on_fatal` (окно «программа завершилась»).
/// Сборка идёт с `panic = abort`, поэтому после хука процесс закроется.
pub fn install_panic_hook(on_fatal: fn(&str)) {
    std::panic::set_hook(Box::new(move |info| {
        let trace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        log(&format!(
            "АВАРИЯ в потоке «{}», версия {}: {info}\n{trace}",
            thread.name().unwrap_or("?"),
            env!("CARGO_PKG_VERSION")
        ));
        on_fatal(&log_path().display().to_string());
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(fmt_utc(0), "1970-01-01 00:00:00");
        assert_eq!(fmt_utc(951_782_400), "2000-02-29 00:00:00"); // високосный день
        assert_eq!(fmt_utc(1_700_000_000), "2023-11-14 22:13:20");
        assert_eq!(fmt_utc(1_792_000_000), "2026-10-14 17:46:40");
    }

    #[test]
    fn big_log_is_rotated() {
        let dir = std::env::temp_dir().join(format!("radial_menu_diag_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("RadialMenu.log");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        rotate_if_big(&path);
        assert!(!path.exists() && dir.join("RadialMenu.old.log").exists());
        fs::write(&path, b"small").unwrap();
        rotate_if_big(&path);
        assert!(path.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
