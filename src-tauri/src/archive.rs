//! (W51-B1) Archive engine local: nén + mã hoá profile sau stop.
//!
//! Sau khi phiên dừng (và sanitize W49 chạy xong), nén `Default/` + `First Run`
//! của user_data_dir thành container mã hoá `profile-<id>.bxa` đặt CẠNH
//! user_data_dir. Archive là BACKUP — KHÔNG xoá run-dir. Retention local:
//! 1 bản, overwrite atomic (ghi `.bxa.tmp` → fsync → rename).
//!
//! Format file `.bxa` (binary):
//! - `[0..4)`  magic `b"BXA1"`
//! - `[4]`     version = 1
//! - `[5..21)` salt Argon2 (16 byte, random mỗi archive)
//! - `[21..]`  chuỗi chunk: `[ct_len u32 LE][nonce 12][ciphertext + tag 16]`
//!
//! Plaintext = zip(`Default/` + `First Run`), mã hoá AES-256-GCM theo chunk
//! 64 KiB. AAD mỗi chunk = index u64 LE + 1 byte cờ "chunk cuối" — chặn cả
//! reorder lẫn truncate. Khoá derive Argon2id từ MASTER KEY trong keystore
//! (crypto.rs — cùng hạ tầng mã hoá proxy credential), KHÔNG cần passphrase.
//!
//! (W52-A1) Cache dirs Chromium tự tạo lại được ([`storage::CACHE_DIRS`] —
//! cùng nguồn với sanitize W49) bị LOẠI khỏi zip: giảm mạnh kích thước `.bxa`
//! mà không mất dữ liệu phiên (Cookies/Local Storage/IndexedDB/Login Data…
//! không nằm trong danh sách đó — có guard test cả 2 phía).
//!
//! Dirty-check fast-skip: không file dữ liệu chính nào (Cookies/Login Data/
//! History/Bookmarks/Web Data) mới hơn archive VÀ archive <30 ngày → skip nén.
//! Caller (commands.rs) chạy archive BEST-EFFORT trong background sau stop,
//! giới hạn [`acquire_slot`] tối đa 2 archive song song.

use std::fs;
use std::io::{BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use tokio::sync::{Semaphore, SemaphorePermit};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::crypto;
use crate::error::{AppError, Result};
use crate::storage;

/// Magic 4 byte đầu file — nhận diện container `.bxa`.
pub const MAGIC: &[u8; 4] = b"BXA1";
/// Version format hiện tại; restore chỉ chấp nhận đúng version này.
pub const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = MAGIC.len() + 1 + SALT_LEN;
/// Kích thước chunk plaintext khi mã hoá GCM.
const CHUNK_SIZE: usize = 64 * 1024;
/// Archive mới hơn mốc này (30 ngày) mới đủ điều kiện fast-skip.
const FRESH_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// File marker "First Run" ở root user_data_dir — nén kèm Default/.
const FIRST_RUN: &str = "First Run";
/// File dữ liệu chính để dirty-check mtime (cả 2 layout Cookies ≤/≥ M91).
const DIRTY_CHECK_FILES: &[&str] = &[
    "Default/Cookies",
    "Default/Network/Cookies",
    "Default/Login Data",
    "Default/History",
    "Default/Bookmarks",
    "Default/Web Data",
];

/// Tối đa 2 archive chạy song song (nén + mã hoá là CPU/IO nặng).
static ARCHIVE_SLOTS: Semaphore = Semaphore::const_new(2);

/// Chờ slot archive (max 2 song song) — giữ permit đến khi archive xong.
pub async fn acquire_slot() -> SemaphorePermit<'static> {
    ARCHIVE_SLOTS
        .acquire()
        .await
        .expect("archive semaphore never closed")
}

