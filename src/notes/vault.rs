//! Хранилище Obsidian: индекс заметок, подсказки `[[ссылок]]`, сохранение Ring_NN.md.
//! Без привязки к окну — окно (см. `platform`) только показывает результат.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, PartialEq)]
pub struct NoteRef {
    /// Что показывать в списке.
    pub display: String,
    /// Что подставлять в `[[…]]`.
    pub link: String,
    /// Время изменения, мс от эпохи (свежие выше).
    pub mtime: u64,
}

#[derive(Clone, Debug, Default)]
pub struct VaultIndex {
    pub notes: Vec<NoteRef>,
    /// Наибольший номер среди существующих Ring_NN.md.
    pub max_ring: u32,
}

pub fn ring_name(n: u32) -> String {
    format!("Ring_{n:02}")
}

fn ring_number(file: &str) -> Option<u32> {
    let stem = file.get(..file.len().checked_sub(3)?)?;
    if !file.to_ascii_lowercase().ends_with(".md") {
        return None;
    }
    let digits = stem
        .strip_prefix("Ring_")
        .or_else(|| stem.strip_prefix("ring_"))
        .or_else(|| stem.strip_prefix("RING_"))?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Обход хранилища: `.md` и `.canvas`, скрытые папки (`.obsidian`, `.git`…) пропускаются.
pub fn scan(vault: &Path) -> VaultIndex {
    struct Found {
        base: String,
        rel: String,
        mtime: u64,
    }
    let mut found: Vec<Found> = Vec::new();
    let mut max_ring = 0;
    let mut stack = vec![vault.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.') {
                    stack.push(entry.path());
                }
                continue;
            }
            let low = name.to_lowercase();
            let base = if low.ends_with(".md") {
                name[..name.len() - 3].to_string()
            } else if low.ends_with(".canvas") {
                name.clone() // у canvas в ссылке остаётся расширение
            } else {
                continue;
            };
            if let Some(n) = ring_number(&name) {
                max_ring = max_ring.max(n);
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            let rel = entry
                .path()
                .strip_prefix(vault)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            found.push(Found { base, rel, mtime });
        }
    }
    let mut count: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for f in &found {
        *count.entry(f.base.as_str()).or_insert(0) += 1;
    }
    let mut notes: Vec<NoteRef> = found
        .iter()
        .map(|f| {
            let link = if count[f.base.as_str()] == 1 {
                f.base.clone()
            } else if f.rel.to_lowercase().ends_with(".md") {
                f.rel[..f.rel.len() - 3].to_string()
            } else {
                f.rel.clone()
            };
            NoteRef {
                display: f.base.clone(),
                link,
                mtime: f.mtime,
            }
        })
        .collect();
    notes.sort_by(|a, b| {
        b.mtime
            .cmp(&a.mtime)
            .then_with(|| a.display.cmp(&b.display))
    });
    VaultIndex { notes, max_ring }
}

/// Незакрытая ссылка перед курсором (`[[текст` без `]`, `|`, `#`): байтовое начало запроса.
pub fn link_query(before_caret: &str) -> Option<usize> {
    let b = before_caret.as_bytes();
    (0..b.len().saturating_sub(1))
        .filter(|&p| &b[p..p + 2] == b"[[")
        .find(|&p| !before_caret[p + 2..].contains(['[', ']', '|', '#', '\n']))
        .map(|p| p + 2)
}

/// Подсказки по набранному: сначала начинающиеся с запроса, затем содержащие его.
pub fn suggest<'a>(index: &'a VaultIndex, query: &str, limit: usize) -> Vec<&'a NoteRef> {
    let q = query.to_lowercase();
    let mut items: Vec<&NoteRef> = index
        .notes
        .iter()
        .filter(|n| n.display.to_lowercase().contains(&q))
        .collect();
    items.sort_by_key(|n| !n.display.to_lowercase().starts_with(&q)); // стабильно: порядок свежести сохраняется
    items.truncate(limit);
    items
}

/// Записать заметку как Ring_NN.md, не затирая существующие. Возвращает имя без расширения.
/// Имя файла заметки из того, что ввёл пользователь: без `.md`, запрещённых в Windows знаков,
/// хвостовых точек и пробелов. Пустая строка — имя не задано.
pub fn clean_name(raw: &str) -> String {
    let t = raw.trim();
    let t = if t.len() > 3 && t.to_ascii_lowercase().ends_with(".md") { &t[..t.len() - 3] } else { t };
    let t: String = t
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c| c == '.' || c == ' ');
    // зарезервированные имена Windows
    let stem = t.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()));
    if reserved {
        return format!("_{t}");
    }
    t.chars().take(120).collect()
}

/// Сохранить под своим именем; занято — «Имя (2)», «Имя (3)»…
pub fn save_named(vault: &Path, content: &str, name: &str) -> io::Result<String> {
    let text = content.replace("\r\n", "\n");
    let text = text.trim_end();
    for n in 1..=50 {
        let candidate = if n == 1 { name.to_string() } else { format!("{name} ({n})") };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(vault.join(format!("{candidate}.md")))
        {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                f.write_all(b"\n")?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "не удалось подобрать свободное имя"))
}

