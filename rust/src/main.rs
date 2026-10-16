#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    radial_menu::platform::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("RadialMenu работает только в Windows (ядро собирается и тестируется везде).");
    std::process::exit(1);
}
