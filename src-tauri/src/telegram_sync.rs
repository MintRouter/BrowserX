//! (W51-B2) Telegram cloud sync cho archive `.bxa` (W51-B1).
//!
//! Upload archive lên 1 chat Telegram qua Bot API (`sendDocument`) làm cloud
//! backup. Bot Token + Chat ID lưu MÃ HOÁ (crypto.rs — cùng hạ tầng proxy
//! credential) dạng base64 trong bảng settings. File >[`PART_SIZE`] được split
//! part `.bxa.001`, `.bxa.002`… (bot limit 50MB/file); mỗi part là 1 message
//! riêng, metadata ghi bảng `cloud_backups` (db.rs — 1 row/part, sha256 của
//! TOÀN file trước split). Retention cloud [`RETENTION`] bản/profile — bản cũ
//! xoá qua `deleteMessage` rồi xoá row.
//!
//! Retry: tối đa [`MAX_ATTEMPTS`] lần/request, tôn trọng 429 `retry_after`
//! (fallback backoff 1s→2s→4s). [`acquire_upload_slot`] giới hạn 1 upload/lần
//! (tránh flood Bot API). Restore: `getFile` → tải từng part → ghép → verify
//! sha256 → ghi `.bxa` để archive.rs (W51-B1) giải mã/giải nén.
//!
//! Test KHÔNG gọi mạng thật: `api_base` tham số hoá, test trỏ vào HTTP server
//! loopback local (xem `tests`).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::crypto;
use crate::db::{CloudBackupPart, CloudBackupPartInput, Db};
use crate::error::{AppError, Result};

/// Settings key: bật/tắt auto-upload sau archive ("true"/"false").
pub const ENABLED_SETTING: &str = "telegram_sync_enabled";
/// Settings key: Bot Token đã mã hoá (base64 của blob crypto::encrypt_secret).
pub const BOT_TOKEN_SETTING: &str = "telegram_bot_token_enc";
/// Settings key: Chat ID đã mã hoá (base64 của blob crypto::encrypt_secret).
pub const CHAT_ID_SETTING: &str = "telegram_chat_id_enc";

/// Kích thước tối đa 1 part (bot limit 50MB — chừa headroom multipart).
pub const PART_SIZE: usize = 48 * 1024 * 1024;
/// Số bản backup cloud giữ lại mỗi profile.
pub const RETENTION: usize = 3;
/// Số lần thử tối đa cho 1 request Bot API.
const MAX_ATTEMPTS: u32 = 4;
/// Base URL Bot API production.
const API_BASE: &str = "https://api.telegram.org";
/// Timeout 1 request upload/download (part tối đa 48MB).
const HTTP_TIMEOUT: Duration = Duration::from_secs(300);

/// Chỉ 1 upload chạy tại một thời điểm (tránh flood Bot API).
static UPLOAD_SLOT: Semaphore = Semaphore::const_new(1);

/// Chờ slot upload — giữ permit đến khi upload xong.
pub async fn acquire_upload_slot() -> SemaphorePermit<'static> {
    UPLOAD_SLOT
        .acquire()
        .await
        .expect("telegram upload semaphore never closed")
}

// ---------------------------------------------------------------------------
// Credentials (mã hoá trong settings)
// ---------------------------------------------------------------------------

/// Lưu Bot Token + Chat ID (mã hoá). Chuỗi rỗng → xoá credential (ghi "").
pub fn save_credentials(db: &Db, token: &str, chat_id: &str) -> Result<()> {
    let enc = |v: &str| -> Result<String> {
        if v.is_empty() {
            Ok(String::new())
        } else {
            Ok(B64.encode(crypto::encrypt_secret(v)?))
        }
    };
    db.set_setting(BOT_TOKEN_SETTING, &enc(token)?)?;
    db.set_setting(CHAT_ID_SETTING, &enc(chat_id)?)?;
    Ok(())
}

