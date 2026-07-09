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

/// (W56) Map seed dạng STRING (như `Profile.fingerprint_seed`) sang u64 để
/// pick GPU/screen — mapping thống nhất 1 nơi ở backend:
/// - chuỗi toàn số: parse as-is (u64);
/// - chuỗi khác: FNV-1a 64-bit;
/// - rỗng/whitespace: random (mỗi lần gọi một giá trị mới).
pub fn seed_to_u64(seed: &str) -> u64 {
    let s = seed.trim();
    if s.is_empty() {
        return rand::random::<u64>();
    }
    if let Ok(n) = s.parse::<u64>() {
        return n;
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// (W56) Pool screen resolution phổ biến per-platform (weighted, `(w, h, weight)`).
/// mac dùng logical px (Retina scaled).
const SCREENS_WINDOWS: &[(u32, u32, u32)] = &[
    (1920, 1080, 35),
    (1366, 768, 12),
    (2560, 1440, 10),
    (1536, 864, 8),
    (1440, 900, 5),
    (1600, 900, 4),
    (3840, 2160, 4),
    (1280, 720, 3),
    (1680, 1050, 2),
    (2560, 1080, 2),
];
const SCREENS_MAC: &[(u32, u32, u32)] = &[
    (1440, 900, 20),
    (1512, 982, 15),
    (1728, 1117, 8),
    (1680, 1050, 6),
    (2560, 1440, 6),
    (1920, 1080, 4),
    (2560, 1600, 3),
];
const SCREENS_LINUX: &[(u32, u32, u32)] = &[
    (1920, 1080, 30),
    (2560, 1440, 8),
    (1366, 768, 6),
    (1600, 900, 3),
    (3840, 2160, 3),
    (1680, 1050, 2),
];

/// Trộn bit kiểu splitmix64 — để pick_screen KHÔNG tương quan thứ tự với
/// pick_gpu dù cùng nhận 1 seed (cả hai đều `% total_weight`).
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

/// (W56) Chọn screen resolution cho `platform`, weighted-deterministic theo
/// `seed` (đã trộn splitmix64). Platform lạ → fallback 1920×1080.
pub fn pick_screen(platform: &str, seed: u64) -> (u32, u32) {
    let pool = match normalize_platform(platform) {
        "windows" => SCREENS_WINDOWS,
        "mac" => SCREENS_MAC,
        "linux" => SCREENS_LINUX,
        _ => return (1920, 1080),
    };
    let total: u64 = pool.iter().map(|&(_, _, w)| w.max(1) as u64).sum();
    let mut pick = splitmix64(seed) % total;
    for &(w, h, weight) in pool {
        let weight = weight.max(1) as u64;
        if pick < weight {
            return (w, h);
        }
        pick -= weight;
    }
    (1920, 1080)
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
        "linux" if is_metal => Some(format!(
            "Platform Linux nhưng GPU {v} dùng Metal (\"{renderer}\") — \
             combo bất khả thi (Metal chỉ có trên macOS). Chất lượng fingerprint giảm."
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
    fn seed_to_u64_numeric_fnv_empty() {
        // Numeric string: parse as-is.
        assert_eq!(seed_to_u64("12345"), 12345);
        assert_eq!(seed_to_u64(" 42 "), 42);
        // Non-numeric: FNV-1a 64 deterministic, khác nhau theo input.
        assert_eq!(seed_to_u64("abc"), seed_to_u64("abc"));
        assert_ne!(seed_to_u64("abc"), seed_to_u64("abd"));
        // FNV-1a 64 reference: "a" → 0xaf63dc4c8601ec8c.
        assert_eq!(seed_to_u64("a"), 0xaf63dc4c8601ec8c);
        // Rỗng/whitespace: random — chỉ kiểm không panic (giá trị bất kỳ).
        let _ = seed_to_u64("");
        let _ = seed_to_u64("   ");
    }

    #[test]
    fn pick_screen_deterministic_and_in_platform_pool() {
        for (platform, pool) in [
            ("windows", SCREENS_WINDOWS),
            ("macos", SCREENS_MAC),
            ("linux", SCREENS_LINUX),
        ] {
            for seed in [0u64, 1, 12345, u64::MAX] {
                let (w, h) = pick_screen(platform, seed);
                assert_eq!(pick_screen(platform, seed), (w, h), "deterministic");
                assert!(
                    pool.iter().any(|&(pw, ph, _)| (pw, ph) == (w, h)),
                    "{platform} seed {seed}: {w}x{h} phải nằm trong pool"
                );
            }
        }
        // Platform lạ → fallback.
        assert_eq!(pick_screen("plan9", 7), (1920, 1080));
        // Phân bố: tồn tại 2 seed cho kết quả khác nhau.
        let first = pick_screen("windows", 0);
        assert!(
            (1..200u64).any(|s| pick_screen("windows", s) != first),
            "weighted pick phải trả >1 resolution khác nhau trên nhiều seed"
        );
    }

    #[test]
    fn pick_screen_independent_from_pick_gpu_order() {
        // Cùng seed: pick_screen trộn splitmix64 nên vị trí trong pool không
        // tương quan tuyến tính với pick_gpu (seed nhỏ liên tiếp không map
        // cùng thứ tự đầu pool).
        let picks: Vec<(u32, u32)> = (0..10).map(|s| pick_screen("windows", s)).collect();
        assert!(
            picks.iter().any(|&p| p != picks[0]),
            "10 seed liên tiếp phải cho >1 resolution (đã trộn bit)"
        );
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
        assert!(gpu_platform_mismatch("linux", "Apple", "ANGLE Metal Renderer: Apple M1").is_some());
        // Combo hợp lệ → None.
        assert!(gpu_platform_mismatch("macos", "Apple", "ANGLE Metal Renderer: Apple M1").is_none());
        assert!(gpu_platform_mismatch("windows", "Google Inc. (NVIDIA)", "ANGLE (NVIDIA ... Direct3D11 ...)").is_none());
    }
}
