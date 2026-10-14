//! Where settings, user data and caches live (D6): `QUICKRS_DATA` if set (tests, screenshots); else next to the
//! program when a `config.json` or `userdata.json` is there (a portable install); else the OS data folder
//! (`%APPDATA%\quickRS` on Windows).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct DataDir {
    pub root: PathBuf,
}

impl DataDir {
    pub fn locate() -> DataDir {
        if let Some(d) = std::env::var_os("QUICKRS_DATA") {
            return DataDir::at(PathBuf::from(d));
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            && (exe_dir.join("config.json").is_file() || exe_dir.join("userdata.json").is_file())
        {
            return DataDir::at(exe_dir);
        }
        let root = directories::ProjectDirs::from("", "", "quickRS")
            .map(|p| p.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        DataDir::at(root)
    }

    pub fn at(root: PathBuf) -> DataDir {
        let _ = std::fs::create_dir_all(&root);
        DataDir { root }
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn userdata(&self) -> PathBuf {
        self.root.join("userdata.json")
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join(".quickrs-cache")
    }

    pub fn index(&self) -> PathBuf {
        self.cache().join("library.json")
    }

    pub fn art(&self) -> PathBuf {
        self.cache().join("art")
    }

    /// keys.json next to the data (the Python app kept it in its folder).
    pub fn keys(&self) -> PathBuf {
        self.root.join("keys.json")
    }
}

/// Write `text` to `path` through a temporary file, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
