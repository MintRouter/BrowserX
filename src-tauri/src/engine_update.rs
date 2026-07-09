//! (W58b) Auto-update engine CloakBrowser: check GitHub Releases + apply update.
//!
//! Port từ refs/CloakBrowser/cloakbrowser/download.py (W58):
//! - check_engine_update: _get_latest_chromium_version (#L943-L962) — GET
//!   GitHub Releases API, lọc tag `chromium-v*` không draft CÓ asset đúng
//!   platform (tự loại release `-pro`/thiếu binary), so với default hiệu lực
//!   bằng version_newer. Null khi: không có bản mới / override env / lỗi mạng
//!   (nuốt im lặng, chỉ log warn — không Err ra FE).
//! - apply_engine_update: check_for_update (#L899-L920) — tải qua pipeline
//!   ensure_binary (verify Ed25519 + SHA-256 nguyên vẹn), progress emit
//!   `engine-update://progress`, xong ghi marker atomic (W58a).

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::binary;
use crate::config;
use crate::error::{AppError, Result};

/// GitHub Releases API của repo binary (config.py#L258).
const GITHUB_API_URL: &str = "https://api.github.com/repos/CloakHQ/cloakbrowser/releases";

/// Timeout check metadata (download.py#L950 timeout=10.0).
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Kết quả check update theo Hợp đồng Tauri command W58 (snake_case như spec).
#[derive(Debug, Clone, Serialize)]
pub struct EngineUpdateInfo {
    /// Default version hiện hành cho profile mới (`get_effective_version`).
    pub current: String,
    /// Version mới nhất có binary cho platform này trên GitHub Releases.
    pub latest: String,
    /// Trang release trên GitHub (html_url) để user xem changelog.
    pub release_url: String,
    /// Thời điểm publish release (ISO 8601), nếu API trả về.
    pub published_at: Option<String>,
}

/// Payload event `engine-update://progress` — giống `BinaryProgressEvent`
/// (`binary://progress`, camelCase cho FE).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineUpdateProgressEvent {
    phase: String,
    pct: u8,
    downloaded_bytes: u64,
    total_bytes: u64,
}

