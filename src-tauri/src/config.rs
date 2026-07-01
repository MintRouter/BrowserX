//! Config: hằng số phiên bản/platform/pubkey, cache dir `~/.cloakbrowser/`,
//! đường dẫn binary CloakBrowser theo OS, stealth args mặc định, URL tải.
//!
//! Port từ refs/CloakBrowser/cloakbrowser/config.py (Wave 2b):
//! - PLATFORM_CHROMIUM_VERSIONS (#L20-L26), SUPPORTED_PLATFORMS (#L91-L98)
//! - BINARY_SIGNING_PUBKEYS (#L37-L39, pin nguyên văn)
//! - get_cache_dir (#L150-L159), get_binary_path (#L169-L181)
//! - get_default_stealth_args (#L54-L76), get_download_url (#L274-L283)

use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Phiên bản Chromium mới nhất across platforms (hiển thị/fallback).
pub const CHROMIUM_VERSION: &str = "146.0.7680.177.5";

/// Phiên bản Chromium theo platform tag (config.py#L20-L26).
pub const PLATFORM_CHROMIUM_VERSIONS: &[(&str, &str)] = &[
    ("linux-x64", "146.0.7680.177.5"),
    ("linux-arm64", "146.0.7680.177.3"),
    ("darwin-arm64", "145.0.7632.109.2"),
    ("darwin-x64", "145.0.7632.109.2"),
    ("windows-x64", "146.0.7680.177.5"),
];

/// Map (OS, ARCH) → platform tag (config.py#L91-L98), theo
/// `std::env::consts::{OS, ARCH}` của Rust.
pub const SUPPORTED_PLATFORMS: &[((&str, &str), &str)] = &[
    (("linux", "x86_64"), "linux-x64"),
    (("linux", "aarch64"), "linux-arm64"),
    (("macos", "aarch64"), "darwin-arm64"),
    (("macos", "x86_64"), "darwin-x64"),
    (("windows", "x86_64"), "windows-x64"),
];

/// Ed25519 pubkey (base64 của 32 byte raw) pin để verify SHA256SUMS.sig.
/// Copy nguyên văn từ config.py#L37-L39. Nhiều entry cho phép key rotation.
pub const BINARY_SIGNING_PUBKEYS: &[&str] = &["MKFKwIhUcKWq5xTuNA0Ovg99njcDEcEJvmWYYhApvaU="];

/// URL primary mặc định (override bằng env CLOAKBROWSER_DOWNLOAD_URL).
pub const DEFAULT_DOWNLOAD_BASE_URL: &str = "https://cloakbrowser.dev";

/// URL fallback GitHub Releases (config.py#L260).
pub const GITHUB_DOWNLOAD_BASE_URL: &str =
    "https://github.com/CloakHQ/cloakbrowser/releases/download";

/// Tag platform cho một cặp (os, arch) — pure, testable.
pub fn platform_tag_for(os: &str, arch: &str) -> Option<&'static str> {
    SUPPORTED_PLATFORMS
        .iter()
        .find(|((o, a), _)| *o == os && *a == arch)
        .map(|(_, tag)| *tag)
}

