//! Рисует кадры меню и окно настроек в PNG без Windows — для проверки внешнего вида.
//! Запуск: cargo run --release --example preview -- <папка для PNG> [light|dark|auto] [menu.yaml]
//! `auto` — тема системы (на любой ОС). `SCALE=1.5` — другой масштаб.
//! С путём к menu.yaml рисуется ещё и это меню (`9_custom.png`, все кнопки видны разом).

use radial_menu::engine::Engine;
use radial_menu::model::menu_yaml;
use radial_menu::model::{default_menu, Context, Flag, Menu, Settings, Theme, ThemeMode};
use radial_menu::platform::theme::system_prefers_dark;
use radial_menu::render::compose::Canvas;
use radial_menu::render::painter::Painter;
use radial_menu::render::{assets, raster};
use radial_menu::ui::settings_form::{Fonts, SettingsForm};

/// Сохранить кадр меню: предумноженный BGRA поверх «рабочего стола» нужного цвета.
fn save_bgra(path: &std::path::Path, w: usize, h: usize, bgra: &[u8], desk: [u8; 3]) {
    let mut rgb = Vec::with_capacity(w * h * 3);
    for p in bgra.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        for (c, d) in [p[2], p[1], p[0]].into_iter().zip(desk) {
            rgb.push((c as f32 + d as f32 * (1.0 - a)).min(255.0) as u8);
        }
    }
    write_png(path, w, h, &rgb);
}

fn write_png(path: &std::path::Path, w: usize, h: usize, rgb: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgb).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let mode = args
        .next()
        .and_then(|m| ThemeMode::from_key(&m))
        .unwrap_or(ThemeMode::Dark);
    let theme = mode.resolve(system_prefers_dark());
    // фон «рабочего стола» под тему, чтобы было видно, как стекло ложится на обои
    let desk = match theme {
        Theme::Dark => [52, 52, 52],
        Theme::Light => [214, 222, 235],
    };
    std::fs::create_dir_all(&dir).unwrap();
    let scale: f32 = std::env::var("SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let font = raster::load_font();
    let t0 = std::time::Instant::now();
    let mut e = Engine::new(
        Settings::default(),
        scale,
        (1920, 1080),
        default_menu(),
        font.as_ref(),
        theme,
        &raster::no_program_icons,
    );
    eprintln!("тема: {theme:?}, запуск ядра: {:?}", t0.elapsed());
    let n = e.window_size() as usize;
    let mut buf = vec![0u8; n * n * 4];
    let ctx = Context::default().with(Flag::Media, true);
    let mut t = 0.0;
    e.press(t);
    // (имя, секунд, сдвиг мыши за кадр)
    let script: [(&str, f64, (f32, f32)); 5] = [
        ("1_appear", 0.35, (0.0, 0.0)),
        ("2_hover_top", 0.35, (0.0, -12.0)), // вверх → «Открыть» + веер
        ("3_submenu", 0.35, (-6.0, -14.0)),  // в подпункт веера
        ("4_media", 1.2, (20.0, 30.0)),      // вниз → «Медиа» (играет)
        ("5_copy", 1.4, (-40.0, -25.0)),     // влево → «Копировать» с задержкой
    ];
    let mut frames = 0u32;
    let mut render_time = std::time::Duration::ZERO;
    for (name, secs, d) in script {
        let end = t + secs;
        while t < end {
            t += 0.008;
            let mut cv = Canvas::new(n, n, &mut buf);
            let s = std::time::Instant::now();
            e.frame(t, d, &ctx, &mut cv);
            render_time += s.elapsed();
            frames += 1;
        }
        save_bgra(&dir.join(format!("{name}.png")), n, n, &buf, desk);
    }
    e.wheel(true, t);
    let mut cv = Canvas::new(n, n, &mut buf);
    e.frame(t + 0.008, (0.0, 0.0), &ctx, &mut cv);
    save_bgra(&dir.join("6_toast.png"), n, n, &buf, desk);
    eprintln!(
        "кадров: {frames}, среднее на кадр: {:?}, окно {n}×{n}",
        render_time / frames
    );

    // большое меню: 12 кнопок, веер на 6, свои цвета, буквы вместо иконок
    let big = menu_yaml::parse(BIG, None, &assets::exists).expect("демо-меню");
    sweep(&dir.join("8_big.png"), &big.main, theme, scale, font.as_ref(), desk, 4);
    if let Some(path) = args.next() {
        let p = std::path::PathBuf::from(&path);
        let text = std::fs::read_to_string(&p).expect("menu.yaml");
        match menu_yaml::parse(&text, p.parent(), &assets::exists) {
            Ok(set) => {
                sweep(&dir.join("9_custom.png"), &set.main, theme, scale, font.as_ref(), desk, 0);
                // если есть вложенные веера — кадр с указателем на самом глубоком пункте
                if let Some(deep) = set.main.ids().find(|&i| {
                    let up = |i: usize| set.main.node(i).parent.filter(|&p| p != 0);
                    up(i).and_then(up).is_some()
                }) {
                    sweep_to(&dir.join("10_deep.png"), &set.main, theme, scale, font.as_ref(), desk, deep);
                }
            }
            Err(e) => eprintln!("menu.yaml: {e}"),
        }
    }

    // подсказки `[[` в окне заметки
    if let Some(fonts) = Fonts::load() {
        let items: Vec<String> = ["Задания", "Куки", "NoteDrop - T3", "Задачи", "Логины-пароли", "Правила", "Wallet keyword", "Ответы на вопросы", "Чистый лист", "Заметки"]
            .iter()
            .map(|t| t.to_string())
            .collect();
        let pal = radial_menu::render::palette::ui(theme);
        if let Some(img) = radial_menu::ui::suggest_view::render(&items, 1, Some(3), 0, scale, &pal, &fonts) {
            let (w, h) = (img.width() as usize, img.height() as usize);
            let mut bgra = vec![0u8; w * h * 4];
            img.to_bgra(&mut bgra);
            save_bgra(&dir.join("11_suggest.png"), w, h, &bgra, [0x26, 0x2c, 0x3d]);
        }
    }

    // окно настроек
    let Some(fonts) = Fonts::load() else {
        eprintln!("нет шрифтов — окно настроек не нарисовано");
        return;
    };
    let mut s = Settings {
        vault: "C:/Users/me/Documents/Obsidian/Work".into(),
        theme: mode,
        ..Settings::default()
    };
    s.dwell_ms = 450;
    let form = SettingsForm::new(s, true, scale);
    let (w, h) = form.size();
    let mut p = Painter::new(w as u32, h as u32).unwrap();
    form.draw(&mut p, &fonts, theme, true);
    let rgb: Vec<u8> = p
        .rgba()
        .chunks_exact(4)
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    write_png(&dir.join("7_settings.png"), w as usize, h as usize, &rgb);
}

