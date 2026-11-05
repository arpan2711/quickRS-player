//! The Android start: `android_main` is what the NativeActivity calls.
//!
//! Everything quickRS keeps (settings, scores, keys.json) lives in the app's external files folder,
//! `/sdcard/Android/data/<package>/files`, and a `songs` folder inside it is read for psarcs and feedpaks.
//! Copy songs and keys.json there over USB or with `adb push`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eframe::egui;
use jni::JavaVM;
use jni::objects::{JObject, JValue};
use winit::platform::android::activity::AndroidApp;

const MIC: &str = "android.permission.RECORD_AUDIO";
static MIC_GRANTED: AtomicBool = AtomicBool::new(false);

/// True once, on the first frame after the microphone permission was granted (the audio then opens again).
pub fn mic_just_granted() -> bool {
    MIC_GRANTED.swap(false, Ordering::Relaxed)
}

/// Whether the activity holds `perm` (Activity.checkSelfPermission).
fn has_permission(vm: &JavaVM, activity: &JObject, perm: &str) -> jni::errors::Result<bool> {
    let mut env = vm.attach_current_thread()?;
    let p = env.new_string(perm)?;
    let r = env
        .call_method(activity, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&p)])?
        .i()?;
    Ok(r == 0) // PackageManager.PERMISSION_GRANTED
}

/// The guitar input needs RECORD_AUDIO, a runtime permission: ask for it (the system's dialog) when it isn't held,
/// then watch for the answer and flag it so the app opens its input again.
fn ask_for_microphone(app: &AndroidApp) {
    let (vm_ptr, act_ptr) = (app.vm_as_ptr() as usize, app.activity_as_ptr() as usize);
    std::thread::spawn(move || {
        let result = (|| -> jni::errors::Result<()> {
            // SAFETY: the VM and the activity outlive the app's threads
            let vm = unsafe { JavaVM::from_raw(vm_ptr as *mut jni::sys::JavaVM)? };
            let activity = unsafe { JObject::from_raw(act_ptr as jni::sys::jobject) };
            if has_permission(&vm, &activity, MIC)? {
                return Ok(());
            }
            {
                let mut env = vm.attach_current_thread()?;
                let p = env.new_string(MIC)?;
                let arr = env.new_object_array(1, "java/lang/String", &p)?;
                env.call_method(
                    &activity,
                    "requestPermissions",
                    "([Ljava/lang/String;I)V",
                    &[JValue::Object(&arr), JValue::Int(1)],
                )?;
            }
            log::info!("asked for the microphone");
            // the answer arrives in onRequestPermissionsResult, which NativeActivity doesn't pass on: poll
            for _ in 0..600 {
                std::thread::sleep(Duration::from_millis(500));
                if has_permission(&vm, &activity, MIC)? {
                    log::info!("microphone granted");
                    MIC_GRANTED.store(true, Ordering::Relaxed);
                    return Ok(());
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            log::warn!("microphone permission: {e}");
        }
    });
}

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
    ask_for_microphone(&app);
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