/// Kết quả một lần archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveOutcome {
    /// Đã ghi archive mới — `bytes` = kích thước file `.bxa`.
    Written { bytes: u64 },
    /// Fast-skip: dữ liệu không đổi kể từ archive hiện có (<30 ngày).
    SkippedClean,
    /// `Default/` không tồn tại (profile chưa từng launch) — không có gì để nén.
    SkippedNoData,
}

/// Đường dẫn archive `profile-<id>.bxa` CẠNH user_data_dir (cùng thư mục cha).
/// `None` nếu user_data_dir không có parent.
pub fn archive_path(user_data_dir: &Path, profile_id: &str) -> Option<PathBuf> {
    user_data_dir
        .parent()
        .map(|p| p.join(format!("profile-{profile_id}.bxa")))
}

/// Nén `Default/` + `First Run` của `user_data_dir` → container mã hoá
/// `profile-<id>.bxa` cạnh user_data_dir. Run-dir KHÔNG bị đụng tới.
/// Dirty-check fast-skip trước khi nén (xem doc module).
pub fn archive_profile(user_data_dir: &Path, profile_id: &str) -> Result<ArchiveOutcome> {
    archive_profile_inner(user_data_dir, profile_id, false)
}

/// (W52-B C5) Như [`archive_profile`] nhưng BỎ dirty-check fast-skip — dùng
/// cho "Sync now": user chủ động yêu cầu snapshot mới nên luôn nén lại
/// (vẫn skip khi không có `Default/`).
pub fn archive_profile_forced(user_data_dir: &Path, profile_id: &str) -> Result<ArchiveOutcome> {
    archive_profile_inner(user_data_dir, profile_id, true)
}

fn archive_profile_inner(
    user_data_dir: &Path,
    profile_id: &str,
    force: bool,
) -> Result<ArchiveOutcome> {
    if !user_data_dir.join("Default").is_dir() {
        return Ok(ArchiveOutcome::SkippedNoData);
    }
    let dest = archive_path(user_data_dir, profile_id).ok_or_else(|| {
        AppError::InvalidInput(format!(
            "user_data_dir has no parent: {}",
            user_data_dir.display()
        ))
    })?;
    if !force && archive_is_fresh(user_data_dir, &dest) {
        return Ok(ArchiveOutcome::SkippedClean);
    }
    let master = crypto::master_key_material()?;
    let plain = build_zip(user_data_dir)?;
    let bytes = write_encrypted(&master, &dest, &plain)?;
    Ok(ArchiveOutcome::Written { bytes })
}

/// (W55c) Mã hoá bytes bất kỳ thành container `.bxa` tại `dest` — cùng
/// pipeline AES-256-GCM chunked + Argon2id từ master key như archive profile
/// (KHÔNG zip: caller tự quyết plaintext, ví dụ snapshot SQLite app DB).
/// Ghi atomic `.tmp` → fsync → rename. Trả kích thước file.
pub fn encrypt_bytes_to_file(dest: &Path, plain: &[u8]) -> Result<u64> {
    let master = crypto::master_key_material()?;
    write_encrypted(&master, dest, plain)
}

/// (W55c) Giải mã container `.bxa` tại `src` → plaintext bytes (verify GCM
/// từng chunk — sai khoá/tamper/truncate → lỗi, không trả plaintext một phần).
pub fn decrypt_file_to_bytes(src: &Path) -> Result<Vec<u8>> {
    let master = crypto::master_key_material()?;
    read_decrypted(&master, src)
}

/// Giải mã + verify (GCM auth từng chunk) + giải nén archive vào
/// `user_data_dir`. Decrypt fail (sai khoá/tamper) → lỗi TRƯỚC khi đụng đĩa.
/// Caller chịu trách nhiệm chỉ gọi khi run-dir thiếu/hỏng.
pub fn restore_archive(user_data_dir: &Path, profile_id: &str) -> Result<()> {
    let src = archive_path(user_data_dir, profile_id)
        .filter(|p| p.is_file())
        .ok_or_else(|| AppError::NotFound(format!("no local archive for profile {profile_id}")))?;
    let master = crypto::master_key_material()?;
    let plain = read_decrypted(&master, &src)?;
    unzip_into(&plain, user_data_dir)
}

