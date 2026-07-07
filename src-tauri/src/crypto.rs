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

// ---------------------------------------------------------------------------
// (W52-E1) Recovery Key — export/import master key material để khôi phục
// backup .bxa trên máy mới. Chuỗi = `BXRK1-` + Crockford base32 của
// `[key 32B][checksum 4B]` (checksum = SHA-256(domain ‖ key) cắt 4 byte).
// KHÔNG log, KHÔNG lưu, KHÔNG gửi mạng — hiển thị đúng MỘT lần cho user.
// ---------------------------------------------------------------------------

/// Prefix + version của chuỗi recovery key.
const RECOVERY_PREFIX: &str = "BXRK1";
/// Domain-separation cho checksum (đổi format → đổi domain + prefix).
const RECOVERY_CHECKSUM_DOMAIN: &[u8] = b"browserx-recovery-v1";
/// Số byte checksum SHA-256 (cắt) gắn sau key — bắt lỗi gõ tay/paste thiếu.
const RECOVERY_CHECKSUM_LEN: usize = 4;
/// Số ký tự base32 của body (36 byte × 8 bit / 5, làm tròn lên = 58).
const RECOVERY_BODY_CHARS: usize = ((KEY_LEN + RECOVERY_CHECKSUM_LEN) * 8).div_ceil(5);
/// Bảng chữ Crockford base32 — không có I/L/O/U (tránh nhầm với 1/0).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Export master key hiện tại thành chuỗi recovery key (caller hiển thị 1 lần).
pub fn export_recovery_key() -> Result<String> {
    Ok(encode_recovery_key_material(&master_key()?))
}

/// Import recovery key trên máy mới: validate → persist (keychain/file) →
/// cập nhật cache process. Sau đó mọi seal/open + KDF archive dùng key này.
/// Env `BROWSERX_MASTER_KEY` đang set → lỗi rõ (env ưu tiên tuyệt đối khi
/// load nên persist sẽ vô tác dụng âm thầm).
pub fn import_recovery_key(input: &str) -> Result<()> {
    let key = parse_recovery_key(input)?;
    persist_imported_key(&key)?;
    let mut guard = MASTER_KEY
        .lock()
        .map_err(|_| AppError::Crypto("master key lock poisoned".into()))?;
    *guard = Some(key);
    Ok(())
}

