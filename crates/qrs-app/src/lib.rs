//! quickRS app as a library: the desktop binary (main.rs) and the Android build (android.rs) both start it.

mod ampbutton;
mod app;
mod applog;
#[cfg(feature = "nux")]
mod assets;
mod backdrop;
mod calibrate;
mod chrome;
mod covers;
mod devstatus;
mod dialogs;
mod gallery;
mod gearindex;
mod gpu;
mod guitar_setup;
mod home;
mod keyfile;
mod library;
mod listen;
mod loading;
#[cfg(feature = "nux")]
mod mg300;
mod notewidget;
mod pedalboard;
mod pedalfaces;
mod picks;
mod player;
mod preview;
mod recording;
mod room;
mod scales;
mod settings;
mod setup;
mod shotclock;
mod shots;
mod state;
mod tones;
mod transport;
mod update;
#[cfg(target_os = "android")]
pub mod android;

pub fn icon() -> egui::IconData {
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
pub fn first_size() -> [f32; 2] {
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
pub fn attach_console() {
    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}


/// The app's own options for a window of `size` (points), then run until it closes.
pub fn run(args: app::Args, mut opts: eframe::NativeOptions) -> anyhow::Result<()> {
    gpu::choose(&mut opts);
    if args.screenshots.is_some() {
        // (frames mustn't wait on a display that's gone to sleep)
        opts.wgpu_options.surface.present_mode = eframe::egui_wgpu::wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "quickRS",
        opts,
        Box::new(move |cc| Ok(Box::new(app::QuickRs::new(cc, args)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// The desktop start (main.rs).
pub fn desktop_main() -> anyhow::Result<()> {
    applog::mark_start();
    if std::env::args().len() > 1 {
        attach_console();
    }
    let args = app::Args::parse(std::env::args().skip(1))?;
    // started again by the updater: once the old app has closed, its leftovers go (P20.1)
    if let Some(pid) = args.after_update {
        update::wait_for_exit(pid);
    }
    update::cleanup_this();
    // the Python app's look (screenshot parity, tools/parity/shots_compare.py); otherwise round 3
    if std::env::var("QUICKRS_LOOK").is_ok_and(|v| v == "classic") {
        qrs_ui::theme::set_classic(true);
    }
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
    run(args, opts)
}
