//! Иконки, вшитые в exe.

pub enum IconSrc {
    Svg(&'static str),
    Png(&'static [u8]),
}

macro_rules! svg {
    ($n:literal) => {
        Some(IconSrc::Svg(include_str!(concat!(
            "../assets/icons/",
            $n,
            ".svg"
        ))))
    };
}
macro_rules! png {
    ($n:literal) => {
        Some(IconSrc::Png(include_bytes!(concat!(
            "../assets/icons/",
            $n,
            ".png"
        ))))
    };
}

pub fn icon(key: &str) -> Option<IconSrc> {
    match key {
        "blank" => None,
        "folder" => png!("folder"),
        "gear" => png!("gear"),
        "square-plus" => png!("square-plus"),
        "paste" => png!("paste"),
        "copy" => png!("copy"),
        "headphones" => png!("headphones"),
        "backward-step" => png!("backward-step"),
        "forward-step" => png!("forward-step"),
        "play" => png!("play"),
        "pause" => svg!("pause"),
        "pencil-to-square" => svg!("pencil-to-square"),
        "clock-arrow-rotate-left" => svg!("clock-arrow-rotate-left"),
        "square-dashed" => svg!("square-dashed"),
        "scissors" => svg!("scissors"),
        _ => None,
    }
}
