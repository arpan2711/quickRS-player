//! The window's title bar in the theme's colours (Windows 11: DWM caption, text and border colours; Windows 10 just
//! gets a dark or light bar). Port of rsplay/app.py `style_title_bar` (REDESIGN.md "Window title bar", option A).

use qrs_ui::theme::t;

#[cfg(windows)]
pub fn style_title_bar(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
    use windows::core::BOOL;
    let th = t();
    let colorref = |c: egui::Color32| c.r() as u32 | (c.g() as u32) << 8 | (c.b() as u32) << 16;
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    let dark = BOOL::from(th.dark);
    // 20: immersive dark mode (19 before Windows 10 20H1); 35: caption colour (the app's one surface colour);
    // 36: text colour; 34: border colour (the dark shadow's), as the Python app sets them in every theme
    unsafe {
        if DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(20), &dark as *const BOOL as *const _, 4).is_err() {
            let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(19), &dark as *const BOOL as *const _, 4);
        }
        for (attr, c) in [(35, th.base), (36, th.text), (34, th.shade)] {
            let v = colorref(c);
            let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(attr), &v as *const u32 as *const _, 4);
        }
    }
}

#[cfg(not(windows))]
pub fn style_title_bar(_hwnd: isize) {}

use eframe::egui;

/// The native window handle, if this is a Win32 window.
pub fn hwnd_of(h: &impl raw_window_handle::HasWindowHandle) -> Option<isize> {
    match h.window_handle().ok()?.as_raw() {
        raw_window_handle::RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}