/// Đọc + giải mã credential từ settings. `None` khi chưa cấu hình đủ cả 2.
pub fn load_credentials(db: &Db) -> Result<Option<(String, String)>> {
    let dec = |key: &str| -> Result<Option<String>> {
        match db.get_setting(key)? {
            Some(b64) if !b64.is_empty() => {
                let blob = B64
                    .decode(&b64)
                    .map_err(|e| AppError::Crypto(format!("bad credential encoding: {e}")))?;
                Ok(Some(crypto::decrypt_secret(&blob)?))
            }
            _ => Ok(None),
        }
    };
    match (dec(BOT_TOKEN_SETTING)?, dec(CHAT_ID_SETTING)?) {
        (Some(t), Some(c)) => Ok(Some((t, c))),
        _ => Ok(None),
    }
}

/// Sync đang bật (setting enabled = "true") VÀ đã có đủ credential.
pub fn sync_ready(db: &Db) -> bool {
    matches!(db.get_setting(ENABLED_SETTING), Ok(Some(v)) if v == "true")
        && matches!(load_credentials(db), Ok(Some(_)))
}

// ---------------------------------------------------------------------------
// Split / hash (pure — test không cần mạng)
// ---------------------------------------------------------------------------

/// SHA-256 hex (lowercase) của toàn bộ dữ liệu.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Chia dữ liệu thành các part ≤ [`PART_SIZE`] theo thứ tự. Dữ liệu rỗng → 1
/// part rỗng (không xảy ra thực tế — `.bxa` luôn có header).
pub fn split_parts(data: &[u8]) -> Vec<&[u8]> {
    split_parts_with(data, PART_SIZE)
}

/// Lõi split với part size tham số hoá (test dùng size nhỏ, khỏi cấp phát 96MB).
fn split_parts_with(data: &[u8], part_size: usize) -> Vec<&[u8]> {
    if data.is_empty() {
        return vec![data];
    }
    data.chunks(part_size).collect()
}

/// Tên file part gửi lên Telegram: 1 part → `profile-<id>.bxa`, nhiều part →
/// `profile-<id>.bxa.001`… (đánh số 1-based, 3 chữ số).
pub fn part_file_name(profile_id: &str, part_index: usize, part_count: usize) -> String {
    if part_count <= 1 {
        format!("profile-{profile_id}.bxa")
    } else {
        format!("profile-{profile_id}.bxa.{:03}", part_index + 1)
    }
}

// ---------------------------------------------------------------------------
// Bot API client
// ---------------------------------------------------------------------------

/// Client Bot API — `api_base` tham số hoá để test trỏ server loopback local.
pub struct TelegramClient {
    http: reqwest::Client,
    api_base: String,
    token: String,
    chat_id: String,
}

/// Envelope chuẩn Bot API: `{ ok, result?, description?, parameters? }`.
#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    parameters: Option<ApiParameters>,
}

#[derive(Debug, Deserialize)]
struct ApiParameters {
    retry_after: Option<u64>,
}

/// Kết quả `sendDocument` — chỉ field cần dùng.
#[derive(Debug, Deserialize)]
struct SentMessage {
    message_id: i64,
    document: Option<SentDocument>,
}

#[derive(Debug, Deserialize)]
struct SentDocument {
    file_id: String,
}

/// Kết quả `getFile` — `file_path` để tải qua `/file/bot<token>/<path>`.
#[derive(Debug, Deserialize)]
struct FileInfo {
    file_path: Option<String>,
}

/// Kết quả `getMe` — xác nhận token hợp lệ.
#[derive(Debug, Deserialize)]
pub struct BotInfo {
    pub username: Option<String>,
}

impl TelegramClient {
    /// Client production (api.telegram.org).
    pub fn new(token: String, chat_id: String) -> Result<Self> {
        Self::with_api_base(API_BASE.to_string(), token, chat_id)
    }

