//! The Windows exe's resources: the app icon (Explorer, the taskbar, shortcuts) and its version information.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/rsplay.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/rsplay.ico")
            .set("ProductName", "quickRS Bristol 2026")
            .set(
                "FileDescription",
                "quickRS Bristol 2026: Rocksmith song library and player",
            )
            .set("OriginalFilename", "quickrs.exe")
            .set("InternalName", "quickrs");
        if let Err(e) = res.compile() {
            // no Windows SDK resource compiler: the exe works, just without its icon in Explorer
            println!("cargo:warning=the exe has no icon resource: {e}");
        }
    }
}
