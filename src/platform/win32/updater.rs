//! Обновление программы: проверка GitHub Releases, скачивание установщика с проверкой SHA-256 и
//! его тихий запуск. Состояние (для окна настроек) лежит в `VIEW`; окно опрашивает его таймером.
//! Решения о том, какое уведомление показать и когда выйти, принимает `app` — отсюда он их получает
//! сообщениями `WM_UPDATE` (пришёл ответ проверки) и `WM_UPDATE_EXIT` (установщик запущен, пора выйти).

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::system::{http, util};
use crate::platform::diag;
use crate::platform::update::{self, Release, Version};
use crate::ui::settings_form::UpdateView;

/// Пришёл ответ проверки (сам ответ лежит в `ANSWER`).
pub const WM_UPDATE: u32 = WM_APP + 40;
/// Установщик запущен: программе пора выйти, чтобы он смог заменить файлы.
pub const WM_UPDATE_EXIT: u32 = WM_APP + 41;

const FIRST_DELAY_S: u64 = 20;
const EVERY_S: u64 = 24 * 60 * 60;
/// Установщик весит около мегабайта; больше сотни — явно не он.
const MAX_INSTALLER: u64 = 100 << 20;

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Кто спросил: от этого зависит, какие уведомления показывать.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Раз в сутки сама: сказать только о новой версии, и один раз.
    Background,
    /// «Проверить обновления сейчас» в трее: сказать о любом итоге.
    Tray,
    /// Кнопка в настройках: итог и так виден в окне.
    Settings,
}

/// Ответ проверки.
pub type Answer = (Mode, Result<Release, String>);

static ANSWER: Mutex<Option<Answer>> = Mutex::new(None);
static VIEW: Mutex<UpdateView> = Mutex::new(UpdateView::Idle);
static RELEASE: Mutex<Option<Release>> = Mutex::new(None);
static TOLD: Mutex<Option<Version>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(true);
/// Идёт скачивание/установка: вторую не начинаем.
static BUSY: AtomicBool = AtomicBool::new(false);
static MAIN: AtomicIsize = AtomicIsize::new(0);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Что показать в окне настроек.
pub fn view() -> UpdateView {
    lock(&VIEW).clone()
}

fn set_view(v: UpdateView) {
    *lock(&VIEW) = v;
}

/// Версия, которую можно установить (если найдена и не идёт установка).
pub fn available() -> Option<String> {
    if BUSY.load(Ordering::Relaxed) {
        return None;
    }
    lock(&RELEASE).as_ref().map(|r| r.version.to_string())
}

/// Раз в сутки проверять ли обновления.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Запустить суточную проверку (первая — вскоре после запуска).
pub fn start(hwnd: isize, enabled: bool) {
    MAIN.store(hwnd, Ordering::Relaxed);
    set_enabled(enabled);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(FIRST_DELAY_S));
        loop {
            if ENABLED.load(Ordering::Relaxed) {
                check(Mode::Background);
            }
            std::thread::sleep(Duration::from_secs(EVERY_S));
        }
    });
}

fn post(msg: u32) {
    let hwnd = MAIN.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(Some(util::h(hwnd)), msg, WPARAM(0), LPARAM(0));
        }
    }
}

fn user_agent() -> String {
    format!("RadialMenu/{}", env!("CARGO_PKG_VERSION"))
}

/// Спросить GitHub о последней версии (в отдельном потоке); итог придёт как `WM_UPDATE`.
pub fn check(mode: Mode) {
    if BUSY.load(Ordering::Relaxed) {
        return; // уже качаем: окну важнее ход скачивания
    }
    if mode != Mode::Background {
        set_view(UpdateView::Checking);
    }
    std::thread::spawn(move || {
        let answer = http::get(&update::latest_url(), &user_agent(), 10_000).and_then(|body| {
            update::parse_release(&String::from_utf8_lossy(&body))
                .ok_or_else(|| "непонятный ответ сервера".to_string())
        });
        *lock(&ANSWER) = Some((mode, answer));
        post(WM_UPDATE);
    });
}

/// Что сделать с ответом проверки: обновить состояние и решить, нужно ли уведомление.
pub enum Notice {
    None,
    /// Вышла новая версия.
    Found(Version),
    /// Новее нет (только если спросили из трея).
    UpToDate,
    /// Не удалось (только если спросили из трея).
    Failed(String),
}

