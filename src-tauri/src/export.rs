//! Export/Import profile (W19a) — file `.bxprofile` (JSON) kiểu Multilogin X.
//!
//! Format: `{version, format, exported_at, note, profile: {...}, proxy?}`.
//! - Profile: name, fingerprint seed, browser/hardware config, startup, notes,
//!   tags + tên folder. KHÔNG export cookies/cache/user_data_dir.
//! - Proxy: protocol/host/port/username — KHÔNG BAO GIỜ export password
//!   (ghi rõ trong field `note`); import tạo proxy mới không password.
//! - Import luôn tạo profile MỚI (id mới, tên "Imported — {name}"), validate
//!   version + field bắt buộc, JSON rác → `AppError::InvalidInput` rõ ràng.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::crypto;
use crate::db::{Db, ProfileInput, ProxyInput};
use crate::error::{AppError, Result};
use crate::models::Profile;

/// Giá trị field `format` — nhận diện file .bxprofile.
pub const EXPORT_FORMAT: &str = "bxprofile";
/// Version format hiện tại; import chỉ chấp nhận đúng version này.
pub const EXPORT_VERSION: u32 = 1;
/// Ghi chú nhúng trong file để người nhận biết password không được kèm theo.
const PASSWORD_NOTE: &str =
    "Proxy password is never exported. Re-enter it after importing this profile.";

/// Toàn bộ nội dung file `.bxprofile`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileExport {
    pub version: u32,
    pub format: String,
    pub exported_at: String,
    #[serde(default)]
    pub note: String,
    pub profile: ExportedProfile,
    #[serde(default)]
    pub proxy: Option<ExportedProxy>,
}

/// Cấu hình profile được export — KHÔNG gồm id/user_data_dir/cookies/cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedProfile {
    pub name: String,
    pub fingerprint_seed: String,
    pub platform: String,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    pub screen_width: u32,
    pub screen_height: u32,
    #[serde(default)]
    pub gpu_vendor: Option<String>,
    #[serde(default)]
    pub gpu_renderer: Option<String>,
    pub hardware_concurrency: u32,
    #[serde(default)]
    pub humanize: bool,
    #[serde(default)]
    pub human_preset: Option<String>,
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub geoip: bool,
    #[serde(default)]
    pub color_scheme: Option<String>,
    #[serde(default)]
    pub launch_args: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default = "default_startup_behavior")]
    pub startup_behavior: String,
    #[serde(default)]
    pub startup_urls: Vec<String>,
    /// (W24b) Đường dẫn unpacked extension local — file cũ không có field → rỗng.
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub folder_name: Option<String>,
    /// (W20b) Storage options — file cũ không có field → default true (giữ dữ liệu).
    #[serde(default = "default_true")]
    pub store_history: bool,
    #[serde(default = "default_true")]
    pub store_passwords: bool,
    #[serde(default = "default_true")]
    pub store_sw_cache: bool,
}

/// Proxy gán với profile — username plaintext, KHÔNG có password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedProxy {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
}

fn default_startup_behavior() -> String {
    "restore".into()
}

fn default_true() -> bool {
    true
}

