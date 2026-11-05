//! quickRS desktop app.
//!
//!   quickrs                      the app
//!   quickrs --silent             the app on no sound devices (nothing is heard; a simulated MG-300)
//!   quickrs --gallery            the component gallery (a debug screen)
//!   quickrs --screenshots DIR    render the screens in every theme to PNGs in DIR, then quit
//!           [--size 1440x900]    (physical pixels)
//!   quickrs --after-update PID   (the updater's restart: waits for the old app to close, P20.1)
//!   quickrs --resume FILE        (a new dev build swapped in: opens where the old one was, D67)

// release builds are a windowed app (no console window); messages for the command line reach the terminal it
// was started from (see `attach_console`)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ampbutton;
mod app;
mod applog;
#[cfg(feature = "nux")]
mod assets;
mod backdrop;
mod calibrate;
mod captures;
mod chrome;
mod covers;
mod devstatus;
mod devswap;
mod dialogs;
mod downloads;
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
mod namcheck;
mod notewidget;
mod pedalboard;
mod pedalfaces;
mod picks;
mod playcheck;
mod player;
mod preview;
mod recording;
mod room;
mod settings;
mod setup;
mod shotclock;
mod shots;
mod songtones;
mod state;
mod timing_cal;
mod tones;
mod transport;
mod update;
mod whatsnew;

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
    // the log from the start, so a window that never opens still leaves a reason (P28.3)
    applog::init(qrs_library::paths::DataDir::locate().cache());
    if std::env::args().len() > 1 {
        attach_console();
    }
    // (P33.7: the NAM picker checked on the library, no window)
    let argv: Vec<String> = std::env::args().collect();
    if let Some(i) = argv.iter().position(|a| a == "--nam-check") {
        let out = argv.get(i + 1).map(String::as_str).unwrap_or("nam-check");
        let n = argv.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(60);
        return namcheck::run(std::path::Path::new(out), n);
    }
    // (P38.2: every song tone in the library on our models and with NAM, no window)
    if let Some(i) = argv.iter().position(|a| a == "--song-tones") {
        let out = argv.get(i + 1).map(String::as_str).unwrap_or("song-tones");
        return songtones::run(std::path::Path::new(out));
    }
    if argv.iter().any(|a| a == "--nam-calibrate") {
        let data = qrs_library::paths::DataDir::locate();
        captures::init_quiet(&data.root, &qrs_library::Config::load(data.config()));
        let n = namcheck::calibrate(true);
        println!("nam-calibrate: {n} trims measured");
        return Ok(());
    }
    let mut args = app::Args::parse(std::env::args().skip(1))?;
    // a new dev build swapped in (D67): where the old one was
    args.resumed = args.resume.as_deref().and_then(devswap::take_resume);
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("quickRS Bristol 2026")
        // (Linux: the desktop matches the window to quickrs.desktop, its name and icon, by this)
        .with_app_id("quickrs")
        .with_inner_size(size)
        .with_min_inner_size([980.0, 574.0])
        .with_icon(icon());
    if let Some(r) = &args.resumed {
        if let Some([x, y, w, h]) = r.window {
            viewport = viewport.with_position([x, y]).with_inner_size([w, h]);
        }
        viewport = viewport.with_maximized(r.maximized).with_fullscreen(r.fullscreen);
    }
    let mut opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
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
    .map_err(|e| {
        // (no window: started from a desktop, the log is the only place this shows. P28.3)
        applog::write(&format!("couldn't open the window: {e}"));
        anyhow::anyhow!("{e}")
    })
}
