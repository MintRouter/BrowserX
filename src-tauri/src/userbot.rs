//! (W55b-core) Userbot MTProto client (tdlib-rs) — transport thứ 2 TUỲ CHỌN cho
//! Cloud Sync (Bot API vẫn mặc định). Bọc TDLib thành client singleton có auth
//! state machine, expose qua 6 commands + event `userbot-auth-state` (commands.rs).
//!
//! Bảo mật (theo phản biện spec W55b):
//! - api_hash lưu MÃ HOÁ XChaCha20-Poly1305 (crypto.rs, như bot token); api_id lưu thường.
//! - Session TDLib tại `<data_dir>/userbot/td_db`, DB mã hoá bằng key derive
//!   (SHA-256 domain-separated) từ master key keychain.
//! - KHÔNG log api_hash/phone/code/password/nội dung session. Logout xoá session dir.
//!
//! Update-loop tái dùng pattern POC `bin/tdlib_poc.rs`: spawn_blocking(receive)
//! → route Update::AuthorizationState về state machine, MessageSendSucceeded/
//! Failed về broadcast channel cho upload (wave W55b-transport).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tdlib_rs::enums::{
    AuthorizationState, Chat, Chats, InputFile, InputMessageContent, MessageContent, Update,
};
use tdlib_rs::{functions, types};
use tokio::sync::broadcast;

use crate::crypto;
use crate::db::Db;
use crate::error::{AppError, Result};

/// Settings key: api_id (my.telegram.org) — số, không nhạy cảm bằng hash, lưu thường.
pub const API_ID_SETTING: &str = "userbot_api_id";
/// Settings key: api_hash đã mã hoá (base64 của blob crypto::encrypt_secret).
pub const API_HASH_SETTING: &str = "userbot_api_hash_enc";
/// Tên event Tauri phát mỗi lần auth state đổi (payload = [`UserbotStatus`]).
pub const AUTH_STATE_EVENT: &str = "userbot-auth-state";
/// Tên private channel chứa backup cloud qua userbot (wave transport).
const SYNC_CHANNEL_TITLE: &str = "BrowserX Cloud Sync";
/// Domain separation cho key mã hoá TDLib DB (derive từ master key).
const DB_KEY_DOMAIN: &[u8] = b"browserx-userbot-tdlib-v1";

// ---------------------------------------------------------------------------
// Status (API surface CHỐT CỨNG trong spec)
// ---------------------------------------------------------------------------

/// Payload `userbot_get_status` + event `userbot-auth-state`:
/// `{ state, phoneHint?, username? }` (camelCase cho FE).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserbotStatus {
    /// "no_credentials"|"disconnected"|"waiting_phone"|"waiting_code"|"waiting_password"|"ready"
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

impl UserbotStatus {
    fn new(state: &str) -> Self {
        Self { state: state.into(), phone_hint: None, username: None }
    }
}

/// Map TDLib authorizationState → (state string spec, phone hint nếu có).
/// WaitTdlibParameters/Closing/Closed và state không hỗ trợ → "disconnected"
/// (side-effect như set_tdlib_parameters/wipe do update-loop xử lý riêng).
fn map_auth_state(s: &AuthorizationState) -> (&'static str, Option<String>) {
    match s {
        AuthorizationState::WaitPhoneNumber => ("waiting_phone", None),
        AuthorizationState::WaitCode(c) => {
            ("waiting_code", Some(mask_phone(&c.code_info.phone_number)))
        }
        AuthorizationState::WaitPassword(_) => ("waiting_password", None),
        AuthorizationState::Ready => ("ready", None),
        _ => ("disconnected", None),
    }
}

/// Che số điện thoại cho hint hiển thị FE: 4 ký tự đầu + "•••" + 2 cuối
/// (số ngắn → chỉ 2 đầu + "•••"). KHÔNG bao giờ trả nguyên số.
fn mask_phone(phone: &str) -> String {
    let chars: Vec<char> = phone.chars().collect();
    if chars.len() <= 6 {
        let head: String = chars.iter().take(2).collect();
        return format!("{head}•••");
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}•••{tail}")
}

