//! (W55b-transport) Chọn transport cho Cloud Sync + đường upload/restore/delete
//! qua Userbot MTProto (tdlib-rs, W55b-core).
//!
//! Hai transport:
//! - "bot_api" (MẶC ĐỊNH): đường cũ telegram_sync.rs — split part 48MB, KHÔNG đổi.
//! - "userbot": upload NGUYÊN file `.bxa` ≤2GB vào private channel (1 row/bản,
//!   không split); >2GB → từ chối với lỗi rõ (split chỉ tồn tại ở Bot API).
//!
//! Backup upload bằng transport nào PHẢI restore/delete bằng transport đó —
//! cột `transport` trong `cloud_backups` (migration v16) là nguồn sự thật,
//! caller (commands.rs) route theo `parts[0].transport`.
//!
//! Flood-wait: lỗi FLOOD_WAIT từ userbot → lưu `flood_wait_until` (settings);
//! upload trong thời gian đó → skip ngay với lỗi "flood wait until <time>"
//! (persist vào `cloud_upload_state` như C1, xem commands.rs).

use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::db::{CloudBackupPart, CloudBackupPartInput, Db};
use crate::error::{AppError, Result};
use crate::telegram_sync::{self, ProgressFn, RETENTION};
use crate::userbot::{self, UserbotError};

/// Settings key: transport Cloud Sync đang chọn ("bot_api" | "userbot").
pub const TRANSPORT_SETTING: &str = "cloud_transport";
/// Settings key: RFC3339 UTC — userbot bị Telegram flood-wait đến thời điểm này.
pub const FLOOD_WAIT_UNTIL_SETTING: &str = "userbot_flood_wait_until";
/// Giới hạn file userbot (tài khoản free 2GB — KHÔNG split, quá thì từ chối).
pub const MAX_USERBOT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Transport setting (API surface CHỐT CỨNG: cloud_get_transport/cloud_set_transport)
// ---------------------------------------------------------------------------

/// Transport hiện tại — thiếu setting/giá trị lạ → "bot_api" (an toàn mặc định).
pub fn get_transport(db: &Db) -> String {
    match db.get_setting(TRANSPORT_SETTING) {
        Ok(Some(v)) if v == "userbot" => v,
        _ => "bot_api".to_string(),
    }
}

/// Đổi transport. "userbot" CHỈ nhận khi userbot status = ready (spec chốt) —
/// tránh chọn transport mà không upload/restore được.
pub fn set_transport(db: &Db, transport: &str) -> Result<()> {
    match transport {
        "bot_api" => {}
        "userbot" => {
            let status = userbot::current_status(db)?;
            if status.state != "ready" {
                return Err(AppError::InvalidInput(format!(
                    "userbot chưa sẵn sàng (state: {}) — đăng nhập userbot trước khi chọn transport này",
                    status.state
                )));
            }
        }
        other => {
            return Err(AppError::InvalidInput(format!(
                "transport không hợp lệ: {other} (chỉ nhận \"bot_api\" | \"userbot\")"
            )))
        }
    }
    db.set_setting(TRANSPORT_SETTING, transport)
}

/// Cloud sync sẵn sàng auto-upload sau archive: setting enabled bật VÀ
/// transport đang chọn có credentials (bot_api: token+chat; userbot: api_id+hash
/// — trạng thái ready kiểm tra lúc upload, lỗi NotReady sẽ persist rõ ràng).
pub fn sync_ready(db: &Db) -> bool {
    let enabled = matches!(
        db.get_setting(telegram_sync::ENABLED_SETTING),
        Ok(Some(v)) if v == "true"
    );
    enabled
        && match get_transport(db).as_str() {
            "userbot" => matches!(userbot::load_credentials(db), Ok(Some(_))),
            _ => matches!(telegram_sync::load_credentials(db), Ok(Some(_))),
        }
}

// ---------------------------------------------------------------------------
// Flood-wait window (pattern VFlowX: lưu hạn, skip upload đến hạn)
// ---------------------------------------------------------------------------