pub fn save(vault: &Path, content: &str, first_try: u32) -> io::Result<String> {
    let text = content.replace("\r\n", "\n");
    let text = text.trim_end();
    let mut n = first_try.max(1);
    for _ in 0..50 {
        let name = ring_name(n);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(vault.join(format!("{name}.md")))
        {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                f.write_all(b"\n")?;
                return Ok(name);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1, // гонка с другим файлом
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "не удалось подобрать свободное имя",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(std::path::PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let p = std::env::temp_dir().join(format!("radial_menu_{tag}_{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn file(&self, rel: &str, body: &str) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn link_query_matches_obsidian_rules() {
        assert_eq!(link_query("see [[Pro"), Some(6));
        assert_eq!(link_query("[["), Some(2));
        assert_eq!(link_query("[[done]] and [[ne"), Some(15));
        assert_eq!(link_query("[[done]] tail"), None);
        assert_eq!(link_query("[[a|b"), None);
        assert_eq!(link_query("[[a#h"), None);
        assert_eq!(link_query("plain [x"), None);
        assert_eq!(link_query("[[[x"), Some(3));
    }

    #[test]
    fn ring_numbers() {
        assert_eq!(ring_number("Ring_07.md"), Some(7));
        assert_eq!(ring_number("ring_120.MD"), Some(120));
        assert_eq!(ring_number("Ring_.md"), None);
        assert_eq!(ring_number("Ring_1.txt"), None);
        assert_eq!(ring_number("Ring_1a.md"), None);
        assert_eq!(ring_name(7), "Ring_07");
        assert_eq!(ring_name(123), "Ring_123");
    }

    #[test]
    fn scan_finds_notes_skips_hidden_and_disambiguates() {
        let t = Tmp::new("scan");
        t.file("Ring_03.md", "x");
        t.file("Идеи.md", "x");
        t.file("a/Дубль.md", "x");
        t.file("b/Дубль.md", "x");
        t.file("схема.canvas", "{}");
        t.file(".obsidian/hidden.md", "x");
        t.file("pic.png", "x");
        let idx = scan(&t.0);
        assert_eq!(idx.max_ring, 3);
        let names: Vec<&str> = idx.notes.iter().map(|n| n.display.as_str()).collect();
        assert!(names.contains(&"Идеи") && names.contains(&"схема.canvas"));
        assert!(!names.contains(&"hidden") && !names.contains(&"pic"));
        let dup: Vec<&str> = idx
            .notes
            .iter()
            .filter(|n| n.display == "Дубль")
            .map(|n| n.link.as_str())
            .collect();
        assert_eq!(dup.len(), 2);
        assert!(dup.contains(&"a/Дубль") && dup.contains(&"b/Дубль"));
        assert_eq!(
            idx.notes.iter().find(|n| n.display == "Идеи").unwrap().link,
            "Идеи"
        );
    }

    #[test]
    fn suggestions_prefix_first() {
        let mk = |d: &str, m| NoteRef {
            display: d.into(),
            link: d.into(),
            mtime: m,
        };
        let idx = VaultIndex {
            notes: vec![
                mk("Мой план", 3),
                mk("План", 2),
                mk("Планета", 1),
                mk("Другое", 0),
            ],
            max_ring: 0,
        };
        let got: Vec<&str> = suggest(&idx, "пла", 10)
            .iter()
            .map(|n| n.display.as_str())
            .collect();
        assert_eq!(got, ["План", "Планета", "Мой план"]);
        assert_eq!(suggest(&idx, "", 2).len(), 2);
    }

    #[test]
    fn save_never_overwrites_and_normalises() {
        let t = Tmp::new("save");
        t.file("Ring_01.md", "старое");
        let name = save(&t.0, "строка 1\r\nстрока 2  \r\n\r\n", 1).unwrap();
        assert_eq!(name, "Ring_02");
        assert_eq!(
            fs::read_to_string(t.0.join("Ring_01.md")).unwrap(),
            "старое"
        );
        assert_eq!(
            fs::read_to_string(t.0.join("Ring_02.md")).unwrap(),
            "строка 1\nстрока 2\n"
        );
        let again = save(&t.0, "ещё", 2).unwrap();
        assert_eq!(again, "Ring_03");
    }

    #[test]
    fn clean_name_strips_what_windows_dislikes() {
        assert_eq!(clean_name("  Идея: про <всё>?.md "), "Идея про всё");
        assert_eq!(clean_name("a/b\\c"), "a b c");
        assert_eq!(clean_name("..."), "");
        assert_eq!(clean_name("   "), "");
        assert_eq!(clean_name("Заметка."), "Заметка");
        assert_eq!(clean_name("con"), "_con");
        assert_eq!(clean_name("note.md"), "note");
    }

    #[test]
    fn save_named_never_overwrites() {
        let t = Tmp::new("named");
        assert_eq!(save_named(&t.0, "one", "Идея").unwrap(), "Идея");
        assert_eq!(save_named(&t.0, "two", "Идея").unwrap(), "Идея (2)");
        assert_eq!(fs::read_to_string(t.0.join("Идея.md")).unwrap(), "one\n");
        assert_eq!(fs::read_to_string(t.0.join("Идея (2).md")).unwrap(), "two\n");
    }
}

