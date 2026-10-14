//! The Python app's layout numbers, as points. It scales each one to whole pixels on its own: `int(n * scale)`
//! (rounded toward zero) in the screens, `int(round(n * scale))` (`neu.px`) in the components. Doing the same
//! here puts every edge where the Python app puts it, to the pixel; plain `n` points drift by up to a pixel per
//! number, which adds up down a column of tiles or a list.

use std::sync::atomic::{AtomicU64, Ordering};

/// The window's scale, worked out as the Python app does (`max(0.7, min(2.2, min(w / 1400, h / 820)))` of the
/// window's whole-pixel size, in f64). 0 until the app sets it: numbers then pass through unchanged.
static SCALE: AtomicU64 = AtomicU64::new(0);

pub fn set_scale(s: f64) {
    SCALE.store(s.to_bits(), Ordering::Relaxed);
}

pub fn scale() -> f64 {
    f64::from_bits(SCALE.load(Ordering::Relaxed))
}

/// `int(n * s)`: `n` scale-1 pixels as the Python app's screens make them, whole pixels rounded toward zero, in
/// points.
pub fn px(n: f32) -> f32 {
    let s = scale();
    if s <= 0.0 {
        return n;
    }
    ((n as f64 * s).trunc() / s) as f32
}

/// `int(round(n * s))` (`neu.px`), in points: Python rounds halves to even.
pub fn pxr(n: f32) -> f32 {
    let s = scale();
    if s <= 0.0 {
        return n;
    }
    ((n as f64 * s).round_ties_even() / s) as f32
}

/// `n` physical pixels (a Python constant that isn't scaled, like `xa + 1` or `max(6, ...)`), in points.
pub fn pixels(n: f32) -> f32 {
    let s = scale();
    if s <= 0.0 {
        return n;
    }
    (n as f64 / s) as f32
}

/// Points as the Python app's (fractional) pixels, for its pixel-valued arithmetic (`max(6, int(w * 0.034))`).
pub fn to_px(v: f32) -> f64 {
    let s = scale();
    if s <= 0.0 { v as f64 } else { v as f64 * s }
}

/// A position or length already in points (a sum of `px` numbers, a rect edge) as the whole pixel the Python app
/// would have there (`int(v)` of its pixels). The allowance (1/1000 px, toward the sign) covers f32's rounding of
/// whole pixels held as points, which can come out a hair under the pixel.
pub fn snap(v: f32) -> f32 {
    let s = scale();
    if s <= 0.0 {
        return v;
    }
    let x = v as f64 * s;
    ((x + 1e-3f64.copysign(x)).trunc() / s) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_python() {
        set_scale(1440.0 / 1400.0); // 1.0285714285714285
        let s = 1440.0f64 / 1400.0;
        // Python: int(27 * s) = 27, int(32 * s) = 32, int(35 * s) = 36 (35 * s is exactly 36.0), int(-4.5 * s) = -4
        for (n, want) in [
            (27.0, 27.0),
            (32.0, 32.0),
            (35.0, 36.0),
            (-4.5, -4.0),
            (100.0, 102.0),
        ] {
            assert!(
                ((px(n) as f64) * s - want).abs() < 1e-4,
                "{n}: {}",
                px(n) as f64 * s
            );
        }
        set_scale(1.0);
        assert_eq!(pxr(2.5), 2.0); // round half to even, as Python
        assert_eq!(pxr(3.5), 4.0);
        set_scale(0.0);
        assert_eq!(px(16.5), 16.5);
    }
}
