//! Скриншоты для документации: кадры меню и окно настроек, нарисованные тем же кодом,
//! что и в приложении (Windows не нужна). Пересобрать картинки после изменений интерфейса:
//!   cargo run --release --example docshots -- docs/img

use radial_menu::engine::layout::Layout;
use radial_menu::engine::metrics::Metrics;
use radial_menu::engine::Engine;
use radial_menu::model::menu_yaml::{self, NAV_TEMPLATE};
use radial_menu::model::{default_menu, Context, Flag, Menu, Settings, Theme, ThemeMode};
use radial_menu::render::compose::Canvas;
use radial_menu::render::painter::Painter;
use radial_menu::render::{assets, raster};
use radial_menu::ui::settings_form::{Fonts, SettingsForm};

/// «Обои»: мягкий диагональный градиент под тему — на нём видно, как стекло кнопок прозрачно.
fn wallpaper(theme: Theme, x: usize, y: usize, w: usize, h: usize) -> [f32; 3] {
    let (a, b) = match theme {
        Theme::Dark => ([34.0, 46.0, 78.0], [78.0, 52.0, 104.0]),
        Theme::Light => ([186.0, 208.0, 246.0], [236.0, 214.0, 244.0]),
    };
    let t = (x as f32 / w as f32 + y as f32 / h as f32) / 2.0;
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn write_png(path: &std::path::Path, w: usize, h: usize, rgb: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Best);
    enc.write_header().unwrap().write_image_data(rgb).unwrap();
}

