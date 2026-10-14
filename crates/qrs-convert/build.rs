//! The Windows exe's resources: quickRS's icon and the converter's version information.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/rsplay.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/rsplay.ico")
            .set("ProductName", "quickRS Converter")
            .set(
                "FileDescription",
                "quickRS Converter: Rocksmith .psarc songs to feedpaks",
            )
            .set("OriginalFilename", "quickrs-convert.exe")
            .set("InternalName", "quickrs-convert");
        if let Err(e) = res.compile() {
            println!("cargo:warning=the exe has no icon resource: {e}");
        }
    }
}