    /// Client với base URL tuỳ ý (test trỏ server local).
    pub fn with_api_base(api_base: String, token: String, chat_id: String) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder().timeout(HTTP_TIMEOUT).build()?,
            api_base,
            token,
            chat_id,
        })
    }

    fn method_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base, self.token, method)
    }

    /// Gọi 1 method Bot API với retry: 429 → chờ đúng `retry_after` (fallback
    /// backoff), lỗi mạng/5xx → backoff 1s→2s→4s. Lỗi 4xx khác → fail ngay.
    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        build: impl Fn() -> Result<reqwest::RequestBuilder>,
    ) -> Result<T> {
        let mut last_err = String::new();
        let mut next_delay = Duration::ZERO;
        for attempt in 0..MAX_ATTEMPTS {
            tokio::time::sleep(next_delay).await;
            next_delay = Duration::from_secs(1 << attempt.min(2));
            let resp = match build()?.send().await {
                Ok(r) => r,
                Err(e) => {
                    last_err = format!("network error: {e}");
                    continue;
                }
            };
            let status = resp.status();
            let env: ApiEnvelope<T> = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    last_err = format!("bad response ({status}): {e}");
                    continue;
                }
            };
            if env.ok {
                return env.result.ok_or_else(|| {
                    AppError::InvalidInput(format!("telegram {method}: empty result"))
                });
            }
            let desc = env.description.unwrap_or_else(|| status.to_string());
            if status.as_u16() == 429 {
                if let Some(wait) = env.parameters.and_then(|p| p.retry_after) {
                    next_delay = Duration::from_secs(wait);
                }
                last_err = format!("rate limited: {desc}");
                continue;
            }
            if status.is_server_error() {
                last_err = format!("server error: {desc}");
                continue;
            }
            // 4xx khác (token sai, chat không tồn tại…) — retry vô ích.
            return Err(AppError::InvalidInput(format!("telegram {method}: {desc}")));
        }
        Err(AppError::InvalidInput(format!(
            "telegram {method} failed after {MAX_ATTEMPTS} attempts: {last_err}"
        )))
    }

    /// `getMe` — verify token.
    pub async fn get_me(&self) -> Result<BotInfo> {
        self.call("getMe", || Ok(self.http.get(self.method_url("getMe"))))
            .await
    }

    /// `sendMessage` text đơn giản (test kết nối tới chat).
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let _: serde_json::Value = self
            .call("sendMessage", || {
                Ok(self
                    .http
                    .post(self.method_url("sendMessage"))
                    .json(&serde_json::json!({ "chat_id": self.chat_id, "text": text })))
            })
            .await?;
        Ok(())
    }

    /// `sendDocument` multipart — upload 1 part, trả (message_id, file_id).
    pub async fn send_document(&self, file_name: &str, data: Vec<u8>) -> Result<(i64, String)> {
        let name = file_name.to_string();
        let sent: SentMessage = self
            .call("sendDocument", || {
                let part = reqwest::multipart::Part::bytes(data.clone())
                    .file_name(name.clone())
                    .mime_str("application/octet-stream")
                    .map_err(|e| AppError::InvalidInput(format!("multipart: {e}")))?;
                let form = reqwest::multipart::Form::new()
                    .text("chat_id", self.chat_id.clone())
                    .part("document", part);
                Ok(self
                    .http
                    .post(self.method_url("sendDocument"))
                    .multipart(form))
            })
            .await?;
        let file_id = sent
            .document
            .map(|d| d.file_id)
            .ok_or_else(|| AppError::InvalidInput("sendDocument: no document in reply".into()))?;
        Ok((sent.message_id, file_id))
    }

    /// `deleteMessage` — xoá 1 message (best-effort ở caller khi retention).
    pub async fn delete_message(&self, message_id: i64) -> Result<()> {
        let _: serde_json::Value = self
            .call("deleteMessage", || {
                Ok(self.http.post(self.method_url("deleteMessage")).json(
                    &serde_json::json!({ "chat_id": self.chat_id, "message_id": message_id }),
                ))
            })
            .await?;
        Ok(())
    }

    /// `getFile` + tải nội dung part qua `/file/bot<token>/<file_path>`.
    pub async fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        let info: FileInfo = self
            .call("getFile", || {
                Ok(self
                    .http
                    .post(self.method_url("getFile"))
                    .json(&serde_json::json!({ "file_id": file_id })))
            })
            .await?;
        let path = info
            .file_path
            .ok_or_else(|| AppError::InvalidInput("getFile: no file_path".into()))?;
        let url = format!("{}/file/bot{}/{}", self.api_base, self.token, path);
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        Ok(resp.bytes().await?.to_vec())
    }
}

