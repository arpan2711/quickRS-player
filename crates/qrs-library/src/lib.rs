//! Song library: config.json and userdata.json in the Python app's shapes, the scan of psarc and feedpak folders
//! with a cached index, duplicates grouped into one row per song, and album art. PLAN.md section 5, D6, D7.
//! Also the psarc -> feedpak exporter (`feedpak_export`, P9.3), and the lessons and their progress (P13.3).

pub mod art;
pub mod config;
pub mod difficulty;
pub mod feedpak_export;
pub mod group;
pub mod lessons;
pub mod migrate;
pub mod paths;
pub mod scan;
pub mod userdata;

pub use config::Config;
pub use difficulty::PartLevel;
pub use group::{Group, Prefer, group};
pub use lessons::{Lesson, Riff, RiffProgress};
pub use paths::DataDir;
pub use scan::{Library, PartEntry, SongEntry};
pub use userdata::{Lick, UserData};
