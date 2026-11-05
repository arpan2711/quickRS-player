//! The system's file dialogs (rfd: Windows' own, and the Mac's): a folder of songs, a keys file. They block the
//! window while open, as any native dialog does.

use std::path::{Path, PathBuf};

// Android has no rfd: the pickers find nothing (songs and keys go in the app's files folder, see android.rs)
#[cfg(target_os = "android")]
mod rfd {
    use std::path::{Path, PathBuf};
    pub struct FileDialog;
    impl FileDialog {
        pub fn new() -> Self { FileDialog }
        pub fn set_title(self, _: &str) -> Self { self }
        pub fn set_directory(self, _: &Path) -> Self { self }
        pub fn add_filter(self, _: &str, _: &[&str]) -> Self { self }
        pub fn pick_folder(self) -> Option<PathBuf> { None }
        pub fn pick_file(self) -> Option<PathBuf> { None }
    }
}

/// A folder of songs, starting at `from` when it's a folder.
pub fn pick_folder(from: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title("Add a folder of songs");
    if let Some(f) = from.filter(|f| f.is_dir()) {
        d = d.set_directory(f);
    }
    d.pick_folder()
}

/// A folder for something else (`title` says what), starting at `from` when it's a folder.
pub fn pick_folder_titled(title: &str, from: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title);
    if let Some(f) = from.filter(|f| f.is_dir()) {
        d = d.set_directory(f);
    }
    d.pick_folder()
}

/// A keys file (keys.json, or Notepad's keys.json.txt).
pub fn pick_keys_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load a keys file")
        .add_filter("Keys file", &["json", "txt"])
        .add_filter("All files", &["*"])
        .pick_file()
}
