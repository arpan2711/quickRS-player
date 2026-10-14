//! Small animation helpers for subtle motion (port of rsplay/anim.py). `smooth` eases a named value toward a
//! target every frame (frame-rate independent exponential approach): hover fades, expanding rows, tiles sliding,
//! a segmented control's key gliding across. Asks egui for another frame while anything is still moving.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;

/// Hover / press feedback (seconds).
pub const FAST: f32 = 0.09;
/// Expand / collapse, tiles settling after a drag.
pub const MEDIUM: f32 = 0.16;

thread_local! {
    static VALUES: RefCell<HashMap<u64, f32>> = RefCell::new(HashMap::new());
}

/// Cubic ease-out on 0..1.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Move `key` toward `target` with time constant `tau` (about 95% of the way in 3 tau). The first call starts at
/// `start` (or the target, so nothing animates on first sight).
pub fn smooth(ctx: &egui::Context, key: impl Hash, target: f32, tau: f32, start: Option<f32>) -> f32 {
    let k = crate::paint::key_of(key);
    let dt = ctx.input(|i| i.stable_dt).min(0.1);
    VALUES.with(|m| {
        let mut m = m.borrow_mut();
        let v = m.entry(k).or_insert(start.unwrap_or(target));
        let f = if dt > 0.0 {
            1.0 - (-dt / tau.max(1e-4)).exp()
        } else {
            0.0
        };
        *v += (target - *v) * f;
        if (target - *v).abs() < 0.25 / 255.0 {
            *v = target;
        } else {
            ctx.request_repaint();
        }
        *v
    })
}

/// Forget a value (it starts afresh next time).
pub fn forget(key: impl Hash) {
    let k = crate::paint::key_of(key);
    VALUES.with(|m| m.borrow_mut().remove(&k));
}
