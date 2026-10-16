// Иконка самого exe (в Проводнике). Работает при сборке на Windows; в остальных случаях пропускается.
fn main() {
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/RadialMenu.ico");
            res.set("ProductName", "Радиальное меню");
            res.set("FileDescription", "Радиальное меню на боковой кнопке мыши");
            if let Err(e) = res.compile() {
                println!("cargo:warning=иконка exe не встроена: {e}");
            }
        }
    }
}
