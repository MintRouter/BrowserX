//! Tauri commands (invoke handlers) — theo Hợp đồng command trong spec:
//! - Profiles: list_profiles, get_profile, create_profile, update_profile, delete_profile, search_profiles
//! - Proxies: list_proxies, create_proxy, update_proxy, delete_proxy, assign_proxy, check_proxy
//! - Proxy templates (P3-3a): list_proxy_templates, create_proxy_template,
//!   update_proxy_template, delete_proxy_template, create_proxy_from_template
//! - Session: launch_profile, stop_profile, list_running, bring_to_front (W20a),
//!   get_cdp_ws_url (W24c)
//! - Binary: ensure_binary (emit `binary://progress`)
//! - Settings/tags: get_settings, set_setting, list_tags, set_profile_tags
//! - Folders/favorites: list_folders, create_folder, rename_folder, delete_folder,
//!   set_favorite, move_profiles_to_folder
//! - Trash: trash_profiles, restore_profiles, purge_profiles, list_trash
//! - Quick profile: convert_quick_profile, delete_quick_profile
//! - Storage: profile_storage_sizes, clear_profile_cache
//! - Templates (W20b): list_templates, save_as_template, delete_template,
//!   create_profile_from_template
//! - Export/Import (W19a): export_profile, import_profile
//!
//! Đăng ký vào `tauri::Builder` trong lib.rs. Tham số Rust snake_case
//! (Tauri v2 tự map camelCase JS → snake_case).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::db::{self, Db, ProfileFilter, ProfileInput, ProfileUpdate, TagInfo};
use crate::error::{AppError, Result};
use crate::models::{
    Extension, Folder, Profile, ProfileTemplate, Proxy, ProxyTemplate, RunningSession,
};
use crate::metrics::LaunchMetrics;
use crate::process::ProcessManager;
use crate::proxy_check::{self, ProxyCheckResult};
use crate::{
    app_db_backup, archive, binary, cdp, cloud_transport, cookierobot, cookies, crypto,
    extensions, geoip, launcher, profile_lock, storage, telegram_sync, userbot,
};

/// State toàn app — khởi tạo trong `tauri::Builder::setup` (lib.rs) rồi `.manage()`.
pub struct AppState {
    pub db: Arc<Db>,
    pub procs: ProcessManager,
    /// (W23a) true khi `stop_all_and_quit` đang thoát app — để ExitRequested
    /// trong lib.rs không chặn lần thoát này nữa.
    pub quitting: AtomicBool,
    /// (P3-4a) CookieRobot đang chạy theo profile_id — cancel token để stop.
    pub robots: cookierobot::RobotRegistry,
    /// (W26b) Counter launch success/fail + ring buffer duration — since app start.
    pub metrics: LaunchMetrics,
}

// ---------------------------------------------------------------------------
// Event payloads
// ---------------------------------------------------------------------------

/// Payload event `profile://status` (camelCase cho FE).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileStatusEvent {
    profile_id: String,
    status: String,
    pid: Option<u32>,
    cdp_url: Option<String>,
}

/// Payload event `binary://progress` (camelCase cho FE).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BinaryProgressEvent {
    phase: String,
    pct: u8,
    downloaded_bytes: u64,
    total_bytes: u64,
}

pub(crate) fn emit_status(
    app: &AppHandle,
    profile_id: &str,
    status: &str,
    pid: Option<u32>,
    cdp_url: Option<String>,
) {
    let _ = app.emit(
        "profile://status",
        ProfileStatusEvent {
            profile_id: profile_id.to_string(),
            status: status.to_string(),
            pid,
            cdp_url,
        },
    );
}

/// (W52-B C6) Payload event `cloud://progress` (camelCase cho FE) — phát khi
/// upload/download backup cloud nhiều part qua telegram_sync.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudProgressEvent {
    profile_id: String,
    /// "upload" | "download".
    phase: String,
    /// Số part đã xử lý xong (0 trước part đầu, = part_count khi xong).
    part_index: usize,
    part_count: usize,
    bytes_done: u64,
    bytes_total: u64,
}

/// (W52-B C6) Emit `cloud://progress` — best-effort, lỗi emit bỏ qua.
fn emit_cloud_progress(
    app: &AppHandle,
    profile_id: &str,
    phase: &str,
    part_index: usize,
    part_count: usize,
    bytes_done: u64,
    bytes_total: u64,
) {
    let _ = app.emit(
        "cloud://progress",
        CloudProgressEvent {
            profile_id: profile_id.to_string(),
            phase: phase.to_string(),
            part_index,
            part_count,
            bytes_done,
            bytes_total,
        },
    );
}

/// Payload event `app://exit-requested` (W23a) — FE mở dialog "Stop all & quit".
#[derive(Debug, Clone, Serialize)]
struct ExitRequestedEvent {
    count: usize,
}

