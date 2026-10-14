//! For a parity check against rsplay/material.py: each material's sheet at scale 1, as its mean colour and a few
//! pixels.   cargo run -p qrs-ui --example sheet_dump
use qrs_ui::material::{Mat, build_sheet};

fn main() {
    for (name, m) in [
        ("tortoise", Mat::Tortoise),
        ("abalone", Mat::Abalone),
        ("aluminium", Mat::Aluminium),
    ] {
        let s = build_sheet(m, 1.0);
        let n = s.rgb.len() as f32;
        let mean: Vec<f32> = (0..3)
            .map(|c| s.rgb.iter().map(|p| p[c]).sum::<f32>() / n)
            .collect();
        let px = |x: usize, y: usize| s.rgb[y * s.w + x];
        println!(
            "{name} {}x{} mean {:.2} {:.2} {:.2} | {:?} {:?} {:?}",
            s.w,
            s.h,
            mean[0],
            mean[1],
            mean[2],
            px(0, 0).map(|v| v.round()),
            px(100, 50).map(|v| v.round()),
            px(359, 139).map(|v| v.round())
        );
    }
}