// ---------------------------------------------------------------------------
// Lỗi typed cho wave transport (flood-wait phải phân biệt được)
// ---------------------------------------------------------------------------

/// Lỗi userbot — wave transport cần bắt riêng [`UserbotError::FloodWait`] để
/// lưu `flood_wait_until` và skip upload đến hạn.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum UserbotError {
    /// Telegram yêu cầu chờ N giây (429/FLOOD_WAIT) — KHÔNG retry ngay.
    #[error("telegram flood wait: retry after {0}s")]
    FloodWait(u64),
    /// Client chưa ở trạng thái ready (chưa login/đang auth dở).
    #[error("userbot not ready (state: {0})")]
    NotReady(String),
    /// Lỗi Telegram khác (message TDLib không chứa secrets).
    #[error("telegram error {code}: {message}")]
    Telegram { code: i32, message: String },
}

impl From<UserbotError> for AppError {
    fn from(e: UserbotError) -> Self {
        AppError::Other(anyhow::anyhow!(e))
    }
}

/// Nếu error là FLOOD_WAIT (code 429 / "Too Many Requests: retry after N" /
/// "FLOOD_WAIT_N") → Some(N giây). Pattern từ POC W55a.
fn flood_wait_secs(error: &types::Error) -> Option<u64> {
    let msg = &error.message;
    if error.code == 429 || msg.contains("Too Many Requests") || msg.contains("FLOOD_WAIT") {
        let digits: String = msg
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return digits.parse::<u64>().ok().or(Some(0));
    }
    None
}

/// types::Error TDLib → UserbotError (flood-wait tách riêng thành variant typed).
fn map_td_error(e: types::Error) -> UserbotError {
    match flood_wait_secs(&e) {
        Some(secs) => UserbotError::FloodWait(secs),
        None => UserbotError::Telegram { code: e.code, message: e.message },
    }
}

// ---------------------------------------------------------------------------
// Credentials (api_id thường + api_hash mã hoá trong settings)
// ---------------------------------------------------------------------------

/// Lưu api_id + api_hash (hash mã hoá như bot token). `api_hash` rỗng → xoá cả 2.
pub fn save_credentials(db: &Db, api_id: i32, api_hash: &str) -> Result<()> {
    if api_hash.is_empty() {
        db.set_setting(API_ID_SETTING, "")?;
        db.set_setting(API_HASH_SETTING, "")?;
        return Ok(());
    }
    if api_id <= 0 {
        return Err(AppError::InvalidInput(
            "api_id không hợp lệ — lấy từ my.telegram.org → API development tools".into(),
        ));
    }
    db.set_setting(API_ID_SETTING, &api_id.to_string())?;
    db.set_setting(
        API_HASH_SETTING,
        &B64.encode(crypto::encrypt_secret(api_hash)?),
    )?;
    Ok(())
}

/// Đọc + giải mã credentials. `None` khi chưa cấu hình đủ cả 2.
pub fn load_credentials(db: &Db) -> Result<Option<(i32, String)>> {
    let id = match db.get_setting(API_ID_SETTING)? {
        Some(v) if !v.is_empty() => v.parse::<i32>().ok(),
        _ => None,
    };
    let hash = match db.get_setting(API_HASH_SETTING)? {
        Some(b64) if !b64.is_empty() => {
            let blob = B64
                .decode(&b64)
                .map_err(|e| AppError::Crypto(format!("bad credential encoding: {e}")))?;
            Some(crypto::decrypt_secret(&blob)?)
        }
        _ => None,
    };
    Ok(match (id, hash) {
        (Some(i), Some(h)) => Some((i, h)),
        _ => None,
    })
}

/// Key mã hoá TDLib DB: SHA-256(domain ‖ master key) → base64 (TDLib nhận
/// bytes dạng base64 qua JSON interface). KHÔNG dùng master key trực tiếp.
fn derive_db_key() -> Result<String> {
    let mk = crypto::master_key_material()?;
    let mut h = Sha256::new();
    h.update(DB_KEY_DOMAIN);
    h.update(mk);
    Ok(B64.encode(h.finalize()))
}