/// Демо: сколько всего помещается.
const BIG: &str = r##"
menu:
  - { label: Почта, open: "mailto:", color: "#E5484D" }
  - { label: Браузер, open: "https://ya.ru" }
  - { label: Терминал, run: wt, color: "#2E9E5B" }
  - { label: Код, run: code }
  - label: Медиа
    icon: headphones
    items:
      - { label: Назад, icon: backward-step, media: prev }
      - { label: Пауза, icon: pause, media: play-pause }
      - { label: Вперёд, icon: forward-step, media: next }
      - { label: Тише, volume: down }
      - { label: Громче, volume: up }
      - { label: Без звука, media: mute }
  - { label: Заметка, icon: pencil-to-square, action: note }
  - { label: Калькулятор, run: calc }
  - { label: Скриншот, keys: win+shift+s, color: "#8E4EC6" }
  - { label: Загрузки, icon: folder, open: "%USERPROFILE%/Downloads" }
  - { label: Настройки, icon: gear, action: settings }
  - { label: Копировать, icon: copy, keys: ctrl+c }
  - { label: Вставить, icon: paste, keys: ctrl+v }
"##;

/// Открыть меню, навести на кнопку `hover` (или никуда) и сохранить кадр.
fn sweep(
    path: &std::path::Path,
    menu: &Menu,
    theme: Theme,
    scale: f32,
    font: Option<&fontdue::Font>,
    desk: [u8; 3],
    hover: usize,
) {
    let mut e = Engine::new(Settings::default(), scale, (1920, 1080), menu.clone(), font, theme, &raster::no_program_icons);
    let n = e.window_size() as usize;
    let mut buf = vec![0u8; n * n * 4];
    let top = menu.top().len().max(1);
    let a = (menu.shape.start_angle + if menu.shape.clockwise { 1.0 } else { -1.0 } * 360.0 / top as f32 * hover as f32).to_radians();
    let d = if hover < top && hover > 0 { (a.cos() * 9.0, a.sin() * 9.0) } else { (0.0, 0.0) };
    e.press(0.0);
    let mut t = 0.0;
    while t < 0.8 {
        t += 0.008;
        let mut cv = Canvas::new(n, n, &mut buf);
        e.frame(t, if t < 0.4 { d } else { (0.0, 0.0) }, &Context::default(), &mut cv);
    }
    save_bgra(path, n, n, &buf, desk);
}

/// Провести указатель по цепочке кнопок до `target` (основная → пункт веера → вложенный).
fn sweep_to(path: &std::path::Path, menu: &Menu, theme: Theme, scale: f32, font: Option<&fontdue::Font>, desk: [u8; 3], target: usize) {
    use radial_menu::engine::layout::Layout;
    use radial_menu::engine::metrics::Metrics;
    let s = Settings::default();
    let m = Metrics::new(&s, scale, (1920, 1080), menu);
    let lay = Layout::new(menu, &m);
    let mut chain = vec![target];
    while let Some(p) = menu.node(*chain.last().unwrap()).parent.filter(|&p| p != 0) {
        chain.push(p);
    }
    chain.reverse();
    let mut e = Engine::new(s, scale, (1920, 1080), menu.clone(), font, theme, &raster::no_program_icons);
    let n = e.window_size() as usize;
    let mut buf = vec![0u8; n * n * 4];
    e.press(0.0);
    let (mut px, mut py) = (0.0f32, 0.0f32);
    let mut t = 0.0;
    let mut step = 0;
    while t < 1.4 {
        t += 0.008;
        let mut d = (0.0, 0.0);
        if step < chain.len() && t > 0.15 + 0.2 * step as f64 {
            let off = lay.slot(chain[step]).off;
            d = ((off.0 - px) / m.sens, (off.1 - py) / m.sens);
            (px, py) = off;
            step += 1;
        }
        let mut cv = Canvas::new(n, n, &mut buf);
        e.frame(t, d, &Context::default(), &mut cv);
    }
    save_bgra(path, n, n, &buf, desk);
}