/// Tag platform của host hiện tại; lỗi rõ nếu không hỗ trợ (config.py#L134-L144).
pub fn get_platform_tag() -> Result<&'static str> {
    platform_tag_for(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        AppError::Binary(format!(
            "Unsupported platform: {} {}. Supported: {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            SUPPORTED_PLATFORMS
                .iter()
                .map(|((o, a), _)| format!("{o}-{a}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// Phiên bản Chromium cho một tag — pure, testable.
pub fn chromium_version_for_tag(tag: &str) -> Option<&'static str> {
    PLATFORM_CHROMIUM_VERSIONS
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(_, v)| *v)
}

/// Phiên bản Chromium cho host hiện tại (fallback CHROMIUM_VERSION — config.py#L128-L131).
pub fn get_chromium_version() -> String {
    get_platform_tag()
        .ok()
        .and_then(chromium_version_for_tag)
        .unwrap_or(CHROMIUM_VERSION)
        .to_string()
}

/// Cache dir từ override tường minh — pure, testable (config.py#L150-L159).
pub fn cache_dir_from(custom: Option<&str>) -> PathBuf {
    match custom.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cloakbrowser"),
    }
}

/// Cache dir cho binary tải về: env CLOAKBROWSER_CACHE_DIR → `~/.cloakbrowser/`.
pub fn get_cache_dir() -> PathBuf {
    cache_dir_from(std::env::var("CLOAKBROWSER_CACHE_DIR").ok().as_deref())
}

/// Thư mục chứa binary của một version: `<cache>/chromium-<version>` (config.py#L162-L166).
pub fn get_binary_dir(version: Option<&str>) -> PathBuf {
    let v = version
        .map(str::to_string)
        .unwrap_or_else(get_chromium_version);
    get_cache_dir().join(format!("chromium-{v}"))
}

/// Đường dẫn executable trong `dir` theo OS — pure, testable (config.py#L169-L181).
pub fn binary_path_in(dir: &Path, os: &str) -> PathBuf {
    match os {
        "macos" => dir
            .join("Chromium.app")
            .join("Contents")
            .join("MacOS")
            .join("Chromium"),
        "windows" => dir.join("chrome.exe"),
        _ => dir.join("chrome"),
    }
}

/// Đường dẫn executable kỳ vọng của một version trên host hiện tại.
pub fn get_binary_path(version: Option<&str>) -> PathBuf {
    binary_path_in(&get_binary_dir(version), std::env::consts::OS)
}

/// Platform fingerprint mặc định theo host OS — pure, testable (config.py#L68-L76).
/// Chỉ là DEFAULT của wrapper; profile được phép override (docs/03 §6).
pub fn default_fingerprint_platform(os: &str) -> &'static str {
    if os == "macos" {
        "macos"
    } else {
        "windows"
    }
}

/// Stealth args mặc định với seed ngẫu nhiên mỗi lần gọi (config.py#L54-L76).
/// KHÔNG hardcode platform: giá trị chỉ là default theo host, launcher cho override.
pub fn get_default_stealth_args() -> Vec<String> {
    let seed: u32 = rand::random_range(10000..=99999);
    vec![
        "--no-sandbox".to_string(),
        format!("--fingerprint={seed}"),
        format!(
            "--fingerprint-platform={}",
            default_fingerprint_platform(std::env::consts::OS)
        ),
    ]
}

// ---------------------------------------------------------------------------
// Download URL (config.py#L253-L294)
// ---------------------------------------------------------------------------

/// Đuôi archive theo OS — pure, testable (config.py#L263-L265).
pub fn archive_ext_for(os: &str) -> &'static str {
    if os == "windows" {
        ".zip"
    } else {
        ".tar.gz"
    }
}

/// Đuôi archive cho host hiện tại.
pub fn get_archive_ext() -> &'static str {
    archive_ext_for(std::env::consts::OS)
}

/// Tên file archive cho một tag, đuôi theo OS — pure, testable (config.py#L268-L271).
pub fn archive_name_for(tag: &str, os: &str) -> String {
    format!("cloakbrowser-{tag}{}", archive_ext_for(os))
}

/// Tên file archive cho host hiện tại (vd. `cloakbrowser-darwin-arm64.tar.gz`).
pub fn get_archive_name() -> Result<String> {
    Ok(archive_name_for(get_platform_tag()?, std::env::consts::OS))
}

