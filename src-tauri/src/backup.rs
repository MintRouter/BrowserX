//! Backup/Restore mã hoá toàn bộ thư mục dữ liệu `~/.browserx` (W25a).
//!
//! Format file `.browserx-backup` (binary):
//! - `[0..8)`   magic `b"BXBACKUP"`
//! - `[8]`      version = 1
//! - `[9..25)`  salt Argon2 (16 byte, random mỗi backup)
//! - `[25..37)` nonce AES-256-GCM (12 byte, random mỗi backup)
//! - `[37..]`   ciphertext = AES-256-GCM( gzip( tar(data_dir) ) )
//!
//! KDF: Argon2id v19 params mặc định của crate (m=19 MiB, t=2, p=1) từ
//! passphrase user nhập — KHÔNG dùng OS keychain (máy chết là mất key).
//! Passphrase sai → GCM tag fail NGAY khi decrypt → `InvalidInput` rõ ràng,
//! KHÔNG đụng gì tới dữ liệu hiện có. Restore giải nén vào thư mục tạm CẠNH
//! `data_dir` (cùng filesystem) rồi swap bằng 2 lần `rename` atomic; dữ liệu
//! cũ giữ nguyên tại `<data_dir>.pre-restore-<timestamp>` để còn đường lùi.
//!
//! Caller (commands.rs) chịu trách nhiệm: WAL checkpoint TRƯỚC khi backup
//! (pattern W23a — file `-wal`/`-shm` bị bỏ qua khi nén) và bảo đảm mọi
//! profile đã DỪNG trước khi backup/restore.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::error::{AppError, Result};

/// Magic 8 byte đầu file — nhận diện file `.browserx-backup`.
pub const MAGIC: &[u8; 8] = b"BXBACKUP";
/// Version format hiện tại; restore chỉ chấp nhận đúng version này.
pub const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = MAGIC.len() + 1 + SALT_LEN + NONCE_LEN;

/// File cấp-1 bỏ qua khi nén: WAL/SHM đã được checkpoint trước backup
/// (DB nhất quán trong file chính) — copy chúng chỉ gây lệch snapshot.
const SKIP_TOP_FILES: &[&str] = &["browserx.db-wal", "browserx.db-shm"];

/// Callback tiến độ `(phase, pct 0..=100)` — commands.rs emit `backup://progress`.
pub type Progress<'a> = &'a (dyn Fn(&str, u8) + Send + Sync);

fn report(progress: Option<Progress<'_>>, phase: &str, pct: u8) {
    if let Some(p) = progress {
        p(phase, pct);
    }
}

/// Argon2id (params default crate) → khoá AES-256 32 byte.
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| AppError::Crypto(format!("Argon2 KDF failed: {e}")))?;
    Ok(key)
}

/// Liệt kê đệ quy mọi FILE thường trong `dir` kèm size (symlink bỏ qua —
/// chỉ là lock artifact runtime của Chromium, không phải dữ liệu).
fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, u64)>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            collect_files(base, &path, out)?;
        } else if ft.is_file() {
            if dir == base {
                let name = entry.file_name();
                if SKIP_TOP_FILES.iter().any(|s| name == *s) {
                    continue;
                }
            }
            out.push((path, entry.metadata()?.len()));
        }
    }
    Ok(())
}

/// Nén + mã hoá toàn bộ `data_dir` → file `dest`. Trả về số byte đã ghi.
pub fn create_backup(
    data_dir: &Path,
    dest: &Path,
    passphrase: &str,
    progress: Option<Progress<'_>>,
) -> Result<u64> {
    if passphrase.is_empty() {
        return Err(AppError::InvalidInput("passphrase must not be empty".into()));
    }
    if !data_dir.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "data directory not found: {}",
            data_dir.display()
        )));
    }

    report(progress, "compress", 0);
    let mut files = Vec::new();
    collect_files(data_dir, data_dir, &mut files)?;
    let total: u64 = files.iter().map(|(_, s)| *s).sum::<u64>().max(1);

    // tar → gzip vào buffer; pct 0..=70 dành cho pha nén (pha nặng nhất).
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = tar::Builder::new(&mut enc);
        let mut done: u64 = 0;
        for (path, size) in &files {
            let rel = path
                .strip_prefix(data_dir)
                .map_err(|e| AppError::InvalidInput(format!("bad path in backup: {e}")))?;
            tar.append_path_with_name(path, rel)?;
            done += size;
            report(progress, "compress", ((done * 70) / total) as u8);
        }
        tar.finish()?;
    }
    let plain = enc.finish()?;

    report(progress, "encrypt", 75);
    let salt: [u8; SALT_LEN] = rand::random();
    let nonce: [u8; NONCE_LEN] = rand::random();
    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Crypto(format!("AES key init failed: {e}")))?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|e| AppError::Crypto(format!("encryption failed: {e}")))?;

    report(progress, "write", 90);
    let mut out = fs::File::create(dest)?;
    out.write_all(MAGIC)?;
    out.write_all(&[VERSION])?;
    out.write_all(&salt)?;
    out.write_all(&nonce)?;
    out.write_all(&ciphertext)?;
    out.sync_all()?;

    report(progress, "done", 100);
    Ok((HEADER_LEN + ciphertext.len()) as u64)
}

