//! Which graphics API the window draws with: new in quickRS (the Python app drew with pygame).
//!
//! wgpu's own choice on Windows is Vulkan first. Some Intel integrated-graphics drivers crash inside their Vulkan
//! driver (igvk64.dll) before the first frame, so the window flashes and the process is gone (a 0.1.0 user's report,
//! 2026-10-16). Windows draws with DirectX 12 instead, as RS and most games do. A start that dies before its
//! first frame leaves a marker behind, and the next start falls back to OpenGL. `QUICKRS_GPU` (or wgpu's own
//! `WGPU_BACKEND`): `dx12`, `vulkan`, `gl` or a comma list picks it by hand. Other systems keep wgpu's choice.

use eframe::egui_wgpu::{WgpuSetup, wgpu};
use std::path::PathBuf;

/// The marker a start writes before the window opens and removes once it has drawn.
fn marker() -> PathBuf {
    std::env::temp_dir().join("quickrs-gpu-starting")
}

/// The order Windows tries them in, one further on after each start that closed before drawing.
const WINDOWS_ORDER: [wgpu::Backends; 3] = [wgpu::Backends::DX12, wgpu::Backends::GL, wgpu::Backends::VULKAN];

/// The backends to try: by hand, else the one after the backend (`failed`) of a start that never drew, else this
/// system's default.
pub fn backends(env: Option<&str>, failed: Option<wgpu::Backends>) -> Option<wgpu::Backends> {
    if let Some(v) = env.map(str::trim).filter(|v| !v.is_empty()) {
        let b = wgpu::Backends::from_comma_list(&v.replace("opengl", "gl"));
        return (!b.is_empty()).then_some(b);
    }
    if !cfg!(windows) {
        return None;
    }
    let next = failed
        .and_then(|f| WINDOWS_ORDER.iter().position(|&b| b == f))
        .map_or(0, |i| (i + 1) % WINDOWS_ORDER.len());
    Some(WINDOWS_ORDER[next])
}

/// Sets the window's backends before it opens; call `drawn` after the first frame.
pub fn choose(opts: &mut eframe::NativeOptions) {
    let env = std::env::var("QUICKRS_GPU")
        .or_else(|_| std::env::var("WGPU_BACKEND"))
        .ok();
    // (the marker holds the backends that start used)
    let failed = std::fs::read_to_string(marker()).ok().map(|s| {
        s.trim()
            .parse::<u32>()
            .map_or(wgpu::Backends::DX12, wgpu::Backends::from_bits_truncate)
    });
    let Some(b) = backends(env.as_deref(), failed) else {
        return;
    };
    let _ = std::fs::write(marker(), b.bits().to_string());
    crate::applog::write(&format!(
        "graphics: {b:?}{}",
        match failed {
            Some(f) if env.is_none() => format!(" (the last start, on {f:?}, closed before drawing)"),
            _ => String::new(),
        }
    ));
    if let WgpuSetup::CreateNew(c) = &mut opts.wgpu_options.wgpu_setup {
        c.instance_descriptor.backends = b;
    }
}

/// The first frame is up: this backend works, so the next start uses the usual one again.
pub fn drawn() {
    let _ = std::fs::remove_file(marker());
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::Backends as B;

    #[test]
    fn by_hand_wins() {
        assert_eq!(backends(Some("vulkan"), Some(B::DX12)), Some(B::VULKAN));
        assert_eq!(backends(Some("opengl"), None), Some(B::GL));
        assert_eq!(backends(Some("dx12,gl"), None), Some(B::DX12 | B::GL));
    }

    #[test]
    fn moves_on_after_a_start_that_never_drew() {
        if cfg!(windows) {
            assert_eq!(backends(None, None), Some(B::DX12));
            assert_eq!(backends(None, Some(B::DX12)), Some(B::GL));
            assert_eq!(backends(None, Some(B::GL)), Some(B::VULKAN));
            assert_eq!(backends(None, Some(B::VULKAN)), Some(B::DX12));
            assert_eq!(backends(Some(" "), Some(B::DX12 | B::GL)), Some(B::DX12));
        } else {
            assert_eq!(backends(None, Some(B::DX12)), None);
        }
    }
}