/// Base URL primary: env CLOAKBROWSER_DOWNLOAD_URL → cloakbrowser.dev (config.py#L253-L256).
pub fn download_base_url() -> String {
    std::env::var("CLOAKBROWSER_DOWNLOAD_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_DOWNLOAD_BASE_URL.to_string())
}

/// Có đang dùng mirror tự host (đổi ngữ nghĩa verify — xem binary.rs)?
pub fn has_custom_download_url() -> bool {
    std::env::var("CLOAKBROWSER_DOWNLOAD_URL")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// URL tải primary cho version (mặc định version của platform) — config.py#L274-L277.
pub fn get_download_url(version: Option<&str>) -> Result<String> {
    let v = version
        .map(str::to_string)
        .unwrap_or_else(get_chromium_version);
    Ok(format!(
        "{}/chromium-v{v}/{}",
        download_base_url(),
        get_archive_name()?
    ))
}

/// URL fallback GitHub Releases cho version — config.py#L280-L283.
pub fn get_fallback_download_url(version: Option<&str>) -> Result<String> {
    let v = version
        .map(str::to_string)
        .unwrap_or_else(get_chromium_version);
    Ok(format!(
        "{GITHUB_DOWNLOAD_BASE_URL}/chromium-v{v}/{}",
        get_archive_name()?
    ))
}

/// Override binary local qua env CLOAKBROWSER_BINARY_PATH (config.py#L289-L294).
pub fn get_local_binary_override() -> Option<String> {
    std::env::var("CLOAKBROWSER_BINARY_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_map_covers_all_supported_combos() {
        assert_eq!(platform_tag_for("linux", "x86_64"), Some("linux-x64"));
        assert_eq!(platform_tag_for("linux", "aarch64"), Some("linux-arm64"));
        assert_eq!(platform_tag_for("macos", "aarch64"), Some("darwin-arm64"));
        assert_eq!(platform_tag_for("macos", "x86_64"), Some("darwin-x64"));
        assert_eq!(platform_tag_for("windows", "x86_64"), Some("windows-x64"));
        assert_eq!(platform_tag_for("freebsd", "x86_64"), None);
        assert_eq!(platform_tag_for("windows", "aarch64"), None);
    }

    #[test]
    fn every_platform_tag_has_a_version() {
        for (_, tag) in SUPPORTED_PLATFORMS {
            assert!(
                chromium_version_for_tag(tag).is_some(),
                "missing version for tag {tag}"
            );
        }
        assert_eq!(
            chromium_version_for_tag("darwin-arm64"),
            Some("145.0.7632.109.2")
        );
        assert_eq!(chromium_version_for_tag("nope"), None);
    }

    #[test]
    fn binary_path_per_os() {
        let dir = Path::new("/cache/chromium-1.2.3.4");
        assert_eq!(
            binary_path_in(dir, "macos"),
            Path::new("/cache/chromium-1.2.3.4/Chromium.app/Contents/MacOS/Chromium")
        );
        assert_eq!(
            binary_path_in(dir, "windows"),
            Path::new("/cache/chromium-1.2.3.4/chrome.exe")
        );
        assert_eq!(
            binary_path_in(dir, "linux"),
            Path::new("/cache/chromium-1.2.3.4/chrome")
        );
    }

    #[test]
    fn cache_dir_override_and_default() {
        assert_eq!(
            cache_dir_from(Some("/tmp/custom-cache")),
            PathBuf::from("/tmp/custom-cache")
        );
        assert!(cache_dir_from(None).ends_with(".cloakbrowser"));
        assert!(cache_dir_from(Some("  ")).ends_with(".cloakbrowser"));
    }

    #[test]
    fn archive_naming_per_os() {
        assert_eq!(archive_ext_for("windows"), ".zip");
        assert_eq!(archive_ext_for("macos"), ".tar.gz");
        assert_eq!(archive_ext_for("linux"), ".tar.gz");
        assert_eq!(
            archive_name_for("windows-x64", "windows"),
            "cloakbrowser-windows-x64.zip"
        );
        assert_eq!(
            archive_name_for("darwin-arm64", "macos"),
            "cloakbrowser-darwin-arm64.tar.gz"
        );
    }

    #[test]
    fn default_stealth_args_shape() {
        let args = get_default_stealth_args();
        assert_eq!(args[0], "--no-sandbox");
        let seed: u32 = args[1]
            .strip_prefix("--fingerprint=")
            .expect("has --fingerprint=")
            .parse()
            .expect("numeric seed");
        assert!((10000..=99999).contains(&seed));
        let plat = args[2].strip_prefix("--fingerprint-platform=").unwrap();
        assert!(plat == "macos" || plat == "windows");
        assert_eq!(default_fingerprint_platform("macos"), "macos");
        assert_eq!(default_fingerprint_platform("linux"), "windows");
        assert_eq!(default_fingerprint_platform("windows"), "windows");
        // Không được lộ automation flags (IGNORE_DEFAULT_ARGS config.py#L47).
        assert!(!args.iter().any(|a| a.contains("--enable-automation")));
    }

    #[test]
    fn download_urls_contain_version_and_archive() {
        let url = get_download_url(Some("145.0.7632.109.2")).unwrap();
        assert!(url.contains("/chromium-v145.0.7632.109.2/"));
        assert!(url.contains("cloakbrowser-"));
        let fb = get_fallback_download_url(Some("145.0.7632.109.2")).unwrap();
        assert!(fb.starts_with(GITHUB_DOWNLOAD_BASE_URL));
        assert!(fb.contains("/chromium-v145.0.7632.109.2/"));
    }

    #[test]
    fn host_platform_is_supported_and_versioned() {
        // CI/dev chạy trên platform được hỗ trợ — tag + version phải resolve được.
        let tag = get_platform_tag().unwrap();
        assert!(PLATFORM_CHROMIUM_VERSIONS.iter().any(|(t, _)| t == &tag));
        assert!(!get_chromium_version().is_empty());
        let bp = get_binary_path(Some("9.9.9.9"));
        assert!(bp.to_string_lossy().contains("chromium-9.9.9.9"));
    }
}