/// Lấy mảng chuỗi từ `serde_json::Value` dạng array (bỏ phần tử không phải chuỗi).
fn json_strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Xuất profile `id` → chuỗi JSON (pretty) cho file `.bxprofile`.
/// Proxy: giải mã username để mang theo; password KHÔNG export.
pub fn export_profile_json(db: &Db, id: &str) -> Result<String> {
    let profile = db.get_profile(id)?;
    let folder_name = match &profile.folder_id {
        Some(fid) => Some(db.get_folder(fid)?.name),
        None => None,
    };
    let proxy = match &profile.proxy_id {
        Some(pid) => {
            let rec = db.get_proxy(pid)?;
            Some(ExportedProxy {
                protocol: rec.protocol,
                host: rec.host,
                port: rec.port,
                username: rec
                    .username_enc
                    .as_deref()
                    .map(crypto::decrypt_secret)
                    .transpose()?,
            })
        }
        None => None,
    };
    let export = ProfileExport {
        version: EXPORT_VERSION,
        format: EXPORT_FORMAT.into(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        note: PASSWORD_NOTE.into(),
        profile: ExportedProfile {
            name: profile.name,
            fingerprint_seed: profile.fingerprint_seed,
            platform: profile.platform,
            timezone: profile.timezone,
            locale: profile.locale,
            screen_width: profile.screen_width,
            screen_height: profile.screen_height,
            gpu_vendor: profile.gpu_vendor,
            gpu_renderer: profile.gpu_renderer,
            hardware_concurrency: profile.hardware_concurrency,
            humanize: profile.humanize,
            human_preset: profile.human_preset,
            headless: profile.headless,
            geoip: profile.geoip,
            color_scheme: profile.color_scheme,
            launch_args: json_strings(&profile.launch_args),
            notes: profile.notes,
            startup_behavior: profile.startup_behavior,
            startup_urls: json_strings(&profile.startup_urls),
            extensions: json_strings(&profile.extensions),
            tags: profile.tags,
            folder_name,
            store_history: profile.store_history,
            store_passwords: profile.store_passwords,
            store_sw_cache: profile.store_sw_cache,
        },
        proxy,
    };
    Ok(serde_json::to_string_pretty(&export)?)
}

/// Nhập chuỗi JSON `.bxprofile` → tạo profile MỚI (id mới, tên
/// "Imported — {name}", user_data_dir mới). Proxy (nếu có) được tạo mới
/// KHÔNG password; folder khớp theo tên nếu đã tồn tại.
pub fn import_profile_json(db: &Db, json: &str) -> Result<Profile> {
    let data: ProfileExport = serde_json::from_str(json.trim())
        .map_err(|e| AppError::InvalidInput(format!("invalid .bxprofile file: {e}")))?;
    if data.format != EXPORT_FORMAT {
        return Err(AppError::InvalidInput(format!(
            "unsupported export format {:?} (expected {EXPORT_FORMAT:?})",
            data.format
        )));
    }
    if data.version != EXPORT_VERSION {
        return Err(AppError::InvalidInput(format!(
            "unsupported export version {} (expected {EXPORT_VERSION})",
            data.version
        )));
    }
    let p = data.profile;
    if p.name.trim().is_empty() {
        return Err(AppError::InvalidInput("profile name must not be empty".into()));
    }
    if p.fingerprint_seed.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "fingerprint_seed must not be empty".into(),
        ));
    }
    if !matches!(p.platform.as_str(), "windows" | "macos" | "linux") {
        return Err(AppError::InvalidInput(format!(
            "platform must be \"windows\", \"macos\" or \"linux\", got {:?}",
            p.platform
        )));
    }
    if !matches!(p.startup_behavior.as_str(), "restore" | "custom") {
        return Err(AppError::InvalidInput(format!(
            "startup_behavior must be \"restore\" or \"custom\", got {:?}",
            p.startup_behavior
        )));
    }

    // Tạo proxy trước (nếu có) để gán ngay lúc create_profile. Password không
    // có trong file → password_enc = None, user nhập lại sau khi import.
    let proxy_id = match data.proxy {
        Some(px) => {
            if px.host.trim().is_empty() {
                return Err(AppError::InvalidInput("proxy host must not be empty".into()));
            }
            let rec = db.create_proxy(ProxyInput {
                name: format!("{} — {}:{}", p.name.trim(), px.host, px.port),
                protocol: px.protocol,
                host: px.host,
                port: px.port,
                username_enc: px
                    .username
                    .as_deref()
                    .map(crypto::encrypt_secret)
                    .transpose()?,
                password_enc: None,
            })?;
            Some(rec.id)
        }
        None => None,
    };

    let profile = db.create_profile(ProfileInput {
        name: format!("Imported — {}", p.name.trim()),
        fingerprint_seed: Some(p.fingerprint_seed),
        platform: Some(p.platform),
        timezone: p.timezone,
        locale: p.locale,
        screen_width: Some(p.screen_width),
        screen_height: Some(p.screen_height),
        gpu_vendor: p.gpu_vendor,
        gpu_renderer: p.gpu_renderer,
        hardware_concurrency: Some(p.hardware_concurrency),
        humanize: Some(p.humanize),
        human_preset: p.human_preset,
        headless: Some(p.headless),
        geoip: Some(p.geoip),
        color_scheme: p.color_scheme,
        launch_args: Some(json!(p.launch_args)),
        user_data_dir: None,
        notes: p.notes,
        startup_behavior: Some(p.startup_behavior),
        startup_urls: Some(json!(p.startup_urls)),
        extensions: Some(json!(p.extensions)),
        proxy_id,
        tags: Some(p.tags),
        is_quick: None,
        // (W19c) File export cũ không mang fingerprint controls → default DB.
        fp_noise: None,
        webrtc_mode: None,
        webrtc_ip: None,
        geolocation_mode: None,
        geo_latitude: None,
        geo_longitude: None,
        // (W20b) Storage options mang qua export; file cũ default true.
        store_history: Some(p.store_history),
        store_passwords: Some(p.store_passwords),
        store_sw_cache: Some(p.store_sw_cache),
        // (P3-5a) File export không mang fingerprint controls sâu → default DB.
        nav_brand: None,
        nav_brand_version: None,
        platform_version: None,
        device_memory: None,
        fonts_dir: None,
        windows_font_metrics: None,
        storage_quota: None,
    })?;

    // Folder khớp theo TÊN (id không mang qua máy khác); không có → bỏ qua.
    if let Some(name) = p.folder_name.as_deref() {
        if let Some(folder) = db.list_folders()?.into_iter().find(|f| f.name == name) {
            db.move_profiles_to_folder(std::slice::from_ref(&profile.id), Some(&folder.id))?;
            return db.get_profile(&profile.id);
        }
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().expect("open in-memory db")
    }

    fn sample_input(name: &str) -> ProfileInput {
        ProfileInput {
            name: name.into(),
            fingerprint_seed: Some("54321".into()),
            platform: Some("macos".into()),
            timezone: Some("Asia/Ho_Chi_Minh".into()),
            locale: Some("vi-VN".into()),
            screen_width: Some(1440),
            screen_height: Some(900),
            gpu_vendor: Some("Apple".into()),
            gpu_renderer: Some("Apple M2".into()),
            hardware_concurrency: Some(10),
            humanize: Some(true),
            human_preset: Some("careful".into()),
            headless: Some(false),
            geoip: Some(true),
            color_scheme: Some("dark".into()),
            launch_args: Some(json!(["--lang=vi"])),
            notes: Some("ghi chú".into()),
            startup_behavior: Some("custom".into()),
            startup_urls: Some(json!(["https://example.com"])),
            extensions: Some(json!(["/data/ext/ublock"])),
            tags: Some(vec!["shop".into(), "vn".into()]),
            ..Default::default()
        }
    }

    #[test]
    fn roundtrip_preserves_seed_and_config() {
        let db = db();
        let src = db.create_profile(sample_input("Shop #1")).unwrap();
        let json = export_profile_json(&db, &src.id).unwrap();
        let imported = import_profile_json(&db, &json).unwrap();

        assert_ne!(imported.id, src.id);
        assert_ne!(imported.user_data_dir, src.user_data_dir);
        assert_eq!(imported.name, "Imported — Shop #1");
        assert_eq!(imported.fingerprint_seed, src.fingerprint_seed);
        assert_eq!(imported.platform, src.platform);
        assert_eq!(imported.timezone, src.timezone);
        assert_eq!(imported.locale, src.locale);
        assert_eq!(imported.screen_width, src.screen_width);
        assert_eq!(imported.screen_height, src.screen_height);
        assert_eq!(imported.gpu_vendor, src.gpu_vendor);
        assert_eq!(imported.gpu_renderer, src.gpu_renderer);
        assert_eq!(imported.hardware_concurrency, src.hardware_concurrency);
        assert_eq!(imported.humanize, src.humanize);
        assert_eq!(imported.human_preset, src.human_preset);
        assert_eq!(imported.geoip, src.geoip);
        assert_eq!(imported.color_scheme, src.color_scheme);
        assert_eq!(imported.launch_args, src.launch_args);
        assert_eq!(imported.notes, src.notes);
        assert_eq!(imported.startup_behavior, src.startup_behavior);
        assert_eq!(imported.startup_urls, src.startup_urls);
        assert_eq!(imported.extensions, src.extensions);
        assert_eq!(imported.tags, src.tags);
    }

    #[test]
    fn export_carries_proxy_without_password_and_import_recreates_it() {
        crypto::install_test_master_key();
        let db = db();
        let proxy = db
            .create_proxy(ProxyInput {
                name: "px".into(),
                protocol: "socks5".into(),
                host: "1.2.3.4".into(),
                port: 1080,
                username_enc: Some(crypto::encrypt_secret("alice").unwrap()),
                password_enc: Some(crypto::encrypt_secret("s3cret").unwrap()),
            })
            .unwrap();
        let mut input = sample_input("With proxy");
        input.proxy_id = Some(proxy.id.clone());
        let src = db.create_profile(input).unwrap();

        let json = export_profile_json(&db, &src.id).unwrap();
        assert!(!json.contains("s3cret"), "password must never be exported");
        let parsed: ProfileExport = serde_json::from_str(&json).unwrap();
        let px = parsed.proxy.expect("proxy exported");
        assert_eq!(px.host, "1.2.3.4");
        assert_eq!(px.username.as_deref(), Some("alice"));

        let imported = import_profile_json(&db, &json).unwrap();
        let new_proxy_id = imported.proxy_id.expect("proxy assigned");
        assert_ne!(new_proxy_id, proxy.id);
        let rec = db.get_proxy(&new_proxy_id).unwrap();
        assert_eq!(rec.protocol, "socks5");
        assert_eq!(rec.port, 1080);
        assert_eq!(
            crypto::decrypt_secret(rec.username_enc.as_deref().unwrap()).unwrap(),
            "alice"
        );
        assert!(rec.password_enc.is_none(), "imported proxy has no password");
    }

    #[test]
    fn import_rejects_garbage_json() {
        let db = db();
        let err = import_profile_json(&db, "not json at all").unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert!(err.to_string().contains("invalid .bxprofile"));

        let err = import_profile_json(&db, r#"{"version":1,"format":"bxprofile"}"#).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn import_rejects_wrong_version_or_format() {
        let db = db();
        let src = db.create_profile(sample_input("V")).unwrap();
        let json = export_profile_json(&db, &src.id).unwrap();

        let bumped = json.replacen("\"version\": 1", "\"version\": 99", 1);
        let err = import_profile_json(&db, &bumped).unwrap_err();
        assert!(err.to_string().contains("unsupported export version"));

        let other = json.replacen("bxprofile", "otherformat", 1);
        let err = import_profile_json(&db, &other).unwrap_err();
        assert!(err.to_string().contains("unsupported export format"));
    }

    #[test]
    fn import_rejects_invalid_fields() {
        let db = db();
        let src = db.create_profile(sample_input("F")).unwrap();
        let json = export_profile_json(&db, &src.id).unwrap();

        let bad = json.replacen("\"macos\"", "\"amiga\"", 1);
        let err = import_profile_json(&db, &bad).unwrap_err();
        assert!(err.to_string().contains("platform"));

        let bad = json.replacen("\"name\": \"F\"", "\"name\": \"  \"", 1);
        let err = import_profile_json(&db, &bad).unwrap_err();
        assert!(err.to_string().contains("name must not be empty"));
    }

    #[test]
    fn import_matches_folder_by_name() {
        let db = db();
        let folder = db.create_folder("Team A").unwrap();
        let src = db.create_profile(sample_input("In folder")).unwrap();
        db.move_profiles_to_folder(std::slice::from_ref(&src.id), Some(&folder.id))
            .unwrap();

        let json = export_profile_json(&db, &src.id).unwrap();
        let imported = import_profile_json(&db, &json).unwrap();
        assert_eq!(imported.folder_id.as_deref(), Some(folder.id.as_str()));
    }
}
