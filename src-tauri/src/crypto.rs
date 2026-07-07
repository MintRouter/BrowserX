//! Crypto: XChaCha20-Poly1305 (chacha20poly1305) mã hoá proxy credential + bản export/backup
//! at-rest; khoá gốc sinh ngẫu nhiên, lưu trong OS keychain (keyring).
//!
//! Layout ciphertext: `[nonce 24 byte][ciphertext + tag 16 byte]`.
//! Khoá gốc 32 byte: env `BROWSERX_MASTER_KEY` (base64, ưu tiên tuyệt đối —
//! CI/test/headless không đụng keychain) → keychain (service "browserx",
//! user "master-key", base64) → fallback file `master.key` trong app-data
//! (cảnh báo, không panic). `BROWSERX_KEYSTORE=file` ép bỏ qua keychain.

use std::path::PathBuf;
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

use crate::error::{AppError, Result};

/// Tên service trong OS keychain.
const KEYCHAIN_SERVICE: &str = "browserx";
/// Tên user (account) của entry khoá gốc.
const KEYCHAIN_USER: &str = "master-key";
/// Biến môi trường chứa khoá gốc base64 (ưu tiên trước keychain; CI/headless).
const MASTER_KEY_ENV: &str = "BROWSERX_MASTER_KEY";
/// Biến môi trường chọn keystore: `file` = bỏ qua keychain, dùng file fallback.
const KEYSTORE_ENV: &str = "BROWSERX_KEYSTORE";
/// Độ dài khoá gốc (XChaCha20-Poly1305 = 256-bit).
const KEY_LEN: usize = 32;
/// Độ dài nonce XChaCha20 (192-bit) prepend vào ciphertext.
const NONCE_LEN: usize = 24;

/// Cache khoá gốc trong process (Mutex để tránh race sinh 2 khoá khác nhau).
static MASTER_KEY: Mutex<Option<[u8; KEY_LEN]>> = Mutex::new(None);

/// Mã hoá một secret UTF-8 (vd. proxy password) → blob `[nonce][ct+tag]` lưu DB.
pub fn encrypt_secret(plaintext: &str) -> Result<Vec<u8>> {
    seal(plaintext.as_bytes())
}

/// Giải mã blob từ [`encrypt_secret`] → chuỗi UTF-8 gốc.
pub fn decrypt_secret(ciphertext: &[u8]) -> Result<String> {
    let plain = open(ciphertext)?;
    String::from_utf8(plain)
        .map_err(|_| AppError::Crypto("decrypted data is not valid UTF-8".into()))
}

/// Mã hoá bytes tuỳ ý bằng khoá gốc (dùng cho export/backup).
pub fn seal(plaintext: &[u8]) -> Result<Vec<u8>> {
    seal_with_key(&master_key()?, plaintext)
}

/// Giải mã blob từ [`seal`] bằng khoá gốc.
pub fn open(ciphertext: &[u8]) -> Result<Vec<u8>> {
    open_with_key(&master_key()?, ciphertext)
}

/// Hằng số key-check: seal lần đầu rồi lưu vào settings; các lần mở app sau
/// decrypt so khớp để phát hiện master key đã đổi (keychain mất/reset) —
/// xem `commands::master_key_status`.
const KEY_CHECK_VALUE: &[u8] = b"browserx-key-check-v1";

/// Tạo key-check blob mới bằng khoá gốc hiện tại (caller lưu vào settings).
pub fn new_key_check_blob() -> Result<Vec<u8>> {
    seal(KEY_CHECK_VALUE)
}

/// Blob key-check có giải mã đúng bằng khoá gốc hiện tại không.
/// `false` = master key đã đổi (hoặc blob hỏng) — credential cũ cần nhập lại.
pub fn key_check_matches(blob: &[u8]) -> bool {
    matches!(open(blob), Ok(v) if v == KEY_CHECK_VALUE)
}

