//! Правки текста в окне заметки. Работает с UTF-16, как поле ввода Windows: позиции —
//! в единицах UTF-16. Окно только читает текст и курсор и применяет готовую правку.

use super::vault::link_query;

/// Заменить `[start, end)` на `insert` и поставить курсор в `caret`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub insert: String,
    pub caret: usize,
}

fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Текст текущей строки до курсора и позиция начала этой строки.
pub fn line_before_caret(buf: &[u16], caret: usize) -> (String, usize) {
    let caret = caret.min(buf.len());
    let start = buf[..caret]
        .iter()
        .rposition(|&c| c == 0x0A || c == 0x0D)
        .map_or(0, |p| p + 1);
    (String::from_utf16_lossy(&buf[start..caret]), start)
}

/// Что набрано после незакрытой `[[` в текущей строке (`None` — подсказки не нужны).
pub fn query_at(buf: &[u16], caret: usize) -> Option<String> {
    let (before, _) = line_before_caret(buf, caret);
    link_query(&before).map(|q| before[q..].to_string())
}

/// Набрали `[` сразу после `[` (без выделения): дописать `]]` и встать внутрь.
pub fn auto_close(buf: &[u16], sel: (usize, usize)) -> Option<TextEdit> {
    let (caret, end) = sel;
    if caret != end || caret == 0 || buf.get(caret - 1) != Some(&(b'[' as u16)) {
        return None;
    }
    Some(TextEdit {
        start: caret,
        end: caret,
        insert: "[]]".into(),
        caret: caret + 1,
    })
}

/// Подставить `link` вместо набранного после `[[`; закрыть `]]`, если их ещё нет,
/// и поставить курсор после ссылки.
pub fn accept_link(buf: &[u16], caret: usize, link: &str) -> Option<TextEdit> {
    let caret = caret.min(buf.len());
    let (before, line_start) = line_before_caret(buf, caret);
    let q = link_query(&before)?;
    let start = line_start + len16(&before[..q]);
    let closed = buf.get(caret) == Some(&(b']' as u16)) && buf.get(caret + 1) == Some(&(b']' as u16));
    let insert = if closed {
        link.to_string()
    } else {
        format!("{link}]]")
    };
    Some(TextEdit {
        start,
        end: caret,
        caret: start + len16(link) + 2,
        insert,
    })
}

/// Применить правку к буферу (для проверки и для платформ без родного поля ввода).
pub fn apply(buf: &[u16], e: &TextEdit) -> Vec<u16> {
    let mut out = buf[..e.start].to_vec();
    out.extend(e.insert.encode_utf16());
    out.extend_from_slice(&buf[e.end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn caret_of(s: &str) -> (Vec<u16>, usize) {
        let i = s.find('|').unwrap();
        (u(&s.replace('|', "")), len16(&s[..i]))
    }

    fn show(buf: &[u16], caret: usize) -> String {
        let mut b = buf.to_vec();
        b.insert(caret, b'|' as u16);
        String::from_utf16(&b).unwrap()
    }

    #[test]
    fn double_bracket_closes_itself() {
        let (buf, c) = caret_of("см. [|");
        let e = auto_close(&buf, (c, c)).unwrap();
        let out = apply(&buf, &e);
        assert_eq!(show(&out, e.caret), "см. [[|]]");
        // одиночная скобка и выделение — не трогаем
        let (buf, c) = caret_of("см. |");
        assert!(auto_close(&buf, (c, c)).is_none());
        let (buf, _) = caret_of("[ab|");
        assert!(auto_close(&buf, (1, 3)).is_none());
    }

    #[test]
    fn accept_replaces_query_and_closes() {
        // после авто-закрытия: ]] уже стоят
        let (buf, c) = caret_of("идея [[Пла|]] дальше");
        let e = accept_link(&buf, c, "План").unwrap();
        let out = apply(&buf, &e);
        assert_eq!(show(&out, e.caret), "идея [[План]]| дальше");
        // ]] нет — дописываем
        let (buf, c) = caret_of("[[пл|");
        let e = accept_link(&buf, c, "папка/План").unwrap();
        assert_eq!(show(&apply(&buf, &e), e.caret), "[[папка/План]]|");
    }

    #[test]
    fn works_on_later_lines_and_non_ascii() {
        let (buf, c) = caret_of("первая [[a]]\r\nвторая 😀 [[Дн|");
        assert_eq!(query_at(&buf, c).as_deref(), Some("Дн"));
        let e = accept_link(&buf, c, "Дневник").unwrap();
        assert_eq!(
            show(&apply(&buf, &e), e.caret),
            "первая [[a]]\r\nвторая 😀 [[Дневник]]|"
        );
    }

    #[test]
    fn no_open_link_no_edit() {
        let (buf, c) = caret_of("[[готово]] и всё|");
        assert_eq!(query_at(&buf, c), None);
        assert!(accept_link(&buf, c, "x").is_none());
        let (buf, c) = caret_of("строка [[a\nновая|");
        assert_eq!(query_at(&buf, c), None);
    }
}
