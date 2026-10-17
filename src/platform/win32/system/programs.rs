//! Программы: найти exe по имени (как «Выполнить»), достать иконку файла или программы,
//! узнать, какая программа сейчас активна.

use std::path::{Path, PathBuf};

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Storage::FileSystem::{SearchPathW, FILE_FLAGS_AND_ATTRIBUTES};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE, SHIL_JUMBO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetForegroundWindow, GetIconInfo, GetWindowThreadProcessId, HICON, ICONINFO,
};

use super::util::{pcwstr, wide};
use crate::render::raster::{fit, Rgba};

/// Полный путь к программе или файлу: как есть, через PATH или через «App Paths» реестра.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return Some(path.to_path_buf());
    }
    // только имя (notepad, code) — ищем так же, как «Выполнить»
    if path.components().count() != 1 {
        return None;
    }
    let name = path.to_string_lossy();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { SearchPathW(None, pcwstr(&wide(&name)), pcwstr(&wide(".exe")), Some(&mut buf), None) };
    if n > 0 && (n as usize) < buf.len() {
        return Some(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])));
    }
    let exe = if name.to_lowercase().ends_with(".exe") {
        name.to_string()
    } else {
        format!("{name}.exe")
    };
    let key = wide(&format!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe}"));
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut out = vec![0u16; 1024];
        let mut size = (out.len() * 2) as u32;
        let ok = unsafe {
            RegGetValueW(
                root,
                pcwstr(&key),
                None,
                RRF_RT_REG_SZ,
                None,
                Some(out.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        }
        .is_ok();
        if ok {
            let len = out.iter().position(|&c| c == 0).unwrap_or(0);
            let p = PathBuf::from(String::from_utf16_lossy(&out[..len]).trim_matches('"'));
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Иконка файла, папки или программы — как в Проводнике, крупного размера.
/// Возвращает квадрат `size`×`size`, RGBA с предумноженной альфой.
pub fn icon(path: &Path, size: u32) -> Option<Rgba> {
    let full = resolve(path)?;
    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let p = wide(&full.to_string_lossy());
        let ok = SHGetFileInfoW(
            pcwstr(&p),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        );
        if ok == 0 {
            return None;
        }
        let which = if size > 48 { SHIL_JUMBO } else { SHIL_EXTRALARGE };
        let list: IImageList = SHGetImageList(which as i32).ok()?;
        let hicon = list.GetIcon(sfi.iIcon, ILD_TRANSPARENT.0).ok()?;
        let img = hicon_rgba(hicon);
        let _ = DestroyIcon(hicon);
        Some(fit(crop(img?), size as usize))
    }
}

/// Пиксели значка (RGBA, предумноженная альфа).
unsafe fn hicon_rgba(hicon: HICON) -> Option<Rgba> {
    let mut ii = ICONINFO::default();
    GetIconInfo(hicon, &mut ii).ok()?;
    let color = bits(ii.hbmColor);
    let mask = bits(ii.hbmMask);
    let _ = DeleteObject(ii.hbmColor.into());
    let _ = DeleteObject(ii.hbmMask.into());
    let (w, h, mut px) = color?;
    // старые значки без альфы: прозрачность берём из маски (белое — прозрачно)
    if px.chunks_exact(4).all(|p| p[3] == 0) {
        if let Some((mw, _, m)) = mask {
            if mw == w {
                for (p, mk) in px.chunks_exact_mut(4).zip(m.chunks_exact(4)) {
                    p[3] = if mk[0] > 127 { 0 } else { 255 };
                }
            }
        }
    }
    // BGRA (обычная альфа) → RGBA (предумноженная)
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        let (b, g, r) = (p[0], p[1], p[2]);
        p[0] = m(r);
        p[1] = m(g);
        p[2] = m(b);
    }
    Some(Rgba { w, h, data: px })
}

/// Битмап → 32-битные пиксели сверху вниз.
unsafe fn bits(bmp: HBITMAP) -> Option<(usize, usize, Vec<u8>)> {
    if bmp.is_invalid() {
        return None;
    }
    let mut bm = BITMAP::default();
    if GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth.max(0) as usize, bm.bmHeight.max(0) as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; w * h * 4];
    let dc = GetDC(None);
    let got = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (got > 0).then_some((w, h, buf))
}

/// Обрезать прозрачные поля (у крупных списков значков маленькая иконка сидит в углу).
fn crop(img: Rgba) -> Rgba {
    let (mut x0, mut y0, mut x1, mut y1) = (img.w, img.h, 0, 0);
    for y in 0..img.h {
        for x in 0..img.w {
            if img.data[(y * img.w + x) * 4 + 3] > 8 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return img;
    }
    // квадрат вокруг содержимого, чтобы значок не растягивался
    let side = (x1 - x0).max(y1 - y0);
    let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
    let mut out = vec![0u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let sx = (cx + x) as isize - (side / 2) as isize;
            let sy = (cy + y) as isize - (side / 2) as isize;
            if sx >= 0 && sy >= 0 && (sx as usize) < img.w && (sy as usize) < img.h {
                let s = (sy as usize * img.w + sx as usize) * 4;
                out[(y * side + x) * 4..(y * side + x) * 4 + 4].copy_from_slice(&img.data[s..s + 4]);
            }
        }
    }
    Rgba {
        w: side,
        h: side,
        data: out,
    }
}

/// Имя exe активного окна (`code.exe`), чтобы выбрать меню для этой программы.
pub fn foreground_exe() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&full).file_name().map(|n| n.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_programs_by_name_and_gets_icons() {
        let notepad = resolve(Path::new("notepad")).expect("notepad в PATH");
        assert!(notepad.to_string_lossy().to_lowercase().ends_with("notepad.exe"));
        assert!(resolve(Path::new("точно-нет-такой-программы")).is_none());
        // для иконок нужен COM на потоке
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let img = icon(Path::new("notepad"), 48).expect("иконка блокнота");
        assert_eq!((img.w, img.h), (48, 48));
        assert!(img.data.chunks_exact(4).filter(|p| p[3] > 200).count() > 300, "иконка не пустая");
        let dir = icon(&std::env::temp_dir(), 32).expect("иконка папки");
        assert_eq!(dir.w, 32);
    }

    #[test]
    fn crop_squares_content() {
        let mut data = vec![0u8; 10 * 10 * 4];
        for y in 0..2 {
            for x in 0..4 {
                data[(y * 10 + x) * 4 + 3] = 255;
            }
        }
        let c = crop(Rgba { w: 10, h: 10, data });
        assert_eq!((c.w, c.h), (4, 4));
    }
}