/// Seal với khoá tường minh (tách riêng để unit-test không đụng keychain).
fn seal_with_key(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| AppError::Crypto(format!("invalid key length: {e}")))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::fill(&mut nonce_bytes[..]);
    let nonce = XNonce::from(nonce_bytes);
    let ct = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| AppError::Crypto("encryption failed".into()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Open với khoá tường minh. Sai khoá/nonce/tag → lỗi, không lộ plaintext.
fn open_with_key(key: &[u8; KEY_LEN], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() < NONCE_LEN + 16 {
        return Err(AppError::Crypto("ciphertext too short".into()));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| AppError::Crypto(format!("invalid key length: {e}")))?;
    let nonce = XNonce::try_from(&ciphertext[..NONCE_LEN])
        .map_err(|_| AppError::Crypto("invalid nonce".into()))?;
    cipher
        .decrypt(&nonce, &ciphertext[NONCE_LEN..])
        .map_err(|_| AppError::Crypto("decryption failed (wrong key or corrupted data)".into()))
}

/// (W51-B1) Khoá gốc làm INPUT KDF cho module khác (archive.rs derive
/// Argon2id + salt riêng — KHÔNG dùng trực tiếp làm khoá mã hoá).
pub(crate) fn master_key_material() -> Result<[u8; KEY_LEN]> {
    master_key()
}

/// Lấy khoá gốc (cache trong process). Env → keychain → file, không panic.
fn master_key() -> Result<[u8; KEY_LEN]> {
    let mut guard = MASTER_KEY
        .lock()
        .map_err(|_| AppError::Crypto("master key lock poisoned".into()))?;
    if let Some(k) = *guard {
        return Ok(k);
    }
    let k = load_master_key()?;
    *guard = Some(k);
    Ok(k)
}

/// Nạp/sinh khoá gốc: env `BROWSERX_MASTER_KEY` ưu tiên tuyệt đối (bỏ qua
/// keychain; sai định dạng → lỗi rõ, KHÔNG im lặng nhảy sang keychain);
/// `BROWSERX_KEYSTORE=file` ép dùng file fallback; còn lại keychain trước,
/// hỏng cấu trúc → lỗi rõ, không khả dụng → fallback file kèm cảnh báo.
fn load_master_key() -> Result<[u8; KEY_LEN]> {
    if let Ok(b64) = std::env::var(MASTER_KEY_ENV) {
        return decode_key_b64(b64.trim())
            .map_err(|e| AppError::Keychain(format!("invalid {MASTER_KEY_ENV}: {e}")));
    }
    if std::env::var(KEYSTORE_ENV).as_deref() == Ok("file") {
        tracing::warn!("{KEYSTORE_ENV}=file set; skipping OS keychain for master key");
        return key_from_fallback();
    }
    match key_from_keychain() {
        Ok(k) => Ok(k),
        Err(KeychainFailure::Corrupted(msg)) => Err(AppError::Keychain(msg)),
        Err(KeychainFailure::Unavailable(msg)) => {
            tracing::warn!("OS keychain unavailable ({msg}); falling back to env/file master key");
            key_from_fallback()
        }
    }
}

/// Phân loại lỗi keychain: entry hỏng (không được ghi đè) vs store không khả dụng.
enum KeychainFailure {
    Corrupted(String),
    Unavailable(String),
}

fn key_from_keychain() -> std::result::Result<[u8; KEY_LEN], KeychainFailure> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|e| KeychainFailure::Unavailable(e.to_string()))?;
    match entry.get_password() {
        Ok(b64) => decode_key_b64(&b64).map_err(|e| {
            KeychainFailure::Corrupted(format!(
                "master key entry in keychain is corrupted ({e}); refusing to overwrite"
            ))
        }),
        Err(keyring::Error::NoEntry) => {
            let key = generate_key();
            entry
                .set_password(&B64.encode(key))
                .map_err(|e| KeychainFailure::Unavailable(e.to_string()))?;
            Ok(key)
        }
        Err(e) => Err(KeychainFailure::Unavailable(e.to_string())),
    }
}

/// Fallback khi không có keychain (env đã được xử lý ở [`load_master_key`]):
/// file `<app-data>/browserx/master.key` (tự sinh nếu chưa có, chmod 600 trên unix).
fn key_from_fallback() -> Result<[u8; KEY_LEN]> {
    let path = fallback_key_path()?;
    if path.exists() {
        let b64 = std::fs::read_to_string(&path)?;
        return decode_key_b64(b64.trim()).map_err(|e| {
            AppError::Keychain(format!("invalid master key file {}: {e}", path.display()))
        });
    }
    let key = generate_key();
    write_key_file(&path, &B64.encode(key))?;
    tracing::warn!(
        "master key stored UNPROTECTED on disk at {} (keychain unavailable)",
        path.display()
    );
    Ok(key)
}

/// Đường dẫn file khoá fallback: `<data_dir>/browserx/master.key`
/// (hoặc `~/.browserx/master.key` nếu không xác định được data_dir).
fn fallback_key_path() -> Result<PathBuf> {
    let base = dirs::data_dir()
        .map(|d| d.join("browserx"))
        .or_else(|| dirs::home_dir().map(|h| h.join(".browserx")))
        .ok_or_else(|| {
            AppError::Keychain("cannot determine app data directory for fallback key".into())
        })?;
    Ok(base.join("master.key"))
}

/// Ghi file khoá với quyền 0600 trên unix.
fn write_key_file(path: &PathBuf, b64: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, b64)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Sinh khoá 32 byte từ CSPRNG hệ thống.
fn generate_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    rand::fill(&mut key[..]);
    key
}

/// Decode khoá base64, kiểm tra đúng 32 byte.
fn decode_key_b64(b64: &str) -> std::result::Result<[u8; KEY_LEN], String> {
    let bytes = B64.decode(b64).map_err(|e| format!("base64 decode: {e}"))?;
    <[u8; KEY_LEN]>::try_from(bytes.as_slice())
        .map_err(|_| format!("expected {KEY_LEN}-byte key, got {} bytes", bytes.len()))
}

