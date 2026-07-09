//! (W55c) Backup toàn bộ app DB (settings/profiles/proxies metadata) lên cloud
//! — kiểu VFlowX DB sync. Disaster recovery trọn vẹn: mất máy vẫn dựng lại
//! được danh sách profile + settings từ Telegram.
//!
//! Pipeline backup: `Db::vacuum_into` (snapshot NHẤT QUÁN — KHÔNG copy file
//! đang mở) → mã hoá `.bxa` (archive.rs — AES-256-GCM chunk, Argon2id từ
//! master key, KHÔNG KDF mới) → upload qua transport hiện hành (Bot API split
//! / userbot nguyên file) dưới profile_id sentinel [`APP_DB_PROFILE_ID`] —
//! tái dùng nguyên bảng `cloud_backups` + retention 3 bản + upload state.
//!
//! Restore AN TOÀN (áp DB nóng lên connection đang mở quá rủi ro):
//! 1. `restore_app_db` tải + verify sha256 + giải mã + [`validate_snapshot`]
//!    → ghi file STAGING `browserx.db.restore-staged` cạnh DB. KHÔNG đụng DB.
//! 2. User restart app; [`apply_pending_restore`] chạy lúc STARTUP (trước khi
//!    mở DB): DB hiện tại đổi tên `browserx.db.bak-cloud-restore-<ts>` (kèm
//!    -wal/-shm — KHÔNG để SQLite áp WAL cũ lên DB mới) rồi staged vào chỗ.
//!
//! Kill giữa chừng: staged còn → lần startup sau áp tiếp; DB cũ luôn còn bản
//! `.bak-cloud-restore-*` (giữ [`MAX_RESTORE_BAKS`] bản mới nhất).

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::db;
use crate::error::{AppError, Result};

/// Sentinel "profile id" của bản backup app DB trong `cloud_backups` /
/// `cloud_upload_state` — không đụng hàng UUID profile thật.
pub const APP_DB_PROFILE_ID: &str = "__app_db__";

/// Số bản `browserx.db.bak-cloud-restore-*` giữ lại (cũ nhất xoá trước).
const MAX_RESTORE_BAKS: usize = 3;

/// 16 byte đầu mọi file SQLite hợp lệ.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Đường dẫn file staging restore cạnh DB: `<dir>/browserx.db.restore-staged`.
pub fn staged_path(data_dir: &Path) -> PathBuf {
    data_dir.join("browserx.db.restore-staged")
}

/// Đường dẫn `.bxa` app DB cạnh DB: `<dir>/profile-__app_db__.bxa` — cùng
/// naming convention archive profile để upload/caption nhất quán.
pub fn bxa_path(data_dir: &Path) -> PathBuf {
    data_dir.join(format!("profile-{APP_DB_PROFILE_ID}.bxa"))
}

/// Validate 1 file snapshot SQLite TRƯỚC khi staging/áp: đúng magic, mở được
/// read-only, `user_version` không MỚI hơn schema app này (cũ hơn OK — migrate
/// chạy lúc mở), và có bảng `profiles` (đúng là DB BrowserX, không phải DB lạ).
pub fn validate_snapshot(path: &Path) -> Result<()> {
    let raw = fs::read(path)?;
    if raw.len() < SQLITE_MAGIC.len() || &raw[..SQLITE_MAGIC.len()] != SQLITE_MAGIC {
        return Err(AppError::InvalidInput(
            "backup không phải file SQLite hợp lệ".into(),
        ));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > db::SCHEMA_VERSION {
        return Err(AppError::InvalidInput(format!(
            "backup có schema v{version} MỚI hơn app này (v{}) — cập nhật app trước khi restore",
            db::SCHEMA_VERSION
        )));
    }
    let has_profiles: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'profiles'",
        [],
        |r| r.get(0),
    )?;
    if has_profiles == 0 {
        return Err(AppError::InvalidInput(
            "backup thiếu bảng profiles — không phải DB BrowserX".into(),
        ));
    }
    Ok(())
}

