//! The Android start: `android_main` is what the NativeActivity calls.
//!
//! Everything quickRS keeps (settings, scores, keys.json) lives in the app's external files folder,
//! `/sdcard/Android/data/<package>/files`, and a `songs` folder inside it is read for psarcs and feedpaks.
//! Copy songs and keys.json there over USB or with `adb push`.

use eframe::egui;
use winit::platform::android::activity::AndroidApp;

/// The app's files folder, where the data and the songs go.
pub fn files_dir(app: &AndroidApp) -> std::path::PathBuf {
    app.external_data_path()
        .or_else(|| app.internal_data_path())
        .unwrap_or_else(|| std::path::PathBuf::from("/data/local/tmp/quickRS"))
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("quickRS"),
    );
    let files = files_dir(&app);
    // (the first-time setup offers <data>/songs as the songs folder: Android has no Music folder for quickRS)
    let _ = std::fs::create_dir_all(files.join("songs"));
    // SAFETY: set before any other thread starts
    unsafe { std::env::set_var("QUICKRS_DATA", &files) };
    log::info!("quickRS data in {}", files.display());
    crate::applog::mark_start();
    let args = match crate::app::Args::parse(std::iter::empty()) {
        Ok(a) => a,
        Err(e) => {
            log::error!("{e}");
            return;
        }
    };
    let opts = eframe::NativeOptions {
        android_app: Some(app),
        viewport: egui::ViewportBuilder::default().with_title("quickRS"),
        ..Default::default()
    };
    if let Err(e) = crate::run(args, opts) {
        log::error!("quickRS stopped: {e}");
    }
}