/// Кадр меню (предумноженный BGRA) поверх обоев, с обрезкой до содержимого.
fn save_menu(path: &std::path::Path, n: usize, bgra: &[u8], theme: Theme) {
    let (mut x0, mut y0, mut x1, mut y1) = (n, n, 0, 0);
    for y in 0..n {
        for x in 0..n {
            if bgra[(y * n + x) * 4 + 3] > 8 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    // симметрично вокруг центра: меню всегда в середине кадра
    let c = n / 2;
    let r = [c - x0, x1 - c, c - y0, y1 - c].into_iter().max().unwrap() + 28;
    let (lo, hi) = (c.saturating_sub(r), (c + r).min(n));
    let side = hi - lo;
    let mut rgb = Vec::with_capacity(side * side * 3);
    for y in lo..hi {
        for x in lo..hi {
            let p = &bgra[(y * n + x) * 4..(y * n + x) * 4 + 4];
            let a = p[3] as f32 / 255.0;
            let bg = wallpaper(theme, x - lo, y - lo, side, side);
            for (v, d) in [p[2], p[1], p[0]].into_iter().zip(bg) {
                rgb.push((v as f32 + d * (1.0 - a)).clamp(0.0, 255.0) as u8);
            }
        }
    }
    write_png(path, side, side, &rgb);
}

/// Меню на экране, указатель проведён по цепочке кнопок до `target` (основная → пункт веера → вложенный).
/// Веера с задержкой раскрываются, поэтому между шагами пауза длиннее времени задержки.
fn shot(dir: &std::path::Path, name: &str, menu: &Menu, theme: Theme, ctx: &Context, target: usize, font: Option<&fontdue::Font>) {
    let s = Settings::default();
    let m = Metrics::new(&s, 1.0, (1920, 1080), menu);
    let lay = Layout::new(menu, &m);
    let mut chain = vec![target];
    while let Some(p) = menu.node(*chain.last().unwrap()).parent.filter(|&p| p != 0) {
        chain.push(p);
    }
    chain.reverse();
    let mut e = Engine::new(s, 1.0, (1920, 1080), menu.clone(), font, theme, &raster::no_program_icons);
    let n = e.window_size() as usize;
    let mut buf = vec![0u8; n * n * 4];
    e.press(0.0);
    let (mut px, mut py) = (0.0f32, 0.0f32);
    let (mut t, mut step) = (0.0f64, 0);
    let gap = 0.75;
    while t < 0.4 + gap * chain.len() as f64 + 0.6 {
        t += 0.008;
        let mut d = (0.0, 0.0);
        if step < chain.len() && t > 0.3 + gap * step as f64 {
            let off = lay.slot(chain[step]).off;
            d = ((off.0 - px) / m.sens, (off.1 - py) / m.sens);
            (px, py) = off;
            step += 1;
        }
        let mut cv = Canvas::new(n, n, &mut buf);
        e.frame(t, d, ctx, &mut cv);
    }
    save_menu(&dir.join(name), n, &buf, theme);
    eprintln!("{name}");
}

fn find(menu: &Menu, label: &str) -> usize {
    menu.ids().find(|&i| menu.node(i).label == label).unwrap_or_else(|| panic!("нет пункта «{label}»"))
}

const CUSTOM: &str = r##"
menu:
  - { label: Почта, open: "mailto:", color: "#E5484D" }
  - { label: Терминал, run: wt, color: "#2E9E5B" }
  - { label: Заметка, icon: pencil-to-square, action: note }
  - { label: Скриншот, keys: win+shift+s, color: "#8E4EC6" }
  - { label: Загрузки, icon: folder, open: "%USERPROFILE%/Downloads" }
  - label: Медиа
    icon: headphones
    items:
      - { label: Назад, icon: backward-step, media: prev }
      - { label: Пауза, icon: pause, media: play-pause }
      - { label: Вперёд, icon: forward-step, media: next }
      - { label: Тише, volume: down }
      - { label: Громче, volume: up }
  - { label: Калькулятор, run: calc }
  - { label: Копировать, icon: copy, keys: ctrl+c }
  - { label: Вставить, icon: paste, keys: ctrl+v }
  - { label: Настройки, icon: gear, action: settings }
"##;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "docs/img".into()));
    std::fs::create_dir_all(&dir).unwrap();
    let font = raster::load_font();
    let main_menu = default_menu();
    let nav = menu_yaml::parse(NAV_TEMPLATE, None, &assets::exists).expect("меню навигации").main;
    let custom = menu_yaml::parse(CUSTOM, None, &assets::exists).expect("своё меню").main;
    let none = Context::default();
    let playing = Context::default().with(Flag::Media, true);
    let desktops = Context::default().with(Flag::DesktopPrev, true).with(Flag::DesktopNext, true);

    shot(&dir, "menu-main.png", &main_menu, Theme::Dark, &none, find(&main_menu, "Заметка"), font.as_ref());
    shot(&dir, "menu-main-light.png", &main_menu, Theme::Light, &none, find(&main_menu, "Заметка"), font.as_ref());
    shot(&dir, "menu-media.png", &main_menu, Theme::Dark, &playing, find(&main_menu, "Пауза"), font.as_ref());
    shot(&dir, "menu-copy.png", &main_menu, Theme::Dark, &none, find(&main_menu, "Вырезать"), font.as_ref());
    shot(&dir, "menu-nav.png", &nav, Theme::Dark, &desktops, find(&nav, "Стол вперёд"), font.as_ref());
    shot(&dir, "menu-custom.png", &custom, Theme::Dark, &playing, find(&custom, "Громче"), font.as_ref());

    // окно настроек
    let Some(fonts) = Fonts::load() else {
        eprintln!("нет шрифтов — окно настроек не нарисовано");
        return;
    };
    for (mode, theme, name) in [(ThemeMode::Dark, Theme::Dark, "settings-dark.png"), (ThemeMode::Light, Theme::Light, "settings-light.png")] {
        let s = Settings { vault: "C:/Users/me/Documents/Obsidian/Work".into(), theme: mode, ..Settings::default() };
        let form = SettingsForm::new(s, true, 1.0);
        let (w, h) = form.size();
        let mut p = Painter::new(w as u32, h as u32).unwrap();
        form.draw(&mut p, &fonts, theme, true);
        let rgb: Vec<u8> = p.rgba().chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
        write_png(&dir.join(name), w as usize, h as usize, &rgb);
        eprintln!("{name}");
    }
}