/// Fast-skip được khi: archive tồn tại, <30 ngày tuổi, và KHÔNG file dữ liệu
/// chính nào mới hơn archive. Mọi lỗi đọc metadata → coi là "không fresh"
/// (an toàn: thà nén thừa còn hơn mất dữ liệu).
fn archive_is_fresh(user_data_dir: &Path, archive: &Path) -> bool {
    let Ok(archived_at) = fs::metadata(archive).and_then(|m| m.modified()) else {
        return false;
    };
    match SystemTime::now().duration_since(archived_at) {
        Ok(age) if age < FRESH_MAX_AGE => {}
        _ => return false,
    }
    !DIRTY_CHECK_FILES.iter().any(|rel| {
        fs::metadata(user_data_dir.join(rel))
            .and_then(|m| m.modified())
            .map(|mtime| mtime > archived_at)
            .unwrap_or(false)
    })
}

/// Argon2id (params default crate — GIỐNG backup.rs) → khoá AES-256 32 byte
/// từ master key + salt per-archive.
fn derive_key(master: &[u8], salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(master, salt, &mut key)
        .map_err(|e| AppError::Crypto(format!("Argon2 KDF failed: {e}")))?;
    Ok(key)
}

/// AAD của chunk `index`: index u64 LE + cờ `last` (1 byte) — GCM auth luôn
/// cả vị trí lẫn "đây có phải chunk cuối không" → chặn reorder + truncate.
fn chunk_aad(index: u64, last: bool) -> [u8; 9] {
    let mut aad = [0u8; 9];
    aad[..8].copy_from_slice(&index.to_le_bytes());
    aad[8] = last as u8;
    aad
}

/// Zip `Default/` (đệ quy) + file `First Run` của user_data_dir vào memory.
/// Symlink bỏ qua (chỉ là lock artifact runtime của Chromium — như backup.rs).
fn build_zip(user_data_dir: &Path) -> Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    zip_dir(&mut zip, user_data_dir, &user_data_dir.join("Default"), opts)?;
    let first_run = user_data_dir.join(FIRST_RUN);
    if first_run.is_file() {
        zip.start_file(FIRST_RUN, opts)
            .map_err(zip_err("zip First Run"))?;
        zip.write_all(&fs::read(&first_run)?)?;
    }
    let cursor = zip.finish().map_err(zip_err("zip finish"))?;
    Ok(cursor.into_inner())
}