/// Áp bản restore đã staging (nếu có) — gọi lúc STARTUP, TRƯỚC khi mở DB.
/// Trả `Some(bak)` khi đã swap (bak = DB cũ), `None` khi không có gì để áp.
/// Staged hỏng/không validate được → xoá staged, GIỮ NGUYÊN DB hiện tại.
/// Thứ tự crash-safe: -wal/-shm đổi tên trước (thuộc DB cũ), rồi DB cũ →
/// `.bak-cloud-restore-<ts>`, cuối cùng staged → `browserx.db`; kill giữa
/// chừng thì staged còn nguyên → lần startup sau áp tiếp.
pub fn apply_pending_restore(data_dir: &Path) -> Result<Option<PathBuf>> {
    let staged = staged_path(data_dir);
    if !staged.is_file() {
        return Ok(None);
    }
    if let Err(e) = validate_snapshot(&staged) {
        tracing::warn!("app db restore: staged file invalid — dropping it: {e}");
        let _ = fs::remove_file(&staged);
        return Ok(None);
    }
    let db_path = data_dir.join("browserx.db");
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let mut bak = None;
    if db_path.exists() {
        let bak_path = data_dir.join(format!("browserx.db.bak-cloud-restore-{ts}"));
        for ext in ["-wal", "-shm"] {
            let side = data_dir.join(format!("browserx.db{ext}"));
            if side.exists() {
                fs::rename(
                    &side,
                    data_dir.join(format!("browserx.db.bak-cloud-restore-{ts}{ext}")),
                )?;
            }
        }
        fs::rename(&db_path, &bak_path)?;
        bak = Some(bak_path);
    } else {
        // DB chính mất nhưng WAL/SHM mồ côi còn → dọn để không áp lên DB mới.
        for ext in ["-wal", "-shm"] {
            let side = data_dir.join(format!("browserx.db{ext}"));
            let _ = fs::remove_file(&side);
        }
    }
    fs::rename(&staged, &db_path)?;
    prune_restore_baks(data_dir);
    Ok(bak)
}

