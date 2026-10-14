//! Rocksmith 2014 psarc/sng/xml and feedpak song formats, the chart model, tab rendering.
//!
//! Pure code: no audio devices, no UI, no threads. Everything here is a port of the Python reference
//! (`../rocksmith-player/rsplay`, `../rs-open-format/rsfeedpak`); each module names its source.

pub mod ascii_tab;
pub mod compare;
pub mod crypto;
pub mod error;
pub mod feedpak;
pub mod glob;
pub mod keys;
pub mod lyrics;
pub mod model;
pub mod package;
pub mod psarc;
pub mod rhythm;
pub mod sng;
pub mod song_audio;
pub mod songinfo;
pub mod wem;
pub mod xml;

#[cfg(any(test, feature = "testutil"))]
pub mod testutil;

pub use error::{Error, Result};
pub use keys::{Keys, load_keys};
pub use model::{Arrangement, Chord, Note, Techs};
pub use package::{ArrangementInfo, Format, Package, load_arrangement};