/// Release mới nhất khớp platform trong JSON GitHub Releases — pure, testable
/// (download.py#L953-L959). API trả release mới nhất trước → lấy match đầu.
/// Bỏ qua: tag không phải `chromium-v*` (vd `-pro` tag khác), draft, thiếu
/// asset đúng tên archive của platform.
fn latest_release_for_platform(
    releases: &serde_json::Value,
    platform_archive: &str,
) -> Option<EngineUpdateRelease> {
    for release in releases.as_array()? {
        let tag = release.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
        let Some(version) = tag.strip_prefix("chromium-v") else {
            continue;
        };
        if release.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let has_asset = release
            .get("assets")
            .and_then(|v| v.as_array())
            .is_some_and(|assets| {
                assets
                    .iter()
                    .any(|a| a.get("name").and_then(|n| n.as_str()) == Some(platform_archive))
            });
        if !has_asset {
            continue;
        }
        return Some(EngineUpdateRelease {
            version: version.to_string(),
            release_url: release
                .get("html_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            published_at: release
                .get("published_at")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }
    None
}

/// Release đã chọn từ GitHub (chưa so version với default hiện hành).
#[derive(Debug, Clone, PartialEq)]
struct EngineUpdateRelease {
    version: String,
    release_url: String,
    published_at: Option<String>,
}

/// GET GitHub Releases API (User-Agent bắt buộc, timeout 10s) rồi lọc pure.
async fn fetch_latest_release(platform_archive: &str) -> Result<Option<EngineUpdateRelease>> {
    let client = reqwest::Client::builder()
        .timeout(CHECK_TIMEOUT)
        .user_agent(concat!("BrowserX/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let releases: serde_json::Value = client
        .get(format!("{GITHUB_API_URL}?per_page=10"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(latest_release_for_platform(&releases, platform_archive))
}

/// Check có engine mới hơn default hiệu lực không. Null = không có update /
/// skip do override env / lỗi mạng (log warn, không Err ra FE — banner không
/// bao giờ hiện lỗi). CHỈ gọi metadata API, KHÔNG tải binary.
#[tauri::command]
pub async fn check_engine_update() -> Result<Option<EngineUpdateInfo>> {
    // Tôn trọng override (download.py#L927-L930): binary local / mirror tự
    // host → GitHub Releases không phải nguồn engine, check vô nghĩa.
    if config::get_local_binary_override().is_some() || config::has_custom_download_url() {
        return Ok(None);
    }
    // Platform không hỗ trợ → không bao giờ có asset để offer.
    let Ok(platform_archive) = config::get_archive_name() else {
        return Ok(None);
    };

    let current = config::get_effective_version();
    let release = match fetch_latest_release(&platform_archive).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(None),
        Err(e) => {
            tracing::warn!("Engine update check failed (ignored): {e}");
            return Ok(None);
        }
    };
    if !config::version_newer(&release.version, &current) {
        return Ok(None);
    }
    Ok(Some(EngineUpdateInfo {
        current,
        latest: release.version,
        release_url: release.release_url,
        published_at: release.published_at,
    }))
}

/// Tải + verify engine `version` qua NGUYÊN pipeline ensure_binary (Ed25519 →
/// version → SHA-256, download.py#L899-L920), progress emit
/// `engine-update://progress`. Marker default CHỈ ghi (atomic, W58a) SAU khi
/// binary version đó thật sự tồn tại trên đĩa — profile mới dùng bản này,
/// profile cũ giữ engine đang pin. Trả path executable mới.
#[tauri::command]
pub async fn apply_engine_update(app: AppHandle, version: String) -> Result<String> {
    let version = version.trim().to_string();
    if version.is_empty() {
        return Err(AppError::InvalidInput(
            "engine version must not be empty".into(),
        ));
    }

    let progress_app = app.clone();
    let progress = move |phase: &str, pct: u8, downloaded_bytes: u64, total_bytes: u64| {
        let _ = progress_app.emit(
            "engine-update://progress",
            EngineUpdateProgressEvent {
                phase: phase.to_string(),
                pct,
                downloaded_bytes,
                total_bytes,
            },
        );
    };
    let path = binary::ensure_binary(Some(&version), Some(&progress)).await?;

    // Guard marker: xác nhận binary CỦA VERSION NÀY có trên đĩa (không tin
    // path trả về — CLOAKBROWSER_BINARY_PATH override trả path khác version).
    // Marker mồ côi tuy vô hại (effective_version_in kiểm binary tồn tại)
    // nhưng ghi sai là sai hợp đồng "chỉ ghi sau khi binary tồn tại".
    if !config::get_binary_path(Some(&version)).exists() {
        return Err(AppError::Binary(format!(
            "engine {version} binary missing after download — marker not written"
        )));
    }
    config::write_version_marker(&version)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC_ARM: &str = "cloakbrowser-darwin-arm64.tar.gz";

    /// Fixture mô phỏng GitHub Releases API (mới → cũ) với đủ case DoD:
    /// draft, tag không phải chromium-v*, `-pro` không có asset platform free,
    /// release thiếu asset platform này, release free hợp lệ.
    fn fixture() -> serde_json::Value {
        serde_json::json!([
            {
                "tag_name": "chromium-v148.0.0.0.9",
                "draft": true,
                "html_url": "https://github.com/CloakHQ/cloakbrowser/releases/tag/chromium-v148.0.0.0.9",
                "published_at": "2026-07-09T00:00:00Z",
                "assets": [{ "name": MAC_ARM }]
            },
            {
                "tag_name": "wrapper-v1.2.3",
                "draft": false,
                "html_url": "https://github.com/CloakHQ/cloakbrowser/releases/tag/wrapper-v1.2.3",
                "published_at": "2026-07-08T00:00:00Z",
                "assets": [{ "name": MAC_ARM }]
            },
            {
                "tag_name": "chromium-v147.0.0.0.2-pro",
                "draft": false,
                "html_url": "https://github.com/CloakHQ/cloakbrowser/releases/tag/chromium-v147.0.0.0.2-pro",
                "published_at": "2026-07-07T00:00:00Z",
                "assets": [{ "name": "cloakbrowser-pro-darwin-arm64.tar.gz" }]
            },
            {
                "tag_name": "chromium-v147.0.0.0.1",
                "draft": false,
                "html_url": "https://github.com/CloakHQ/cloakbrowser/releases/tag/chromium-v147.0.0.0.1",
                "published_at": "2026-07-06T00:00:00Z",
                "assets": [{ "name": "cloakbrowser-linux-x64.tar.gz" }]
            },
            {
                "tag_name": "chromium-v146.0.7680.177.5",
                "draft": false,
                "html_url": "https://github.com/CloakHQ/cloakbrowser/releases/tag/chromium-v146.0.7680.177.5",
                "published_at": "2026-07-05T00:00:00Z",
                "assets": [
                    { "name": "cloakbrowser-linux-x64.tar.gz" },
                    { "name": MAC_ARM }
                ]
            },
            {
                "tag_name": "chromium-v145.0.7632.109.2",
                "draft": false,
                "html_url": "https://github.com/CloakHQ/cloakbrowser/releases/tag/chromium-v145.0.7632.109.2",
                "published_at": "2026-07-01T00:00:00Z",
                "assets": [{ "name": MAC_ARM }]
            }
        ])
    }

    #[test]
    fn picks_first_free_release_with_platform_asset() {
        // Draft 148 / wrapper tag / -pro (asset tên khác) / 147 linux-only đều
        // bị bỏ → chọn 146 (release free ĐẦU TIÊN có asset darwin-arm64),
        // KHÔNG rơi xuống 145 cũ hơn.
        let r = latest_release_for_platform(&fixture(), MAC_ARM).unwrap();
        assert_eq!(r.version, "146.0.7680.177.5");
        assert_eq!(
            r.release_url,
            "https://github.com/CloakHQ/cloakbrowser/releases/tag/chromium-v146.0.7680.177.5"
        );
        assert_eq!(r.published_at.as_deref(), Some("2026-07-05T00:00:00Z"));
    }

    #[test]
    fn platform_without_matching_asset_gets_none() {
        // Asset match EXACT tên archive — windows-x64 không có trong fixture.
        assert_eq!(
            latest_release_for_platform(&fixture(), "cloakbrowser-windows-x64.zip"),
            None
        );
    }

    #[test]
    fn linux_picks_newer_release_than_mac() {
        // 147 chỉ có linux asset → linux được offer 147 trong khi mac chỉ 146.
        let r = latest_release_for_platform(&fixture(), "cloakbrowser-linux-x64.tar.gz").unwrap();
        assert_eq!(r.version, "147.0.0.0.1");
    }

    #[test]
    fn malformed_or_empty_json_gets_none() {
        assert_eq!(latest_release_for_platform(&serde_json::json!([]), MAC_ARM), None);
        // Không phải array (vd API trả object lỗi rate-limit) → None, không panic.
        assert_eq!(
            latest_release_for_platform(&serde_json::json!({"message": "rate limited"}), MAC_ARM),
            None
        );
        // Release thiếu field → bỏ qua an toàn.
        assert_eq!(
            latest_release_for_platform(&serde_json::json!([{}, {"tag_name": 42}]), MAC_ARM),
            None
        );
    }

    #[test]
    fn pro_version_string_never_beats_current() {
        // Phòng xa: nếu release -pro CÓ asset free (bất thường), version
        // "147.0.0.0.2-pro" không parse được tuple số → version_newer = false,
        // check_engine_update vẫn không offer.
        assert!(!config::version_newer("147.0.0.0.2-pro", "146.0.7680.177.5"));
    }

    /// html_url/published_at thiếu → release_url rỗng, published_at None
    /// (không chặn offer update).
    #[test]
    fn tolerates_missing_optional_fields() {
        let releases = serde_json::json!([
            { "tag_name": "chromium-v146.0.7700.0.1", "draft": false,
              "assets": [{ "name": MAC_ARM }] }
        ]);
        let got = latest_release_for_platform(&releases, MAC_ARM).unwrap();
        assert_eq!(got.release_url, "");
        assert_eq!(got.published_at, None);
    }
}