/// Tỉa bớt `browserx.db.bak-cloud-restore-*` (không đụng `-wal`/`-shm` lẻ vì
/// chúng mang cùng prefix timestamp — xoá theo bộ). Best-effort.
fn prune_restore_baks(data_dir: &Path) {
    let prefix = "browserx.db.bak-cloud-restore-";
    let Ok(entries) = fs::read_dir(data_dir) else { return };
    let mut stamps: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let rest = name.strip_prefix(prefix)?;
            (!rest.ends_with("-wal") && !rest.ends_with("-shm")).then(|| rest.to_string())
        })
        .collect();
    stamps.sort();
    while stamps.len() > MAX_RESTORE_BAKS {
        let ts = stamps.remove(0);
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(data_dir.join(format!("{prefix}{ts}{suffix}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmp_dir() -> (PathBuf, TempDir) {
        let dir = std::env::temp_dir().join(format!("bx-appdb-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let guard = TempDir(dir.clone());
        (dir, guard)
    }

    /// Snapshot VACUUM INTO tạo file SQLite đọc được, đủ dữ liệu, và pass
    /// validate — dù DB nguồn đang mở (WAL chưa checkpoint).
    #[test]
    fn vacuum_snapshot_is_readable_and_valid() {
        let (dir, _g) = tmp_dir();
        let db = Db::open_at_dir(&dir).unwrap();
        db.create_profile(crate::db::ProfileInput {
            name: "p1".into(),
            ..Default::default()
        })
        .unwrap();
        db.set_setting("some_key", "some_value").unwrap();

        let snap = dir.join("appdb-snapshot.db");
        db.vacuum_into(&snap).unwrap();
        validate_snapshot(&snap).unwrap();
        // Snapshot mở lại được như DB độc lập với đầy đủ dữ liệu.
        let snap_dir = dir.join("snap-open");
        fs::create_dir_all(&snap_dir).unwrap();
        fs::copy(&snap, snap_dir.join("browserx.db")).unwrap();
        let db2 = Db::open_at_dir(&snap_dir).unwrap();
        assert_eq!(db2.list_profiles().unwrap().len(), 1);
        assert_eq!(db2.get_setting("some_key").unwrap().unwrap(), "some_value");
        // Snapshot ghi đè được (file sót từ lần trước bị xoá).
        db.vacuum_into(&snap).unwrap();
    }

    /// Roundtrip mã hoá/giải mã snapshot qua pipeline `.bxa` sẵn có.
    #[test]
    fn snapshot_encrypt_decrypt_roundtrip() {
        crate::crypto::install_test_master_key();
        let (dir, _g) = tmp_dir();
        let db = Db::open_at_dir(&dir).unwrap();
        let snap = dir.join("appdb-snapshot.db");
        db.vacuum_into(&snap).unwrap();
        let plain = fs::read(&snap).unwrap();

        let bxa = bxa_path(&dir);
        let bytes = crate::archive::encrypt_bytes_to_file(&bxa, &plain).unwrap();
        assert!(bytes > 0 && bxa.is_file());
        // Ciphertext KHÔNG chứa magic SQLite plaintext.
        let raw = fs::read(&bxa).unwrap();
        assert!(!raw.windows(SQLITE_MAGIC.len()).any(|w| w == SQLITE_MAGIC));
        assert_eq!(crate::archive::decrypt_file_to_bytes(&bxa).unwrap(), plain);
    }

    /// validate_snapshot từ chối file rác / schema mới hơn / thiếu bảng profiles.
    #[test]
    fn validate_rejects_garbage_newer_schema_and_foreign_db() {
        let (dir, _g) = tmp_dir();
        let junk = dir.join("junk.db");
        fs::write(&junk, b"not a database at all").unwrap();
        assert!(matches!(
            validate_snapshot(&junk).unwrap_err(),
            AppError::InvalidInput(_)
        ));

        let db = Db::open_at_dir(&dir).unwrap();
        let snap = dir.join("snap.db");
        db.vacuum_into(&snap).unwrap();
        {
            let conn = Connection::open(&snap).unwrap();
            conn.pragma_update(None, "user_version", db::SCHEMA_VERSION + 1)
                .unwrap();
        }
        let err = validate_snapshot(&snap).unwrap_err();
        assert!(err.to_string().contains("MỚI hơn"));

        let foreign = dir.join("foreign.db");
        let conn = Connection::open(&foreign).unwrap();
        conn.execute_batch("CREATE TABLE x (id INTEGER)").unwrap();
        drop(conn);
        let err = validate_snapshot(&foreign).unwrap_err();
        assert!(err.to_string().contains("profiles"));
    }

    /// apply_pending_restore: swap staged vào chỗ, DB cũ + WAL/SHM giữ ở
    /// `.bak-cloud-restore-*`; không staged → no-op; staged hỏng → drop, DB giữ.
    #[test]
    fn apply_swaps_staged_and_keeps_bak() {
        let (dir, _g) = tmp_dir();
        assert_eq!(apply_pending_restore(&dir).unwrap(), None);

        // DB "hiện tại" + WAL/SHM giả lập.
        {
            let db = Db::open_at_dir(&dir).unwrap();
            db.set_setting("marker", "old").unwrap();
        }
        fs::write(dir.join("browserx.db-wal"), b"wal-bytes").unwrap();
        fs::write(dir.join("browserx.db-shm"), b"shm-bytes").unwrap();

        // Staged = snapshot DB khác với marker "new".
        let staged_src = dir.join("other");
        {
            let db2 = Db::open_at_dir(&staged_src).unwrap();
            db2.set_setting("marker", "new").unwrap();
            db2.vacuum_into(&staged_path(&dir)).unwrap();
        }

        let bak = apply_pending_restore(&dir).unwrap().expect("must swap");
        assert!(bak.is_file());
        assert!(!staged_path(&dir).exists());
        assert!(!dir.join("browserx.db-wal").exists(), "WAL cũ phải đi theo bak");
        assert!(!dir.join("browserx.db-shm").exists());
        let db = Db::open_at_dir(&dir).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().unwrap(), "new");
        drop(db);

        // Staged hỏng → drop staged, DB hiện tại nguyên vẹn.
        fs::write(staged_path(&dir), b"garbage").unwrap();
        assert_eq!(apply_pending_restore(&dir).unwrap(), None);
        assert!(!staged_path(&dir).exists());
        let db = Db::open_at_dir(&dir).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().unwrap(), "new");
    }

    /// Retention bak: chỉ giữ MAX_RESTORE_BAKS bản mới nhất (kèm wal/shm theo bộ).
    #[test]
    fn prune_keeps_only_newest_restore_baks() {
        let (dir, _g) = tmp_dir();
        for i in 1..=5 {
            let ts = format!("2026010{i}-000000");
            fs::write(dir.join(format!("browserx.db.bak-cloud-restore-{ts}")), b"x").unwrap();
            fs::write(
                dir.join(format!("browserx.db.bak-cloud-restore-{ts}-wal")),
                b"w",
            )
            .unwrap();
        }
        prune_restore_baks(&dir);
        let count = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with("browserx.db.bak-cloud-restore-") && !n.ends_with("-wal")
            })
            .count();
        assert_eq!(count, MAX_RESTORE_BAKS);
        assert!(!dir.join("browserx.db.bak-cloud-restore-20260101-000000").exists());
        assert!(!dir
            .join("browserx.db.bak-cloud-restore-20260101-000000-wal")
            .exists());
        assert!(dir.join("browserx.db.bak-cloud-restore-20260105-000000").exists());
    }
}
