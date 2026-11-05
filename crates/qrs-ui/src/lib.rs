//! quickRS look: themes, fonts, the neumorphic component kit (port of rsplay/ui.py, neu.py, material.py,
//! anim.py; assets/design/ holds the design (the brief and boards), docs/DECISIONS.md D15 how it's drawn in egui).
//!
//! One egui point is one scale-1 pixel of the Python app: the app zooms egui so `pixels_per_point` is the Python
//! scale, and every size here is the Python number.

pub mod anim;
pub mod glass;
pub mod hall;
pub mod icons;
pub mod keys;
pub mod knob_art;
pub mod knobs;
pub mod material;
pub mod paint;
pub mod pyfont;
pub mod quiet;
pub mod raster;
pub mod streak;
pub mod text;
pub mod theme;
pub mod units;
pub mod widgets;

pub use icons::Icon;
pub use material::Mat;
pub use paint::Fill;
pub use theme::t;
