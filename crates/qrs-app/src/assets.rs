//! Where the asset files are at run time (the MG-300 pictures; fonts and icons are compiled in): QUICKRS_ASSETS,
//! else an `assets` folder next to the exe (the release zip), else the repo's (running from target/).

use std::path::PathBuf;
use std::sync::OnceLock;

pub fn dir() -> Option<&'static PathBuf> {
    static D: OnceLock<Option<PathBuf>> = OnceLock::new();
    D.get_or_init(|| {
        let mut tries: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::var("QUICKRS_ASSETS") {
            tries.push(d.into());
        }
        if let Some(exe) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            tries.push(exe.join("assets"));
            tries.push(exe.join("../../assets")); // target/release -> the repo
        }
        tries.push(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets"
        )));
        tries.into_iter().find(|d| d.join("mg300").is_dir())
    })
    .as_ref()
}
