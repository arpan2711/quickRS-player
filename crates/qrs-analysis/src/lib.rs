//! Pitch, onset and note detection, scoring and key guessing on audio buffers (no devices, no UI), and the ear
//! training's note-by-note checks.

pub mod chordname;
pub mod detect;
pub mod ear;
pub mod hwcamera;
pub mod keyguess;
pub mod onsets;
pub mod parts;
pub mod pitch;
pub mod similar;
pub mod spectrum;
pub mod take;

pub use detect::{Held, Judgement, Pluck, SampleSource, Scorer};
pub use spectrum::Spectra;