/// Nếu đang trong cửa sổ flood-wait → Some(hạn RFC3339). Giá trị thiếu/hỏng/
/// quá khứ → None (không chặn oan).
pub fn flood_wait_active(db: &Db) -> Result<Option<String>> {
    flood_wait_active_at(db, chrono::Utc::now())
}

/// Lõi tham số hoá `now` để test không phụ thuộc đồng hồ thật.
fn flood_wait_active_at(
    db: &Db,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<String>> {
    let Some(until) = db.get_setting(FLOOD_WAIT_UNTIL_SETTING)? else {
        return Ok(None);
    };
    match chrono::DateTime::parse_from_rfc3339(&until) {
        Ok(t) if t > now => Ok(Some(until)),
        _ => Ok(None),
    }
}

/// Ghi hạn flood-wait = now + secs (Telegram báo qua [`UserbotError::FloodWait`]).
pub fn record_flood_wait(db: &Db, secs: u64) -> Result<String> {
    let until = (chrono::Utc::now() + chrono::Duration::seconds(secs.min(i64::MAX as u64) as i64))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.set_setting(FLOOD_WAIT_UNTIL_SETTING, &until)?;
    Ok(until)
}

/// Nếu `err` là FloodWait từ userbot → lưu hạn vào settings (best-effort).
fn note_flood_wait(db: &Db, err: &AppError) {
    if let AppError::Other(any) = err {
        if let Some(UserbotError::FloodWait(secs)) = any.downcast_ref::<UserbotError>() {
            match record_flood_wait(db, *secs) {
                Ok(until) => tracing::warn!("userbot flood wait until {until}"),
                Err(e) => tracing::warn!("userbot flood wait: không lưu được hạn: {e}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Guards + helpers thuần (unit-test không cần TDLib)
// ---------------------------------------------------------------------------

/// Guard kích thước userbot: >2GB → lỗi rõ ràng, KHÔNG split.
pub fn check_userbot_size(bytes: u64) -> Result<()> {
    if bytes > MAX_USERBOT_BYTES {
        return Err(AppError::InvalidInput(format!(
            "archive {bytes} bytes vượt giới hạn 2GB của userbot transport (không split) — \
             dùng Bot API transport hoặc giảm dữ liệu profile"
        )));
    }
    Ok(())
}

/// Caption message backup trên channel: sha256 + profile id + timestamp —
/// đủ để đối chiếu tay trên Telegram khi cần cứu dữ liệu.
fn backup_caption(sha256: &str, profile_id: &str, uploaded_at: &str) -> String {
    format!("sha256:{sha256} profile:{profile_id} uploaded_at:{uploaded_at}")
}

/// SHA-256 hex của file trên đĩa (streaming — KHÔNG load 2GB vào RAM).
async fn sha256_file(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String> {
        use std::io::Read;
        let mut f = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(hex::encode(h.finalize()))
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("sha256 task panic: {e}")))?
}

// ---------------------------------------------------------------------------
// Userbot flows (upload / restore / delete — gọi hàm pub của userbot.rs)
// ---------------------------------------------------------------------------

/// Upload archive `.bxa` NGUYÊN VẸN qua userbot vào private channel:
/// guard flood-wait + 2GB → ensure_sync_channel → upload_file (caption
/// sha256+profile+timestamp) → ghi 1 row transport='userbot' → prune retention.
/// Caller giữ `telegram_sync::acquire_upload_slot()` như đường Bot API.
/// `progress` gọi (0/1, 0 bytes) trước và (1/1, total) sau — 1 "part" duy nhất.
pub async fn upload_archive_userbot(
    db: &Arc<Db>,
    profile_id: &str,
    archive_path: &Path,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    if let Some(until) = flood_wait_active(db)? {
        return Err(AppError::InvalidInput(format!("flood wait until {until}")));
    }
    let bytes_total = tokio::fs::metadata(archive_path).await?.len();
    check_userbot_size(bytes_total)?;
    let sha256 = sha256_file(archive_path).await?;
    let uploaded_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    if let Some(cb) = progress {
        cb("upload", 0, 1, 0, bytes_total);
    }
    let result: Result<()> = async {
        let chat_id = userbot::ensure_sync_channel().await?;
        let caption = backup_caption(&sha256, profile_id, &uploaded_at);
        let message_id = userbot::upload_file(chat_id, archive_path, &caption).await?;
        db.insert_cloud_backup_with_transport(
            profile_id,
            &sha256,
            &uploaded_at,
            "userbot",
            Some(chat_id),
            &[CloudBackupPartInput {
                message_id,
                file_id: String::new(),
                size: bytes_total as i64,
                part_index: 0,
            }],
        )?;
        prune_retention_userbot(db, profile_id).await;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            if let Some(cb) = progress {
                cb("upload", 1, 1, bytes_total, bytes_total);
            }
            Ok(())
        }
        Err(e) => {
            note_flood_wait(db, &e);
            Err(e)
        }
    }
}

/// Retention chung [`RETENTION`] bản/profile cho đường userbot: xoá message
/// best-effort theo transport gốc CỦA TỪNG BẢN cũ (bản bot_api cũ không xoá
/// được ở đây — thiếu client Bot API — chỉ log; row DB vẫn giữ để user còn
/// restore/delete bằng đường bot_api).
async fn prune_retention_userbot(db: &Arc<Db>, profile_id: &str) {
    let Ok(old_parts) = db.cloud_backups_beyond_retention(profile_id, RETENTION) else {
        return;
    };
    let mut stale: Vec<(String, String)> = Vec::new();
    for p in &old_parts {
        if p.transport != "userbot" {
            tracing::warn!(
                "cloud retention: bản {} transport={} — bỏ qua (xoá bằng đường riêng của nó)",
                p.uploaded_at,
                p.transport
            );
            continue;
        }
        if let Some(chat_id) = p.chat_id {
            if let Err(e) = userbot::delete_message(chat_id, p.message_id).await {
                tracing::warn!(
                    "userbot retention: delete_message {} failed (continuing): {e}",
                    p.message_id
                );
            }
        }
        let key = (p.profile_id.clone(), p.uploaded_at.clone());
        if !stale.contains(&key) {
            stale.push(key);
        }
    }
    for (pid, ts) in stale {
        if let Err(e) = db.delete_cloud_backup(&pid, &ts) {
            tracing::warn!("userbot retention: delete rows {pid}@{ts} failed: {e}");
        }
    }
}

/// Tải bản backup userbot: download_file theo (chat_id, message_id) → verify
/// sha256 → trả bytes `.bxa`. `parts` PHẢI là bản transport='userbot' (1 part).
pub async fn download_backup_userbot(
    parts: &[CloudBackupPart],
    progress: Option<ProgressFn<'_>>,
) -> Result<Vec<u8>> {
    let part = match parts {
        [p] if p.transport == "userbot" => p,
        [] => return Err(AppError::NotFound("cloud backup has no parts".into())),
        _ => {
            return Err(AppError::InvalidInput(
                "backup userbot phải có đúng 1 part transport='userbot'".into(),
            ))
        }
    };
    let chat_id = part.chat_id.ok_or_else(|| {
        AppError::InvalidInput("backup userbot thiếu chat_id — metadata hỏng".into())
    })?;
    let bytes_total = part.size.max(0) as u64;
    if let Some(cb) = progress {
        cb("download", 0, 1, 0, bytes_total);
    }
    let local_path = userbot::download_file(chat_id, part.message_id).await?;
    let data = tokio::fs::read(&local_path).await?;
    let actual = telegram_sync::sha256_hex(&data);
    if actual != part.sha256 {
        return Err(AppError::Crypto(format!(
            "cloud backup sha256 mismatch (expected {}, got {actual})",
            part.sha256
        )));
    }
    if let Some(cb) = progress {
        cb("download", 1, 1, data.len() as u64, bytes_total);
    }
    Ok(data)
}

/// Xoá HẲN 1 bản backup userbot (message channel best-effort + row DB).
/// `parts` là bản đã đọc từ DB (caller route theo transport trước khi gọi).
pub async fn delete_backup_userbot(db: &Arc<Db>, parts: &[CloudBackupPart]) -> Result<()> {
    let Some(first) = parts.first() else {
        return Err(AppError::NotFound("cloud backup has no parts".into()));
    };
    for p in parts {
        if let Some(chat_id) = p.chat_id {
            if let Err(e) = userbot::delete_message(chat_id, p.message_id).await {
                tracing::warn!(
                    "userbot delete: delete_message {} failed (continuing): {e}",
                    p.message_id
                );
            }
        }
    }
    db.delete_cloud_backup(&first.profile_id, &first.uploaded_at)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Transport setting + guard ----------------------------------------

    #[test]
    fn transport_defaults_to_bot_api_and_rejects_unknown() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_transport(&db), "bot_api");
        // Giá trị lạ trong settings → vẫn bot_api (an toàn).
        db.set_setting(TRANSPORT_SETTING, "carrier-pigeon").unwrap();
        assert_eq!(get_transport(&db), "bot_api");

        set_transport(&db, "bot_api").unwrap();
        assert_eq!(get_transport(&db), "bot_api");
        let err = set_transport(&db, "smtp").unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn set_transport_userbot_guards_ready_state() {
        crate::crypto::install_test_master_key();
        let db = Db::open_in_memory().unwrap();
        // Chưa có credentials → state "no_credentials" → refuse.
        let err = set_transport(&db, "userbot").unwrap_err();
        assert!(err.to_string().contains("chưa sẵn sàng"));
        assert_eq!(get_transport(&db), "bot_api", "setting KHÔNG bị ghi khi guard fail");
    }

    #[test]
    fn sync_ready_follows_selected_transport() {
        crate::crypto::install_test_master_key();
        let db = Db::open_in_memory().unwrap();
        db.set_setting(telegram_sync::ENABLED_SETTING, "true").unwrap();
        assert!(!sync_ready(&db), "bot_api chưa có credentials");

        telegram_sync::save_credentials(&db, "T", "C").unwrap();
        assert!(sync_ready(&db));

        // Chuyển sang userbot (ghi thẳng setting — guard ready test riêng):
        // userbot chưa có api_id/hash → not ready dù bot_api đủ credentials.
        db.set_setting(TRANSPORT_SETTING, "userbot").unwrap();
        assert!(!sync_ready(&db));
        crate::userbot::save_credentials(&db, 12345, "hash").unwrap();
        assert!(sync_ready(&db));
    }

    // -- Guard >2GB ---------------------------------------------------------

    #[test]
    fn userbot_size_guard_rejects_over_2gb() {
        check_userbot_size(0).unwrap();
        check_userbot_size(MAX_USERBOT_BYTES).unwrap();
        let err = check_userbot_size(MAX_USERBOT_BYTES + 1).unwrap_err();
        assert!(err.to_string().contains("2GB"));
        assert!(err.to_string().contains("không split"));
    }

    // -- Flood-wait window ----------------------------------------------------

    #[test]
    fn flood_wait_window_blocks_until_deadline_then_clears() {
        let db = Db::open_in_memory().unwrap();
        assert!(flood_wait_active(&db).unwrap().is_none(), "chưa từng flood-wait");

        let until = record_flood_wait(&db, 3600).unwrap();
        let now = chrono::Utc::now();
        assert_eq!(flood_wait_active_at(&db, now).unwrap(), Some(until.clone()));
        // Qua hạn → hết chặn.
        let later = now + chrono::Duration::seconds(3700);
        assert!(flood_wait_active_at(&db, later).unwrap().is_none());

        // Giá trị hỏng trong settings → không chặn oan.
        db.set_setting(FLOOD_WAIT_UNTIL_SETTING, "not-a-timestamp").unwrap();
        assert!(flood_wait_active(&db).unwrap().is_none());
    }

    #[tokio::test]
    async fn upload_skips_during_flood_wait_without_touching_network() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let until = record_flood_wait(&db, 3600).unwrap();
        let dir = std::env::temp_dir().join(format!("bx-ct-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bxa = dir.join("profile-p1.bxa");
        std::fs::write(&bxa, b"BXA1-bytes").unwrap();

        let err = upload_archive_userbot(&db, "p1", &bxa, None).await.unwrap_err();
        assert!(err.to_string().contains("flood wait until"));
        assert!(err.to_string().contains(&until));
        // Skip TRƯỚC mọi bước mạng → không có row backup nào bị ghi.
        assert!(db.list_cloud_backups(Some("p1")).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn note_flood_wait_records_deadline_from_typed_error() {
        let db = Db::open_in_memory().unwrap();
        let err: AppError = UserbotError::FloodWait(120).into();
        note_flood_wait(&db, &err);
        assert!(flood_wait_active(&db).unwrap().is_some());

        // Lỗi khác KHÔNG ghi hạn.
        let db2 = Db::open_in_memory().unwrap();
        note_flood_wait(&db2, &AppError::InvalidInput("x".into()));
        assert!(flood_wait_active(&db2).unwrap().is_none());
    }

    // -- Routing metadata (DB) --------------------------------------------------

    fn userbot_part(db: &Db, pid: &str, ts: &str, chat_id: Option<i64>) {
        db.insert_cloud_backup_with_transport(
            pid,
            "sha-x",
            ts,
            "userbot",
            chat_id,
            &[CloudBackupPartInput {
                message_id: 42,
                file_id: String::new(),
                size: 10,
                part_index: 0,
            }],
        )
        .unwrap();
    }

    #[test]
    fn backup_rows_carry_transport_and_chat_id_for_routing() {
        let db = Db::open_in_memory().unwrap();
        // Đường cũ (không transport) → mặc định bot_api, chat_id NULL.
        db.insert_cloud_backup(
            "p1",
            "sha-a",
            "2026-01-01T00:00:00Z",
            &[CloudBackupPartInput {
                message_id: 1,
                file_id: "F1".into(),
                size: 5,
                part_index: 0,
            }],
        )
        .unwrap();
        userbot_part(&db, "p1", "2026-02-01T00:00:00Z", Some(-1009));

        let old = db.get_cloud_backup_parts("p1", Some("2026-01-01T00:00:00Z")).unwrap();
        assert_eq!(old[0].transport, "bot_api");
        assert_eq!(old[0].chat_id, None);
        let latest = db.get_cloud_backup_parts("p1", None).unwrap();
        assert_eq!(latest[0].transport, "userbot");
        assert_eq!(latest[0].chat_id, Some(-1009));
        // View gộp cho FE cũng mang transport.
        let list = db.list_cloud_backups(Some("p1")).unwrap();
        assert_eq!(list[0].transport, "userbot");
        assert_eq!(list[1].transport, "bot_api");
    }

    #[tokio::test]
    async fn download_userbot_rejects_corrupt_metadata_before_network() {
        // Nhiều part / thiếu chat_id → refuse NGAY, không đụng TDLib.
        let db = Db::open_in_memory().unwrap();
        userbot_part(&db, "p1", "2026-01-01T00:00:00Z", None);
        let parts = db.get_cloud_backup_parts("p1", None).unwrap();
        let err = download_backup_userbot(&parts, None).await.unwrap_err();
        assert!(err.to_string().contains("chat_id"));

        let err = download_backup_userbot(&[], None).await.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));

        // Part transport bot_api lạc vào đường userbot → refuse.
        db.insert_cloud_backup(
            "p2",
            "sha-b",
            "2026-01-01T00:00:00Z",
            &[CloudBackupPartInput {
                message_id: 1,
                file_id: "F1".into(),
                size: 5,
                part_index: 0,
            }],
        )
        .unwrap();
        let parts = db.get_cloud_backup_parts("p2", None).unwrap();
        let err = download_backup_userbot(&parts, None).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn backup_caption_contains_required_fields() {
        let c = backup_caption("deadbeef", "p1", "2026-01-01T00:00:00Z");
        assert_eq!(c, "sha256:deadbeef profile:p1 uploaded_at:2026-01-01T00:00:00Z");
    }
}
