//! Pitch, onset and note detection, scoring and key guessing on audio buffers (no devices, no UI).

pub mod detect;
pub mod keyguess;
pub mod pitch;
pub mod spectrum;

pub use detect::{Judgement, Pluck, SampleSource, Scorer};
pub use spectrum::Spectra;