/// Cài khoá test CỐ ĐỊNH (env + cache) cho unit test module khác (vd. `export`)
/// cần crypto mà không đụng OS keychain. Mọi caller cài đúng MỘT khoá nên an
/// toàn khi test chạy song song (cùng giá trị với `install_env_master_key`).
#[cfg(test)]
pub(crate) fn install_test_master_key() {
    let key = [42u8; KEY_LEN];
    std::env::set_var(MASTER_KEY_ENV, B64.encode(key));
    *MASTER_KEY.lock().unwrap() = Some(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(b: u8) -> [u8; KEY_LEN] {
        [b; KEY_LEN]
    }

    /// Cài khoá test qua env `BROWSERX_MASTER_KEY` đúng MỘT lần (env là
    /// process-global, test chạy song song) và nạp sẵn cache `MASTER_KEY` —
    /// bảo đảm không test nào rơi vào nhánh keychain thật, kể cả khi cache
    /// đã/chưa được nạp bởi test khác.
    fn install_env_master_key() -> [u8; KEY_LEN] {
        static KEY: std::sync::OnceLock<[u8; KEY_LEN]> = std::sync::OnceLock::new();
        *KEY.get_or_init(|| {
            let key = test_key(42);
            std::env::set_var(MASTER_KEY_ENV, B64.encode(key));
            *MASTER_KEY.lock().unwrap() = Some(key);
            key
        })
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = test_key(1);
        let msg = "proxy-p@ssw0rd:🔐";
        let blob = seal_with_key(&key, msg.as_bytes()).unwrap();
        assert!(blob.len() >= NONCE_LEN + 16 + msg.len());
        let plain = open_with_key(&key, &blob).unwrap();
        assert_eq!(plain, msg.as_bytes());
    }

    #[test]
    fn seal_uses_random_nonce_per_message() {
        let key = test_key(2);
        let a = seal_with_key(&key, b"same message").unwrap();
        let b = seal_with_key(&key, b"same message").unwrap();
        assert_ne!(a[..NONCE_LEN], b[..NONCE_LEN]);
        assert_ne!(a[NONCE_LEN..], b[NONCE_LEN..]);
    }

    #[test]
    fn open_with_wrong_key_fails() {
        let blob = seal_with_key(&test_key(3), b"top secret").unwrap();
        let err = open_with_key(&test_key(4), &blob).unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));
        assert!(!err.to_string().contains("top secret"));
    }

    #[test]
    fn open_with_tampered_nonce_fails() {
        let key = test_key(5);
        let mut blob = seal_with_key(&key, b"payload").unwrap();
        blob[0] ^= 0xff;
        assert!(open_with_key(&key, &blob).is_err());
    }

    #[test]
    fn open_with_tampered_ciphertext_fails() {
        let key = test_key(6);
        let mut blob = seal_with_key(&key, b"payload").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        assert!(open_with_key(&key, &blob).is_err());
    }

    #[test]
    fn open_rejects_too_short_input() {
        assert!(open_with_key(&test_key(7), &[0u8; NONCE_LEN + 15]).is_err());
        assert!(open_with_key(&test_key(7), &[]).is_err());
    }

    #[test]
    fn decode_key_b64_validates_length() {
        assert!(decode_key_b64(&B64.encode([9u8; KEY_LEN])).is_ok());
        assert!(decode_key_b64(&B64.encode([9u8; 16])).is_err());
        assert!(decode_key_b64("not base64 !!!").is_err());
    }

    #[test]
    fn load_master_key_prefers_env_over_keychain() {
        let key = install_env_master_key();
        assert_eq!(load_master_key().unwrap(), key);
    }

    #[test]
    fn encrypt_decrypt_secret_roundtrip_via_master_key() {
        install_env_master_key();
        let blob = encrypt_secret("user:pass@proxy").unwrap();
        assert_eq!(decrypt_secret(&blob).unwrap(), "user:pass@proxy");
    }

    #[test]
    fn decrypt_secret_rejects_non_utf8_plaintext() {
        install_env_master_key();
        let blob = seal(&[0xff, 0xfe, 0x00, 0x80]).unwrap();
        let err = decrypt_secret(&blob).unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));
    }

    #[test]
    fn key_check_blob_roundtrip_matches() {
        install_env_master_key();
        let blob = new_key_check_blob().unwrap();
        assert!(key_check_matches(&blob));
    }

    #[test]
    fn key_check_detects_key_change() {
        install_env_master_key();
        // Blob seal bằng khoá KHÁC (master key "cũ" trước khi keychain đổi)
        // → mismatch, không panic; blob rác cũng chỉ trả false.
        let old = seal_with_key(&test_key(9), KEY_CHECK_VALUE).unwrap();
        assert!(!key_check_matches(&old));
        assert!(!key_check_matches(b"garbage"));
    }
}
