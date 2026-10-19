//! Что сейчас играет (название, исполнитель, пауза): сессия мультимедиа Windows (та же, что показывает громкость и плеер в
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

use crate::render::raster::Track;

static TRACK: Mutex<Option<Track>> = Mutex::new(None);
static WANTED: AtomicBool = AtomicBool::new(false);
static START: Once = Once::new();

/// Название и исполнитель; `None`, если оба пусты.
pub fn track(title: &str, artist: &str, paused: bool) -> Option<Track> {
    let (title, artist) = (title.trim(), artist.trim());
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    // нет названия — исполнитель идёт первой строкой
    let (title, artist) = if title.is_empty() { (artist, "") } else { (title, artist) };
    Some(Track { title: title.to_string(), artist: artist.to_string(), paused })
}

fn fetch() -> Option<Track> {
    let manager = Manager::RequestAsync().ok()?.get().ok()?;
    let session = manager.GetCurrentSession().ok()?;
    let status = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?;
    let paused = match status {
        Status::Playing => false,
        Status::Paused => true,
        _ => return None, // остановлено, закрыто, меняется — показывать нечего
    };
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    track(&props.Title().ok()?.to_string(), &props.Artist().map(|a| a.to_string()).unwrap_or_default(), paused)
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
pub fn current() -> Option<Track> {
    TRACK.lock().ok().and_then(|t| t.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks() {
        let t = track(" Песня ", "Группа", false).unwrap();
        assert_eq!((t.title.as_str(), t.artist.as_str(), t.paused), ("Песня", "Группа", false));
        let t = track("", "Группа", true).unwrap();
        assert_eq!((t.title.as_str(), t.artist.as_str(), t.paused), ("Группа", "", true));
        assert_eq!(track(" ", "", false), None);
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
