//! Файлы пользователя в `%APPDATA%\RadialMenu`: настройки (`config.txt`), меню (`menu.yaml`)
//! и папка `icons` для своих значков. Создаются при первом запуске; правки замечаются по
//! времени изменения.

use std::path::PathBuf;
use std::time::SystemTime;

use super::util::log_error;
use crate::model::menu_yaml::{self, MenuError};
use crate::model::{default_menu, MenuSet, Settings};
use crate::render::assets;

fn dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("RadialMenu"))
}

pub fn config_path() -> Option<PathBuf> {
    dir().map(|d| d.join("config.txt"))
}

pub fn menu_path() -> Option<PathBuf> {
    dir().map(|d| d.join("menu.yaml"))
}

/// Время изменения файла (`None` — файла нет).
pub fn mtime(p: &Option<PathBuf>) -> Option<SystemTime> {
    p.as_ref()?.metadata().ok()?.modified().ok()
}

fn write_new(p: &PathBuf, text: &str) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Err(e) = std::fs::write(p, text) {
        log_error(&format!("запись {}: {e}", p.display()));
    }
}

/// Настройки из файла; при первом запуске кладём файл со значениями по умолчанию.
/// Параметры командной строки действуют только на этот запуск и в файл не попадают.
pub fn load_settings() -> Settings {
    let path = config_path();
    let mut s = match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => Settings::parse(&text),
        None => {
            let d = Settings::default();
            if let Some(p) = &path {
                write_new(p, &d.to_text());
            }
            d
        }
    };
    s.apply_args(std::env::args().skip(1));
    s
}

pub fn save_settings(s: &Settings) -> Result<(), String> {
    let p = config_path().ok_or("не найдена папка APPDATA")?;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(&p, s.to_text()).map_err(|e| format!("не удалось записать настройки: {e}"))
}

/// Меню из файла. Ошибка в файле не мешает работать: берём меню по умолчанию,
/// а ошибку возвращаем, чтобы её показать.
pub fn load_menu() -> (MenuSet, Option<MenuError>) {
    let Some(path) = ensure_menu_file() else {
        return (MenuSet::single(default_menu()), None);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            let err = MenuError {
                line: 0,
                msg: format!("не удалось прочитать файл: {e}"),
            };
            return (MenuSet::single(default_menu()), Some(err));
        }
    };
    match menu_yaml::parse(&text, path.parent(), &assets::exists) {
        Ok(m) => (m, None),
        Err(e) => {
            log_error(&format!("menu.yaml, {e}"));
            (MenuSet::single(default_menu()), Some(e))
        }
    }
}

/// Убедиться, что menu.yaml (с описанием формата) и папка icons существуют.
pub fn ensure_menu_file() -> Option<PathBuf> {
    let p = menu_path()?;
    if !p.exists() {
        write_new(&p, menu_yaml::TEMPLATE);
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d.join("icons"));
    }
    Some(p)
}
