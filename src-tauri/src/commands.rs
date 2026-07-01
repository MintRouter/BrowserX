//! Tauri commands (invoke handlers) — theo Hợp đồng command trong spec:
//! - Profiles: list_profiles, get_profile, create_profile, update_profile, delete_profile, search_profiles
//! - Proxies: list_proxies, create_proxy, update_proxy, delete_proxy, assign_proxy
//! - Session: launch_profile, stop_profile, list_running
//! - Binary: ensure_binary (emit `binary://progress`)
//! - Settings/tags: get_settings, set_setting, list_tags, set_profile_tags
//!
//! Đăng ký vào `tauri::Builder` trong lib.rs. Tham số Rust snake_case
//! (Tauri v2 tự map camelCase JS → snake_case).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::db::{self, Db, ProfileInput, ProfileUpdate, TagInfo};
use crate::error::Result;
use crate::models::{Profile, Proxy, RunningSession};
use crate::process::ProcessManager;
use crate::{binary, cdp, crypto, launcher};

/// State toàn app — khởi tạo trong `tauri::Builder::setup` (lib.rs) rồi `.manage()`.
pub struct AppState {
    pub db: Arc<Db>,
    pub procs: ProcessManager,
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

/// Payload event `binary://progress`.
#[derive(Debug, Clone, Serialize)]
struct BinaryProgressEvent {
    phase: String,
    pct: u8,
}

fn emit_status(
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// ProxyRecord (credential mã hoá) → models::Proxy (credential plaintext cho FE).
fn proxy_to_model(rec: db::ProxyRecord) -> Result<Proxy> {
    Ok(Proxy {
        id: rec.id,
        name: rec.name,
        protocol: rec.protocol,
        host: rec.host,
        port: rec.port,
        username: rec
            .username_enc
            .as_deref()
            .map(crypto::decrypt_secret)
            .transpose()?,
        password: rec
            .password_enc
            .as_deref()
            .map(crypto::decrypt_secret)
            .transpose()?,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    })
}

/// Dựng proxy URL đã giải mã credential: `protocol://[user[:pass]@]host:port`.
fn proxy_url_from(rec: &db::ProxyRecord) -> Result<String> {
    let auth = match (&rec.username_enc, &rec.password_enc) {
        (Some(u), Some(p)) => format!(
            "{}:{}@",
            crypto::decrypt_secret(u)?,
            crypto::decrypt_secret(p)?
        ),
        (Some(u), None) => format!("{}@", crypto::decrypt_secret(u)?),
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
    state
        .db
        .list_proxies()?
        .into_iter()
        .map(proxy_to_model)
        .collect()
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
    proxy_to_model(rec)
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
    proxy_to_model(rec)
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
    let progress = move |phase: &str, pct: u8| {
        let _ = progress_app.emit(
            "binary://progress",
            BinaryProgressEvent {
                phase: phase.to_string(),
                pct,
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
        let _ = state.procs.stop(&profile_id).await;
        emit_status(&app, &profile_id, "error", None, None);
        return Err(e);
    }

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
    Ok(())
}

#[tauri::command]
pub async fn list_running(state: State<'_, AppState>) -> Result<Vec<RunningSession>> {
    Ok(state.procs.list_running().await)
}

// ---------------------------------------------------------------------------
// Binary
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ensure_binary(app: AppHandle, version: Option<String>) -> Result<String> {
    let progress_app = app.clone();
    let progress = move |phase: &str, pct: u8| {
        let _ = progress_app.emit(
            "binary://progress",
            BinaryProgressEvent {
                phase: phase.to_string(),
                pct,
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
