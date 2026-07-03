//! Tauri commands (invoke handlers) — theo Hợp đồng command trong spec:
//! - Profiles: list_profiles, get_profile, create_profile, update_profile, delete_profile, search_profiles
//! - Proxies: list_proxies, create_proxy, update_proxy, delete_proxy, assign_proxy, check_proxy
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

use crate::db::{self, Db, ProfileInput, ProfileUpdate, TagInfo};
use crate::error::{AppError, Result};
use crate::models::{Folder, Profile, ProfileTemplate, Proxy, RunningSession};
use crate::process::ProcessManager;
use crate::proxy_check::{self, ProxyCheckResult};
use crate::{binary, cdp, cookies, crypto, launcher, storage};

/// State toàn app — khởi tạo trong `tauri::Builder::setup` (lib.rs) rồi `.manage()`.
pub struct AppState {
    pub db: Arc<Db>,
    pub procs: ProcessManager,
    /// (W23a) true khi `stop_all_and_quit` đang thoát app — để ExitRequested
    /// trong lib.rs không chặn lần thoát này nữa.
    pub quitting: AtomicBool,
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
/// Password KHÔNG giải mã — chỉ trả `has_password`; giải mã duy nhất lúc launch
/// (`proxy_url_from`). Decrypt fail (master key đổi) KHÔNG trả Err — degrade:
/// `credentials_invalid = true`, username rỗng, để `list_proxies` không bao giờ
/// sập vì 1 credential hỏng; FE hiện banner yêu cầu nhập lại password.
fn proxy_to_model(rec: db::ProxyRecord) -> Proxy {
    let mut credentials_invalid = false;
    let username = match rec.username_enc.as_deref().map(crypto::decrypt_secret) {
        Some(Ok(u)) => Some(u),
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
        username,
        has_password: rec.password_enc.is_some(),
        credentials_invalid,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
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

#[tauri::command]
pub fn search_profiles(
    state: State<'_, AppState>,
    query: String,
    tag: Option<String>,
) -> Result<Vec<Profile>> {
    state.db.search_profiles(&query, tag.as_deref())
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

#[tauri::command]
pub async fn launch_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<RunningSession> {
    let profile = state.db.get_profile(&profile_id)?;

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
    let args = launcher::build_args(&profile, proxy_url.as_deref(), cdp_port);
    let program = binary_path.to_string_lossy().into_owned();

    let session = state
        .procs
        .spawn(&profile_id, &program, args, cdp_port)
        .await?;

    if let Err(e) = cdp::attach(cdp_port).await {
        tracing::error!("launch_profile {profile_id}: CDP attach failed: {e}");
        let _ = state.procs.stop(&profile_id).await;
        emit_status(&app, &profile_id, "error", None, None);
        return Err(e);
    }

    state.db.touch_last_start(&profile_id)?;
    state.db.insert_audit(
        "profile.launch",
        Some(&profile_id),
        Some(&json!({ "pid": session.pid, "cdp_port": cdp_port })),
    )?;
    emit_status(
        &app,
        &profile_id,
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
    let _ = auto_clear_cache_if_enabled(&state.db, &profile_id);
    let _ = apply_storage_options_on_stop(&state.db, &profile_id);
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
    let mut args = launcher::build_args(&p, None, cdp_port);
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
        assert_eq!(p.username, None);
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
        assert_eq!(p.username.as_deref(), Some("user"));
    }

    #[test]
    fn proxy_to_model_valid_credentials_not_flagged() {
        crypto::install_test_master_key();
        let p = proxy_to_model(proxy_record(
            Some(crypto::encrypt_secret("user").unwrap()),
            Some(crypto::encrypt_secret("pass").unwrap()),
        ));
        assert!(!p.credentials_invalid);
        assert_eq!(p.username.as_deref(), Some("user"));
        assert!(p.has_password);
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
}