/// Session dir TDLib: `<data_dir>/userbot` (chứa `td_db` — auth key MTProto
/// bên trong = TOÀN QUYỀN tài khoản; DB đã mã hoá bằng [`derive_db_key`]).
fn session_dir(db: &Db) -> PathBuf {
    db.data_dir().join("userbot")
}

/// Xoá session dir (logout). Best-effort — dir chưa tồn tại thì bỏ qua.
fn wipe_session_dir(dir: &Path) {
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(dir) {
            tracing::warn!("userbot: không xoá được session dir: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Client singleton + update loop
// ---------------------------------------------------------------------------

/// Runtime của client đang chạy (None = chưa init / đã Closed).
/// run_flag sống trong update-loop task (loop tự dừng khi Closed).
struct Runtime {
    client_id: i32,
    /// true = logout chủ đích → khi Closed thì xoá session dir.
    wipe_on_close: Arc<AtomicBool>,
}

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
/// Auth state gần nhất TDLib báo (mặc định "disconnected" khi chưa init).
static STATUS: Mutex<Option<UserbotStatus>> = Mutex::new(None);

/// Kết quả gửi message async (updateMessageSendSucceeded/Failed) — broadcast
/// cho [`upload_file`] chờ đúng old_message_id của mình.
#[derive(Debug, Clone)]
enum SendOutcome {
    Succeeded { old_message_id: i64, message: Box<types::Message> },
    Failed { old_message_id: i64, error: types::Error },
}

fn send_results() -> &'static broadcast::Sender<SendOutcome> {
    static CH: std::sync::OnceLock<broadcast::Sender<SendOutcome>> = std::sync::OnceLock::new();
    CH.get_or_init(|| broadcast::channel(64).0)
}

/// client_id của runtime đang chạy; Err(NotReady) nếu chưa init.
fn active_client_id() -> std::result::Result<i32, UserbotError> {
    RUNTIME
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|r| r.client_id))
        .ok_or_else(|| UserbotError::NotReady("disconnected".into()))
}

/// Status hiện tại: chưa có credentials → "no_credentials"; chưa init/chưa có
/// update nào → "disconnected"; còn lại = state TDLib báo gần nhất.
pub fn current_status(db: &Db) -> Result<UserbotStatus> {
    if load_credentials(db)?.is_none() {
        return Ok(UserbotStatus::new("no_credentials"));
    }
    let snapshot = STATUS.lock().ok().and_then(|g| g.clone());
    Ok(snapshot.unwrap_or_else(|| UserbotStatus::new("disconnected")))
}

/// Cập nhật STATUS + emit event [`AUTH_STATE_EVENT`] nếu đổi.
fn set_status(app: &AppHandle, status: UserbotStatus) {
    let changed = match STATUS.lock() {
        Ok(mut g) => {
            let changed = g.as_ref() != Some(&status);
            *g = Some(status.clone());
            changed
        }
        Err(_) => true,
    };
    if changed {
        let _ = app.emit(AUTH_STATE_EVENT, status);
    }
}

/// Init lazy client nếu chưa chạy và đã có credentials. Idempotent — gọi từ
/// mọi command userbot. Trả `true` nếu client đang (hoặc vừa được) chạy.
pub fn ensure_client(app: &AppHandle, db: &Arc<Db>) -> Result<bool> {
    if load_credentials(db)?.is_none() {
        return Ok(false);
    }
    let mut guard = RUNTIME
        .lock()
        .map_err(|_| AppError::Crypto("userbot runtime lock poisoned".into()))?;
    if guard.is_some() {
        return Ok(true);
    }
    let client_id = tdlib_rs::create_client();
    let run_flag = Arc::new(AtomicBool::new(true));
    let wipe_on_close = Arc::new(AtomicBool::new(false));
    *guard = Some(Runtime {
        client_id,
        wipe_on_close: wipe_on_close.clone(),
    });
    drop(guard);

    let app = app.clone();
    let db = db.clone();
    tauri::async_runtime::spawn(async move {
        // Request đầu tiên để TDLib bắt đầu phát update cho client này.
        let _ = functions::set_log_verbosity_level(1, client_id).await;
        run_update_loop(app, db, client_id, run_flag, wipe_on_close).await;
    });
    Ok(true)
}

