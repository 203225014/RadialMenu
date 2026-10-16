//! Рисует кадры меню в PNG без Windows — для проверки внешнего вида.
//! Запуск: cargo run --release --example preview -- <папка для PNG>

use radial_menu::compose::Canvas;
use radial_menu::config::Settings;
use radial_menu::engine::Engine;
use radial_menu::menu::{default_menu, Context};
use radial_menu::raster;

fn save(path: &std::path::Path, w: usize, h: usize, bgra: &[u8]) {
    // кладём предумноженный BGRA на тёмно-серый фон «рабочего стола»
    let mut rgb = Vec::with_capacity(w * h * 3);
    for p in bgra.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        for c in [p[2], p[1], p[0]] {
            rgb.push((c as f32 + 52.0 * (1.0 - a)).min(255.0) as u8);
        }
    }
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgb).unwrap();
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
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
    );
    eprintln!("запуск ядра: {:?}", t0.elapsed());
    let n = e.window_size() as usize;
    let mut buf = vec![0u8; n * n * 4];
    let ctx = Context {
        media_playing: true,
    };
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
        save(&dir.join(format!("{name}.png")), n, n, &buf);
    }
    e.wheel(true, t);
    let mut cv = Canvas::new(n, n, &mut buf);
    e.frame(t + 0.008, (0.0, 0.0), &ctx, &mut cv);
    save(&dir.join("6_toast.png"), n, n, &buf);
    eprintln!(
        "кадров: {frames}, среднее на кадр: {:?}, окно {n}×{n}",
        render_time / frames
    );
}
