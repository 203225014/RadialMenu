//! Что сейчас играет: сессия мультимедиа Windows (та же, что показывает громкость и плеер в
//! системной панели) — любой плеер: браузер, Spotify, проигрыватели. Спрашивать приходится
//! через WinRT, это ждёт ответа, поэтому опрос идёт в своём потоке и только пока меню открыто.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};
use std::time::Duration;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

static TRACK: Mutex<Option<String>> = Mutex::new(None);
static WANTED: AtomicBool = AtomicBool::new(false);
static START: Once = Once::new();

/// «Название — Исполнитель»; на паузе — с пометкой. `None` — ничего не выбрано.
pub fn caption(title: &str, artist: &str, paused: bool) -> Option<String> {
    let (title, artist) = (title.trim(), artist.trim());
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    let base = match (title.is_empty(), artist.is_empty()) {
        (false, false) => format!("{title} — {artist}"),
        (false, true) => title.to_string(),
        _ => artist.to_string(),
    };
    Some(if paused { format!("Пауза: {base}") } else { base })
}

fn fetch() -> Option<String> {
    let manager = Manager::RequestAsync().ok()?.get().ok()?;
    let session = manager.GetCurrentSession().ok()?;
    let status = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?;
    let paused = match status {
        Status::Playing => false,
        Status::Paused => true,
        _ => return None, // остановлено, закрыто, меняется — показывать нечего
    };
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    caption(&props.Title().ok()?.to_string(), &props.Artist().map(|a| a.to_string()).unwrap_or_default(), paused)
}

/// Запустить опрос (один раз за процесс).
fn start() {
    START.call_once(|| {
        std::thread::spawn(|| {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            loop {
                if WANTED.load(Ordering::Relaxed) {
                    let now = fetch();
                    if let Ok(mut t) = TRACK.lock() {
                        *t = now;
                    }
                    std::thread::sleep(Duration::from_millis(1500));
                } else {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
    });
}

/// Меню открыто (или закрыто): когда оно открыто, трек опрашивается раз в полторы секунды.
pub fn set_wanted(on: bool) {
    if on {
        start();
    } else if let Ok(mut t) = TRACK.lock() {
        t.take(); // при следующем открытии не показывать старый трек, пока не придёт новый
    }
    WANTED.store(on, Ordering::Relaxed);
}

/// Последний известный трек.
pub fn current() -> Option<String> {
    TRACK.lock().ok().and_then(|t| t.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captions() {
        assert_eq!(caption("Песня", "Группа", false).as_deref(), Some("Песня — Группа"));
        assert_eq!(caption("  Песня ", "", false).as_deref(), Some("Песня"));
        assert_eq!(caption("", "Группа", false).as_deref(), Some("Группа"));
        assert_eq!(caption("Песня", "Группа", true).as_deref(), Some("Пауза: Песня — Группа"));
        assert_eq!(caption(" ", "", false), None);
    }

    /// Живой запрос к системе: `cargo test live_media -- --ignored --nocapture` (пока что-то играет).
    #[test]
    #[ignore = "нужна медиа-сессия Windows"]
    fn live_media_session() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        eprintln!("сейчас играет: {:?}", fetch());
    }
}