/// Kết quả restore: dữ liệu cũ (nếu có) được giữ tại `previous_data_dir`.
#[derive(Debug)]
pub struct RestoreOutcome {
    pub previous_data_dir: Option<PathBuf>,
}

/// Giải mã + khôi phục `backup_file` vào `data_dir`.
///
/// Thứ tự AN TOÀN: verify header → decrypt (passphrase sai FAIL Ở ĐÂY, chưa
/// đụng dữ liệu) → unpack vào thư mục tạm cạnh `data_dir` → sanity check có
/// `browserx.db` → swap atomic (dữ liệu cũ đổi tên thành
/// `<data_dir>.pre-restore-<ts>`, lỗi swap thì rollback về chỗ cũ).
pub fn restore_backup(
    backup_file: &Path,
    data_dir: &Path,
    passphrase: &str,
    progress: Option<Progress<'_>>,
) -> Result<RestoreOutcome> {
    if passphrase.is_empty() {
        return Err(AppError::InvalidInput("passphrase must not be empty".into()));
    }
    let raw = fs::read(backup_file)?;
    if raw.len() < HEADER_LEN || &raw[..MAGIC.len()] != MAGIC {
        return Err(AppError::InvalidInput(
            "not a .browserx-backup file (bad header)".into(),
        ));
    }
    if raw[MAGIC.len()] != VERSION {
        return Err(AppError::InvalidInput(format!(
            "unsupported backup version {} (expected {VERSION})",
            raw[MAGIC.len()]
        )));
    }
    let salt = &raw[MAGIC.len() + 1..MAGIC.len() + 1 + SALT_LEN];
    let nonce = &raw[MAGIC.len() + 1 + SALT_LEN..HEADER_LEN];

    report(progress, "decrypt", 10);
    let key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Crypto(format!("AES key init failed: {e}")))?;
    // GCM tag fail = passphrase sai HOẶC file hỏng — cả hai đều dừng ở đây.
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce), &raw[HEADER_LEN..])
        .map_err(|_| {
            AppError::InvalidInput("wrong passphrase or corrupted backup file".into())
        })?;

    report(progress, "unpack", 40);
    let parent = data_dir.parent().ok_or_else(|| {
        AppError::InvalidInput(format!("data dir has no parent: {}", data_dir.display()))
    })?;
    fs::create_dir_all(parent)?;
    let dir_name = data_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".browserx");
    let tmp = parent.join(format!(".{dir_name}-restore-{}", uuid::Uuid::new_v4()));

    let unpacked: Result<()> = (|| {
        let mut ar = tar::Archive::new(GzDecoder::new(plain.as_slice()));
        ar.unpack(&tmp)?;
        if !tmp.join("browserx.db").is_file() {
            return Err(AppError::InvalidInput(
                "backup does not contain browserx.db — refusing to restore".into(),
            ));
        }
        Ok(())
    })();
    if let Err(e) = unpacked {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }

    report(progress, "swap", 90);
    let previous = if data_dir.exists() {
        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let bak = parent.join(format!("{dir_name}.pre-restore-{ts}"));
        fs::rename(data_dir, &bak)?;
        Some(bak)
    } else {
        None
    };
    if let Err(e) = fs::rename(&tmp, data_dir) {
        // Rollback: trả dữ liệu cũ về chỗ cũ, dọn thư mục tạm.
        if let Some(prev) = &previous {
            let _ = fs::rename(prev, data_dir);
        }
        let _ = fs::remove_dir_all(&tmp);
        return Err(e.into());
    }

    report(progress, "done", 100);
    Ok(RestoreOutcome {
        previous_data_dir: previous,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Dựng data dir giả: browserx.db + WAL/SHM (phải bị bỏ qua) + user_data_dirs.
    fn make_data_dir(root: &Path) -> PathBuf {
        let data = root.join(".browserx");
        fs::create_dir_all(data.join("profiles/p1/Default")).unwrap();
        fs::write(data.join("browserx.db"), b"sqlite-bytes").unwrap();
        fs::write(data.join("browserx.db-wal"), b"wal-bytes").unwrap();
        fs::write(data.join("browserx.db-shm"), b"shm-bytes").unwrap();
        fs::write(data.join("profiles/p1/Default/Cookies"), b"cookie-db").unwrap();
        fs::write(data.join("profiles/p1/Prefs.json"), b"{\"a\":1}").unwrap();
        data
    }

    fn temp_root() -> (PathBuf, TempDir) {
        let dir = std::env::temp_dir().join(format!("browserx-backup-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        (dir.clone(), TempDir(dir))
    }

    #[test]
    fn backup_restore_round_trip() {
        let (root, _guard) = temp_root();
        let data = make_data_dir(&root);
        let file = root.join("out.browserx-backup");

        let bytes = create_backup(&data, &file, "s3cret pass", None).unwrap();
        assert_eq!(bytes, file.metadata().unwrap().len());
        // File mã hoá: không được chứa plaintext, đúng magic + version.
        let raw = fs::read(&file).unwrap();
        assert_eq!(&raw[..8], MAGIC);
        assert_eq!(raw[8], VERSION);
        assert!(!raw.windows(9).any(|w| w == b"cookie-db"));

        // Restore vào chỗ mới (chưa tồn tại) → đủ file, WAL/SHM không mang theo.
        let target = root.join("restored/.browserx");
        let out = restore_backup(&file, &target, "s3cret pass", None).unwrap();
        assert!(out.previous_data_dir.is_none());
        assert_eq!(fs::read(target.join("browserx.db")).unwrap(), b"sqlite-bytes");
        assert_eq!(
            fs::read(target.join("profiles/p1/Default/Cookies")).unwrap(),
            b"cookie-db"
        );
        assert_eq!(
            fs::read(target.join("profiles/p1/Prefs.json")).unwrap(),
            b"{\"a\":1}"
        );
        assert!(!target.join("browserx.db-wal").exists());
        assert!(!target.join("browserx.db-shm").exists());

        // Restore ĐÈ lên dir đang có → dữ liệu cũ giữ ở pre-restore, mới thay chỗ.
        fs::write(target.join("sentinel.txt"), b"old-data").unwrap();
        let out2 = restore_backup(&file, &target, "s3cret pass", None).unwrap();
        let prev = out2.previous_data_dir.expect("old dir must be kept");
        assert_eq!(fs::read(prev.join("sentinel.txt")).unwrap(), b"old-data");
        assert!(!target.join("sentinel.txt").exists());
        assert_eq!(fs::read(target.join("browserx.db")).unwrap(), b"sqlite-bytes");
    }

    #[test]
    fn wrong_passphrase_fails_early_without_touching_data() {
        let (root, _guard) = temp_root();
        let data = make_data_dir(&root);
        let file = root.join("out.browserx-backup");
        create_backup(&data, &file, "correct horse", None).unwrap();

        let err = restore_backup(&file, &data, "wrong pass", None).unwrap_err();
        assert!(
            matches!(&err, AppError::InvalidInput(m) if m.contains("passphrase")),
            "unexpected error: {err}"
        );
        // Dữ liệu hiện có nguyên vẹn, không sinh pre-restore/thư mục tạm nào.
        assert_eq!(fs::read(data.join("browserx.db")).unwrap(), b"sqlite-bytes");
        let names: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.contains("pre-restore") && !n.contains("-restore-")),
            "leftover dirs: {names:?}"
        );
    }

    #[test]
    fn garbage_file_and_empty_passphrase_are_clear_errors() {
        let (root, _guard) = temp_root();
        let data = make_data_dir(&root);
        let garbage = root.join("garbage.browserx-backup");
        fs::write(&garbage, b"this is not a backup at all").unwrap();
        let err = restore_backup(&garbage, &data, "x", None).unwrap_err();
        assert!(matches!(&err, AppError::InvalidInput(m) if m.contains("bad header")));

        let err = create_backup(&data, &root.join("o.bin"), "", None).unwrap_err();
        assert!(matches!(&err, AppError::InvalidInput(m) if m.contains("passphrase")));
    }
}