/// Receive-loop (pattern POC): spawn_blocking(receive) — receive() đồng thời
/// resolve future của mọi `functions::*` call, nên loop PHẢI sống suốt
/// vòng đời client. Thoát khi run_flag=false (sau authorizationStateClosed).
async fn run_update_loop(
    app: AppHandle,
    db: Arc<Db>,
    client_id: i32,
    run_flag: Arc<AtomicBool>,
    wipe_on_close: Arc<AtomicBool>,
) {
    while run_flag.load(Ordering::Acquire) {
        let received = tokio::task::spawn_blocking(tdlib_rs::receive)
            .await
            .unwrap_or(None);
        let Some((update, cid)) = received else { continue };
        if cid != client_id {
            continue;
        }
        match update {
            Update::AuthorizationState(u) => {
                handle_auth_state(
                    &app,
                    &db,
                    client_id,
                    &run_flag,
                    &wipe_on_close,
                    u.authorization_state,
                )
                .await;
            }
            Update::MessageSendSucceeded(u) => {
                let _ = send_results().send(SendOutcome::Succeeded {
                    old_message_id: u.old_message_id,
                    message: Box::new(u.message),
                });
            }
            Update::MessageSendFailed(u) => {
                let _ = send_results().send(SendOutcome::Failed {
                    old_message_id: u.old_message_id,
                    error: u.error,
                });
            }
            _ => {}
        }
    }
}

/// State machine auth: WaitTdlibParameters → set_tdlib_parameters (side-effect
/// duy nhất loop tự làm); phone/code/password do user submit qua commands;
/// Ready → gắn username; Closed → dọn runtime (+ xoá session nếu logout).
async fn handle_auth_state(
    app: &AppHandle,
    db: &Arc<Db>,
    client_id: i32,
    run_flag: &Arc<AtomicBool>,
    wipe_on_close: &Arc<AtomicBool>,
    state: AuthorizationState,
) {
    match state {
        AuthorizationState::WaitTdlibParameters => {
            if let Err(msg) = send_tdlib_parameters(db, client_id).await {
                tracing::warn!("userbot: set_tdlib_parameters lỗi: {msg}");
                set_status(app, UserbotStatus::new("disconnected"));
            }
        }
        AuthorizationState::Ready => {
            let username = fetch_username(client_id).await;
            let mut status = UserbotStatus::new("ready");
            status.username = username;
            set_status(app, status);
        }
        AuthorizationState::Closed => {
            run_flag.store(false, Ordering::Release);
            if wipe_on_close.load(Ordering::Acquire) {
                wipe_session_dir(&session_dir(db));
            }
            if let Ok(mut g) = RUNTIME.lock() {
                *g = None;
            }
            let state = match load_credentials(db) {
                Ok(Some(_)) => "disconnected",
                _ => "no_credentials",
            };
            set_status(app, UserbotStatus::new(state));
        }
        other => {
            let (state, phone_hint) = map_auth_state(&other);
            let mut status = UserbotStatus::new(state);
            status.phone_hint = phone_hint;
            set_status(app, status);
        }
    }
}

/// Gọi set_tdlib_parameters với credentials đã lưu + DB key derive từ master
/// key. Trả Err(String) KHÔNG chứa api_hash/key.
async fn send_tdlib_parameters(db: &Arc<Db>, client_id: i32) -> std::result::Result<(), String> {
    let (api_id, api_hash) = load_credentials(db)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "credentials missing".to_string())?;
    let db_key = derive_db_key().map_err(|e| e.to_string())?;
    let dir = session_dir(db);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    functions::set_tdlib_parameters(
        false,
        dir.join("td_db").to_string_lossy().into_owned(),
        String::new(),
        db_key,
        true,
        true,
        true,
        false,
        api_id,
        api_hash,
        "en".into(),
        "BrowserX".into(),
        String::new(),
        env!("CARGO_PKG_VERSION").into(),
        client_id,
    )
    .await
    .map_err(|e| format!("telegram error {}: {}", e.code, e.message))
}