/// Encode key material → chuỗi `BXRK1-XXXX-…` (nhóm 4 ký tự cho dễ chép tay).
/// `pub(crate)` để test archive.rs mô phỏng "máy cũ" với key tường minh.
pub(crate) fn encode_recovery_key_material(key: &[u8; KEY_LEN]) -> String {
    let mut payload = [0u8; KEY_LEN + RECOVERY_CHECKSUM_LEN];
    payload[..KEY_LEN].copy_from_slice(key);
    payload[KEY_LEN..].copy_from_slice(&recovery_checksum(key));
    let body = crockford_encode(&payload);
    let mut out = String::with_capacity(RECOVERY_PREFIX.len() + body.len() + body.len() / 4 + 1);
    out.push_str(RECOVERY_PREFIX);
    for (i, c) in body.chars().enumerate() {
        if i % 4 == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// Parse + validate chuỗi recovery key → key material 32 byte. Chấp nhận
/// chữ thường, khoảng trắng, dấu `-` tuỳ ý, có/không prefix. Lỗi KHÔNG echo
/// lại input (tránh lộ key vào log/UI).
pub fn parse_recovery_key(input: &str) -> Result<[u8; KEY_LEN]> {
    let normalized: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    // Prefix chỉ strip khi độ dài KHÔNG khớp body trần (body hợp lệ có thể
    // tình cờ bắt đầu bằng "BXRK1" — 5 ký tự đều thuộc bảng chữ).
    let body = if normalized.len() == RECOVERY_BODY_CHARS {
        normalized.as_str()
    } else {
        normalized.strip_prefix(RECOVERY_PREFIX).unwrap_or(&normalized)
    };
    if body.len() != RECOVERY_BODY_CHARS {
        return Err(AppError::InvalidInput(format!(
            "invalid recovery key: expected {RECOVERY_BODY_CHARS} characters after the {RECOVERY_PREFIX} prefix, got {}",
            body.len()
        )));
    }
    let payload = crockford_decode(body)
        .map_err(|e| AppError::InvalidInput(format!("invalid recovery key: {e}")))?;
    if payload.len() != KEY_LEN + RECOVERY_CHECKSUM_LEN {
        return Err(AppError::InvalidInput("invalid recovery key: wrong length".into()));
    }
    let (key_bytes, checksum) = payload.split_at(KEY_LEN);
    let key = <[u8; KEY_LEN]>::try_from(key_bytes)
        .map_err(|_| AppError::InvalidInput("invalid recovery key: wrong length".into()))?;
    if checksum != recovery_checksum(&key) {
        return Err(AppError::InvalidInput(
            "invalid recovery key: checksum mismatch (typo or truncated key)".into(),
        ));
    }
    Ok(key)
}

/// Checksum = SHA-256(domain ‖ key) cắt [`RECOVERY_CHECKSUM_LEN`] byte.
fn recovery_checksum(key: &[u8; KEY_LEN]) -> [u8; RECOVERY_CHECKSUM_LEN] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(RECOVERY_CHECKSUM_DOMAIN);
    h.update(key);
    let digest = h.finalize();
    let mut out = [0u8; RECOVERY_CHECKSUM_LEN];
    out.copy_from_slice(&digest[..RECOVERY_CHECKSUM_LEN]);
    out
}

/// Crockford base32 encode — MSB-first, group cuối pad 0 bit.
fn crockford_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((acc >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((acc << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

/// Crockford base32 decode — chấp nhận nhầm lẫn O→0, I/L→1; pad bit thừa
/// phải bằng 0 (chặn chuỗi không canonical).
fn crockford_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.chars() {
        let v = match c.to_ascii_uppercase() {
            'O' => 0,
            'I' | 'L' => 1,
            up => CROCKFORD
                .iter()
                .position(|&a| a as char == up)
                .ok_or_else(|| format!("invalid character '{c}'"))? as u32,
        };
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if bits > 0 && acc & ((1 << bits) - 1) != 0 {
        return Err("invalid trailing bits".into());
    }
    Ok(out)
}

/// Persist key import: env đang override → lỗi; `BROWSERX_KEYSTORE=file` →
/// file fallback; còn lại keychain (ghi đè entry cũ — hành động chủ đích của
/// user), keychain không khả dụng → file fallback kèm cảnh báo (như load).
fn persist_imported_key(key: &[u8; KEY_LEN]) -> Result<()> {
    if std::env::var(MASTER_KEY_ENV).is_ok() {
        return Err(AppError::Keychain(format!(
            "{MASTER_KEY_ENV} environment override is active; unset it before importing a recovery key"
        )));
    }
    let b64 = B64.encode(key);
    if std::env::var(KEYSTORE_ENV).as_deref() == Ok("file") {
        return write_key_file(&fallback_key_path()?, &b64);
    }
    let stored = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .and_then(|entry| entry.set_password(&b64));
    match stored {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("OS keychain unavailable for recovery key import ({e}); storing master key in fallback file");
            write_key_file(&fallback_key_path()?, &b64)
        }
    }
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

    // -- (W52-E1) Recovery Key -------------------------------------------

    #[test]
    fn recovery_key_roundtrip_with_formatting_variants() {
        let key = test_key(7);
        let s = encode_recovery_key_material(&key);
        assert!(s.starts_with("BXRK1-"));
        // Chuỗi hiển thị KHÔNG chứa key/checksum dạng thô nào khác ngoài base32.
        assert_eq!(parse_recovery_key(&s).unwrap(), key);
        // Chữ thường + khoảng trắng thay dấu gạch.
        assert_eq!(
            parse_recovery_key(&s.to_lowercase().replace('-', " ")).unwrap(),
            key
        );
        // Body không prefix (58 ký tự) cũng parse được.
        let body: String = s.trim_start_matches("BXRK1").replace('-', "");
        assert_eq!(body.len(), RECOVERY_BODY_CHARS);
        assert_eq!(parse_recovery_key(&body).unwrap(), key);
    }

    #[test]
    fn recovery_key_accepts_crockford_confusables() {
        let key = test_key(8);
        let body: String = encode_recovery_key_material(&key)
            .trim_start_matches("BXRK1")
            .replace('-', "");
        // O→0, I/L→1 phải decode như nhau (Crockford).
        let confused = body.replace('0', "O").replace('1', "l");
        assert_eq!(parse_recovery_key(&confused).unwrap(), key);
    }

    #[test]
    fn recovery_key_rejects_typo_and_bad_length() {
        let key = test_key(9);
        let s = encode_recovery_key_material(&key);
        // Đổi 1 ký tự body sang ký tự hợp lệ khác → checksum bắt được.
        let mut chars: Vec<char> = s.chars().collect();
        let pos = s.len() - 1;
        chars[pos] = if chars[pos] == '2' { '3' } else { '2' };
        let typo: String = chars.into_iter().collect();
        let err = parse_recovery_key(&typo).unwrap_err().to_string();
        assert!(err.contains("checksum") || err.contains("trailing"), "{err}");
        // Quá ngắn → lỗi độ dài; ký tự ngoài bảng chữ (U) → lỗi ký tự.
        assert!(parse_recovery_key("BXRK1-ABCD").is_err());
        let bad_char = format!("{}U", &s[..s.len() - 1]);
        assert!(parse_recovery_key(&bad_char).is_err());
        // Lỗi KHÔNG được echo lại chuỗi key.
        assert!(!err.contains(&s));
    }

    #[test]
    fn recovery_key_decrypts_data_sealed_under_foreign_key() {
        // "Máy cũ" seal bằng key B; "máy mới" chỉ có chuỗi recovery → parse
        // → giải mã được (mô phỏng không đụng cache/keychain process-global).
        let old_key = test_key(11);
        let blob = seal_with_key(&old_key, b"cloud secret").unwrap();
        let recovered = parse_recovery_key(&encode_recovery_key_material(&old_key)).unwrap();
        assert_eq!(open_with_key(&recovered, &blob).unwrap(), b"cloud secret");
    }

    #[test]
    fn export_recovery_key_encodes_current_master_key() {
        let key = install_env_master_key();
        let s = export_recovery_key().unwrap();
        assert_eq!(parse_recovery_key(&s).unwrap(), key);
    }

    #[test]
    fn import_recovery_key_rejected_while_env_override_active() {
        install_env_master_key();
        let s = encode_recovery_key_material(&test_key(13));
        let err = import_recovery_key(&s).unwrap_err().to_string();
        assert!(err.contains(MASTER_KEY_ENV), "{err}");
        // Cache KHÔNG bị đổi khi import fail.
        assert_eq!(master_key().unwrap(), test_key(42));
    }
}