pub fn take_answer() -> Notice {
    let Some((mode, answer)) = lock(&ANSWER).take() else {
        return Notice::None;
    };
    if BUSY.load(Ordering::Relaxed) {
        return Notice::None;
    }
    match answer {
        Ok(rel) if update::is_newer(rel.version, update::current()) => {
            diag::log(&format!("доступна версия {} (сейчас {})", rel.version, update::current()));
            let version = rel.version;
            let told = lock(&TOLD).replace(version) == Some(version);
            *lock(&RELEASE) = Some(rel);
            set_view(UpdateView::Available(version.to_string()));
            match mode {
                Mode::Settings => Notice::None,
                Mode::Background if told => Notice::None,
                _ => Notice::Found(version),
            }
        }
        Ok(_) => {
            *lock(&RELEASE) = None;
            set_view(UpdateView::UpToDate);
            if mode == Mode::Tray { Notice::UpToDate } else { Notice::None }
        }
        Err(e) => {
            diag::log(&format!("проверка обновлений: {e}"));
            set_view(UpdateView::Failed { msg: format!("Не удалось проверить: {e}"), retry: false });
            if mode == Mode::Tray { Notice::Failed(e) } else { Notice::None }
        }
    }
}

/// Скачать найденную версию и запустить установщик (в отдельном потоке).
/// Успех — `WM_UPDATE_EXIT`; неудача — `Failed` в `view()`.
pub fn install() {
    let Some(release) = lock(&RELEASE).clone() else {
        return;
    };
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    set_view(UpdateView::Downloading(0));
    std::thread::spawn(move || match run_install(&release) {
        Ok(()) => {
            set_view(UpdateView::Installing);
            post(WM_UPDATE_EXIT);
        }
        Err(e) => {
            diag::log(&format!("обновление до {}: {e}", release.version));
            set_view(UpdateView::Failed { msg: e, retry: true });
            BUSY.store(false, Ordering::SeqCst);
        }
    });
}

fn run_install(rel: &Release) -> Result<(), String> {
    let (Some(url), Some(name), Some(sums_url)) = (&rel.installer, &rel.installer_name, &rel.sums) else {
        return Err("В релизе нет установщика или контрольных сумм".into());
    };
    if !update::is_trusted_asset(url) || !update::is_trusted_asset(sums_url) || name.contains(['/', '\\']) {
        return Err("Ссылка на установщик ведёт не в наш репозиторий".into());
    }
    let ua = user_agent();
    let sums = http::get(sums_url, &ua, 15_000).map_err(|e| format!("Не скачались контрольные суммы: {e}"))?;
    let expected = update::sum_for(&String::from_utf8_lossy(&sums), name)
        .ok_or("В релизе нет контрольной суммы установщика")?;

    let dir = std::env::temp_dir().join("RadialMenu-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Нет временной папки: {e}"))?;
    if let Ok(old) = std::fs::read_dir(&dir) {
        old.flatten().for_each(|f| drop(std::fs::remove_file(f.path()))); // остатки прошлых попыток
    }
    let (part, done) = (dir.join(format!("{name}.part")), dir.join(name));

    let mut file = std::fs::File::create(&part).map_err(|e| format!("Не создать файл: {e}"))?;
    let mut hasher = Sha256::new();
    let (mut got, mut shown) = (0u64, 0u8);
    let result = http::download(url, &ua, 20_000, MAX_INSTALLER, |chunk, total| {
        file.write_all(chunk).map_err(|e| format!("Не записать файл: {e}"))?;
        hasher.update(chunk);
        got += chunk.len() as u64;
        if let Some(pct) = (got * 100).checked_div(total).map(|p| p.min(100) as u8) {
            if pct != shown {
                shown = pct;
                set_view(UpdateView::Downloading(pct));
            }
        }
        Ok(())
    });
    drop(file);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(format!("Не скачалось: {e}"));
    }
    if update::hex(&hasher.finalize()) != expected {
        let _ = std::fs::remove_file(&part);
        return Err("Контрольная сумма не совпала — файл не запущен".into());
    }
    std::fs::rename(&part, &done).map_err(|e| format!("Не сохранить установщик: {e}"))?;

    // тихая установка поверх; `/UPDATE=1` — чтобы установщик запустил программу уже с уведомлением
    std::process::Command::new(&done)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE=1"])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| format!("Не запустился установщик: {e}"))?;
    diag::log(&format!("запущен установщик {}", done.display()));
    Ok(())
}