// ---------------------------------------------------------------------------
// Upload / restore / retention flows
// ---------------------------------------------------------------------------

/// Upload archive `.bxa` của profile lên Telegram: split part → sendDocument
/// từng part → ghi metadata `cloud_backups` → prune retention. Caller giữ
/// [`acquire_upload_slot`] trong suốt quá trình.
pub async fn upload_archive(
    client: &TelegramClient,
    db: &Arc<Db>,
    profile_id: &str,
    archive_path: &Path,
) -> Result<()> {
    let data = tokio::fs::read(archive_path).await?;
    let sha256 = sha256_hex(&data);
    let parts = split_parts(&data);
    let part_count = parts.len();
    let mut inputs = Vec::with_capacity(part_count);
    for (i, chunk) in parts.into_iter().enumerate() {
        let name = part_file_name(profile_id, i, part_count);
        let (message_id, file_id) = client.send_document(&name, chunk.to_vec()).await?;
        inputs.push(CloudBackupPartInput {
            message_id,
            file_id,
            size: chunk.len() as i64,
            part_index: i as i64,
        });
    }
    let uploaded_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    db.insert_cloud_backup(profile_id, &sha256, &uploaded_at, &inputs)?;
    prune_retention(client, db, profile_id).await;
    Ok(())
}

/// Xoá các bản backup vượt [`RETENTION`]: deleteMessage từng part (best-effort
/// — message có thể đã bị xoá tay trên Telegram) rồi xoá row DB.
pub async fn prune_retention(client: &TelegramClient, db: &Arc<Db>, profile_id: &str) {
    let Ok(old_parts) = db.cloud_backups_beyond_retention(profile_id, RETENTION) else {
        return;
    };
    let mut stale: Vec<(String, String)> = Vec::new();
    for p in &old_parts {
        if let Err(e) = client.delete_message(p.message_id).await {
            tracing::warn!(
                "telegram retention: deleteMessage {} failed (continuing): {e}",
                p.message_id
            );
        }
        let key = (p.profile_id.clone(), p.uploaded_at.clone());
        if !stale.contains(&key) {
            stale.push(key);
        }
    }
    for (pid, ts) in stale {
        if let Err(e) = db.delete_cloud_backup(&pid, &ts) {
            tracing::warn!("telegram retention: delete rows {pid}@{ts} failed: {e}");
        }
    }
}

/// Xoá HẲN 1 bản backup cloud (row DB + message Telegram, best-effort với
/// message đã mất). `uploaded_at = None` → bản mới nhất.
pub async fn delete_backup(
    client: &TelegramClient,
    db: &Arc<Db>,
    profile_id: &str,
    uploaded_at: Option<&str>,
) -> Result<()> {
    let parts = db.get_cloud_backup_parts(profile_id, uploaded_at)?;
    if parts.is_empty() {
        return Err(AppError::NotFound(format!(
            "no cloud backup for profile {profile_id}"
        )));
    }
    for p in &parts {
        if let Err(e) = client.delete_message(p.message_id).await {
            tracing::warn!(
                "telegram delete: deleteMessage {} failed (continuing): {e}",
                p.message_id
            );
        }
    }
    db.delete_cloud_backup(profile_id, &parts[0].uploaded_at)?;
    Ok(())
}

