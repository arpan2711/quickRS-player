//! quickRS desktop app.
//!
//!   quickrs                      the app
//!   quickrs --silent             the app on no sound devices (nothing is heard; a simulated MG-300)
//!   quickrs --gallery            the component gallery (a debug screen)
//!   quickrs --screenshots DIR    render the screens in every theme to PNGs in DIR, then quit
//!           [--size 1440x900]    (physical pixels)

// release builds are a windowed app (no console window); messages for the command line reach the terminal it
// was started from (see `attach_console`)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod applog;
#[cfg(feature = "nux")]
mod assets;
mod backdrop;
mod chrome;
mod covers;
mod devstatus;
mod gallery;
mod home;
mod library;
mod loading;
#[cfg(feature = "nux")]
mod mg300;
mod notewidget;
mod picks;
mod player;
mod preview;
mod settings;
mod shots;
mod state;
mod tones;
mod transport;

fn icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icons/rsplay.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let img = img
                .resize(256, 256, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            egui::IconData {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            }
        }
        Err(_) => egui::IconData::default(),
    }
}

use eframe::egui;

/// The window's first size, as the Python app opens it: 80% of the primary desktop, in whole pixels
/// (`int(desk * 0.8)`), given to eframe as points (with Windows at 125%, 2560 x 1440 pixels open a 2048 x 1152
/// pixel window, which is 1638.4 x 921.6 points).
fn first_size() -> [f32; 2] {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Graphics::Gdi::{
            DESKTOPHORZRES, DESKTOPVERTRES, GetDC, GetDeviceCaps, ReleaseDC,
        };
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext,
        };
        // (winit makes the process DPI-aware too, later; until then the system DPI would read as 96)
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let dc = GetDC(None);
        let (w, h) = (
            GetDeviceCaps(Some(dc), DESKTOPHORZRES),
            GetDeviceCaps(Some(dc), DESKTOPVERTRES),
        );
        ReleaseDC(None, dc);
        let scale = GetDpiForSystem() as f32 / 96.0;
        if w > 0 && h > 0 && scale > 0.0 {
            let (pw, ph) = ((w as f32 * 0.8).floor(), (h as f32 * 0.8).floor());
            return [pw / scale, ph / scale];
        }
    }
    [1440.0, 900.0]
}

/// A windowed exe started from a terminal with arguments: print to that terminal.
fn attach_console() {
    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() -> anyhow::Result<()> {
    applog::mark_start();
    if std::env::args().len() > 1 {
        attach_console();
    }
    let args = app::Args::parse(std::env::args().skip(1))?;
    // --size (screenshots): that many pixels, which the screenshot mode makes exact once the window is up
    let size = match args.size {
        Some((w, h)) => [w as f32, h as f32],
        None => first_size(),
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("quickRS Bristol 2026")
            .with_inner_size(size)
            .with_min_inner_size([980.0, 574.0])
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "quickRS",
        opts,
        Box::new(move |cc| Ok(Box::new(app::QuickRs::new(cc, args)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