/// Username hiển thị sau login (get_me) — None nếu tài khoản không đặt username.
async fn fetch_username(client_id: i32) -> Option<String> {
    let tdlib_rs::enums::User::User(me) = functions::get_me(client_id).await.ok()?;
    let usernames = me.usernames?;
    usernames
        .active_usernames
        .into_iter()
        .next()
        .or(Some(usernames.editable_username))
        .filter(|u| !u.is_empty())
}

// ---------------------------------------------------------------------------
// Auth actions (gọi từ commands.rs)
// ---------------------------------------------------------------------------

/// Submit số điện thoại (state waiting_phone).
pub async fn send_phone(phone: &str) -> Result<()> {
    let client_id = active_client_id()?;
    functions::set_authentication_phone_number(phone.to_string(), None, client_id)
        .await
        .map_err(|e| map_td_error(e).into())
}

/// Submit OTP (state waiting_code).
pub async fn submit_code(code: &str) -> Result<()> {
    let client_id = active_client_id()?;
    functions::check_authentication_code(code.to_string(), client_id)
        .await
        .map_err(|e| map_td_error(e).into())
}

/// Submit mật khẩu 2FA (state waiting_password).
pub async fn submit_password(password: &str) -> Result<()> {
    let client_id = active_client_id()?;
    functions::check_authentication_password(password.to_string(), client_id)
        .await
        .map_err(|e| map_td_error(e).into())
}

/// Logout: revoke session phía Telegram (log_out → LoggingOut → Closing →
/// Closed); update loop xoá session dir khi Closed nhờ cờ wipe_on_close.
pub async fn logout() -> Result<()> {
    let client_id = {
        let guard = RUNTIME
            .lock()
            .map_err(|_| AppError::Crypto("userbot runtime lock poisoned".into()))?;
        let rt = guard
            .as_ref()
            .ok_or(UserbotError::NotReady("disconnected".into()))?;
        rt.wipe_on_close.store(true, Ordering::Release);
        rt.client_id
    };
    functions::log_out(client_id)
        .await
        .map_err(|e| map_td_error(e).into())
}

// ---------------------------------------------------------------------------
// Transport helpers cho wave W55b-transport (pub trong crate, chưa nối UI)
// ---------------------------------------------------------------------------

/// Client đã ready chưa — helper cho transport guard trạng thái trước khi gọi.
fn require_ready() -> std::result::Result<i32, UserbotError> {
    let client_id = active_client_id()?;
    let state = STATUS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.state.clone()))
        .unwrap_or_else(|| "disconnected".into());
    if state != "ready" {
        return Err(UserbotError::NotReady(state));
    }
    Ok(client_id)
}

/// Tìm (hoặc tạo) private channel "BrowserX Cloud Sync" → chat_id. Idempotent:
/// load chat list rồi search theo title; chưa có → create supergroup channel.
pub async fn ensure_sync_channel() -> Result<i64> {
    let client_id = require_ready()?;
    for _ in 0..20 {
        match functions::load_chats(None, 100, client_id).await {
            Ok(_) => continue,
            Err(_) => break, // 404 = đã load hết chat list
        }
    }
    if let Ok(Chats::Chats(chats)) =
        functions::search_chats(SYNC_CHANNEL_TITLE.into(), 20, client_id).await
    {
        for chat_id in chats.chat_ids {
            if let Ok(Chat::Chat(chat)) = functions::get_chat(chat_id, client_id).await {
                if chat.title == SYNC_CHANNEL_TITLE {
                    return Ok(chat.id);
                }
            }
        }
    }
    let chat = functions::create_new_supergroup_chat(
        SYNC_CHANNEL_TITLE.into(),
        false,
        true, // is_channel: private channel, không public username
        "BrowserX cloud backup storage".into(),
        None,
        0,
        false,
        client_id,
    )
    .await
    .map_err(map_td_error)?;
    let Chat::Chat(chat) = chat;
    Ok(chat.id)
}

