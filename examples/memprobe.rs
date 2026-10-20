use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use radial_menu::engine::Engine;
use radial_menu::model::menu_yaml::{self, NAV_TEMPLATE};
use radial_menu::model::{default_menu, Settings, Theme};
use radial_menu::render::{assets, raster};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = System.realloc(p, l, n);
        if !q.is_null() {
            if n > l.size() {
                let now = LIVE.fetch_add(n - l.size(), Relaxed) + n - l.size();
                PEAK.fetch_max(now, Relaxed);
            } else {
                LIVE.fetch_sub(l.size() - n, Relaxed);
            }
        }
        q
    }
}
#[global_allocator]
static A: Counting = Counting;

fn kb() -> usize {
    LIVE.load(Relaxed) / 1024
}

fn main() {
    println!("старт: {} КБ", kb());
    let font = raster::load_font();
    println!("шрифт загружен: {} КБ", kb());
    let before = kb();
    let main_menu = default_menu();
    let nav = menu_yaml::parse(NAV_TEMPLATE, None, &assets::exists).unwrap().main;
    let e1 = Engine::new(Settings::default(), 1.0, (1920, 1080), main_menu, font.as_ref(), Theme::Dark, &raster::no_program_icons);
    println!("+ движок основного меню: {} КБ", kb() - before);
    let b2 = kb();
    let e2 = Engine::new(Settings::default(), 1.0, (1920, 1080), nav, font.as_ref(), Theme::Dark, &raster::no_program_icons);
    println!("+ движок «Навигация»: {} КБ", kb() - b2);
    let b3 = kb();
    drop(font);
    println!("шрифт выгружен: {} КБ (освободилось {} КБ)", kb(), b3 - kb());
    println!("пик за всё время: {} КБ", PEAK.load(Relaxed) / 1024);
    let b4 = kb();
    let fonts = radial_menu::ui::settings_form::Fonts::load();
    println!("шрифты окна настроек: +{} КБ", kb() - b4);
    drop(fonts);
    let _ = (e1, e2);
}