/// (W52-A1) `rel` (path `/`-separated, tương đối user_data_dir) có bị LOẠI
/// khỏi archive không. Nguồn duy nhất = [`storage::CACHE_DIRS`] (sanitize W49):
/// đã an toàn để XOÁ khỏi run-dir thì chắc chắn an toàn để không backup —
/// Chromium tự tạo lại. Match theo ranh giới path: entry `Default/Cache` loại
/// cả cây con nhưng KHÔNG đụng `Default/CacheX`; entry file như
/// `Default/Network/Network Persistent State` match đúng file, GIỮ nguyên
/// `Default/Network/Cookies` bên cạnh.
fn is_archive_excluded(rel: &str) -> bool {
    storage::CACHE_DIRS.iter().any(|ex| {
        rel == *ex
            || rel
                .strip_prefix(ex)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Thêm đệ quy mọi FILE thường trong `dir` vào zip, tên entry là đường dẫn
/// tương đối so với `base` (dùng `/` — chuẩn zip, cross-platform khi restore).
/// Entry cache ([`is_archive_excluded`]) bị bỏ qua — cây dir loại trừ không
/// được đệ quy vào (khỏi tốn IO đọc hàng nghìn file cache).
fn zip_dir(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    base: &Path,
    dir: &Path,
    opts: SimpleFileOptions,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .map_err(|e| AppError::Crypto(format!("zip path outside base: {e}")))?;
        let name = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if is_archive_excluded(&name) {
            continue;
        }
        if ft.is_dir() {
            zip_dir(zip, base, &path, opts)?;
        } else if ft.is_file() {
            zip.start_file(&name, opts).map_err(zip_err("zip entry"))?;
            zip.write_all(&fs::read(&path)?)?;
        }
    }
    Ok(())
}

/// Map ZipError → AppError::Crypto kèm ngữ cảnh.
fn zip_err(ctx: &str) -> impl Fn(zip::result::ZipError) -> AppError + '_ {
    move |e| AppError::Crypto(format!("{ctx}: {e}"))
}

/// Mã hoá `plain` theo chunk → ghi `.bxa.tmp` → fsync → atomic rename `dest`.
/// Trả kích thước file `.bxa`.
fn write_encrypted(master: &[u8], dest: &Path, plain: &[u8]) -> Result<u64> {
    let salt: [u8; SALT_LEN] = rand::random();
    let key = derive_key(master, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Crypto(format!("AES key init failed: {e}")))?;

    let tmp = tmp_path(dest);
    let result = (|| -> Result<u64> {
        let file = fs::File::create(&tmp)?;
        let mut out = BufWriter::new(file);
        out.write_all(MAGIC)?;
        out.write_all(&[VERSION])?;
        out.write_all(&salt)?;
        let mut written = HEADER_LEN as u64;

        // Zip không bao giờ rỗng (luôn có end-of-central-directory) → ≥1 chunk.
        let n_chunks = plain.len().div_ceil(CHUNK_SIZE);
        for (i, chunk) in plain.chunks(CHUNK_SIZE).enumerate() {
            let nonce_bytes: [u8; NONCE_LEN] = rand::random();
            let nonce = Nonce::from_slice(&nonce_bytes);
            let aad = chunk_aad(i as u64, i + 1 == n_chunks);
            let ct = cipher
                .encrypt(nonce, Payload { msg: chunk, aad: &aad })
                .map_err(|e| AppError::Crypto(format!("chunk encryption failed: {e}")))?;
            out.write_all(&(ct.len() as u32).to_le_bytes())?;
            out.write_all(&nonce_bytes)?;
            out.write_all(&ct)?;
            written += (4 + NONCE_LEN + ct.len()) as u64;
        }
        let file = out
            .into_inner()
            .map_err(|e| AppError::Crypto(format!("flush archive: {e}")))?;
        file.sync_all()?;
        Ok(written)
    })();

    match result {
        Ok(bytes) => {
            fs::rename(&tmp, dest)?;
            Ok(bytes)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Đường dẫn tạm `.bxa.tmp` cạnh `dest` (cùng filesystem → rename atomic).
fn tmp_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    dest.with_file_name(name)
}

/// Đọc + giải mã toàn bộ container `.bxa` → plaintext (zip). Sai magic/version/
/// khoá/tamper/truncate → lỗi, KHÔNG trả plaintext một phần.
fn read_decrypted(master: &[u8], src: &Path) -> Result<Vec<u8>> {
    let raw = fs::read(src)?;
    if raw.len() < HEADER_LEN || &raw[..MAGIC.len()] != MAGIC {
        return Err(AppError::InvalidInput("not a BrowserX archive file".into()));
    }
    if raw[MAGIC.len()] != VERSION {
        return Err(AppError::InvalidInput(format!(
            "unsupported archive version {} (expected {VERSION})",
            raw[MAGIC.len()]
        )));
    }
    let salt = &raw[MAGIC.len() + 1..HEADER_LEN];
    let key = derive_key(master, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| AppError::Crypto(format!("AES key init failed: {e}")))?;

    let body = &raw[HEADER_LEN..];
    // Đếm chunk trước (cần biết chunk cuối để dựng AAD đúng cờ `last`).
    let mut offsets = Vec::new();
    let mut pos = 0usize;
    while pos < body.len() {
        if body.len() - pos < 4 + NONCE_LEN + TAG_LEN {
            return Err(AppError::Crypto("archive truncated".into()));
        }
        let ct_len = u32::from_le_bytes(body[pos..pos + 4].try_into().unwrap()) as usize;
        let start = pos + 4 + NONCE_LEN;
        let end = start
            .checked_add(ct_len)
            .filter(|&e| e <= body.len() && ct_len >= TAG_LEN)
            .ok_or_else(|| AppError::Crypto("archive truncated".into()))?;
        offsets.push((pos + 4, start, end));
        pos = end;
    }
    if offsets.is_empty() {
        return Err(AppError::Crypto("archive has no chunks".into()));
    }

    let mut plain = Vec::new();
    let last_idx = offsets.len() - 1;
    for (i, (nonce_at, ct_start, ct_end)) in offsets.into_iter().enumerate() {
        let nonce = Nonce::from_slice(&body[nonce_at..nonce_at + NONCE_LEN]);
        let aad = chunk_aad(i as u64, i == last_idx);
        let msg = &body[ct_start..ct_end];
        let mut chunk = cipher
            .decrypt(nonce, Payload { msg, aad: &aad })
            .map_err(|_| {
                AppError::Crypto(
                    "archive decryption failed (wrong master key or corrupted archive)".into(),
                )
            })?;
        plain.append(&mut chunk);
    }
    Ok(plain)
}

/// Giải nén zip plaintext vào `user_data_dir`. Chặn path traversal bằng
/// `enclosed_name()`; zip không chứa entry `Default/` nào → archive hỏng.
fn unzip_into(plain: &[u8], user_data_dir: &Path) -> Result<()> {
    let mut zip = ZipArchive::new(Cursor::new(plain)).map_err(zip_err("open zip"))?;
    if !zip.file_names().any(|n| n.starts_with("Default/")) {
        return Err(AppError::Crypto("archive contains no Default/ data".into()));
    }
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_err("zip entry"))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(AppError::Crypto(format!(
                "unsafe path in archive: {}",
                entry.name()
            )));
        };
        let dest = user_data_dir.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("browserx-archive-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, content: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
    }

    /// user_data_dir giả kiểu Chromium với dữ liệu session cần backup.
    fn fake_profile(root: &Path) {
        write(root, "First Run", b"");
        write(root, "Default/Cookies", b"cookie-db-contents");
        write(root, "Default/Network/Cookies", b"cookie-db-m91");
        write(root, "Default/Login Data", b"login-db");
        write(root, "Default/History", b"history-db");
        write(root, "Default/Bookmarks", b"{\"roots\":{}}");
        write(root, "Default/Web Data", b"webdata-db");
        write(root, "Default/Local Storage/leveldb/000003.log", b"ls-log");
        // File KHÔNG nén được (xorshift PRNG, deterministic) 200 KiB → zip
        // plaintext >64 KiB → phủ đường đi multi-chunk khi mã hoá/giải mã.
        let mut s: u64 = 0x2545F4914F6CDD1D;
        let big: Vec<u8> = (0..200 * 1024)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s as u8
            })
            .collect();
        write(root, "Default/IndexedDB/big.sqlite", &big);
    }

    fn set_mtime(path: &Path, t: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn archive_restore_roundtrip() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);

        let outcome = archive_profile(&udd, "p1").unwrap();
        assert!(matches!(outcome, ArchiveOutcome::Written { bytes } if bytes > HEADER_LEN as u64));

        let bxa = archive_path(&udd, "p1").unwrap();
        assert!(bxa.is_file());
        assert!(!tmp_path(&bxa).exists());
        let raw = fs::read(&bxa).unwrap();
        assert_eq!(&raw[..4], MAGIC);
        assert_eq!(raw[4], VERSION);
        // Nội dung nhạy cảm KHÔNG được xuất hiện plaintext trong container.
        assert!(!raw
            .windows(b"cookie-db-contents".len())
            .any(|w| w == b"cookie-db-contents"));
        // Run-dir không bị đụng (archive là backup).
        assert!(udd.join("Default/Cookies").is_file());

        // Restore vào user_data_dir mới (cùng thư mục cha → cùng archive).
        let restored = base.join("restored");
        fs::rename(&bxa, archive_path(&restored, "p1").unwrap()).unwrap();
        restore_archive(&restored, "p1").unwrap();
        for rel in [
            "First Run",
            "Default/Cookies",
            "Default/Network/Cookies",
            "Default/Local Storage/leveldb/000003.log",
        ] {
            assert_eq!(
                fs::read(restored.join(rel)).unwrap(),
                fs::read(udd.join(rel)).unwrap(),
                "mismatch: {rel}"
            );
        }
        assert_eq!(
            fs::read(restored.join("Default/IndexedDB/big.sqlite")).unwrap(),
            fs::read(udd.join("Default/IndexedDB/big.sqlite")).unwrap()
        );
        fs::remove_dir_all(&base).unwrap();
    }

    /// (W52-E1) Recovery key roundtrip "máy mới": .bxa mã hoá bằng key A trên
    /// máy cũ; máy mới CHỈ có chuỗi recovery key → parse → giải mã archive
    /// thành công (không đụng cache/keychain process-global).
    #[test]
    fn recovery_key_decrypts_bxa_from_another_machine() {
        let base = tmp_root();
        let old_machine_key: [u8; 32] = [77u8; 32];
        let plain = b"zip-payload-stand-in".to_vec();
        let bxa = base.join("profile-px.bxa");
        write_encrypted(&old_machine_key, &bxa, &plain).unwrap();

        let recovery = crate::crypto::encode_recovery_key_material(&old_machine_key);
        let recovered = crate::crypto::parse_recovery_key(&recovery).unwrap();
        assert_eq!(recovered, old_machine_key);
        assert_eq!(read_decrypted(&recovered, &bxa).unwrap(), plain);

        // Key khác (máy mới chưa import) → KHÔNG giải mã được.
        let other_key: [u8; 32] = [78u8; 32];
        assert!(read_decrypted(&other_key, &bxa).is_err());
        fs::remove_dir_all(&base).unwrap();
    }

    /// (W52-A1) Cache dirs bị loại khỏi archive; dữ liệu phiên GIỮ nguyên.
    #[test]
    fn archive_excludes_cache_dirs_keeps_session_data() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);
        // Cache Chromium tự tạo lại — KHÔNG được vào archive.
        write(&udd, "Default/Cache/Cache_Data/data_1", &[b'c'; 4096]);
        write(&udd, "Default/Code Cache/js/index", b"jscache");
        write(&udd, "Default/GPUCache/data_0", b"gpucache");
        write(&udd, "Default/Service Worker/CacheStorage/x/1_0", b"sw-cache");
        write(&udd, "Default/Service Worker/ScriptCache/index", b"sw-script");
        write(&udd, "Default/Network/Network Persistent State", b"nps");
        // Session data cạnh cache — PHẢI giữ (kể cả trong Service Worker/,
        // Network/ nơi có entry cache bị loại ngay bên cạnh).
        write(&udd, "Default/Service Worker/Database/000003.log", b"sw-reg");
        // Bẫy prefix: tên CHỈ GIỐNG cache dir ở phần đầu — phải giữ.
        write(&udd, "Default/CacheX/keep.txt", b"not-a-cache-dir");

        archive_profile(&udd, "p9").unwrap();
        let restored = base.join("restored");
        fs::rename(
            archive_path(&udd, "p9").unwrap(),
            archive_path(&restored, "p9").unwrap(),
        )
        .unwrap();
        restore_archive(&restored, "p9").unwrap();

        for gone in [
            "Default/Cache",
            "Default/Code Cache",
            "Default/GPUCache",
            "Default/Service Worker/CacheStorage",
            "Default/Service Worker/ScriptCache",
            "Default/Network/Network Persistent State",
        ] {
            assert!(!restored.join(gone).exists(), "cache lọt vào archive: {gone}");
        }
        for kept in [
            "First Run",
            "Default/Cookies",
            "Default/Network/Cookies",
            "Default/Login Data",
            "Default/Local Storage/leveldb/000003.log",
            "Default/IndexedDB/big.sqlite",
            "Default/Service Worker/Database/000003.log",
            "Default/CacheX/keep.txt",
        ] {
            assert_eq!(
                fs::read(restored.join(kept)).unwrap(),
                fs::read(udd.join(kept)).unwrap(),
                "mất dữ liệu phiên: {kept}"
            );
        }
        // Run-dir không bị đụng — cache vẫn nằm nguyên trên đĩa.
        assert!(udd.join("Default/Cache/Cache_Data/data_1").is_file());
        fs::remove_dir_all(&base).unwrap();
    }

    /// (W52-A1) Guard: matcher loại trừ không bao giờ đụng path session,
    /// và match đúng ranh giới path (không nuốt prefix).
    #[test]
    fn archive_exclusion_never_covers_session_paths() {
        for kept in [
            "First Run",
            "Default/Cookies",
            "Default/Network/Cookies",
            "Default/Network/TransportSecurity",
            "Default/Login Data",
            "Default/History",
            "Default/Bookmarks",
            "Default/Web Data",
            "Default/Preferences",
            "Default/Local Storage/leveldb/000003.log",
            "Default/Session Storage/000003.log",
            "Default/IndexedDB/https_example.com_0/1.sqlite",
            "Default/Extensions/abcdef/1.0/manifest.json",
            "Default/Service Worker/Database/000003.log",
            "Default/CacheX/keep.txt",
        ] {
            assert!(!is_archive_excluded(kept), "loại nhầm session path: {kept}");
        }
        for gone in [
            "Default/Cache",
            "Default/Cache/Cache_Data/data_1",
            "Default/Code Cache/js/index",
            "Default/GPUCache/data_0",
            "Default/Service Worker/CacheStorage/x/1_0",
            "Default/Service Worker/ScriptCache/index",
            "Default/Network/Network Persistent State",
        ] {
            assert!(is_archive_excluded(gone), "cache không bị loại: {gone}");
        }
    }

    #[test]
    fn dirty_check_skips_then_rearchives() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);

        assert!(matches!(
            archive_profile(&udd, "p2").unwrap(),
            ArchiveOutcome::Written { .. }
        ));
        let bxa = archive_path(&udd, "p2").unwrap();
        // Archive vừa ghi → mtime ≥ mọi file dữ liệu → lần 2 skip.
        assert_eq!(
            archive_profile(&udd, "p2").unwrap(),
            ArchiveOutcome::SkippedClean
        );
        // (W52-B C5) Forced bỏ dirty-check: nén lại dù dữ liệu không đổi.
        assert!(matches!(
            archive_profile_forced(&udd, "p2").unwrap(),
            ArchiveOutcome::Written { .. }
        ));

        // Cookies mới hơn archive → dirty → nén lại.
        set_mtime(
            &udd.join("Default/Cookies"),
            SystemTime::now() + Duration::from_secs(5),
        );
        assert!(matches!(
            archive_profile(&udd, "p2").unwrap(),
            ArchiveOutcome::Written { .. }
        ));

        // Archive >30 ngày tuổi → hết fresh dù dữ liệu không đổi.
        let old = SystemTime::now() - FRESH_MAX_AGE - Duration::from_secs(60);
        set_mtime(&bxa, old);
        for rel in DIRTY_CHECK_FILES {
            set_mtime(&udd.join(rel), old - Duration::from_secs(60));
        }
        assert!(matches!(
            archive_profile(&udd, "p2").unwrap(),
            ArchiveOutcome::Written { .. }
        ));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archive_skips_when_no_default_dir() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("never-launched");
        fs::create_dir_all(&udd).unwrap();
        assert_eq!(
            archive_profile(&udd, "p3").unwrap(),
            ArchiveOutcome::SkippedNoData
        );
        assert!(!archive_path(&udd, "p3").unwrap().exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn restore_without_archive_is_not_found() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let err = restore_archive(&base.join("run"), "p4").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn wrong_key_fails_cleanly_without_touching_disk() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);
        archive_profile(&udd, "p5").unwrap();

        let restored = base.join("restored");
        fs::rename(
            archive_path(&udd, "p5").unwrap(),
            archive_path(&restored, "p5").unwrap(),
        )
        .unwrap();
        // Khoá SAI: decrypt trực tiếp bằng master khác → lỗi Crypto rõ ràng,
        // và unzip/restore chưa hề tạo run-dir.
        let src = archive_path(&restored, "p5").unwrap();
        let err = read_decrypted(&[7u8; 32], &src).unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));
        assert!(!err.to_string().contains("cookie"));
        assert!(!restored.exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn tampered_byte_fails_gcm_auth() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);
        archive_profile(&udd, "p6").unwrap();

        let bxa = archive_path(&udd, "p6").unwrap();
        let mut raw = fs::read(&bxa).unwrap();
        // Lật 1 byte giữa vùng ciphertext (sau header + ct_len + nonce).
        let idx = HEADER_LEN + 4 + NONCE_LEN + 10;
        raw[idx] ^= 0xff;
        fs::write(&bxa, &raw).unwrap();

        let restored = base.join("restored");
        fs::rename(&bxa, archive_path(&restored, "p6").unwrap()).unwrap();
        let err = restore_archive(&restored, "p6").unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));
        assert!(!restored.exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn truncated_archive_fails() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);
        archive_profile(&udd, "p7").unwrap();

        let bxa = archive_path(&udd, "p7").unwrap();
        let raw = fs::read(&bxa).unwrap();
        // Cắt đúng ranh giới chunk cuối: chunk trước đó thành "chunk cuối"
        // nhưng AAD cờ last không khớp → GCM fail (chặn truncation attack).
        let mut pos = HEADER_LEN;
        let mut boundaries = Vec::new();
        while pos < raw.len() {
            let ct_len =
                u32::from_le_bytes(raw[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4 + NONCE_LEN + ct_len;
            boundaries.push(pos);
        }
        assert!(boundaries.len() >= 2, "cần ≥2 chunk để test truncate");
        let cut = boundaries[boundaries.len() - 2];
        fs::write(&bxa, &raw[..cut]).unwrap();

        let restored = base.join("restored");
        fs::rename(&bxa, archive_path(&restored, "p7").unwrap()).unwrap();
        let err = restore_archive(&restored, "p7").unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn rejects_bad_magic_and_version() {
        crate::crypto::install_test_master_key();
        let base = tmp_root();
        let udd = base.join("run");
        fake_profile(&udd);
        archive_profile(&udd, "p8").unwrap();
        let bxa = archive_path(&udd, "p8").unwrap();
        let good = fs::read(&bxa).unwrap();

        let mut bad_magic = good.clone();
        bad_magic[0] = b'Z';
        fs::write(&bxa, &bad_magic).unwrap();
        assert!(matches!(
            restore_archive(&udd, "p8").unwrap_err(),
            AppError::InvalidInput(_)
        ));

        let mut bad_ver = good;
        bad_ver[4] = 99;
        fs::write(&bxa, &bad_ver).unwrap();
        assert!(matches!(
            restore_archive(&udd, "p8").unwrap_err(),
            AppError::InvalidInput(_)
        ));
        fs::remove_dir_all(&base).unwrap();
    }
}