/// (W23a) Báo FE có yêu cầu thoát app trong khi còn `count` phiên đang chạy.
pub(crate) fn emit_exit_requested(app: &AppHandle, count: usize) {
    let _ = app.emit("app://exit-requested", ExitRequestedEvent { count });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// ProxyRecord (credential mã hoá) → models::Proxy trả về FE.
/// (W5c) KHÔNG credential plaintext nào qua IPC: password chỉ trả `has_password`,
/// username chỉ trả bản đã che (`mask_username`); giải mã plaintext duy nhất lúc
/// launch (`proxy_url_from`) / check (`check_proxy`). Decrypt fail (master key
/// đổi) KHÔNG trả Err — degrade: `credentials_invalid = true`, username rỗng,
/// để `list_proxies` không bao giờ sập vì 1 credential hỏng; FE hiện banner
/// yêu cầu nhập lại password.
fn proxy_to_model(rec: db::ProxyRecord) -> Proxy {
    let mut credentials_invalid = false;
    let masked_username = match rec.username_enc.as_deref().map(crypto::decrypt_secret) {
        Some(Ok(u)) => Some(mask_username(&u)),
        Some(Err(_)) => {
            credentials_invalid = true;
            None
        }
        None => None,
    };
    if let Some(p) = rec.password_enc.as_deref() {
        if crypto::open(p).is_err() {
            credentials_invalid = true;
        }
    }
    if credentials_invalid {
        tracing::warn!(
            "proxy {} credentials cannot be decrypted (master key changed?); password must be re-entered",
            rec.id
        );
    }
    Proxy {
        id: rec.id,
        name: rec.name,
        protocol: rec.protocol,
        host: rec.host,
        port: rec.port,
        masked_username,
        has_password: rec.password_enc.is_some(),
        credentials_invalid,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    }
}

/// (W5c) Che username trước khi trả qua IPC: ký tự đầu + "***" (không lộ độ
/// dài) — đủ để user nhận diện proxy trong danh sách, không đủ để rò plaintext.
fn mask_username(username: &str) -> String {
    match username.chars().next() {
        Some(c) => format!("{c}***"),
        None => "***".into(),
    }
}

/// Dựng proxy URL đã giải mã credential: `protocol://[user[:pass]@]host:port`.
/// Decrypt fail (master key đổi) → lỗi rõ ràng yêu cầu nhập lại credential,
/// không phải lỗi giải mã khó hiểu.
fn proxy_url_from(rec: &db::ProxyRecord) -> Result<String> {
    let dec = |blob: &[u8]| {
        crypto::decrypt_secret(blob).map_err(|_| {
            AppError::Crypto(format!(
                "proxy '{}' credentials cannot be decrypted (master key changed) — re-enter its password in the Proxies tab",
                rec.name
            ))
        })
    };
    let auth = match (&rec.username_enc, &rec.password_enc) {
        (Some(u), Some(p)) => format!("{}:{}@", dec(u)?, dec(p)?),
        (Some(u), None) => format!("{}@", dec(u)?),
        _ => String::new(),
    };
    Ok(format!(
        "{}://{}{}:{}",
        rec.protocol, auth, rec.host, rec.port
    ))
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<Vec<Profile>> {
    state.db.list_profiles()
}

#[tauri::command]
pub fn get_profile(state: State<'_, AppState>, id: String) -> Result<Profile> {
    state.db.get_profile(&id)
}

#[tauri::command]
pub fn create_profile(state: State<'_, AppState>, input: ProfileInput) -> Result<Profile> {
    let profile = state.db.create_profile(input)?;
    state
        .db
        .insert_audit("profile.create", Some(&profile.id), None)?;
    Ok(profile)
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, AppState>,
    id: String,
    input: ProfileUpdate,
) -> Result<Profile> {
    let profile = state.db.update_profile(&id, input)?;
    state.db.insert_audit("profile.update", Some(&id), None)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, id: String) -> Result<bool> {
    let deleted = state.db.delete_profile(&id)?;
    if deleted {
        state.db.insert_audit("profile.delete", Some(&id), None)?;
    }
    Ok(deleted)
}

/// (P3-2a) `filter` là tuỳ chọn — FE cũ không gửi filter thì `None` →
/// `ProfileFilter::default()` = hành vi cũ (chỉ lọc theo tên).
#[tauri::command]
pub fn search_profiles(
    state: State<'_, AppState>,
    query: String,
    filter: Option<ProfileFilter>,
) -> Result<Vec<Profile>> {
    state.db.search_profiles(&query, &filter.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Proxies (credential plaintext từ FE — mã hoá bằng crypto TRƯỚC khi lưu DB)
// ---------------------------------------------------------------------------

/// Input tạo proxy từ FE (credential plaintext).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProxyCreate {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Update proxy từng phần từ FE (credential plaintext).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProxyPatch {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub clear_credentials: bool,
}

#[tauri::command]
pub fn list_proxies(state: State<'_, AppState>) -> Result<Vec<Proxy>> {
    Ok(state
        .db
        .list_proxies()?
        .into_iter()
        .map(proxy_to_model)
        .collect())
}

#[tauri::command]
pub fn create_proxy(state: State<'_, AppState>, input: ProxyCreate) -> Result<Proxy> {
    let rec = state.db.create_proxy(db::ProxyInput {
        name: input.name,
        protocol: input.protocol,
        host: input.host,
        port: input.port,
        username_enc: input
            .username
            .as_deref()
            .map(crypto::encrypt_secret)
            .transpose()?,
        password_enc: input
            .password
            .as_deref()
            .map(crypto::encrypt_secret)
            .transpose()?,
    })?;
    state.db.insert_audit("proxy.create", Some(&rec.id), None)?;
    Ok(proxy_to_model(rec))
}

#[tauri::command]
pub fn update_proxy(state: State<'_, AppState>, id: String, input: ProxyPatch) -> Result<Proxy> {
    let rec = state.db.update_proxy(
        &id,
        db::ProxyUpdate {
            name: input.name,
            protocol: input.protocol,
            host: input.host,
            port: input.port,
            username_enc: input
                .username
                .as_deref()
                .map(crypto::encrypt_secret)
                .transpose()?,
            password_enc: input
                .password
                .as_deref()
                .map(crypto::encrypt_secret)
                .transpose()?,
            clear_credentials: input.clear_credentials,
        },
    )?;
    state.db.insert_audit("proxy.update", Some(&id), None)?;
    Ok(proxy_to_model(rec))
}

/// Trạng thái master key trả về FE (`master_key_status`).
#[derive(Debug, Clone, Serialize)]
pub struct MasterKeyStatus {
    /// `true` = key-check blob không giải mã được → master key đã đổi
    /// (keychain mất/reset); credential đã lưu (proxy password…) cần nhập lại.
    pub changed: bool,
}

/// Key trong bảng `settings` chứa key-check blob (base64).
const MASTER_KEY_CHECK_SETTING: &str = "master_key_check";

/// Key-check blob: lần đầu → seal hằng số và lưu vào settings; các lần sau
/// decrypt so khớp. Mismatch → warn + re-seal bằng khoá hiện tại (cảnh báo một
/// lần cho mỗi lần đổi key) và trả `true`. Tách khỏi command để unit-test.
fn master_key_check(db: &Db) -> Result<bool> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    let store = |db: &Db| -> Result<()> {
        let blob = crypto::new_key_check_blob()?;
        db.set_setting(MASTER_KEY_CHECK_SETTING, &B64.encode(blob))
    };
    match db.get_setting(MASTER_KEY_CHECK_SETTING)? {
        None => {
            store(db)?;
            Ok(false)
        }
        Some(b64) => {
            let matches = B64
                .decode(b64.as_bytes())
                .is_ok_and(|blob| crypto::key_check_matches(&blob));
            if !matches {
                tracing::warn!(
                    "master key check failed — key has changed; stored credentials must be re-entered"
                );
                store(db)?;
            }
            Ok(!matches)
        }
    }
}

/// FE gọi mỗi lần mở app: phát hiện master key đã đổi để hiện cảnh báo.
#[tauri::command]
pub fn master_key_status(state: State<'_, AppState>) -> Result<MasterKeyStatus> {
    Ok(MasterKeyStatus {
        changed: master_key_check(&state.db)?,
    })
}

/// (W52-E1) Kết quả import recovery key trả FE.
#[derive(Debug, Clone, Serialize)]
pub struct RecoveryImportResult {
    /// `true` = key vừa import KHÁC key trước đó của máy này → secret local
    /// đã lưu (proxy password, bot token…) không giải mã được nữa, cần nhập lại.
    pub changed: bool,
}

/// (W52-E1) Export Recovery Key: chuỗi `BXRK1-…` (Crockford base32 +
/// checksum) đại diện master key — user lưu OFFLINE để khôi phục backup
/// `.bxa` trên máy mới. Hiển thị MỘT lần; KHÔNG log/lưu/gửi mạng; audit chỉ
/// ghi sự kiện, không nội dung.
#[tauri::command]
pub fn export_recovery_key(state: State<'_, AppState>) -> Result<String> {
    let key = crypto::export_recovery_key()?;
    state.db.insert_audit("recovery_key.export", None, None)?;
    Ok(key)
}

/// (W52-E1) Import Recovery Key trên máy mới: validate (định dạng +
/// checksum) → persist keychain/file + cache → re-seal key-check. Sau đó
/// `restore_from_cloud` giải mã được `.bxa` tạo bởi máy cũ. Trả `changed`
/// để FE cảnh báo secret local cũ (nếu có) phải nhập lại.
#[tauri::command]
pub fn import_recovery_key(
    state: State<'_, AppState>,
    key: String,
) -> Result<RecoveryImportResult> {
    crypto::import_recovery_key(&key)?;
    let changed = master_key_check(&state.db)?;
    state.db.insert_audit("recovery_key.import", None, None)?;
    Ok(RecoveryImportResult { changed })
}

#[tauri::command]
pub fn delete_proxy(state: State<'_, AppState>, id: String) -> Result<bool> {
    let deleted = state.db.delete_proxy(&id)?;
    if deleted {
        state.db.insert_audit("proxy.delete", Some(&id), None)?;
    }
    Ok(deleted)
}

#[tauri::command]
pub fn assign_proxy(
    state: State<'_, AppState>,
    profile_id: String,
    proxy_id: Option<String>,
) -> Result<()> {
    state.db.assign_proxy(&profile_id, proxy_id.as_deref())?;
    state.db.insert_audit(
        "profile.assign_proxy",
        Some(&profile_id),
        Some(&json!({ "proxy_id": proxy_id })),
    )?;
    Ok(())
}

/// (W36) Xoay proxy cho 1 profile: gán proxy healthy kế tiếp trong pool theo
/// round-robin (bỏ qua proxy fail health-check, wrap-around) — xem
/// [`db::Db::rotate_proxy`]. Trả proxy vừa gán.
#[tauri::command]
pub fn rotate_proxy(state: State<'_, AppState>, profile_id: String) -> Result<Proxy> {
    let rec = state.db.rotate_proxy(&profile_id)?;
    state.db.insert_audit(
        "profile.rotate_proxy",
        Some(&profile_id),
        Some(&json!({ "proxy_id": rec.id })),
    )?;
    Ok(proxy_to_model(rec))
}

/// (W36) Xoay proxy cho nhiều profile — mỗi profile 1 audit, kết quả theo đúng
/// thứ tự `profile_ids`. Fail-fast: lỗi ở profile nào → dừng tại đó, các
/// profile đã xoay trước đó giữ nguyên gán mới.
#[tauri::command]
pub fn rotate_proxies(state: State<'_, AppState>, profile_ids: Vec<String>) -> Result<Vec<Proxy>> {
    let mut out = Vec::with_capacity(profile_ids.len());
    for pid in &profile_ids {
        let rec = state.db.rotate_proxy(pid)?;
        state.db.insert_audit(
            "profile.rotate_proxy",
            Some(pid),
            Some(&json!({ "proxy_id": rec.id })),
        )?;
        out.push(proxy_to_model(rec));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Proxy templates (P3-3a) — cấu hình proxy dùng lại được, credential mã hoá
// như proxies. sticky_session/traffic_saver là metadata ngữ nghĩa NHÀ CUNG CẤP
// proxy (username/host convention riêng từng nhà cung cấp — KHÔNG có flag
// Chromium/CloakBrowser) nên không áp vào launch.
// ---------------------------------------------------------------------------

/// ProxyTemplateRecord (credential mã hoá) → models::ProxyTemplate trả về FE.
/// Cùng chính sách W5c/W23b với `proxy_to_model`: username masked, password
/// chỉ trả `has_password`, decrypt fail → degrade `credentials_invalid`.
fn proxy_template_to_model(rec: db::ProxyTemplateRecord) -> ProxyTemplate {
    let mut credentials_invalid = false;
    let masked_username = match rec.username_enc.as_deref().map(crypto::decrypt_secret) {
        Some(Ok(u)) => Some(mask_username(&u)),
        Some(Err(_)) => {
            credentials_invalid = true;
            None
        }
        None => None,
    };
    if let Some(p) = rec.password_enc.as_deref() {
        if crypto::open(p).is_err() {
            credentials_invalid = true;
        }
    }
    if credentials_invalid {
        tracing::warn!(
            "proxy template {} credentials cannot be decrypted (master key changed?); password must be re-entered",
            rec.id
        );
    }
    ProxyTemplate {
        id: rec.id,
        name: rec.name,
        protocol: rec.protocol,
        host: rec.host,
        port: rec.port,
        masked_username,
        has_password: rec.password_enc.is_some(),
        credentials_invalid,
        sticky_session: rec.sticky_session,
        traffic_saver: rec.traffic_saver,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    }
}

/// Input tạo proxy template từ FE (credential plaintext — mã hoá trước khi lưu).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProxyTemplateCreate {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub sticky_session: bool,
    #[serde(default)]
    pub traffic_saver: bool,
}

/// Update proxy template từng phần từ FE (credential plaintext).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProxyTemplatePatch {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub sticky_session: Option<bool>,
    pub traffic_saver: Option<bool>,
    #[serde(default)]
    pub clear_credentials: bool,
}

#[tauri::command]
pub fn list_proxy_templates(state: State<'_, AppState>) -> Result<Vec<ProxyTemplate>> {
    Ok(state
        .db
        .list_proxy_templates()?
        .into_iter()
        .map(proxy_template_to_model)
        .collect())
}

#[tauri::command]
pub fn create_proxy_template(
    state: State<'_, AppState>,
    input: ProxyTemplateCreate,
) -> Result<ProxyTemplate> {
    let rec = state.db.create_proxy_template(db::ProxyTemplateInput {
        name: input.name,
        protocol: input.protocol,
        host: input.host,
        port: input.port,
        username_enc: input
            .username
            .as_deref()
            .map(crypto::encrypt_secret)
            .transpose()?,
        password_enc: input
            .password
            .as_deref()
            .map(crypto::encrypt_secret)
            .transpose()?,
        sticky_session: input.sticky_session,
        traffic_saver: input.traffic_saver,
    })?;
    state
        .db
        .insert_audit("proxy_template.create", Some(&rec.id), None)?;
    Ok(proxy_template_to_model(rec))
}

#[tauri::command]
pub fn update_proxy_template(
    state: State<'_, AppState>,
    id: String,
    input: ProxyTemplatePatch,
) -> Result<ProxyTemplate> {
    let rec = state.db.update_proxy_template(
        &id,
        db::ProxyTemplateUpdate {
            name: input.name,
            protocol: input.protocol,
            host: input.host,
            port: input.port,
            username_enc: input
                .username
                .as_deref()
                .map(crypto::encrypt_secret)
                .transpose()?,
            password_enc: input
                .password
                .as_deref()
                .map(crypto::encrypt_secret)
                .transpose()?,
            sticky_session: input.sticky_session,
            traffic_saver: input.traffic_saver,
            clear_credentials: input.clear_credentials,
        },
    )?;
    state
        .db
        .insert_audit("proxy_template.update", Some(&id), None)?;
    Ok(proxy_template_to_model(rec))
}

#[tauri::command]
pub fn delete_proxy_template(state: State<'_, AppState>, id: String) -> Result<bool> {
    let deleted = state.db.delete_proxy_template(&id)?;
    if deleted {
        state
            .db
            .insert_audit("proxy_template.delete", Some(&id), None)?;
    }
    Ok(deleted)
}

/// Tạo proxy mới từ template (copy config + credential mã hoá nguyên vẹn —
/// không giải mã trong quá trình copy). `name` None/rỗng → dùng tên template.
#[tauri::command]
pub fn create_proxy_from_template(
    state: State<'_, AppState>,
    template_id: String,
    name: Option<String>,
) -> Result<Proxy> {
    let rec = state
        .db
        .create_proxy_from_template(&template_id, name.as_deref())?;
    state.db.insert_audit(
        "proxy.create_from_template",
        Some(&rec.id),
        Some(&json!({ "template_id": template_id })),
    )?;
    Ok(proxy_to_model(rec))
}

// ---------------------------------------------------------------------------
// Proxy check (W19b) — kết nối QUA proxy tới IP-echo, trả IP/country/latency
// ---------------------------------------------------------------------------

/// Input check proxy: hoặc `proxy_id` (đọc DB + giải mã credential trong
/// backend), hoặc tham số inline từ form (credential plaintext, không lưu).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProxyCheckInput {
    pub proxy_id: Option<String>,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Check proxy on-demand. Lỗi kết nối trả trong `ProxyCheckResult.error`
/// (command chỉ Err khi input không hợp lệ / DB lỗi). Nếu check theo
/// `proxy_id` thì cập nhật `health_status` ("ok"/"fail") + audit.
#[tauri::command]
pub async fn check_proxy(
    state: State<'_, AppState>,
    input: ProxyCheckInput,
) -> Result<ProxyCheckResult> {
    let proxy_id = input.proxy_id.clone();
    let url = match &proxy_id {
        Some(pid) => {
            let rec = state.db.get_proxy(pid)?;
            let username = rec
                .username_enc
                .as_deref()
                .map(crypto::decrypt_secret)
                .transpose()?;
            let password = rec
                .password_enc
                .as_deref()
                .map(crypto::decrypt_secret)
                .transpose()?;
            proxy_check::build_proxy_url(
                &rec.protocol,
                &rec.host,
                rec.port,
                username.as_deref(),
                password.as_deref(),
            )?
        }
        None => {
            let protocol = input
                .protocol
                .as_deref()
                .ok_or_else(|| AppError::InvalidInput("missing proxy protocol".into()))?;
            let host = input
                .host
                .as_deref()
                .ok_or_else(|| AppError::InvalidInput("missing proxy host".into()))?;
            let port = input
                .port
                .ok_or_else(|| AppError::InvalidInput("missing proxy port".into()))?;
            proxy_check::build_proxy_url(
                protocol,
                host,
                port,
                input.username.as_deref(),
                input.password.as_deref(),
            )?
        }
    };

    let result = proxy_check::check_proxy_url(&url).await;

    if let Some(pid) = &proxy_id {
        let status = if result.ok { "ok" } else { "fail" };
        state.db.set_proxy_health(pid, status)?;
        state.db.insert_audit(
            "proxy.check",
            Some(pid),
            Some(&json!({ "ok": result.ok, "latency_ms": result.latency_ms })),
        )?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Session (launch/stop/list_running)
// ---------------------------------------------------------------------------

/// (W26b) Wrapper đo metrics quanh launch: duration chỉ ghi khi THÀNH CÔNG
/// (p95 là latency launch ok), mọi lỗi (kể cả CDP attach) đếm vào `fail`.
#[tauri::command]
pub async fn launch_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<RunningSession> {
    let t0 = std::time::Instant::now();
    let res = launch_profile_inner(&app, &state, &profile_id).await;
    match &res {
        Ok(_) => state
            .metrics
            .record_success(t0.elapsed().as_millis() as u64),
        Err(_) => state.metrics.record_fail(),
    }
    res
}

async fn launch_profile_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    profile_id: &str,
) -> Result<RunningSession> {
    let mut profile = state.db.get_profile(profile_id)?;

    // (W42) Rotate-on-launch: xoay proxy round-robin TRƯỚC khi tính proxy_url.
    // Best-effort — lỗi xoay (pool rỗng, hết proxy healthy…) KHÔNG chặn launch,
    // giữ nguyên proxy hiện tại.
    if profile.proxy_id.is_some() && profile.rotate_on_launch {
        match state.db.rotate_proxy(profile_id) {
            Ok(rec) => {
                state.db.insert_audit(
                    "profile.rotate_on_launch",
                    Some(profile_id),
                    Some(&json!({ "proxy_id": rec.id })),
                )?;
                profile = state.db.get_profile(profile_id)?;
            }
            Err(e) => {
                tracing::warn!(
                    "launch_profile {profile_id}: rotate_on_launch thất bại — giữ proxy hiện tại: {e}"
                );
            }
        }
    }

    // (W51-B1) Run-dir thiếu dữ liệu mà có archive local `.bxa` → restore
    // best-effort TRƯỚC khi spawn (mất run-dir vẫn cứu được session từ backup);
    // lỗi restore chỉ log warn, launch tiếp với profile trống.
    {
        let dir = PathBuf::from(&profile.user_data_dir);
        if !dir.join("Default").is_dir()
            && archive::archive_path(&dir, profile_id).is_some_and(|p| p.is_file())
        {
            let id = profile_id.to_string();
            match tokio::task::spawn_blocking(move || archive::restore_archive(&dir, &id)).await {
                Ok(Ok(())) => {
                    tracing::info!(
                        "launch_profile {profile_id}: restored user_data_dir from local archive"
                    );
                }
                Ok(Err(e)) => tracing::warn!(
                    "launch_profile {profile_id}: archive restore failed — launching fresh: {e}"
                ),
                Err(e) => tracing::warn!(
                    "launch_profile {profile_id}: archive restore panicked — launching fresh: {e}"
                ),
            }
        }
    }

    // (W56a) Dọn stale Chromium SingletonLock TRƯỚC khi spawn: phiên bị kill -9
    // để lại lock → Chromium tưởng instance cũ còn sống và abort. PID trong lock
    // còn sống thật → chặn launch với error rõ ràng (KHÔNG xoá lock — tránh 2
    // Chromium ghi cùng user-data-dir); PID chết/malformed → dọn best-effort.
    {
        let dir = PathBuf::from(&profile.user_data_dir);
        let id = profile_id.to_string();
        match tokio::task::spawn_blocking(move || {
            profile_lock::cleanup_stale_profile_lock(&dir, &id)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(AppError::Launch(e.to_string())),
            Err(e) => tracing::warn!(
                "launch_profile {profile_id}: kiểm tra SingletonLock panicked — bỏ qua: {e}"
            ),
        }
    }

    // (W54) Ghi tên profile vào Preferences/Local State TRƯỚC khi spawn để
    // Chromium hiển thị đúng tên. Best-effort — lỗi chỉ log warn bên trong,
    // KHÔNG chặn launch (panic của task cũng chỉ warn).
    {
        let dir = PathBuf::from(&profile.user_data_dir);
        let name = profile.name.clone();
        if let Err(e) =
            tokio::task::spawn_blocking(move || launcher::write_profile_name_prefs(&dir, &name))
                .await
        {
            tracing::warn!("launch_profile {profile_id}: ghi tên profile panicked — bỏ qua: {e}");
        }
    }

    let proxy_url = match profile.proxy_id.as_deref() {
        Some(pid) => Some(proxy_url_from(&state.db.get_proxy(pid)?)?),
        None => None,
    };

    let progress_app = app.clone();
    let progress = move |phase: &str, pct: u8, downloaded_bytes: u64, total_bytes: u64| {
        let _ = progress_app.emit(
            "binary://progress",
            BinaryProgressEvent {
                phase: phase.to_string(),
                pct,
                downloaded_bytes,
                total_bytes,
            },
        );
    };
    let binary_path = binary::ensure_binary(None, Some(&progress)).await?;

    let cdp_port = state.procs.allocate_cdp_port()?;
    // (P3-1a) Extension gán từ kho trung tâm (chỉ enabled) — merge với legacy
    // profile.extensions bên trong build_args.
    let assigned_exts = state.db.profile_extension_paths(profile_id)?;
    // (W35) GeoIP auto-match: geoip=true + có proxy + còn field trống → resolve
    // tz/locale/geo từ exit IP của proxy. Best-effort: lỗi mạng → launch tiếp
    // với giá trị profile như cũ. Thủ công thắng (xem launcher::build_args).
    let geo = match proxy_url.as_deref() {
        Some(url) if geoip::profile_needs_geoip(&profile) => {
            let g = geoip::resolve_geo(url).await;
            if g.is_none() {
                tracing::warn!(
                    "launch_profile {profile_id}: GeoIP resolve thất bại — bỏ qua auto-match"
                );
            }
            g
        }
        _ => None,
    };
    let args = launcher::build_args(
        &profile,
        proxy_url.as_deref(),
        cdp_port,
        &assigned_exts,
        geo.as_ref(),
    );
    let program = binary_path.to_string_lossy().into_owned();

    let session = state
        .procs
        .spawn(profile_id, &program, args, cdp_port)
        .await?;

    if let Err(e) = cdp::attach(cdp_port).await {
        tracing::error!("launch_profile {profile_id}: CDP attach failed: {e}");
        let _ = state.procs.stop(profile_id).await;
        emit_status(app, profile_id, "error", None, None);
        return Err(e);
    }

    state.db.touch_last_start(profile_id)?;
    state.db.insert_audit(
        "profile.launch",
        Some(profile_id),
        Some(&json!({ "pid": session.pid, "cdp_port": cdp_port })),
    )?;
    emit_status(
        app,
        profile_id,
        "running",
        Some(session.pid),
        Some(session.cdp_url.clone()),
    );
    Ok(session)
}

#[tauri::command]
pub async fn stop_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<()> {
    state.procs.stop(&profile_id).await?;
    state
        .db
        .insert_audit("profile.stop", Some(&profile_id), None)?;
    emit_status(&app, &profile_id, "stopped", None, None);
    let cleanups = auto_clear_cache_if_enabled(&state.db, &profile_id)
        .into_iter()
        .chain(apply_storage_options_on_stop(&state.db, &profile_id))
        .collect();
    // (W51-B1) Archive best-effort SAU sanitize; lỗi không chặn stop-flow.
    archive_profile_after_stop(app, &state.db, &profile_id, "stopped", cleanups);
    Ok(())
}

/// (W51-B1) Nén + mã hoá profile thành `.bxa` SAU khi phiên dừng và các
/// cleanup W49/W20b (`cleanups`) chạy xong — archive phải chụp dữ liệu ĐÃ
/// sanitize. Best-effort trong background: mọi lỗi chỉ log warn, không chặn
/// stop-flow. Emit `profile://status` "archiving" khi bắt đầu nén và
/// `final_status` ("stopped"/"crashed") khi xong. Semaphore trong archive.rs
/// giới hạn tối đa 2 archive song song.
pub(crate) fn archive_profile_after_stop(
    app: AppHandle,
    db: &Arc<Db>,
    profile_id: &str,
    final_status: &str,
    cleanups: Vec<tauri::async_runtime::JoinHandle<()>>,
) -> tauri::async_runtime::JoinHandle<()> {
    let db = Arc::clone(db);
    let profile_id = profile_id.to_string();
    let final_status = final_status.to_string();
    tauri::async_runtime::spawn(async move {
        for h in cleanups {
            let _ = h.await;
        }
        let Ok(profile) = db.get_profile(&profile_id) else {
            return;
        };
        let dir = PathBuf::from(profile.user_data_dir);
        let _slot = archive::acquire_slot().await;
        emit_status(&app, &profile_id, "archiving", None, None);
        let id = profile_id.clone();
        let arch_dir = dir.clone();
        let res =
            tokio::task::spawn_blocking(move || archive::archive_profile(&arch_dir, &id)).await;
        match res {
            Ok(Ok(archive::ArchiveOutcome::Written { bytes })) => {
                tracing::info!("archive {profile_id}: wrote {bytes} bytes to local .bxa");
                let _ = db.insert_audit(
                    "profile.archive",
                    Some(&profile_id),
                    Some(&json!({ "bytes": bytes })),
                );
                // (W51-B2) Cloud sync best-effort: upload .bxa lên Telegram khi
                // setting bật + credential đủ. Task background — lỗi KHÔNG còn
                // bị nuốt: trạng thái + lỗi ghi `cloud_upload_state` (W52-B C1).
                // (W55b-transport) sync_ready xét theo transport đang chọn.
                if cloud_transport::sync_ready(&db) {
                    if let Some(bxa) = archive::archive_path(&dir, &profile_id) {
                        drop(upload_archive_to_cloud(
                            app.clone(),
                            Arc::clone(&db),
                            profile_id.clone(),
                            bxa,
                        ));
                    }
                }
            }
            Ok(Ok(archive::ArchiveOutcome::SkippedClean)) => {
                tracing::info!("archive {profile_id}: skipped — data unchanged since last archive");
            }
            Ok(Ok(archive::ArchiveOutcome::SkippedNoData)) => {
                tracing::debug!("archive {profile_id}: no Default/ dir — nothing to archive");
            }
            Ok(Err(e)) => tracing::warn!("archive {profile_id} failed (best-effort): {e}"),
            Err(e) => tracing::warn!("archive {profile_id} task panicked: {e}"),
        }
        emit_status(&app, &profile_id, &final_status, None, None);
    })
}

/// (W52-B C1) Ghi trạng thái upload — lỗi ghi DB chỉ log warn (không được
/// che lỗi upload gốc hay chặn flow).
fn record_upload_state(profile_id: &str, res: Result<()>) {
    if let Err(e) = res {
        tracing::warn!("cloud upload {profile_id}: state write failed: {e}");
    }
}

/// (W52-B C1+C6) Lõi upload `.bxa` lên Telegram dùng chung cho auto-upload
/// sau stop, `retry_cloud_upload` và `backup_now`: giữ slot upload, chuyển
/// trạng thái "uploading" → "uploaded"/"failed" trong `cloud_upload_state`
/// (lỗi KHÔNG bị nuốt — vừa ghi DB vừa trả Err cho caller), emit
/// `cloud://progress` từng part khi có `app` (None trong test).
pub(crate) async fn run_cloud_upload(
    app: Option<&AppHandle>,
    db: &Arc<Db>,
    profile_id: &str,
    bxa_path: &Path,
    client: &telegram_sync::TelegramClient,
) -> Result<()> {
    let _slot = telegram_sync::acquire_upload_slot().await;
    record_upload_state(profile_id, db.set_cloud_upload_started(profile_id, "uploading"));
    let progress = move |phase: &str, i: usize, n: usize, done: u64, total: u64| {
        if let Some(app) = app {
            emit_cloud_progress(app, profile_id, phase, i, n, done, total);
        }
    };
    match telegram_sync::upload_archive(client, db, profile_id, bxa_path, Some(&progress)).await {
        Ok(()) => {
            tracing::info!("telegram upload {profile_id}: archive uploaded");
            record_upload_state(profile_id, db.set_cloud_upload_succeeded(profile_id));
            let _ = db.insert_audit("profile.cloud_backup", Some(profile_id), None);
            Ok(())
        }
        Err(e) => {
            tracing::warn!("telegram upload {profile_id} failed: {e}");
            record_upload_state(profile_id, db.set_cloud_upload_failed(profile_id, &e.to_string()));
            Err(e)
        }
    }
}

/// (W55b-transport) Lõi upload `.bxa` qua USERBOT (MTProto, không split) —
/// đối xứng [`run_cloud_upload`]: giữ slot upload, "uploading" →
/// "uploaded"/"failed" trong `cloud_upload_state` (flood-wait skip cũng
/// persist "flood wait until <time>" như C1), emit `cloud://progress`.
pub(crate) async fn run_cloud_upload_userbot(
    app: Option<&AppHandle>,
    db: &Arc<Db>,
    profile_id: &str,
    bxa_path: &Path,
) -> Result<()> {
    let _slot = telegram_sync::acquire_upload_slot().await;
    record_upload_state(profile_id, db.set_cloud_upload_started(profile_id, "uploading"));
    let progress = move |phase: &str, i: usize, n: usize, done: u64, total: u64| {
        if let Some(app) = app {
            emit_cloud_progress(app, profile_id, phase, i, n, done, total);
        }
    };
    match cloud_transport::upload_archive_userbot(db, profile_id, bxa_path, Some(&progress)).await
    {
        Ok(()) => {
            tracing::info!("userbot upload {profile_id}: archive uploaded");
            record_upload_state(profile_id, db.set_cloud_upload_succeeded(profile_id));
            let _ = db.insert_audit("profile.cloud_backup", Some(profile_id), None);
            Ok(())
        }
        Err(e) => {
            tracing::warn!("userbot upload {profile_id} failed: {e}");
            record_upload_state(profile_id, db.set_cloud_upload_failed(profile_id, &e.to_string()));
            Err(e)
        }
    }
}

/// (W51-B2) Upload `.bxa` lên Telegram trong background (semaphore 1 upload/
/// lần). (W52-B C1) KHÔNG nuốt lỗi: trạng thái pending → uploading →
/// uploaded/failed + lỗi ghi bảng `cloud_upload_state`; (C6) progress từng
/// part emit `cloud://progress`. Thành công thì audit + retention prune chạy
/// bên trong `telegram_sync::upload_archive`. (W55b-transport) Route theo
/// setting `cloud_transport`: "userbot" → upload nguyên file qua MTProto.
pub(crate) fn upload_archive_to_cloud(
    app: AppHandle,
    db: Arc<Db>,
    profile_id: String,
    bxa_path: PathBuf,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        record_upload_state(&profile_id, db.set_cloud_upload_started(&profile_id, "pending"));
        if cloud_transport::get_transport(&db) == "userbot" {
            // Lỗi upload đã ghi `cloud_upload_state` bên trong.
            let _ = run_cloud_upload_userbot(Some(&app), &db, &profile_id, &bxa_path).await;
            return;
        }
        let creds = match telegram_sync::load_credentials(&db) {
            Ok(Some(c)) => c,
            Ok(None) => {
                record_upload_state(
                    &profile_id,
                    db.set_cloud_upload_failed(&profile_id, "Telegram credentials not configured"),
                );
                return;
            }
            Err(e) => {
                record_upload_state(
                    &profile_id,
                    db.set_cloud_upload_failed(&profile_id, &format!("credentials unreadable: {e}")),
                );
                return;
            }
        };
        let client = match telegram_sync::TelegramClient::new(creds.0, creds.1) {
            Ok(c) => c,
            Err(e) => {
                record_upload_state(
                    &profile_id,
                    db.set_cloud_upload_failed(&profile_id, &format!("client init failed: {e}")),
                );
                return;
            }
        };
        // Lỗi upload đã ghi `cloud_upload_state` bên trong run_cloud_upload.
        let _ = run_cloud_upload(Some(&app), &db, &profile_id, &bxa_path, &client).await;
    })
}

/// (W51-B2) Lưu Bot Token + Chat ID (mã hoá bằng crypto.rs như proxy
/// credential). Chuỗi rỗng cả 2 → xoá credential.
#[tauri::command]
pub async fn telegram_set_credentials(
    state: State<'_, AppState>,
    bot_token: String,
    chat_id: String,
) -> Result<()> {
    telegram_sync::save_credentials(&state.db, &bot_token, &chat_id)?;
    state
        .db
        .insert_audit("telegram.set_credentials", None, None)?;
    Ok(())
}

/// (W51-B2) FE cần biết đã cấu hình credential chưa (không trả plaintext).
#[tauri::command]
pub async fn telegram_credentials_status(state: State<'_, AppState>) -> Result<bool> {
    Ok(telegram_sync::load_credentials(&state.db)?.is_some())
}

/// (W51-B2) Test kết nối: getMe (verify token) + gửi message thử vào chat.
/// Trả username bot khi thành công.
#[tauri::command]
pub async fn telegram_test_connection(state: State<'_, AppState>) -> Result<String> {
    let (token, chat_id) = telegram_sync::load_credentials(&state.db)?.ok_or_else(|| {
        AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
    })?;
    let client = telegram_sync::TelegramClient::new(token, chat_id)?;
    let me = client.get_me().await?;
    client
        .send_message("BrowserX: Telegram cloud sync connected ✅")
        .await?;
    state
        .db
        .insert_audit("telegram.test_connection", None, None)?;
    Ok(me.username.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// (W55b-core) Userbot MTProto — auth commands (API surface CHỐT CỨNG trong spec)
// ---------------------------------------------------------------------------

/// (W55b) Trạng thái userbot cho FE: `{ state, phoneHint?, username? }`.
/// Cũng init lazy client nếu đã có credentials (idempotent).
#[tauri::command]
pub async fn userbot_get_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<userbot::UserbotStatus> {
    userbot::ensure_client(&app, &state.db)?;
    userbot::current_status(&state.db)
}

/// (W55b) Lưu api_id + api_hash từ my.telegram.org (hash mã hoá XChaCha20-
/// Poly1305 như bot token). api_hash rỗng → xoá credentials. Có credentials
/// mới → init client ngay để chạy auth flow.
#[tauri::command]
pub async fn userbot_set_credentials(
    app: AppHandle,
    state: State<'_, AppState>,
    api_id: i32,
    api_hash: String,
) -> Result<()> {
    userbot::save_credentials(&state.db, api_id, &api_hash)?;
    state.db.insert_audit("userbot.set_credentials", None, None)?;
    userbot::ensure_client(&app, &state.db)?;
    Ok(())
}

/// (W55b) Submit số điện thoại (state waiting_phone). KHÔNG log số.
#[tauri::command]
pub async fn userbot_send_phone(
    app: AppHandle,
    state: State<'_, AppState>,
    phone: String,
) -> Result<()> {
    userbot::ensure_client(&app, &state.db)?;
    userbot::send_phone(&phone).await
}

/// (W55b) Submit mã OTP (state waiting_code). KHÔNG log mã.
#[tauri::command]
pub async fn userbot_submit_code(code: String) -> Result<()> {
    userbot::submit_code(&code).await
}

/// (W55b) Submit mật khẩu 2FA (state waiting_password). KHÔNG log password.
#[tauri::command]
pub async fn userbot_submit_password(password: String) -> Result<()> {
    userbot::submit_password(&password).await
}

/// (W55b) Logout: revoke session phía Telegram + xoá session dir local.
#[tauri::command]
pub async fn userbot_logout(state: State<'_, AppState>) -> Result<()> {
    userbot::logout().await?;
    state.db.insert_audit("userbot.logout", None, None)?;
    Ok(())
}

/// (W55b-transport) Transport Cloud Sync đang chọn: "bot_api" | "userbot"
/// (mặc định "bot_api" khi chưa từng set).
#[tauri::command]
pub async fn cloud_get_transport(state: State<'_, AppState>) -> Result<String> {
    Ok(cloud_transport::get_transport(&state.db))
}

/// (W55b-transport) Đổi transport Cloud Sync. "userbot" CHỈ nhận khi userbot
/// status = ready (guard trong cloud_transport::set_transport).
#[tauri::command]
pub async fn cloud_set_transport(state: State<'_, AppState>, transport: String) -> Result<()> {
    cloud_transport::set_transport(&state.db, &transport)?;
    state.db.insert_audit(
        "cloud.set_transport",
        None,
        Some(&json!({ "transport": transport })),
    )?;
    Ok(())
}

/// (W51-B2) Danh sách bản backup cloud (gộp part) — FE map view Cloud sync.
/// (W55c) Bản app DB (sentinel `__app_db__`) KHÔNG lẫn vào danh sách profile
/// — FE lấy riêng qua `app_db_cloud_status`.
#[tauri::command]
pub async fn list_cloud_backups(
    state: State<'_, AppState>,
) -> Result<Vec<db::CloudBackupInfo>> {
    let mut list = state.db.list_cloud_backups(None)?;
    list.retain(|b| b.profile_id != app_db_backup::APP_DB_PROFILE_ID);
    Ok(list)
}

/// (W52-B C1) Trạng thái upload cloud của MỌI profile (status/lỗi/retry_count)
/// — FE hiển thị lý do fail + nút retry trong view Cloud sync.
/// (W55c) Trạng thái app DB (sentinel) cũng bị lọc — trả riêng.
#[tauri::command]
pub async fn list_cloud_upload_states(
    state: State<'_, AppState>,
) -> Result<Vec<db::CloudUploadState>> {
    let mut list = state.db.list_cloud_upload_states()?;
    list.retain(|s| s.profile_id != app_db_backup::APP_DB_PROFILE_ID);
    Ok(list)
}

/// (W52-B C1) Chạy lại upload bản `.bxa` LOCAL gần nhất của profile (bản đã
/// archive sau lần stop cuối). `NotFound` khi chưa có file `.bxa`; trả Err
/// upload để FE hiện lỗi ngay (trạng thái + lỗi cũng ghi `cloud_upload_state`).
/// (W55b-transport) Route theo transport đang chọn.
#[tauri::command]
pub async fn retry_cloud_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<()> {
    let profile = state.db.get_profile(&profile_id)?;
    let dir = PathBuf::from(profile.user_data_dir);
    let bxa = archive::archive_path(&dir, &profile_id)
        .filter(|p| p.is_file())
        .ok_or_else(|| {
            AppError::NotFound(format!("no local archive for profile {profile_id}"))
        })?;
    if cloud_transport::get_transport(&state.db) == "userbot" {
        return run_cloud_upload_userbot(Some(&app), &state.db, &profile_id, &bxa).await;
    }
    let (token, chat_id) = telegram_sync::load_credentials(&state.db)?.ok_or_else(|| {
        AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
    })?;
    let client = telegram_sync::TelegramClient::new(token, chat_id)?;
    run_cloud_upload(Some(&app), &state.db, &profile_id, &bxa, &client).await
}

/// (W52-B C5) "Sync now": archive NGAY (bỏ dirty-check fast-skip — user chủ
/// động yêu cầu backup) + upload lên Telegram. CHỈ cho profile đang STOPPED:
/// SQLite của phiên đang mở không snapshot an toàn được → chặn với lỗi rõ ràng.
/// Tái dùng pipeline archive (semaphore 2 slot) + upload (slot 1) hiện có.
#[tauri::command]
pub async fn backup_now(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<()> {
    backup_now_impl(Some(&app), &state.db, &state.procs, &profile_id, None).await
}

/// Lõi `backup_now` tách khỏi `State`/`AppHandle` để unit-test được guard +
/// full flow (test truyền `api_base` stub loopback; production None → Bot API
/// thật). `app = None` → không emit progress. (W55b-transport) Transport
/// "userbot" → upload nguyên file qua MTProto (bỏ qua `api_base`).
///
/// TOCTOU chấp nhận được: giữa check `is_running` và bước nén có cửa sổ nhỏ —
/// profile có thể được launch giữa chừng. Hệ quả xấu nhất là archive snapshot
/// hơi cũ (không hỏng dữ liệu), nên không cần lock chặt hơn.
pub(crate) async fn backup_now_impl(
    app: Option<&AppHandle>,
    db: &Arc<Db>,
    procs: &ProcessManager,
    profile_id: &str,
    api_base: Option<String>,
) -> Result<()> {
    if procs.is_running(profile_id).await {
        return Err(AppError::InvalidInput(format!(
            "profile {profile_id} đang chạy — stop trước khi backup now"
        )));
    }
    let profile = db.get_profile(profile_id)?;
    let dir = PathBuf::from(profile.user_data_dir);
    if !dir.join("Default").is_dir() {
        return Err(AppError::InvalidInput(
            "profile chưa có dữ liệu (chưa từng launch) — không có gì để backup".into(),
        ));
    }
    // Credential check TRƯỚC khi nén (fail sớm, không tốn CPU vô ích).
    let use_userbot = cloud_transport::get_transport(db) == "userbot";
    let client = if use_userbot {
        if userbot::load_credentials(db)?.is_none() {
            return Err(AppError::InvalidInput(
                "Userbot chưa cấu hình — nhập api_id + api_hash và đăng nhập trước".into(),
            ));
        }
        None
    } else {
        let (token, chat_id) = telegram_sync::load_credentials(db)?.ok_or_else(|| {
            AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
        })?;
        Some(match api_base {
            Some(base) => telegram_sync::TelegramClient::with_api_base(base, token, chat_id)?,
            None => telegram_sync::TelegramClient::new(token, chat_id)?,
        })
    };

    // Archive FORCED (bỏ dirty-check fast-skip): user bấm Sync now kỳ vọng
    // snapshot mới bất kể dữ liệu có đổi từ lần archive trước hay không.
    let _slot = archive::acquire_slot().await;
    let arch_dir = dir.clone();
    let id = profile_id.to_string();
    let outcome =
        tokio::task::spawn_blocking(move || archive::archive_profile_forced(&arch_dir, &id))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("archive task panic: {e}")))??;
    if let archive::ArchiveOutcome::Written { bytes } = outcome {
        let _ = db.insert_audit(
            "profile.archive",
            Some(profile_id),
            Some(&json!({ "bytes": bytes, "manual": true })),
        );
    }
    let bxa = archive::archive_path(&dir, profile_id)
        .filter(|p| p.is_file())
        .ok_or_else(|| AppError::NotFound(format!("no local archive for profile {profile_id}")))?;
    match client {
        Some(client) => run_cloud_upload(app, db, profile_id, &bxa, &client).await,
        None => run_cloud_upload_userbot(app, db, profile_id, &bxa).await,
    }
}

/// (W51-B2) Restore từ cloud: tải các part 1 bản backup → ghép → verify sha256
/// → ghi `.bxa` cạnh user_data_dir → archive.rs giải mã/giải nén (W51-B1).
/// Cùng guard như restore local: profile phải dừng + run-dir thiếu/hỏng.
/// (W52-B C6) Progress từng part emit `cloud://progress` phase "download".
/// (W52-F) `uploaded_at = None` → bản MỚI NHẤT; Some → đúng bản version đó.
#[tauri::command]
pub async fn restore_from_cloud(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    uploaded_at: Option<String>,
) -> Result<()> {
    if state.procs.is_running(&profile_id).await {
        return Err(AppError::InvalidInput(format!(
            "profile {profile_id} đang chạy — stop trước khi restore từ cloud"
        )));
    }
    let profile = state.db.get_profile(&profile_id)?;
    let dir = PathBuf::from(profile.user_data_dir);
    if dir.join("Default").is_dir() {
        return Err(AppError::InvalidInput(
            "user_data_dir vẫn còn dữ liệu — chỉ restore khi run-dir thiếu/hỏng".into(),
        ));
    }
    let parts = state
        .db
        .get_cloud_backup_parts(&profile_id, uploaded_at.as_deref())?;
    if parts.is_empty() {
        return Err(AppError::NotFound(format!(
            "no cloud backup for profile {profile_id}"
        )));
    }
    let progress = |phase: &str, i: usize, n: usize, done: u64, total: u64| {
        emit_cloud_progress(&app, &profile_id, phase, i, n, done, total);
    };
    // (W55b-transport) Route theo transport GỐC của bản backup (cột DB) —
    // upload bằng đường nào thì restore bằng đường đó.
    let data = if parts[0].transport == "userbot" {
        cloud_transport::download_backup_userbot(&parts, Some(&progress)).await?
    } else {
        let (token, chat_id) = telegram_sync::load_credentials(&state.db)?.ok_or_else(|| {
            AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
        })?;
        let client = telegram_sync::TelegramClient::new(token, chat_id)?;
        telegram_sync::download_backup(&client, &parts, Some(&progress)).await?
    };

    let bxa = archive::archive_path(&dir, &profile_id).ok_or_else(|| {
        AppError::InvalidInput(format!("user_data_dir has no parent: {}", dir.display()))
    })?;
    tokio::fs::write(&bxa, &data).await?;
    let id = profile_id.clone();
    tokio::task::spawn_blocking(move || archive::restore_archive(&dir, &id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("restore archive panic: {e}")))??;
    state
        .db
        .insert_audit("profile.cloud_restore", Some(&profile_id), None)?;
    Ok(())
}

/// (W51-B2) Xoá 1 bản backup cloud của profile (message Telegram + row DB).
/// (W52-F) `uploaded_at = None` → bản MỚI NHẤT; Some → đúng bản version đó.
/// (W55b-transport) Route theo transport GỐC của bản backup (cột DB).
#[tauri::command]
pub async fn delete_cloud_backup(
    state: State<'_, AppState>,
    profile_id: String,
    uploaded_at: Option<String>,
) -> Result<()> {
    let parts = state
        .db
        .get_cloud_backup_parts(&profile_id, uploaded_at.as_deref())?;
    if parts.is_empty() {
        return Err(AppError::NotFound(format!(
            "no cloud backup for profile {profile_id}"
        )));
    }
    if parts[0].transport == "userbot" {
        cloud_transport::delete_backup_userbot(&state.db, &parts).await?;
    } else {
        let (token, chat_id) = telegram_sync::load_credentials(&state.db)?.ok_or_else(|| {
            AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
        })?;
        let client = telegram_sync::TelegramClient::new(token, chat_id)?;
        telegram_sync::delete_backup(&client, &state.db, &profile_id, uploaded_at.as_deref())
            .await?;
    }
    state
        .db
        .insert_audit("profile.cloud_backup_delete", Some(&profile_id), None)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (W55c) App DB cloud backup — kiểu VFlowX DB sync
// ---------------------------------------------------------------------------

/// (W55c) Trạng thái cloud backup của APP DB cho FE: lịch sử bản backup
/// (retention 3), trạng thái upload, và cờ "đã staging restore — cần restart".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDbCloudStatus {
    pub backups: Vec<db::CloudBackupInfo>,
    pub upload_state: Option<db::CloudUploadState>,
    pub pending_restore: bool,
}

/// (W55c) Trạng thái backup/restore app DB — FE card "Application database".
#[tauri::command]
pub async fn app_db_cloud_status(state: State<'_, AppState>) -> Result<AppDbCloudStatus> {
    let id = app_db_backup::APP_DB_PROFILE_ID;
    Ok(AppDbCloudStatus {
        backups: state.db.list_cloud_backups(Some(id))?,
        upload_state: state.db.get_cloud_upload_state(id)?,
        pending_restore: app_db_backup::staged_path(state.db.data_dir()).is_file(),
    })
}

/// (W55c) Backup app DB lên cloud NGAY: snapshot an toàn `VACUUM INTO`
/// (KHÔNG copy file đang mở) → mã hoá `.bxa` (pipeline archive.rs) → upload
/// qua transport hiện hành dưới sentinel `__app_db__` (retention 3 bản dùng
/// chung `cloud_backups`). Không cần dừng profile — DB app luôn mở, VACUUM
/// INTO tự chốt snapshot nhất quán.
#[tauri::command]
pub async fn backup_app_db_now(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    backup_app_db_now_impl(Some(&app), &state.db, None).await?;
    state.db.insert_audit("appdb.cloud_backup", None, None)?;
    Ok(())
}

/// Lõi `backup_app_db_now` tách khỏi `State`/`AppHandle` để unit-test được
/// (test truyền `api_base` stub loopback như [`backup_now_impl`]).
pub(crate) async fn backup_app_db_now_impl(
    app: Option<&AppHandle>,
    db: &Arc<Db>,
    api_base: Option<String>,
) -> Result<()> {
    let id = app_db_backup::APP_DB_PROFILE_ID;
    // Credential check TRƯỚC khi snapshot/nén (fail sớm) — pattern backup_now.
    let use_userbot = cloud_transport::get_transport(db) == "userbot";
    let client = if use_userbot {
        if userbot::load_credentials(db)?.is_none() {
            return Err(AppError::InvalidInput(
                "Userbot chưa cấu hình — nhập api_id + api_hash và đăng nhập trước".into(),
            ));
        }
        None
    } else {
        let (token, chat_id) = telegram_sync::load_credentials(db)?.ok_or_else(|| {
            AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
        })?;
        Some(match api_base {
            Some(base) => telegram_sync::TelegramClient::with_api_base(base, token, chat_id)?,
            None => telegram_sync::TelegramClient::new(token, chat_id)?,
        })
    };

    let data_dir = db.data_dir().to_path_buf();
    let snap = data_dir.join("appdb-snapshot.db");
    db.vacuum_into(&snap)?;
    let bxa = app_db_backup::bxa_path(&data_dir);
    let (snap_bg, bxa_bg) = (snap.clone(), bxa.clone());
    let encrypt = tokio::task::spawn_blocking(move || -> Result<()> {
        let plain = std::fs::read(&snap_bg)?;
        archive::encrypt_bytes_to_file(&bxa_bg, &plain)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("app db encrypt task panic: {e}")));
    // Snapshot plaintext là file tạm — xoá dù encrypt thành công hay không.
    let _ = std::fs::remove_file(&snap);
    encrypt??;
    match client {
        Some(client) => run_cloud_upload(app, db, id, &bxa, &client).await,
        None => run_cloud_upload_userbot(app, db, id, &bxa).await,
    }
}

/// (W55c) Restore app DB từ cloud — bước 1 (STAGING, KHÔNG đụng DB đang mở):
/// tải các part (route theo transport gốc của bản backup) → verify sha256 →
/// giải mã → validate (SQLite hợp lệ, schema không mới hơn app, có bảng
/// profiles) → ghi `browserx.db.restore-staged` atomic. Áp dụng THẬT diễn ra
/// lúc restart app ([`crate::app_db_backup::apply_pending_restore`] — DB cũ
/// giữ ở `.bak-cloud-restore-<ts>`). Guard: mọi profile phải DỪNG.
/// `uploaded_at = None` → bản mới nhất.
#[tauri::command]
pub async fn restore_app_db(
    app: AppHandle,
    state: State<'_, AppState>,
    uploaded_at: Option<String>,
) -> Result<()> {
    let id = app_db_backup::APP_DB_PROFILE_ID;
    if !state.procs.list_running().await.is_empty() {
        return Err(AppError::InvalidInput(
            "còn phiên đang chạy — stop mọi profile trước khi restore app DB".into(),
        ));
    }
    let parts = state.db.get_cloud_backup_parts(id, uploaded_at.as_deref())?;
    if parts.is_empty() {
        return Err(AppError::NotFound("no cloud backup for app db".into()));
    }
    let progress = |phase: &str, i: usize, n: usize, done: u64, total: u64| {
        emit_cloud_progress(&app, id, phase, i, n, done, total);
    };
    let data = if parts[0].transport == "userbot" {
        cloud_transport::download_backup_userbot(&parts, Some(&progress)).await?
    } else {
        let (token, chat_id) = telegram_sync::load_credentials(&state.db)?.ok_or_else(|| {
            AppError::InvalidInput("Telegram chưa cấu hình — nhập Bot Token + Chat ID trước".into())
        })?;
        let client = telegram_sync::TelegramClient::new(token, chat_id)?;
        telegram_sync::download_backup(&client, &parts, Some(&progress)).await?
    };

    let data_dir = state.db.data_dir().to_path_buf();
    let staged = app_db_backup::staged_path(&data_dir);
    tokio::task::spawn_blocking(move || -> Result<()> {
        // Giải mã từ file tạm (decrypt helper nhận path) rồi validate TRƯỚC
        // khi staging — file rác/DB lạ không bao giờ thành staged.
        let enc_tmp = data_dir.join("appdb-restore.bxa.tmp");
        std::fs::write(&enc_tmp, &data)?;
        let plain = archive::decrypt_file_to_bytes(&enc_tmp);
        let _ = std::fs::remove_file(&enc_tmp);
        let plain = plain?;

        let staged_tmp = data_dir.join("browserx.db.restore-staged.tmp");
        let result = (|| -> Result<()> {
            std::fs::write(&staged_tmp, &plain)?;
            app_db_backup::validate_snapshot(&staged_tmp)?;
            std::fs::File::open(&staged_tmp)?.sync_all()?;
            std::fs::rename(&staged_tmp, &staged)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&staged_tmp);
        }
        result
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("app db restore task panic: {e}")))??;
    state.db.insert_audit(
        "appdb.cloud_restore_staged",
        None,
        Some(&json!({ "uploaded_at": uploaded_at })),
    )?;
    Ok(())
}

/// (W55c) Huỷ bản restore app DB đã staging (chưa restart) — xoá file staged,
/// DB hiện tại không bị đụng tới.
#[tauri::command]
pub async fn cancel_app_db_restore(state: State<'_, AppState>) -> Result<()> {
    let staged = app_db_backup::staged_path(state.db.data_dir());
    if staged.is_file() {
        std::fs::remove_file(&staged)?;
        state.db.insert_audit("appdb.cloud_restore_cancelled", None, None)?;
    }
    Ok(())
}

/// (W51-B1) Restore thủ công từ archive local `.bxa`: giải mã → verify GCM →
/// giải nén vào user_data_dir. CHỈ khi run-dir thiếu/hỏng (không có `Default/`)
/// — có dữ liệu rồi thì REFUSE để không ghi đè session mới bằng backup cũ.
#[tauri::command]
pub async fn restore_profile_archive(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<()> {
    if state.procs.is_running(&profile_id).await {
        return Err(AppError::InvalidInput(format!(
            "profile {profile_id} đang chạy — stop trước khi restore archive"
        )));
    }
    let profile = state.db.get_profile(&profile_id)?;
    let dir = PathBuf::from(profile.user_data_dir);
    if dir.join("Default").is_dir() {
        return Err(AppError::InvalidInput(
            "user_data_dir vẫn còn dữ liệu — chỉ restore khi run-dir thiếu/hỏng".into(),
        ));
    }
    let id = profile_id.clone();
    tokio::task::spawn_blocking(move || archive::restore_archive(&dir, &id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("restore archive panic: {e}")))??;
    state
        .db
        .insert_audit("profile.archive_restore", Some(&profile_id), None)?;
    Ok(())
}

/// (W23a) "Stop all & quit": dừng TẤT CẢ phiên đang chạy với cleanup đầy đủ
/// như `stop_profile` (audit + auto clear cache + storage options cho TỪNG
/// phiên), chờ cleanup xong, checkpoint WAL rồi `app.exit(0)`. Cờ `quitting`
/// để `RunEvent::ExitRequested` (lib.rs) cho lần thoát này đi qua.
#[tauri::command]
pub async fn stop_all_and_quit(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    state.quitting.store(true, Ordering::SeqCst);
    let mut cleanups = Vec::new();
    for session in state.procs.list_running().await {
        let profile_id = session.profile_id;
        if let Err(e) = state.procs.stop(&profile_id).await {
            tracing::warn!("stop_all_and_quit: stop {profile_id} thất bại: {e}");
            continue;
        }
        let _ = state.db.insert_audit("profile.stop", Some(&profile_id), None);
        emit_status(&app, &profile_id, "stopped", None, None);
        cleanups.extend(auto_clear_cache_if_enabled(&state.db, &profile_id));
        cleanups.extend(apply_storage_options_on_stop(&state.db, &profile_id));
    }
    for handle in cleanups {
        let _ = handle.await;
    }
    if let Err(e) = state.db.wal_checkpoint_truncate() {
        tracing::warn!("stop_all_and_quit: WAL checkpoint thất bại: {e}");
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn list_running(state: State<'_, AppState>) -> Result<Vec<RunningSession>> {
    Ok(state.procs.list_running().await)
}

/// (W20a) Đưa cửa sổ phiên ĐANG chạy lên trước: ưu tiên CDP `Page.bringToFront`;
/// nếu CDP lỗi thì fallback kích hoạt cửa sổ theo PID ở mức OS (macOS: AppleScript
/// qua System Events). Trả `NotFound` nếu profile không chạy.
#[tauri::command]
pub async fn bring_to_front(state: State<'_, AppState>, profile_id: String) -> Result<()> {
    let session = state
        .procs
        .list_running()
        .await
        .into_iter()
        .find(|s| s.profile_id == profile_id)
        .ok_or_else(|| AppError::NotFound(format!("profile {profile_id} không chạy")))?;

    match cdp::bring_to_front(session.cdp_port).await {
        Ok(()) => Ok(()),
        Err(cdp_err) => {
            if activate_window_by_pid(session.pid) {
                Ok(())
            } else {
                Err(cdp_err)
            }
        }
    }
}

/// (W24c) Trả `webSocketDebuggerUrl` (`ws://127.0.0.1:<port>/devtools/browser/…`)
/// của phiên ĐANG chạy — để copy vào Playwright/Puppeteer (connectOverCDP).
/// Trả `NotFound` nếu profile không chạy.
#[tauri::command]
pub async fn get_cdp_ws_url(state: State<'_, AppState>, profile_id: String) -> Result<String> {
    let session = state
        .procs
        .list_running()
        .await
        .into_iter()
        .find(|s| s.profile_id == profile_id)
        .ok_or_else(|| AppError::NotFound(format!("profile {profile_id} không chạy")))?;
    cdp::ws_url(session.cdp_port).await
}

// ---------------------------------------------------------------------------
// CookieRobot (P3-4a) — bot nuôi cookie tuần tự 1 profile (xem cookierobot.rs)
// ---------------------------------------------------------------------------

/// Bắt đầu CookieRobot cho MỘT profile: proxy-guard trước khi start, launch
/// profile nếu chưa chạy, rồi chạy vòng lặp goto/consent/dwell trong task nền
/// (progress qua event `cookierobot://progress`). Err nếu list URL rỗng,
/// proxy chết, hoặc profile đã có robot chạy.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_cookie_robot(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    urls: Vec<String>,
    dwell_secs: u32,
    random_order: bool,
    process_consent: bool,
    close_when_done: bool,
) -> Result<()> {
    let mut urls = cookierobot::normalize_urls(&urls);
    if urls.is_empty() {
        return Err(AppError::InvalidInput(
            "cookie robot cần ít nhất 1 URL http(s) hợp lệ".into(),
        ));
    }
    if random_order {
        cookierobot::shuffle_with(&mut urls, |n| rand::random_range(0..n));
    }

    let profile = state.db.get_profile(&profile_id)?;
    let proxy_url = match profile.proxy_id.as_deref() {
        Some(pid) => Some(proxy_url_from(&state.db.get_proxy(pid)?)?),
        None => None,
    };

    // Proxy-guard TRƯỚC khi start: proxy chết thì không launch/không chạy gì cả.
    if let Some(purl) = &proxy_url {
        let check = proxy_check::check_proxy_url(purl).await;
        if !check.ok {
            return Err(AppError::InvalidInput(format!(
                "proxy của profile không hoạt động — không start cookie robot: {}",
                check.error.unwrap_or_else(|| "unknown".into())
            )));
        }
    }

    // Đăng ký TRƯỚC khi launch: chặn double-start; launch fail → guard drop
    // tự gỡ đăng ký.
    let guard = state.robots.begin(&profile_id)?;

    let running = state
        .procs
        .list_running()
        .await
        .into_iter()
        .find(|s| s.profile_id == profile_id);
    let cdp_port = match running {
        Some(s) => s.cdp_port,
        None => {
            launch_profile(app.clone(), state.clone(), profile_id.clone())
                .await?
                .cdp_port
        }
    };

    state.db.insert_audit(
        "cookierobot.start",
        Some(&profile_id),
        Some(&json!({ "urls": urls.len(), "dwell_secs": dwell_secs })),
    )?;

    let job = cookierobot::RobotJob {
        profile_id,
        urls,
        proxy_url,
        cdp_port,
        dwell_secs,
        process_consent,
        close_when_done,
    };
    tauri::async_runtime::spawn(cookierobot::run(
        app,
        state.procs.clone(),
        guard,
        job,
    ));
    Ok(())
}

/// Huỷ CookieRobot đang chạy của profile — cancel token đánh thức mọi điểm
/// chờ ngay lập tức (robot emit phase "cancelled" rồi tự gỡ đăng ký).
/// Phiên browser KHÔNG bị đóng (user stop_profile riêng nếu muốn).
#[tauri::command]
pub fn stop_cookie_robot(state: State<'_, AppState>, profile_id: String) -> Result<()> {
    if !state.robots.cancel(&profile_id) {
        return Err(AppError::NotFound(format!(
            "không có cookie robot đang chạy cho profile {profile_id}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cookies (W24a) — import/export qua CDP Storage.getCookies/setCookies
// ---------------------------------------------------------------------------

/// Kết quả export cookie: nội dung đã serialize + số cookie.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieExportResult {
    pub data: String,
    pub count: usize,
}

/// Phiên headless TẠM cho thao tác cookie khi profile KHÔNG chạy: spawn trực
/// tiếp (không qua ProcessManager → không hiện "running" trên UI, không chiếm
/// slot semaphore), đóng bằng `Browser.close` khi xong để Chromium flush
/// cookie xuống đĩa (kill cứng có thể mất cookie chưa commit).
struct TempCookieSession {
    child: tokio::process::Child,
    cdp_port: u16,
}

/// Lấy cổng CDP cho thao tác cookie: dùng phiên ĐANG chạy nếu có; nếu không,
/// spawn headless tạm (`--headless=new`, không restore session/URL startup,
/// không proxy — thao tác cookie không cần mạng ra ngoài).
async fn open_cookie_session(
    state: &AppState,
    profile: &Profile,
) -> Result<(u16, Option<TempCookieSession>)> {
    if let Some(s) = state
        .procs
        .list_running()
        .await
        .into_iter()
        .find(|s| s.profile_id == profile.id)
    {
        return Ok((s.cdp_port, None));
    }

    let binary_path = binary::ensure_binary(None, None).await?;
    let cdp_port = state.procs.allocate_cdp_port()?;

    let mut p = profile.clone();
    p.headless = true;
    p.startup_behavior = "custom".into(); // không --restore-last-session
    p.startup_urls = json!([]); // không mở URL nào
    // Phiên tạm thao tác cookie: không cần extension/GeoIP → truyền &[] + None.
    let mut args = launcher::build_args(&p, None, cdp_port, &[], None);
    args.insert(0, "--headless=new".into());

    let child = tokio::process::Command::new(binary_path.as_os_str())
        .args(&args)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| AppError::Launch(format!("spawn headless thất bại: {e}")))?;
    let temp = TempCookieSession { child, cdp_port };

    if let Err(e) = cdp::attach(cdp_port).await {
        close_cookie_session(temp).await;
        return Err(e);
    }
    Ok((cdp_port, Some(temp)))
}

/// Đóng phiên tạm: ưu tiên `Browser.close` (shutdown mềm — flush profile),
/// chờ tối đa 10s cho tiến trình thoát; nếu vẫn còn thì kill. Best-effort.
async fn close_cookie_session(mut temp: TempCookieSession) {
    if cdp::close_browser(temp.cdp_port).await.is_ok() {
        let _ =
            tokio::time::timeout(std::time::Duration::from_secs(10), temp.child.wait()).await;
    }
    let _ = temp.child.start_kill();
    let _ = temp.child.wait().await;
}

/// Export toàn bộ cookie của profile theo `format` ("json" | "netscape").
/// Profile không chạy → mở phiên headless tạm rồi đóng mềm sau khi đọc.
#[tauri::command]
pub async fn export_cookies(
    state: State<'_, AppState>,
    profile_id: String,
    format: String,
) -> Result<CookieExportResult> {
    let format = cookies::Format::parse(&format)?;
    let profile = state.db.get_profile(&profile_id)?;

    let (port, temp) = open_cookie_session(&state, &profile).await?;
    let fetched = cdp::get_all_cookies(port).await;
    if let Some(t) = temp {
        close_cookie_session(t).await;
    }
    let items = fetched?;

    let data = cookies::serialize(&items, format)?;
    state.db.insert_audit(
        "cookies.export",
        Some(&profile_id),
        Some(&json!({ "count": items.len(), "format": format.as_str() })),
    )?;
    Ok(CookieExportResult {
        data,
        count: items.len(),
    })
}

/// Import cookie (JSON hoặc Netscape — auto-detect) vào profile qua
/// `Storage.setCookies`. Trả về số cookie đã ghi.
#[tauri::command]
pub async fn import_cookies(
    state: State<'_, AppState>,
    profile_id: String,
    data: String,
) -> Result<usize> {
    let items = cookies::parse(&data)?;
    let profile = state.db.get_profile(&profile_id)?;

    let (port, temp) = open_cookie_session(&state, &profile).await?;
    let set = cdp::set_cookies(port, &items).await;
    if let Some(t) = temp {
        close_cookie_session(t).await;
    }
    let count = set?;

    state.db.insert_audit(
        "cookies.import",
        Some(&profile_id),
        Some(&json!({ "count": count })),
    )?;
    Ok(count)
}

/// Kết quả export storage_state: JSON + số cookie + số key localStorage.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStateExportResult {
    pub data: String,
    pub cookie_count: usize,
    pub local_storage_count: usize,
}

/// Kết quả import storage_state: số cookie + số key localStorage đã ghi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStateImportResult {
    pub cookie_count: usize,
    pub local_storage_count: usize,
}

/// (W33b) Export full storage_state (cookies + localStorage) của profile theo
/// shape Playwright `{ cookies, origins: [{origin, localStorage}] }`.
/// localStorage chỉ đọc được theo origin → caller truyền danh sách `origins`
/// cần export (rỗng → cookie-only, tương đương export_cookies format json).
/// Lưu ý: đọc localStorage navigate tab đầu tiên tới từng origin.
#[tauri::command]
pub async fn export_storage_state(
    state: State<'_, AppState>,
    profile_id: String,
    origins: Vec<String>,
) -> Result<StorageStateExportResult> {
    let profile = state.db.get_profile(&profile_id)?;

    let (port, temp) = open_cookie_session(&state, &profile).await?;
    let fetched: Result<cookies::StorageState> = async {
        let cookie_items = cdp::get_all_cookies(port).await?;
        let mut origin_states = Vec::with_capacity(origins.len());
        for origin in &origins {
            let entries = cdp::get_local_storage(port, origin).await?;
            origin_states.push(cookies::OriginState {
                origin: origin.clone(),
                local_storage: entries
                    .into_iter()
                    .map(|(name, value)| cookies::LocalStorageEntry { name, value })
                    .collect(),
            });
        }
        Ok(cookies::StorageState {
            cookies: cookie_items,
            origins: origin_states,
        })
    }
    .await;
    if let Some(t) = temp {
        close_cookie_session(t).await;
    }
    let storage_state = fetched?;

    let cookie_count = storage_state.cookies.len();
    let local_storage_count = storage_state
        .origins
        .iter()
        .map(|o| o.local_storage.len())
        .sum();
    let data = cookies::serialize_storage_state(&storage_state)?;
    state.db.insert_audit(
        "storage_state.export",
        Some(&profile_id),
        Some(&json!({ "cookies": cookie_count, "localStorageKeys": local_storage_count })),
    )?;
    Ok(StorageStateExportResult {
        data,
        cookie_count,
        local_storage_count,
    })
}

/// (W33b) Import storage_state vào profile: cookies qua `Storage.setCookies`,
/// localStorage qua navigate + `setItem` theo từng origin. BACKWARD-COMPATIBLE:
/// nhận cả dữ liệu cookie-only cũ (JSON array/Netscape → chỉ ghi cookie).
#[tauri::command]
pub async fn import_storage_state(
    state: State<'_, AppState>,
    profile_id: String,
    data: String,
) -> Result<StorageStateImportResult> {
    let storage_state = cookies::parse_storage_state(&data)?;
    let profile = state.db.get_profile(&profile_id)?;

    let (port, temp) = open_cookie_session(&state, &profile).await?;
    let written: Result<(usize, usize)> = async {
        let cookie_count = if storage_state.cookies.is_empty() {
            0
        } else {
            cdp::set_cookies(port, &storage_state.cookies).await?
        };
        let mut local_storage_count = 0;
        for o in &storage_state.origins {
            let items: Vec<(String, String)> = o
                .local_storage
                .iter()
                .map(|e| (e.name.clone(), e.value.clone()))
                .collect();
            if !items.is_empty() {
                local_storage_count += cdp::set_local_storage(port, &o.origin, &items).await?;
            }
        }
        Ok((cookie_count, local_storage_count))
    }
    .await;
    if let Some(t) = temp {
        close_cookie_session(t).await;
    }
    let (cookie_count, local_storage_count) = written?;

    state.db.insert_audit(
        "storage_state.import",
        Some(&profile_id),
        Some(&json!({ "cookies": cookie_count, "localStorageKeys": local_storage_count })),
    )?;
    Ok(StorageStateImportResult {
        cookie_count,
        local_storage_count,
    })
}

/// AppleScript kích hoạt cửa sổ theo PID (tách riêng để unit-test không cần
/// gọi osascript — tránh popup xin quyền Automation lúc chạy test).
#[cfg(any(target_os = "macos", test))]
fn frontmost_script(pid: u32) -> String {
    format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
    )
}

/// Fallback OS-level cho `bring_to_front`: kích hoạt cửa sổ theo PID bằng
/// AppleScript (`System Events … frontmost = true`). Trả `false` nếu thất bại
/// để caller giữ nguyên lỗi CDP gốc.
#[cfg(target_os = "macos")]
fn activate_window_by_pid(pid: u32) -> bool {
    std::process::Command::new("osascript")
        .args(["-e", &frontmost_script(pid)])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Fallback OS-level chưa hỗ trợ ngoài macOS — luôn `false` (giữ lỗi CDP gốc).
#[cfg(not(target_os = "macos"))]
fn activate_window_by_pid(_pid: u32) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Binary
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ensure_binary(app: AppHandle, version: Option<String>) -> Result<String> {
    let progress_app = app.clone();
    let progress = move |phase: &str, pct: u8, downloaded_bytes: u64, total_bytes: u64| {
        let _ = progress_app.emit(
            "binary://progress",
            BinaryProgressEvent {
                phase: phase.to_string(),
                pct,
                downloaded_bytes,
                total_bytes,
            },
        );
    };
    let path = binary::ensure_binary(version.as_deref(), Some(&progress)).await?;
    Ok(path.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Settings + tags
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>> {
    Ok(state.db.get_settings()?.into_iter().collect())
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<()> {
    state.db.set_setting(&key, &value)
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<Vec<TagInfo>> {
    state.db.list_tags()
}

#[tauri::command]
pub fn set_profile_tags(
    state: State<'_, AppState>,
    profile_id: String,
    tags: Vec<String>,
) -> Result<()> {
    state.db.set_profile_tags(&profile_id, &tags)
}

// ---------------------------------------------------------------------------
// Folders + favorites
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<Folder>> {
    state.db.list_folders()
}

#[tauri::command]
pub fn create_folder(state: State<'_, AppState>, name: String) -> Result<Folder> {
    let folder = state.db.create_folder(&name)?;
    state
        .db
        .insert_audit("folder.create", Some(&folder.id), None)?;
    Ok(folder)
}

#[tauri::command]
pub fn rename_folder(state: State<'_, AppState>, id: String, name: String) -> Result<Folder> {
    let folder = state.db.rename_folder(&id, &name)?;
    state.db.insert_audit("folder.rename", Some(&id), None)?;
    Ok(folder)
}

#[tauri::command]
pub fn delete_folder(state: State<'_, AppState>, id: String) -> Result<bool> {
    let deleted = state.db.delete_folder(&id)?;
    if deleted {
        state.db.insert_audit("folder.delete", Some(&id), None)?;
    }
    Ok(deleted)
}

#[tauri::command]
pub fn set_favorite(state: State<'_, AppState>, id: String, favorite: bool) -> Result<()> {
    state.db.set_favorite(&id, favorite)?;
    state.db.insert_audit(
        "profile.favorite",
        Some(&id),
        Some(&json!({ "favorite": favorite })),
    )?;
    Ok(())
}

#[tauri::command]
pub fn move_profiles_to_folder(
    state: State<'_, AppState>,
    profile_ids: Vec<String>,
    folder_id: Option<String>,
) -> Result<()> {
    let n = state
        .db
        .move_profiles_to_folder(&profile_ids, folder_id.as_deref())?;
    state.db.insert_audit(
        "profile.move_folder",
        None,
        Some(&json!({ "profile_ids": profile_ids, "folder_id": folder_id, "moved": n })),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Trash (soft-delete)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn trash_profiles(state: State<'_, AppState>, profile_ids: Vec<String>) -> Result<()> {
    let n = state.db.trash_profiles(&profile_ids)?;
    state.db.insert_audit(
        "profile.trash",
        None,
        Some(&json!({ "profile_ids": profile_ids, "trashed": n })),
    )?;
    Ok(())
}

#[tauri::command]
pub fn restore_profiles(state: State<'_, AppState>, profile_ids: Vec<String>) -> Result<()> {
    let n = state.db.restore_profiles(&profile_ids)?;
    state.db.insert_audit(
        "profile.restore",
        None,
        Some(&json!({ "profile_ids": profile_ids, "restored": n })),
    )?;
    Ok(())
}

#[tauri::command]
pub fn purge_profiles(state: State<'_, AppState>, profile_ids: Vec<String>) -> Result<()> {
    let n = state.db.purge_profiles(&profile_ids)?;
    state.db.insert_audit(
        "profile.purge",
        None,
        Some(&json!({ "profile_ids": profile_ids, "purged": n })),
    )?;
    Ok(())
}

#[tauri::command]
pub fn list_trash(state: State<'_, AppState>) -> Result<Vec<Profile>> {
    state.db.list_trash()
}

// ---------------------------------------------------------------------------
// Quick profile (W18b): khi Stop, UI hỏi Save as regular / Close & delete
// ---------------------------------------------------------------------------

/// "Save as regular": bỏ cờ quick, giữ nguyên user_data_dir + mọi dữ liệu —
/// profile xuất hiện trong danh sách thường.
#[tauri::command]
pub fn convert_quick_profile(state: State<'_, AppState>, profile_id: String) -> Result<Profile> {
    state.db.set_quick(&profile_id, false)?;
    state
        .db
        .insert_audit("profile.quick_to_regular", Some(&profile_id), None)?;
    state.db.get_profile(&profile_id)
}

/// "Close & delete": dừng phiên nếu còn chạy, xoá user_data_dir trên đĩa rồi
/// xoá hàng DB. REFUSE nếu profile không phải quick (tránh purge nhầm profile thường).
#[tauri::command]
pub async fn delete_quick_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<()> {
    let profile = state.db.get_profile(&profile_id)?;
    if !profile.is_quick {
        return Err(AppError::InvalidInput(format!(
            "profile {profile_id} is not a quick profile"
        )));
    }
    if state.procs.is_running(&profile_id).await {
        state.procs.stop(&profile_id).await?;
        emit_status(&app, &profile_id, "stopped", None, None);
    }
    let dir = PathBuf::from(profile.user_data_dir);
    tokio::task::spawn_blocking(move || -> Result<()> {
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    })
    .await
    .unwrap_or_else(|e| {
        Err(AppError::Other(anyhow::anyhow!(
            "xoá user_data_dir panic: {e}"
        )))
    })?;
    state.db.delete_profile(&profile_id)?;
    state
        .db
        .insert_audit("profile.quick_delete", Some(&profile_id), None)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Storage (W16): đo dung lượng + dọn cache profile
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn profile_storage_sizes(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<storage::ProfileStorageSize>> {
    let mut targets = Vec::with_capacity(ids.len());
    for id in &ids {
        let profile = state.db.get_profile(id)?;
        targets.push((id.clone(), PathBuf::from(profile.user_data_dir)));
    }
    // Walk đĩa có thể chậm với profile lớn → chạy trên blocking pool.
    let sizes = tokio::task::spawn_blocking(move || {
        targets
            .into_iter()
            .map(|(profile_id, dir)| storage::ProfileStorageSize {
                profile_id,
                bytes: storage::dir_size(&dir),
            })
            .collect()
    })
    .await
    .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("đo dung lượng panic: {e}")))?;
    Ok(sizes)
}

#[tauri::command]
pub async fn clear_profile_cache(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<storage::ClearCacheResult>> {
    let mut targets = Vec::with_capacity(ids.len());
    for id in &ids {
        let profile = state.db.get_profile(id)?;
        targets.push((id.clone(), PathBuf::from(profile.user_data_dir)));
    }
    let results = storage::clear_profiles_cache(&state.procs, targets).await;
    for r in &results {
        if r.error.is_none() {
            state.db.insert_audit(
                "profile.clear_cache",
                Some(&r.profile_id),
                Some(&json!({ "freed_bytes": r.freed_bytes })),
            )?;
        }
    }
    Ok(results)
}

/// Tự dọn cache khi phiên dừng nếu setting `auto_clear_cache_on_stop` = "true"
/// (default tắt). Best-effort — gọi từ `stop_profile` và watchdog callback
/// (lib.rs) kiểu fire-and-forget; `stop_all_and_quit` (W23a) await JoinHandle
/// trả về để cleanup chạy xong trước khi thoát app. Lúc này phiên đã ra khỏi
/// registry nên không cần check is_running.
pub(crate) fn auto_clear_cache_if_enabled(
    db: &Arc<Db>,
    profile_id: &str,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    let enabled = matches!(
        db.get_setting(storage::AUTO_CLEAR_SETTING),
        Ok(Some(v)) if v == "true"
    );
    if !enabled {
        return None;
    }
    let Ok(profile) = db.get_profile(profile_id) else {
        return None;
    };
    let db = Arc::clone(db);
    let profile_id = profile_id.to_string();
    Some(tauri::async_runtime::spawn_blocking(move || {
        if let Ok(freed) = storage::clear_cache(Path::new(&profile.user_data_dir)) {
            let _ = db.insert_audit(
                "profile.clear_cache",
                Some(&profile_id),
                Some(&json!({ "freed_bytes": freed, "auto": true })),
            );
        }
    }))
}

/// (W20b) Áp storage options SAU khi phiên dừng: xoá history / passwords /
/// service-worker cache nếu profile tắt lưu loại đó. Cơ chế là CLEANUP —
/// binary không có flag disable (xem `storage::clear_storage_options`).
/// Best-effort như `auto_clear_cache_if_enabled`: fire-and-forget từ
/// `stop_profile`/watchdog (lib.rs); `stop_all_and_quit` (W23a) await
/// JoinHandle trả về trước khi thoát app.
pub(crate) fn apply_storage_options_on_stop(
    db: &Arc<Db>,
    profile_id: &str,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    let Ok(profile) = db.get_profile(profile_id) else {
        return None;
    };
    if profile.store_history && profile.store_passwords && profile.store_sw_cache {
        return None;
    }
    let db = Arc::clone(db);
    let profile_id = profile_id.to_string();
    Some(tauri::async_runtime::spawn_blocking(move || {
        if let Ok(freed) = storage::clear_storage_options(
            Path::new(&profile.user_data_dir),
            profile.store_history,
            profile.store_passwords,
            profile.store_sw_cache,
        ) {
            let _ = db.insert_audit(
                "profile.storage_cleanup",
                Some(&profile_id),
                Some(&json!({
                    "freed_bytes": freed,
                    "history": !profile.store_history,
                    "passwords": !profile.store_passwords,
                    "sw_cache": !profile.store_sw_cache,
                })),
            );
        }
    }))
}

// ---------------------------------------------------------------------------
// Profile templates (W20b): lưu form config làm mẫu, tạo profile điền sẵn
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> Result<Vec<ProfileTemplate>> {
    state.db.list_templates()
}

/// Lưu config (JSON shape `ProfileInput` — payload form hiện tại) thành template.
#[tauri::command]
pub fn save_as_template(
    state: State<'_, AppState>,
    name: String,
    config: serde_json::Value,
) -> Result<ProfileTemplate> {
    let tpl = state.db.create_template(&name, &config)?;
    state
        .db
        .insert_audit("template.create", Some(&tpl.id), None)?;
    Ok(tpl)
}

/// Cập nhật template (F2b): đổi tên + tuỳ chọn thay config (None = giữ nguyên).
#[tauri::command]
pub fn update_template(
    state: State<'_, AppState>,
    id: String,
    name: String,
    config: Option<serde_json::Value>,
) -> Result<ProfileTemplate> {
    let tpl = state.db.update_template(&id, &name, config.as_ref())?;
    state.db.insert_audit("template.update", Some(&id), None)?;
    Ok(tpl)
}

#[tauri::command]
pub fn delete_template(state: State<'_, AppState>, id: String) -> Result<bool> {
    let deleted = state.db.delete_template(&id)?;
    if deleted {
        state.db.insert_audit("template.delete", Some(&id), None)?;
    }
    Ok(deleted)
}

/// Tạo profile mới điền sẵn field từ template (seed + user_data_dir cấp mới).
#[tauri::command]
pub fn create_profile_from_template(
    state: State<'_, AppState>,
    template_id: String,
    name: Option<String>,
) -> Result<Profile> {
    let profile = state
        .db
        .create_profile_from_template(&template_id, name.as_deref())?;
    state.db.insert_audit(
        "profile.create_from_template",
        Some(&profile.id),
        Some(&json!({ "template_id": template_id })),
    )?;
    Ok(profile)
}

/// (W29a) Bulk: tạo `count` profile từ template trong 1 transaction duy nhất
/// (lỗi giữa chừng → 0 profile). `count` ngoài 1..=1000 trả lỗi rõ. Audit
/// đúng MỘT dòng "profile.bulk_create_from_template" meta {template_id, count}
/// — không ghi mỗi profile một dòng.
#[tauri::command]
pub fn create_profiles_from_template(
    state: State<'_, AppState>,
    template_id: String,
    count: u32,
    name_prefix: Option<String>,
) -> Result<Vec<Profile>> {
    let profiles =
        state
            .db
            .create_profiles_from_template(&template_id, count, name_prefix.as_deref())?;
    state.db.insert_audit(
        "profile.bulk_create_from_template",
        Some(&template_id),
        Some(&json!({ "template_id": template_id, "count": profiles.len() })),
    )?;
    Ok(profiles)
}

// ---------------------------------------------------------------------------
// Export / Import profile (W19a): file .bxprofile JSON — xem module `export`.
// Proxy password KHÔNG BAO GIỜ nằm trong file export.
// ---------------------------------------------------------------------------

/// Xuất profile → chuỗi JSON `.bxprofile`. REFUSE khi profile đang chạy
/// (tránh export cấu hình đang bị phiên live thay đổi).
#[tauri::command]
pub async fn export_profile(state: State<'_, AppState>, id: String) -> Result<String> {
    if state.procs.is_running(&id).await {
        return Err(AppError::InvalidInput(format!(
            "profile {id} is running; stop it before exporting"
        )));
    }
    let json = crate::export::export_profile_json(&state.db, &id)?;
    state.db.insert_audit("profile.export", Some(&id), None)?;
    Ok(json)
}

/// Nhập chuỗi JSON `.bxprofile` → tạo profile MỚI (id mới, tên
/// "Imported — {name}"). JSON rác/version lạ → InvalidInput rõ ràng.
#[tauri::command]
pub fn import_profile(state: State<'_, AppState>, json: String) -> Result<Profile> {
    let profile = crate::export::import_profile_json(&state.db, &json)?;
    state
        .db
        .insert_audit("profile.import", Some(&profile.id), None)?;
    Ok(profile)
}

// ---------------------------------------------------------------------------
// Logs (W21b)
// ---------------------------------------------------------------------------

/// Lệnh mở thư mục theo OS (macOS Finder / Windows Explorer / Linux xdg-open).
#[cfg(target_os = "macos")]
const OPEN_FOLDER_CMD: &str = "open";
#[cfg(target_os = "windows")]
const OPEN_FOLDER_CMD: &str = "explorer";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const OPEN_FOLDER_CMD: &str = "xdg-open";

/// Mở thư mục log `~/.browserx/logs` trong file manager của OS (Settings →
/// "Open logs folder"). Tạo thư mục nếu chưa có.
#[tauri::command]
pub fn open_logs_folder() -> Result<()> {
    let dir = crate::logging::logs_dir();
    std::fs::create_dir_all(&dir)?;
    std::process::Command::new(OPEN_FOLDER_CMD)
        .arg(&dir)
        .spawn()
        .map_err(|e| {
            tracing::error!("open_logs_folder: {OPEN_FOLDER_CMD} failed: {e}");
            AppError::Io(e)
        })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Backup / Restore mã hoá ~/.browserx (W25a) — xem module `backup`.
// ---------------------------------------------------------------------------

/// Payload event `backup://progress` (camelCase cho FE).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupProgressEvent {
    phase: String,
    pct: u8,
}

fn emit_backup_progress(app: &AppHandle, phase: &str, pct: u8) {
    let _ = app.emit(
        "backup://progress",
        BackupProgressEvent {
            phase: phase.to_string(),
            pct,
        },
    );
}

/// Thư mục đích mặc định cho file backup: ~/Downloads → home → ".".
fn default_backup_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Kết quả `create_backup` (camelCase cho FE).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub path: String,
    pub bytes: u64,
}

/// Nén + mã hoá toàn bộ `~/.browserx` → file `.browserx-backup` trong
/// `dest_dir` (mặc định ~/Downloads). REFUSE khi còn phiên chạy (user data dir
/// đang bị Chromium ghi → snapshot hỏng). WAL checkpoint TRƯỚC khi nén (W23a)
/// để DB nhất quán trong file chính. Progress emit `backup://progress`.
#[tauri::command]
pub async fn create_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
    dest_dir: Option<String>,
) -> Result<BackupResult> {
    if !state.procs.list_running().await.is_empty() {
        return Err(AppError::InvalidInput(
            "stop all running profiles before creating a backup".into(),
        ));
    }
    // (W25b) Đóng race TOCTOU sau check trên: từ đây đến hết lệnh, launch bị
    // chặn (spawn check cờ dưới cùng lock) — browser không thể ghi vào dữ
    // liệu đang được nén. Guard tự clear khi hàm return (kể cả error path).
    let _maintenance = state.procs.begin_maintenance().await?;
    state.db.wal_checkpoint_truncate()?;
    let data_dir = state.db.data_dir().to_path_buf();

    let dir = dest_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_backup_dir);
    std::fs::create_dir_all(&dir)?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = dir.join(format!("browserx-{ts}.browserx-backup"));

    let app2 = app.clone();
    let dest2 = dest.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        let cb = |phase: &str, pct: u8| emit_backup_progress(&app2, phase, pct);
        crate::backup::create_backup(&data_dir, &dest2, &passphrase, Some(&cb))
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("backup task panic: {e}")))??;

    state.db.insert_audit(
        "backup.create",
        None,
        Some(&json!({ "path": dest.to_string_lossy(), "bytes": bytes })),
    )?;
    Ok(BackupResult {
        path: dest.to_string_lossy().into_owned(),
        bytes,
    })
}

/// Kết quả `restore_backup` (camelCase cho FE). Sau khi restore PHẢI restart
/// app (`restart_app`) — DB connection đang mở vẫn trỏ file cũ đã đổi tên.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// Dữ liệu cũ được giữ tại đây (`<data_dir>.pre-restore-<ts>`), None nếu
    /// trước đó chưa có data dir.
    pub previous_data_dir: Option<String>,
}

/// Giải mã + khôi phục file backup vào `~/.browserx`. REFUSE khi còn phiên
/// chạy. Passphrase sai → fail sớm, dữ liệu hiện tại KHÔNG bị đụng tới
/// (xem `backup::restore_backup`). Progress emit `backup://progress`.
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    backup_path: String,
    passphrase: String,
) -> Result<RestoreResult> {
    if !state.procs.list_running().await.is_empty() {
        return Err(AppError::InvalidInput(
            "stop all running profiles before restoring a backup".into(),
        ));
    }
    // (W25b) Đóng race TOCTOU sau check trên: chặn launch trong suốt quá
    // trình restore đụng filesystem. Guard tự clear khi hàm return.
    let _maintenance = state.procs.begin_maintenance().await?;
    // Checkpoint để bản dữ liệu cũ (giữ lại làm pre-restore) cũng nhất quán.
    state.db.wal_checkpoint_truncate()?;
    let data_dir = state.db.data_dir().to_path_buf();

    let app2 = app.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let cb = |phase: &str, pct: u8| emit_backup_progress(&app2, phase, pct);
        crate::backup::restore_backup(Path::new(&backup_path), &data_dir, &passphrase, Some(&cb))
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("restore task panic: {e}")))??;

    // KHÔNG insert_audit: connection đang mở trỏ vào DB cũ (đã đổi tên) —
    // ghi audit vào đó là vô nghĩa. Log local là đủ.
    tracing::info!(
        "restore_backup: restored; previous data kept at {:?}",
        outcome.previous_data_dir
    );
    Ok(RestoreResult {
        previous_data_dir: outcome
            .previous_data_dir
            .map(|p| p.to_string_lossy().into_owned()),
    })
}

/// (W25a) Restart app sau khi restore — nạp DB/data dir mới. Cờ `quitting`
/// để ExitRequested (lib.rs) không chặn lần thoát này.
#[tauri::command]
pub fn restart_app(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    state.quitting.store(true, Ordering::SeqCst);
    app.restart()
}

// ---------------------------------------------------------------------------
// Extensions (P3-1a) — kho trung tâm + gán N-N với profile. Xem module `extensions`.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_extensions(state: State<'_, AppState>) -> Result<Vec<Extension>> {
    state.db.list_extensions()
}

/// Thêm extension từ folder unpacked local: validate manifest.json rồi lưu
/// nguyên path folder (không copy — user quản lý file gốc).
#[tauri::command]
pub fn add_extension_from_folder(state: State<'_, AppState>, path: String) -> Result<Extension> {
    let dir = PathBuf::from(path.trim());
    let name = extensions::validate_unpacked_dir(&dir)?;
    let dir_str = dir.to_string_lossy();
    let ext = state.db.create_extension(&name, "folder", &dir_str, &dir_str)?;
    state
        .db
        .insert_audit("extension.add", Some(&ext.id), Some(&json!({ "source": "folder" })))?;
    Ok(ext)
}

/// Thêm extension từ URL Chrome Web Store: tải CRX → unpack vào
/// `<data_dir>/extensions/<ext_id>/` → validate manifest → lưu vào kho.
#[tauri::command]
pub async fn add_extension_from_store_url(
    state: State<'_, AppState>,
    url: String,
) -> Result<Extension> {
    let data_dir = state.db.data_dir().to_path_buf();
    let installed = extensions::install_from_store_url(&url, &data_dir).await?;
    let ext = state.db.create_extension(
        &installed.name,
        "store",
        &installed.ext_id,
        &installed.unpacked_path.to_string_lossy(),
    )?;
    state
        .db
        .insert_audit("extension.add", Some(&ext.id), Some(&json!({ "source": "store" })))?;
    Ok(ext)
}

/// Xoá extension khỏi kho (hàng gán profile CASCADE theo). Bản tải từ store
/// nằm trong `<data_dir>/extensions/` → xoá luôn folder unpacked (best-effort);
/// nguồn "folder" là file của user — không đụng.
#[tauri::command]
pub fn remove_extension(state: State<'_, AppState>, id: String) -> Result<()> {
    let ext = state.db.get_extension(&id)?;
    if !state.db.delete_extension(&id)? {
        return Err(AppError::NotFound(format!("extension {id}")));
    }
    if ext.source_type == "store" {
        let dir = PathBuf::from(&ext.unpacked_path);
        if dir.starts_with(extensions::extensions_dir(state.db.data_dir())) {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    state.db.insert_audit("extension.remove", Some(&id), None)?;
    Ok(())
}

/// Bật/tắt extension trong kho — tắt = giữ gán nhưng không nạp khi launch.
#[tauri::command]
pub fn set_extension_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<()> {
    state.db.set_extension_enabled(&id, enabled)
}

/// Gán TOÀN BỘ danh sách extension cho profile (thay thế danh sách cũ).
#[tauri::command]
pub fn assign_extensions(
    state: State<'_, AppState>,
    profile_id: String,
    ext_ids: Vec<String>,
) -> Result<()> {
    state.db.assign_extensions(&profile_id, &ext_ids)?;
    state.db.insert_audit(
        "extension.assign",
        Some(&profile_id),
        Some(&json!({ "count": ext_ids.len() })),
    )?;
    Ok(())
}

/// Danh sách extension đã gán cho profile (kể cả bản đang tắt).
#[tauri::command]
pub fn get_profile_extensions(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<Vec<Extension>> {
    state.db.get_profile_extensions(&profile_id)
}

// ---------------------------------------------------------------------------
// Audit log (W26a)
// ---------------------------------------------------------------------------

/// (W26a) Đọc audit log — mới nhất trước. Filter tuỳ chọn theo action-prefix
/// + target_id; phân trang cursor bằng `before_id` (id nhỏ dần, không offset).
/// `limit` mặc định 50, backend clamp 1..=200.
#[tauri::command]
pub fn list_audit(
    state: State<'_, AppState>,
    action_prefix: Option<String>,
    target_id: Option<String>,
    before_id: Option<i64>,
    limit: Option<u32>,
) -> Result<Vec<db::AuditEntry>> {
    state.db.list_audit(
        action_prefix.as_deref(),
        target_id.as_deref(),
        before_id,
        limit.unwrap_or(50),
    )
}

// ---------------------------------------------------------------------------
// Observability (W26b)
// ---------------------------------------------------------------------------

/// (W26b) Snapshot metrics cho panel System trong Settings.
/// Counter launch + p95 là in-memory "since app start" (xem metrics.rs).
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub live_sessions: u32,
    /// Tổng RSS các phiên đo được; `None` khi có phiên nhưng không đo được
    /// (Windows, hoặc `ps` lỗi) — UI hiển thị N/A thay vì fake 0.
    pub ram_total_mb: Option<u64>,
    /// RSS từng phiên đo được (MB) — có thể ít hơn `live_sessions`.
    pub ram_per_session_mb: Vec<u64>,
    /// p95 duration launch THÀNH CÔNG (100 mẫu gần nhất); `None` khi chưa có mẫu.
    pub launch_p95_ms: Option<u64>,
    pub launch_success: u64,
    pub launch_fail: u64,
}

/// RSS (MB) của pid qua `ps -o rss= -p <pid>` (kB → MB). CHỈ đo process
/// Chromium chính — renderer con không tính (không thêm dep sysinfo).
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn rss_mb(pid: u32) -> Option<u64> {
    let out = tokio::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb / 1024)
}

/// Windows: chưa đo RSS (không có `ps`, không thêm dep) → UI hiển thị N/A.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn rss_mb(_pid: u32) -> Option<u64> {
    None
}

/// (W26b) Metrics realtime: phiên live (registry đã reap zombie), RSS/phiên,
/// launch p95 + success/fail counter. An toàn khi 0 phiên (list rỗng, Some(0)).
#[tauri::command]
pub async fn get_metrics(state: State<'_, AppState>) -> Result<MetricsSnapshot> {
    let sessions = state.procs.list_running().await;
    let live_sessions = sessions.len() as u32;

    let mut ram_per_session_mb = Vec::with_capacity(sessions.len());
    for s in &sessions {
        if let Some(mb) = rss_mb(s.pid).await {
            ram_per_session_mb.push(mb);
        }
    }
    let ram_total_mb = if ram_per_session_mb.is_empty() && live_sessions > 0 {
        None
    } else {
        Some(ram_per_session_mb.iter().sum())
    };

    let (launch_success, launch_fail, launch_p95_ms) = state.metrics.snapshot();
    Ok(MetricsSnapshot {
        live_sessions,
        ram_total_mb,
        ram_per_session_mb,
        launch_p95_ms,
        launch_success,
        launch_fail,
    })
}

// ---------------------------------------------------------------------------
// Fingerprint GPU (W52-D): gợi ý cặp vendor↔renderer CÓ THẬT theo platform +
// cảnh báo combo bất khả thi. Pure — không dùng state. Xem `fingerprint_gpu`.
// ---------------------------------------------------------------------------

/// Gợi ý cặp GPU nhất quán với `platform` ("windows"|"macos"|"linux"), chọn
/// weighted-deterministic theo `seed` (regenerate cùng seed → cùng kết quả).
/// None nếu platform không có entry trong pool.
#[tauri::command]
pub fn suggest_gpu(platform: String, seed: u64) -> Option<crate::fingerprint_gpu::GpuSuggestion> {
    crate::fingerprint_gpu::pick_gpu(&platform, seed).map(|e| {
        crate::fingerprint_gpu::GpuSuggestion {
            vendor: e.vendor.clone(),
            renderer: e.renderer.clone(),
        }
    })
}

/// Kiểm tra nhất quán platform ↔ GPU do user set thủ công. Trả cảnh báo (không
/// chặn) khi combo bất khả thi (vd macOS + Direct3D11, Windows + Metal); None
/// khi hợp lệ hoặc chưa set renderer (auto theo seed).
#[tauri::command]
pub fn check_gpu_consistency(
    platform: String,
    gpu_vendor: Option<String>,
    gpu_renderer: Option<String>,
) -> Option<String> {
    let renderer = gpu_renderer.unwrap_or_default();
    if renderer.trim().is_empty() {
        return None;
    }
    let vendor = gpu_vendor.unwrap_or_default();
    crate::fingerprint_gpu::gpu_platform_mismatch(&platform, &vendor, &renderer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Script AppleScript fallback phải nhúng đúng PID và target System Events.
    #[test]
    fn frontmost_script_embeds_pid() {
        let script = frontmost_script(4242);
        assert!(script.contains("unix id is 4242"));
        assert!(script.starts_with("tell application \"System Events\""));
        assert!(script.ends_with("frontmost of (first process whose unix id is 4242) to true"));
    }

    /// ProxyRecord test với credential blob tuỳ ý.
    fn proxy_record(
        username_enc: Option<Vec<u8>>,
        password_enc: Option<Vec<u8>>,
    ) -> db::ProxyRecord {
        db::ProxyRecord {
            id: "px-test".into(),
            name: "test proxy".into(),
            protocol: "http".into(),
            host: "127.0.0.1".into(),
            port: 8080,
            username_enc,
            password_enc,
            health_status: None,
            last_checked_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    /// Blob không giải mã được bằng master key hiện tại (mô phỏng seal bằng
    /// key cũ trước khi keychain đổi): flip byte cuối → auth tag fail.
    fn undecryptable_blob(plaintext: &str) -> Vec<u8> {
        let mut blob = crypto::encrypt_secret(plaintext).unwrap();
        *blob.last_mut().unwrap() ^= 0xff;
        blob
    }

    /// (W23b) Decrypt fail KHÔNG Err — degrade credentials_invalid=true,
    /// username rỗng, has_password giữ nguyên → list_proxies không sập.
    #[test]
    fn proxy_to_model_degrades_on_decrypt_failure() {
        crypto::install_test_master_key();
        let p = proxy_to_model(proxy_record(
            Some(undecryptable_blob("user")),
            Some(undecryptable_blob("pass")),
        ));
        assert!(p.credentials_invalid);
        assert_eq!(p.masked_username, None);
        assert!(p.has_password);
    }

    #[test]
    fn proxy_to_model_flags_password_only_failure() {
        crypto::install_test_master_key();
        let p = proxy_to_model(proxy_record(
            Some(crypto::encrypt_secret("user").unwrap()),
            Some(undecryptable_blob("pass")),
        ));
        assert!(p.credentials_invalid);
        assert_eq!(p.masked_username.as_deref(), Some("u***"));
    }

    #[test]
    fn proxy_to_model_valid_credentials_not_flagged() {
        crypto::install_test_master_key();
        let p = proxy_to_model(proxy_record(
            Some(crypto::encrypt_secret("user").unwrap()),
            Some(crypto::encrypt_secret("pass").unwrap()),
        ));
        assert!(!p.credentials_invalid);
        assert_eq!(p.masked_username.as_deref(), Some("u***"));
        assert!(p.has_password);
    }

    /// (W5c) Username KHÔNG bao giờ qua IPC ở dạng plaintext — chỉ bản đã che
    /// (ký tự đầu + "***", không lộ độ dài).
    #[test]
    fn proxy_to_model_never_returns_plaintext_username() {
        crypto::install_test_master_key();
        let p = proxy_to_model(proxy_record(
            Some(crypto::encrypt_secret("alice-secret-user").unwrap()),
            None,
        ));
        let masked = p.masked_username.as_deref().unwrap();
        assert_eq!(masked, "a***");
        assert!(!masked.contains("alice"));
        assert!(!p.has_password);
    }

    #[test]
    fn mask_username_hides_all_but_first_char() {
        assert_eq!(mask_username("alice"), "a***");
        assert_eq!(mask_username("x"), "x***");
        assert_eq!(mask_username("ơi-unicode"), "ơ***");
        assert_eq!(mask_username(""), "***");
    }

    /// (W23b) launch với proxy hỏng → lỗi rõ ràng yêu cầu nhập lại password.
    #[test]
    fn proxy_url_from_reports_clear_error_on_decrypt_failure() {
        crypto::install_test_master_key();
        let rec = proxy_record(
            Some(crypto::encrypt_secret("user").unwrap()),
            Some(undecryptable_blob("pass")),
        );
        let err = proxy_url_from(&rec).unwrap_err().to_string();
        assert!(err.contains("re-enter"));
        assert!(err.contains("test proxy"));
    }

    /// (W23b) Key-check blob: tạo lần đầu, khớp các lần sau; blob hỏng
    /// (key đổi) → true một lần rồi re-seal bằng key hiện tại.
    #[test]
    fn master_key_check_creates_then_detects_mismatch() {
        crypto::install_test_master_key();
        let db = Db::open_in_memory().unwrap();
        assert!(!master_key_check(&db).unwrap());
        assert!(!master_key_check(&db).unwrap());
        db.set_setting(MASTER_KEY_CHECK_SETTING, "AAAA").unwrap();
        assert!(master_key_check(&db).unwrap());
        assert!(!master_key_check(&db).unwrap());
    }

    // -- (W52-B) backup_now guard + upload state persistence ----------------

    /// Server HTTP/1.1 stub tối giản trên loopback (như telegram_sync tests):
    /// mọi request POST chứa `path` trả `(status, body)` cố định.
    async fn spawn_one_route_stub(path: &'static str, status: u16, body: String) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let (head_end, header) = loop {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break (pos + 4, String::from_utf8_lossy(&buf[..pos]).to_string());
                        }
                    };
                    let content_len = header
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    while buf.len() - head_end < content_len {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let request_line = header.lines().next().unwrap_or("");
                    let (st, bd) = if request_line.contains(path) {
                        (status, body.as_str())
                    } else {
                        (404, r#"{"ok":false,"description":"no route"}"#)
                    };
                    let resp = format!(
                        "HTTP/1.1 {st} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{bd}",
                        bd.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    /// Profile trong DB in-memory với user_data_dir tuỳ ý (kèm Default/ khi cần).
    fn profile_with_dir(db: &Db, dir: &Path, with_data: bool) -> String {
        if with_data {
            std::fs::create_dir_all(dir.join("Default")).unwrap();
            std::fs::write(dir.join("Default/Cookies"), b"cookie-bytes").unwrap();
        } else {
            std::fs::create_dir_all(dir).unwrap();
        }
        let p = db
            .create_profile(ProfileInput {
                name: "cloud-p".into(),
                user_data_dir: Some(dir.to_string_lossy().into_owned()),
                ..Default::default()
            })
            .unwrap();
        p.id
    }

    /// (W52-B C5) Guard: profile ĐANG chạy → refuse với lỗi rõ ràng, không
    /// đụng archive/upload; sau stop thì các guard tiếp theo mới chạy.
    #[tokio::test]
    async fn backup_now_refuses_running_profile() {
        crypto::install_test_master_key();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let procs = ProcessManager::new(2);
        let base = std::env::temp_dir().join(format!("bx-bn-test-{}", uuid::Uuid::new_v4()));
        let pid = profile_with_dir(&db, &base.join("run"), true);

        #[cfg(unix)]
        let (prog, args) = ("sleep", vec!["30".to_string()]);
        #[cfg(windows)]
        let (prog, args) = ("ping", vec!["-n".into(), "31".into(), "127.0.0.1".into()]);
        procs.spawn(&pid, prog, args, 1).await.unwrap();

        let err = backup_now_impl(None, &db, &procs, &pid, None)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert!(err.to_string().contains("đang chạy"));
        // Guard fail TRƯỚC mọi bước upload → không có trạng thái nào bị ghi.
        assert!(db.get_cloud_upload_state(&pid).unwrap().is_none());

        procs.stop(&pid).await.unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// (W52-B C5) Guard thứ tự: không dữ liệu → InvalidInput; có dữ liệu nhưng
    /// chưa cấu hình Telegram → InvalidInput nhắc cấu hình.
    #[tokio::test]
    async fn backup_now_requires_data_then_credentials() {
        crypto::install_test_master_key();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let procs = ProcessManager::new(1);
        let base = std::env::temp_dir().join(format!("bx-bn-test-{}", uuid::Uuid::new_v4()));

        let empty = profile_with_dir(&db, &base.join("empty"), false);
        let err = backup_now_impl(None, &db, &procs, &empty, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("chưa có dữ liệu"));

        let with_data = profile_with_dir(&db, &base.join("run"), true);
        let err = backup_now_impl(None, &db, &procs, &with_data, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("chưa cấu hình"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// (W52-B C1+C5) Full flow qua stub loopback: backup_now archive FORCED +
    /// upload OK → state "uploaded" + row cloud_backups; upload FAIL (4xx) →
    /// state "failed" giữ lỗi + retry_count tăng, và backup_now trả Err.
    #[tokio::test]
    async fn backup_now_uploads_and_persists_state_transitions() {
        crypto::install_test_master_key();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let procs = ProcessManager::new(1);
        let base = std::env::temp_dir().join(format!("bx-bn-test-{}", uuid::Uuid::new_v4()));
        let pid = profile_with_dir(&db, &base.join("run"), true);
        telegram_sync::save_credentials(&db, "T", "C").unwrap();

        // Stub OK: sendDocument thành công.
        let ok_body = serde_json::json!({
            "ok": true,
            "result": { "message_id": 11, "document": { "file_id": "F1" } }
        })
        .to_string();
        let ok_base = spawn_one_route_stub("/sendDocument", 200, ok_body).await;
        backup_now_impl(None, &db, &procs, &pid, Some(ok_base))
            .await
            .unwrap();
        let s = db.get_cloud_upload_state(&pid).unwrap().unwrap();
        assert_eq!(s.status, "uploaded");
        assert_eq!(s.retry_count, 0);
        assert!(s.last_error.is_none());
        assert_eq!(db.list_cloud_backups(Some(&pid)).unwrap().len(), 1);

        // Stub FAIL 400 → fail fast, lỗi persist + Err trả về caller.
        let bad_base = spawn_one_route_stub(
            "/sendDocument",
            400,
            r#"{"ok":false,"description":"Bad Request: chat not found"}"#.into(),
        )
        .await;
        let err = backup_now_impl(None, &db, &procs, &pid, Some(bad_base))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("chat not found"));
        let s = db.get_cloud_upload_state(&pid).unwrap().unwrap();
        assert_eq!(s.status, "failed");
        assert_eq!(s.retry_count, 1);
        assert!(s.last_error.as_deref().unwrap().contains("chat not found"));
        assert!(s.last_error_at.is_some());
        // Bản backup thành công trước đó vẫn còn nguyên.
        assert_eq!(db.list_cloud_backups(Some(&pid)).unwrap().len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }

    // -- (W55c) App DB cloud backup -------------------------------------------

    /// Full flow qua stub loopback: snapshot VACUUM INTO → mã hoá `.bxa` →
    /// upload OK dưới sentinel `__app_db__` (row cloud_backups + state
    /// "uploaded"); snapshot plaintext tạm bị xoá; `.bxa` giải mã lại đúng
    /// SQLite. Credentials thiếu → fail sớm TRƯỚC snapshot.
    #[tokio::test]
    async fn backup_app_db_uploads_via_stub_and_cleans_snapshot() {
        crypto::install_test_master_key();
        let base = std::env::temp_dir().join(format!("bx-appdb-cmd-{}", uuid::Uuid::new_v4()));
        let db = Arc::new(Db::open_at_dir(&base).unwrap());
        db.set_setting("some_key", "some_value").unwrap();
        telegram_sync::save_credentials(&db, "T", "C").unwrap();

        // Chưa cấu hình credentials → fail sớm TRƯỚC snapshot.
        telegram_sync::save_credentials(&db, "", "").unwrap();
        let err = backup_app_db_now_impl(None, &db, None).await.unwrap_err();
        assert!(err.to_string().contains("chưa cấu hình"));
        telegram_sync::save_credentials(&db, "T", "C").unwrap();

        let ok_body = serde_json::json!({
            "ok": true,
            "result": { "message_id": 21, "document": { "file_id": "F9" } }
        })
        .to_string();
        let ok_base = spawn_one_route_stub("/sendDocument", 200, ok_body).await;
        backup_app_db_now_impl(None, &db, Some(ok_base)).await.unwrap();

        let id = app_db_backup::APP_DB_PROFILE_ID;
        let s = db.get_cloud_upload_state(id).unwrap().unwrap();
        assert_eq!(s.status, "uploaded");
        assert_eq!(db.list_cloud_backups(Some(id)).unwrap().len(), 1);
        // Snapshot plaintext tạm đã bị dọn; .bxa local còn và giải mã lại được.
        assert!(!base.join("appdb-snapshot.db").exists());
        let bxa = app_db_backup::bxa_path(&base);
        assert!(bxa.is_file());
        let plain = archive::decrypt_file_to_bytes(&bxa).unwrap();
        assert_eq!(&plain[..16], b"SQLite format 3\0");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