/// Upload 1 file NGUYÊN VẸN (không split) làm document vào `chat_id`, chờ
/// updateMessageSendSucceeded → trả message_id thật. Flood-wait → typed error.
pub async fn upload_file(chat_id: i64, path: &Path, caption: &str) -> Result<i64> {
    let client_id = require_ready()?;
    // Subscribe TRƯỚC khi gửi để không lỡ update succeeded/failed.
    let mut rx = send_results().subscribe();
    let content = InputMessageContent::InputMessageDocument(types::InputMessageDocument {
        document: InputFile::Local(types::InputFileLocal {
            path: path.to_string_lossy().into_owned(),
        }),
        thumbnail: None,
        disable_content_type_detection: true,
        caption: Some(types::FormattedText { text: caption.to_string(), entities: vec![] }),
    });
    let sent = functions::send_message(chat_id, None, None, None, content, client_id)
        .await
        .map_err(map_td_error)?;
    let tdlib_rs::enums::Message::Message(sent) = sent;
    let temp_id = sent.id;
    loop {
        match rx.recv().await {
            Ok(SendOutcome::Succeeded { old_message_id, message }) if old_message_id == temp_id => {
                return Ok(message.id);
            }
            Ok(SendOutcome::Failed { old_message_id, error }) if old_message_id == temp_id => {
                return Err(map_td_error(error).into());
            }
            Ok(_) => continue,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                return Err(AppError::Other(anyhow::anyhow!(
                    "userbot update loop closed while waiting for upload result"
                )));
            }
        }
    }
}

/// Download document từ message → path file local TDLib đã tải về (synchronous
/// download, ưu tiên cao). Caller copy/move file trước khi dọn cache TDLib.
pub async fn download_file(chat_id: i64, message_id: i64) -> Result<PathBuf> {
    let client_id = require_ready()?;
    let msg = functions::get_message(chat_id, message_id, client_id)
        .await
        .map_err(map_td_error)?;
    let tdlib_rs::enums::Message::Message(msg) = msg;
    let file_id = match &msg.content {
        MessageContent::MessageDocument(doc) => doc.document.document.id,
        _ => {
            return Err(AppError::InvalidInput(
                "cloud message không phải document — backup hỏng hoặc sai message_id".into(),
            ))
        }
    };
    let file = functions::download_file(file_id, 32, 0, 0, true, client_id)
        .await
        .map_err(map_td_error)?;
    let tdlib_rs::enums::File::File(file) = file;
    Ok(PathBuf::from(file.local.path))
}

