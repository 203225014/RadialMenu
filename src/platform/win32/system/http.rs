//! Простой HTTPS GET средствами Windows (WinHTTP): без сторонних клиентов и TLS-библиотек.
//! Нужен для проверки и скачивания обновлений; системные настройки прокси учитываются самой WinHTTP.

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use super::util::{pcwstr, wide};

/// Не больше 1 МБ ответа: нам нужен маленький JSON, а не файл.
const MAX_BODY: usize = 1 << 20;

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// Разбор `https://host/path?query` на узел и путь (только https).
pub fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    match rest.find('/') {
        Some(i) => Some((&rest[..i], &rest[i..])),
        None => Some((rest, "/")),
    }
}

/// Читать ответ кусками по 64 КБ: `sink(кусок, всего_байт)`, где `всего_байт` — Content-Length или 0.
fn stream(
    url: &str,
    user_agent: &str,
    accept: &str,
    timeout_ms: i32,
    mut sink: impl FnMut(&[u8], u64) -> Result<(), String>,
) -> Result<(), String> {
    let (host, path) = split_url(url).ok_or("поддерживается только https")?;
    unsafe {
        let ua = wide(user_agent);
        let session = Handle(
            WinHttpOpen(pcwstr(&ua), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
        );
        if session.0.is_null() {
            return Err("WinHttpOpen не удался".into());
        }
        let _ = WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
        let h = wide(host);
        let conn = Handle(WinHttpConnect(session.0, pcwstr(&h), 443, 0));
        if conn.0.is_null() {
            return Err(format!("нет соединения с {host}"));
        }
        let (verb, p) = (wide("GET"), wide(path));
        let accept = wide(accept);
        let accepts = [pcwstr(&accept), PCWSTR::null()];
        let req = Handle(WinHttpOpenRequest(
            conn.0,
            pcwstr(&verb),
            pcwstr(&p),
            PCWSTR::null(),
            PCWSTR::null(),
            accepts.as_ptr(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return Err("запрос не создан".into());
        }
        // переадресации (GitHub отдаёт файлы релиза с другого узла) WinHTTP проходит сама
        WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(|e| format!("запрос не отправлен: {e}"))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| format!("нет ответа: {e}"))?;

        let number = |what: u32| -> Option<u32> {
            let mut v: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                req.0,
                what | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut v as *mut u32 as *mut c_void),
                &mut size,
                std::ptr::null_mut(),
            )
            .ok()
            .map(|_| v)
        };
        let status = number(WINHTTP_QUERY_STATUS_CODE).ok_or("нет кода ответа")?;
        if status != 200 {
            return Err(format!("сервер ответил {status}"));
        }
        let total = number(WINHTTP_QUERY_CONTENT_LENGTH).unwrap_or(0) as u64;

        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(req.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)
                .map_err(|e| format!("чтение: {e}"))?;
            if read == 0 {
                return Ok(());
            }
            sink(&chunk[..read as usize], total)?;
        }
    }
}

/// GET по https; вернёт тело при коде 200, иначе описание ошибки.
pub fn get(url: &str, user_agent: &str, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    stream(url, user_agent, "application/vnd.github+json", timeout_ms, |chunk, _| {
        body.extend_from_slice(chunk);
        if body.len() > MAX_BODY {
            return Err("ответ слишком большой".into());
        }
        Ok(())
    })?;
    Ok(body)
}

/// Скачать файл кусками: `sink(кусок, всего_байт)`; больше `max` байт — ошибка.
pub fn download(
    url: &str,
    user_agent: &str,
    timeout_ms: i32,
    max: u64,
    mut sink: impl FnMut(&[u8], u64) -> Result<(), String>,
) -> Result<(), String> {
    let mut got = 0u64;
    stream(url, user_agent, "application/octet-stream", timeout_ms, |chunk, total| {
        got += chunk.len() as u64;
        if got > max || total > max {
            return Err("файл слишком большой".into());
        }
        sink(chunk, total)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_split() {
        assert_eq!(split_url("https://api.github.com/repos/a/b/releases/latest"), Some(("api.github.com", "/repos/a/b/releases/latest")));
        assert_eq!(split_url("https://example.com"), Some(("example.com", "/")));
        assert_eq!(split_url("http://example.com/x"), None);
    }

    /// Живое скачивание установщика релиза с проверкой суммы: `cargo test live_download -- --ignored`.
    #[test]
    #[ignore = "нужна сеть"]
    fn live_download_matches_published_sum() {
        use sha2::{Digest, Sha256};
        let base = "https://github.com/203225014/RadialMenu/releases/download/v0.8.0/";
        let sums = get(&format!("{base}SHA256SUMS.txt"), "RadialMenu-test", 15_000).unwrap();
        let name = "RadialMenu-Setup-0.8.0.exe";
        let want = crate::platform::update::sum_for(&String::from_utf8_lossy(&sums), name).expect("сумма в файле");
        let (mut h, mut n, mut last_total) = (Sha256::new(), 0u64, 0u64);
        download(&format!("{base}{name}"), "RadialMenu-test", 20_000, 100 << 20, |c, total| {
            h.update(c);
            n += c.len() as u64;
            last_total = total;
            Ok(())
        })
        .unwrap();
        eprintln!("скачано {n} байт, Content-Length {last_total}");
        assert!(n > 100_000 && (last_total == 0 || last_total == n));
        assert_eq!(crate::platform::update::hex(&h.finalize()), want);
    }

    /// Живой запрос к публичному репозиторию: `cargo test live_github -- --ignored`.
    #[test]
    #[ignore = "нужна сеть"]
    fn live_github_release_is_fetched_and_parsed() {
        let body = get("https://api.github.com/repos/rust-lang/rust/releases/latest", "RadialMenu-test", 10_000).unwrap();
        let rel = crate::platform::update::parse_release(&String::from_utf8_lossy(&body)).expect("ответ разобран");
        assert!(rel.version.0 >= 1 && rel.page.starts_with("https://github.com/"));
        // закрытый репозиторий отвечает 404 — это ошибка, а не пустой успех
        let e = get("https://api.github.com/repos/203225014/RadialMenu/releases/latest", "RadialMenu-test", 10_000);
        eprintln!("наш репозиторий: {e:?}");
    }
}
