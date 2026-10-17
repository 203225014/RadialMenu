//! Иконки, вшитые в exe — все векторные (SVG). Новая иконка — файл в `assets/icons`
//! и строка в `NAMES` и `icon`.

macro_rules! svg {
    ($n:literal) => {
        Some(include_str!(concat!("../../assets/icons/", $n, ".svg")))
    };
}

/// Все имена, которые можно писать в файле меню (`blank` — кнопка без иконки).
pub const NAMES: [&str; 17] = [
    "blank",
    "folder",
    "file",
    "gear",
    "square-plus",
    "paste",
    "copy",
    "headphones",
    "backward-step",
    "forward-step",
    "play",
    "pause",
    "pencil-to-square",
    "clock-arrow-rotate-left",
    "square-dashed",
    "scissors",
    "xmark",
];

pub fn exists(key: &str) -> bool {
    NAMES.contains(&key)
}

/// Текст SVG встроенной иконки.
pub fn icon(key: &str) -> Option<&'static str> {
    match key {
        "blank" => None,
        "folder" => svg!("folder"),
        "file" => svg!("file"),
        "gear" => svg!("gear"),
        "square-plus" => svg!("square-plus"),
        "paste" => svg!("paste"),
        "copy" => svg!("copy"),
        "headphones" => svg!("headphones"),
        "backward-step" => svg!("backward-step"),
        "forward-step" => svg!("forward-step"),
        "play" => svg!("play"),
        "pause" => svg!("pause"),
        "pencil-to-square" => svg!("pencil-to-square"),
        "clock-arrow-rotate-left" => svg!("clock-arrow-rotate-left"),
        "square-dashed" => svg!("square-dashed"),
        "scissors" => svg!("scissors"),
        "xmark" => svg!("xmark"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_has_a_source() {
        for n in NAMES {
            assert_eq!(icon(n).is_some(), n != "blank", "{n}");
        }
        assert!(!exists("nope"));
    }

    #[test]
    fn all_builtin_icons_are_svg() {
        for n in NAMES.iter().filter(|n| **n != "blank") {
            assert!(icon(n).unwrap().contains("<svg"), "{n}");
        }
    }
}