/// Xoá message backup trên channel (revoke cho mọi member).
pub async fn delete_message(chat_id: i64, message_id: i64) -> Result<()> {
    let client_id = require_ready()?;
    functions::delete_messages(chat_id, vec![message_id], true, client_id)
        .await
        .map_err(|e| map_td_error(e).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- State mapping ------------------------------------------------------

    #[test]
    fn map_auth_state_covers_spec_states() {
        assert_eq!(map_auth_state(&AuthorizationState::WaitPhoneNumber).0, "waiting_phone");
        assert_eq!(map_auth_state(&AuthorizationState::WaitPassword(Default::default())).0, "waiting_password");
        assert_eq!(map_auth_state(&AuthorizationState::Ready).0, "ready");
        assert_eq!(map_auth_state(&AuthorizationState::WaitTdlibParameters).0, "disconnected");
        assert_eq!(map_auth_state(&AuthorizationState::LoggingOut).0, "disconnected");
        assert_eq!(map_auth_state(&AuthorizationState::Closing).0, "disconnected");
        assert_eq!(map_auth_state(&AuthorizationState::Closed).0, "disconnected");
    }

    #[test]
    fn map_auth_state_wait_code_masks_phone() {
        let code_info = types::AuthenticationCodeInfo {
            phone_number: "+84901234567".into(),
            r#type: tdlib_rs::enums::AuthenticationCodeType::Sms(
                types::AuthenticationCodeTypeSms { length: 5 },
            ),
            next_type: None,
            timeout: 60,
        };
        let state =
            AuthorizationState::WaitCode(types::AuthorizationStateWaitCode { code_info });
        let (name, hint) = map_auth_state(&state);
        assert_eq!(name, "waiting_code");
        let hint = hint.unwrap();
        assert_eq!(hint, "+849•••67");
        assert!(!hint.contains("0123456"), "hint không được lộ giữa số: {hint}");
    }

    #[test]
    fn mask_phone_short_and_empty() {
        assert_eq!(mask_phone("+8490"), "+8•••");
        assert_eq!(mask_phone(""), "•••");
    }

    #[test]
    fn status_serializes_camel_case_and_skips_none() {
        let s = serde_json::to_string(&UserbotStatus::new("no_credentials")).unwrap();
        assert_eq!(s, r#"{"state":"no_credentials"}"#);
        let mut full = UserbotStatus::new("waiting_code");
        full.phone_hint = Some("+849•••67".into());
        full.username = None;
        let s = serde_json::to_string(&full).unwrap();
        assert!(s.contains(r#""phoneHint":"#), "{s}");
        assert!(!s.contains("username"), "{s}");
    }

    // -- Flood-wait parse ----------------------------------------------------

    #[test]
    fn flood_wait_parsing() {
        let e = types::Error { code: 429, message: "Too Many Requests: retry after 17".into() };
        assert_eq!(flood_wait_secs(&e), Some(17));
        let e2 = types::Error { code: 420, message: "FLOOD_WAIT_33".into() };
        assert_eq!(flood_wait_secs(&e2), Some(33));
        let e3 = types::Error { code: 400, message: "PHONE_NUMBER_INVALID".into() };
        assert_eq!(flood_wait_secs(&e3), None);
    }

    #[test]
    fn map_td_error_produces_typed_flood_wait() {
        let e = types::Error { code: 429, message: "Too Many Requests: retry after 5".into() };
        assert_eq!(map_td_error(e), UserbotError::FloodWait(5));
        let e = types::Error { code: 400, message: "CHAT_NOT_FOUND".into() };
        assert_eq!(
            map_td_error(e),
            UserbotError::Telegram { code: 400, message: "CHAT_NOT_FOUND".into() }
        );
    }

    // -- Credentials at rest --------------------------------------------------

    #[test]
    fn credentials_roundtrip_encrypted_at_rest() {
        crate::crypto::install_test_master_key();
        let db = Db::open_in_memory().unwrap();
        assert!(load_credentials(&db).unwrap().is_none());

        save_credentials(&db, 12345, "abcdef0123456789").unwrap();
        let (id, hash) = load_credentials(&db).unwrap().unwrap();
        assert_eq!(id, 12345);
        assert_eq!(hash, "abcdef0123456789");
        // Ở REST không có plaintext hash trong settings.
        let raw = db.get_setting(API_HASH_SETTING).unwrap().unwrap();
        assert!(!raw.contains("abcdef0123456789"));

        // api_hash rỗng → xoá credentials.
        save_credentials(&db, 0, "").unwrap();
        assert!(load_credentials(&db).unwrap().is_none());
    }

    #[test]
    fn save_credentials_rejects_bad_api_id() {
        crate::crypto::install_test_master_key();
        let db = Db::open_in_memory().unwrap();
        assert!(save_credentials(&db, 0, "somehash").is_err());
        assert!(save_credentials(&db, -1, "somehash").is_err());
        assert!(load_credentials(&db).unwrap().is_none());
    }

    // -- DB key derive ---------------------------------------------------------

    #[test]
    fn derived_db_key_is_stable_and_not_master_key() {
        crate::crypto::install_test_master_key();
        let k1 = derive_db_key().unwrap();
        let k2 = derive_db_key().unwrap();
        assert_eq!(k1, k2, "derive phải deterministic");
        let mk_b64 = B64.encode(crypto::master_key_material().unwrap());
        assert_ne!(k1, mk_b64, "KHÔNG dùng master key trực tiếp làm DB key");
        assert_eq!(B64.decode(&k1).unwrap().len(), 32);
    }
}
