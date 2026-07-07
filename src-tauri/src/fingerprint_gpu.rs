//! (W52-D) Pool GPU vendor↔renderer CÓ THẬT (ADAPT từ VFlowX_V2
//! `internal/gologin/fingerprint_data.json` webglPool, chỉ desktop win/mac/linux)
//! nhúng static để gợi ý `--fingerprint-gpu-vendor`/`--fingerprint-gpu-renderer`
//! nhất quán với `--fingerprint-platform`, và cảnh báo combo bất khả thi.
//!
//! KHÔNG port generator gologin: binary CloakBrowser tự derive tham số tương
//! quan; ta chỉ cấp bộ đôi vendor↔renderer khớp thật (ràng buộc W52-A2).

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Một entry trong pool WebGL (subset field cần dùng; field thừa trong JSON
/// như `chrome_version_min`/`source` bị serde bỏ qua).
#[derive(Debug, Clone, Deserialize)]
pub struct GpuEntry {
    pub vendor: String,
    pub renderer: String,
    /// "windows" | "mac" | "linux".
    pub platform: String,
    pub gpu_class: String,
    pub weight: u32,
}

/// Gợi ý cặp GPU trả về FE khi generate/regenerate fingerprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSuggestion {
    pub vendor: String,
    pub renderer: String,
}

static POOL: LazyLock<Vec<GpuEntry>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/webgl_pool.json"))
        .expect("webgl_pool.json phải parse được thành Vec<GpuEntry>")
});

/// Toàn bộ pool (lazy-parsed 1 lần).
pub fn pool() -> &'static [GpuEntry] {
    &POOL
}

/// Map platform fingerprint (`--fingerprint-platform`: "macos"|"windows"|"linux")
/// sang giá trị `platform` trong pool ("mac"|"windows"|"linux").
fn normalize_platform(platform: &str) -> &str {
    match platform.trim() {
        "macos" => "mac",
        p => p,
    }
}

/// Chọn 1 entry GPU cho `platform`, weighted-deterministic theo `seed`
/// (reproducible — KHÔNG rand runtime). None nếu platform không có entry.
pub fn pick_gpu(platform: &str, seed: u64) -> Option<&'static GpuEntry> {
    let target = normalize_platform(platform);
    let entries: Vec<&'static GpuEntry> = POOL.iter().filter(|e| e.platform == target).collect();
    if entries.is_empty() {
        return None;
    }
    let total: u64 = entries.iter().map(|e| e.weight.max(1) as u64).sum();
    let mut pick = seed % total;
    for e in entries {
        let w = e.weight.max(1) as u64;
        if pick < w {
            return Some(e);
        }
        pick -= w;
    }
    None
}

/// Cảnh báo khi combo platform ↔ GPU renderer bất khả thi (KHÔNG chặn cứng,
/// chỉ mô tả để FE hiển thị):
/// - macOS/Linux + renderer Direct3D/D3D11 (chỉ Windows dùng D3D).
/// - Windows + renderer Metal (chỉ macOS dùng Metal).
pub fn gpu_platform_mismatch(platform: &str, vendor: &str, renderer: &str) -> Option<String> {
    let plat = normalize_platform(platform);
    let r = renderer.to_ascii_lowercase();
    let is_d3d = r.contains("direct3d") || r.contains("d3d11");
    let is_metal = r.contains("metal");
    let v = if vendor.trim().is_empty() {
        "(không rõ)"
    } else {
        vendor.trim()
    };
    match plat {
        "mac" if is_d3d => Some(format!(
            "Platform macOS nhưng GPU {v} dùng Direct3D/D3D11 (\"{renderer}\") — \
             combo bất khả thi (macOS dùng Metal/OpenGL). Chất lượng fingerprint giảm."
        )),
        "windows" if is_metal => Some(format!(
            "Platform Windows nhưng GPU {v} dùng Metal (\"{renderer}\") — \
             combo bất khả thi (Metal chỉ có trên macOS). Chất lượng fingerprint giảm."
        )),
        "linux" if is_d3d => Some(format!(
            "Platform Linux nhưng GPU {v} dùng Direct3D/D3D11 (\"{renderer}\") — \
             combo bất khả thi (Linux dùng OpenGL/Vulkan). Chất lượng fingerprint giảm."
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_loads_desktop_entries() {
        assert!(pool().len() > 200, "pool phải >200 entry, có {}", pool().len());
        for e in pool() {
            assert!(
                matches!(e.platform.as_str(), "windows" | "mac" | "linux"),
                "platform lạ: {}",
                e.platform
            );
        }
    }

    #[test]
    fn pick_gpu_matches_platform_and_is_deterministic() {
        let a = pick_gpu("windows", 12345).expect("windows có entry");
        assert_eq!(a.platform, "windows");
        assert_eq!(pick_gpu("windows", 12345).unwrap().renderer, a.renderer);
        // "macos" map sang "mac".
        assert_eq!(pick_gpu("macos", 7).expect("mac có entry").platform, "mac");
        assert_eq!(pick_gpu("linux", 99).expect("linux có entry").platform, "linux");
        assert!(pick_gpu("plan9", 1).is_none());
    }

    #[test]
    fn no_impossible_platform_gpu_combos_in_pool() {
        for e in pool() {
            let r = e.renderer.to_ascii_lowercase();
            let d3d = r.contains("direct3d") || r.contains("d3d11");
            let metal = r.contains("metal");
            assert!(!(e.platform == "mac" && d3d), "mac + D3D11: {}", e.renderer);
            assert!(
                !(e.platform == "windows" && metal),
                "windows + Metal: {}",
                e.renderer
            );
            assert!(
                !(e.platform == "linux" && d3d),
                "linux + D3D11: {}",
                e.renderer
            );
        }
    }

    #[test]
    fn mismatch_flags_impossible_combos_only() {
        assert!(gpu_platform_mismatch(
            "macos",
            "Google Inc. (NVIDIA)",
            "ANGLE (NVIDIA, ... Direct3D11 ...)"
        )
        .is_some());
        assert!(gpu_platform_mismatch("windows", "Apple", "ANGLE Metal Renderer: Apple M1").is_some());
        assert!(gpu_platform_mismatch("linux", "Google Inc.", "... D3D11 ...").is_some());
        // Combo hợp lệ → None.
        assert!(gpu_platform_mismatch("macos", "Apple", "ANGLE Metal Renderer: Apple M1").is_none());
        assert!(gpu_platform_mismatch("windows", "Google Inc. (NVIDIA)", "ANGLE (NVIDIA ... Direct3D11 ...)").is_none());
    }
}