/// Tải bản backup mới nhất: getFile từng part → ghép theo part_index → verify
/// sha256 → trả bytes `.bxa` nguyên vẹn (caller ghi file + gọi archive.rs).
pub async fn download_backup(
    client: &TelegramClient,
    parts: &[CloudBackupPart],
) -> Result<Vec<u8>> {
    if parts.is_empty() {
        return Err(AppError::NotFound("cloud backup has no parts".into()));
    }
    let expected = parts.len() as i64;
    if parts[0].part_count != expected {
        return Err(AppError::InvalidInput(format!(
            "cloud backup incomplete: {expected}/{} parts in DB",
            parts[0].part_count
        )));
    }
    let mut data = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if p.part_index != i as i64 {
            return Err(AppError::InvalidInput(format!(
                "cloud backup part order corrupt (expected {i}, got {})",
                p.part_index
            )));
        }
        let chunk = client.download_file(&p.file_id).await?;
        data.extend_from_slice(&chunk);
    }
    let actual = sha256_hex(&data);
    if actual != parts[0].sha256 {
        return Err(AppError::Crypto(format!(
            "cloud backup sha256 mismatch (expected {}, got {actual})",
            parts[0].sha256
        )));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // -- Pure logic: split / ghép / sha256 / tên part --------------------

    #[test]
    fn split_join_roundtrip_sha256() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let sha = sha256_hex(&data);
        let parts = split_parts_with(&data, 30_000);
        assert_eq!(parts.len(), 4);
        assert!(parts[..3].iter().all(|p| p.len() == 30_000));
        assert_eq!(parts[3].len(), 10_000);
        let joined: Vec<u8> = parts.concat();
        assert_eq!(joined, data);
        assert_eq!(sha256_hex(&joined), sha);
        // Nhỏ hơn part size → đúng 1 part, không copy thừa.
        assert_eq!(split_parts_with(&data, 200_000).len(), 1);
        assert_eq!(split_parts_with(&[], 100).len(), 1);
    }

    #[test]
    fn part_names() {
        assert_eq!(part_file_name("p1", 0, 1), "profile-p1.bxa");
        assert_eq!(part_file_name("p1", 0, 3), "profile-p1.bxa.001");
        assert_eq!(part_file_name("p1", 2, 3), "profile-p1.bxa.003");
    }

    // -- Credentials mã hoá trong settings --------------------------------

    #[test]
    fn credentials_roundtrip_encrypted_at_rest() {
        crate::crypto::install_test_master_key();
        let db = Db::open_in_memory().unwrap();
        assert!(load_credentials(&db).unwrap().is_none());
        assert!(!sync_ready(&db));

        save_credentials(&db, "123:ABC-token", "-100987").unwrap();
        let (t, c) = load_credentials(&db).unwrap().unwrap();
        assert_eq!(t, "123:ABC-token");
        assert_eq!(c, "-100987");
        // Ở REST không có plaintext: settings chỉ chứa blob base64 mã hoá.
        let raw = db.get_setting(BOT_TOKEN_SETTING).unwrap().unwrap();
        assert!(!raw.contains("ABC-token"));

        assert!(!sync_ready(&db), "chưa bật enabled thì chưa ready");
        db.set_setting(ENABLED_SETTING, "true").unwrap();
        assert!(sync_ready(&db));

        // Chuỗi rỗng → xoá credential.
        save_credentials(&db, "", "").unwrap();
        assert!(load_credentials(&db).unwrap().is_none());
        assert!(!sync_ready(&db));
    }

    // -- Retention DB logic ------------------------------------------------

    fn insert_backup(db: &Db, pid: &str, ts: &str, n_parts: usize) {
        let parts: Vec<CloudBackupPartInput> = (0..n_parts)
            .map(|i| CloudBackupPartInput {
                message_id: (ts.len() * 1000 + i) as i64,
                file_id: format!("file-{ts}-{i}"),
                size: 100,
                part_index: i as i64,
            })
            .collect();
        db.insert_cloud_backup(pid, &format!("sha-{ts}"), ts, &parts)
            .unwrap();
    }

    #[test]
    fn retention_selects_only_oldest_beyond_keep() {
        let db = Db::open_in_memory().unwrap();
        for (i, ts) in ["2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z", "2026-03-01T00:00:00Z", "2026-04-01T00:00:00Z"]
            .iter()
            .enumerate()
        {
            insert_backup(&db, "p1", ts, i % 2 + 1);
        }
        insert_backup(&db, "p2", "2026-01-15T00:00:00Z", 1);

        // 4 bản, giữ 3 → chỉ bản 2026-01-01 (1 part) bị coi là thừa.
        let old = db.cloud_backups_beyond_retention("p1", RETENTION).unwrap();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].uploaded_at, "2026-01-01T00:00:00Z");
        // Profile khác không bị ảnh hưởng.
        assert!(db
            .cloud_backups_beyond_retention("p2", RETENTION)
            .unwrap()
            .is_empty());

        db.delete_cloud_backup("p1", "2026-01-01T00:00:00Z").unwrap();
        let list = db.list_cloud_backups(Some("p1")).unwrap();
        assert_eq!(list.len(), 3, "còn đúng 3 bản sau prune");
        assert_eq!(list[0].uploaded_at, "2026-04-01T00:00:00Z", "mới nhất trước");
        assert!(db
            .cloud_backups_beyond_retention("p1", RETENTION)
            .unwrap()
            .is_empty());

        // list gộp part đúng: bản 2 part có size tổng 200.
        let two_part = list.iter().find(|b| b.part_count == 2).unwrap();
        assert_eq!(two_part.size, 200);

        // get parts theo thứ tự part_index; None = bản mới nhất.
        let parts = db.get_cloud_backup_parts("p1", None).unwrap();
        assert_eq!(parts[0].uploaded_at, "2026-04-01T00:00:00Z");
        assert!(parts.windows(2).all(|w| w[0].part_index < w[1].part_index));
    }

    // -- HTTP flows qua server loopback (KHÔNG mạng thật) ------------------

    /// Server HTTP/1.1 tối giản trên loopback: map "METHOD /path-suffix" →
    /// queue body JSON trả về (mỗi request pop 1 response; hết queue → dùng
    /// response cuối). Trả (base_url, handle).
    async fn spawn_stub(
        routes: Vec<(&'static str, Vec<(u16, String)>)>,
        hits: std::sync::Arc<Mutex<Vec<String>>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut map: HashMap<&'static str, Vec<(u16, String)>> = HashMap::new();
        for (k, v) in routes {
            map.insert(k, v);
        }
        let map = std::sync::Arc::new(Mutex::new(map));
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let map = map.clone();
                let hits = hits.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    // Đọc tới hết header, rồi drain body theo Content-Length.
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
                    let request_line = header.lines().next().unwrap_or("").to_string();
                    hits.lock().unwrap().push(request_line.clone());
                    // Match theo suffix path (bỏ /bot<token> prefix).
                    let (status, body) = {
                        let mut map = map.lock().unwrap();
                        let key = map
                            .keys()
                            .find(|k| {
                                let (m, p) = k.split_once(' ').unwrap_or(("", ""));
                                request_line.starts_with(m) && request_line.contains(p)
                            })
                            .copied();
                        match key {
                            Some(k) => {
                                let q = map.get_mut(k).unwrap();
                                if q.len() > 1 {
                                    q.remove(0)
                                } else {
                                    q[0].clone()
                                }
                            }
                            None => (404, r#"{"ok":false,"description":"no route"}"#.into()),
                        }
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn ok_json(v: serde_json::Value) -> (u16, String) {
        (200, serde_json::json!({ "ok": true, "result": v }).to_string())
    }

    #[tokio::test]
    async fn upload_retries_on_429_then_succeeds_and_records_db() {
        let hits = std::sync::Arc::new(Mutex::new(Vec::new()));
        let base = spawn_stub(
            vec![(
                "POST /sendDocument",
                vec![
                    // Lần 1: 429 retry_after 0 (test nhanh) → phải retry.
                    (429, serde_json::json!({
                        "ok": false, "description": "Too Many Requests",
                        "parameters": { "retry_after": 0 }
                    }).to_string()),
                    ok_json(serde_json::json!({
                        "message_id": 11, "document": { "file_id": "F1" }
                    })),
                ],
            )],
            hits.clone(),
        )
        .await;

        let client = TelegramClient::with_api_base(base, "T".into(), "C".into()).unwrap();
        let db = std::sync::Arc::new(Db::open_in_memory().unwrap());
        let dir = std::env::temp_dir().join(format!("bx-tg-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bxa = dir.join("profile-p1.bxa");
        std::fs::write(&bxa, b"BXA1-fake-archive-bytes").unwrap();

        upload_archive(&client, &db, "p1", &bxa).await.unwrap();

        let list = db.list_cloud_backups(Some("p1")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].part_count, 1);
        assert_eq!(list[0].size, 23);
        assert_eq!(list[0].sha256, sha256_hex(b"BXA1-fake-archive-bytes"));
        // 2 request sendDocument: 429 rồi thành công.
        let n = hits
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("sendDocument"))
            .count();
        assert_eq!(n, 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn bad_token_fails_fast_without_retry() {
        let hits = std::sync::Arc::new(Mutex::new(Vec::new()));
        let base = spawn_stub(
            vec![(
                "GET /getMe",
                vec![(401, r#"{"ok":false,"description":"Unauthorized"}"#.into())],
            )],
            hits.clone(),
        )
        .await;
        let client = TelegramClient::with_api_base(base, "BAD".into(), "C".into()).unwrap();
        let err = client.get_me().await.unwrap_err();
        assert!(err.to_string().contains("Unauthorized"));
        assert_eq!(hits.lock().unwrap().len(), 1, "4xx không retry");
    }

    #[tokio::test]
    async fn download_backup_joins_parts_and_verifies_sha256() {
        let part_a = b"first-part-".to_vec();
        let part_b = b"second-part".to_vec();
        let full: Vec<u8> = [part_a.clone(), part_b.clone()].concat();
        let sha = sha256_hex(&full);

        let hits = std::sync::Arc::new(Mutex::new(Vec::new()));
        let base = spawn_stub(
            vec![
                (
                    "POST /getFile",
                    vec![
                        ok_json(serde_json::json!({ "file_path": "docs/a.bin" })),
                        ok_json(serde_json::json!({ "file_path": "docs/b.bin" })),
                    ],
                ),
                ("GET /file/botT/docs/a.bin", vec![(200, String::from_utf8(part_a).unwrap())]),
                ("GET /file/botT/docs/b.bin", vec![(200, String::from_utf8(part_b).unwrap())]),
            ],
            hits.clone(),
        )
        .await;
        let client = TelegramClient::with_api_base(base, "T".into(), "C".into()).unwrap();

        let mk_part = |i: i64, sha: &str| CloudBackupPart {
            id: i + 1,
            profile_id: "p1".into(),
            message_id: 100 + i,
            file_id: format!("f{i}"),
            size: 11,
            sha256: sha.to_string(),
            part_index: i,
            part_count: 2,
            uploaded_at: "2026-05-01T00:00:00Z".into(),
        };
        let parts = vec![mk_part(0, &sha), mk_part(1, &sha)];
        let data = download_backup(&client, &parts).await.unwrap();
        assert_eq!(data, full);

        // sha sai → lỗi Crypto, KHÔNG trả dữ liệu.
        let bad = vec![mk_part(0, "deadbeef"), mk_part(1, "deadbeef")];
        let err = download_backup(&client, &bad).await.unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));

        // Thiếu part (part_count=2 nhưng chỉ có 1 row) → refuse.
        let missing = vec![mk_part(0, &sha)];
        let err = download_backup(&client, &missing).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }
}
