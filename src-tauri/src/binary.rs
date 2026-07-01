//! Binary manager: tải binary CloakBrowser lúc runtime (reqwest + stream),
//! verify chữ ký Ed25519 + SHA-256, giải nén (tar/zip/flate2).
//!
//! Port từ refs/CloakBrowser/cloakbrowser/download.py (Wave 2b):
//! - ensure_binary (#L131-L259, bỏ Pro tier / auto-update / welcome banner)
//! - _download_and_extract (#L262-L304), _download_file (#L689-L723)
//! - _verify_download_checksum (#L474-L544): fetch SHA256SUMS + SHA256SUMS.sig,
//!   verify Ed25519 pinned pubkey TRƯỚC (non-bypassable trên kênh official),
//!   ràng version= trong manifest chống downgrade, rồi verify SHA-256 archive.
//! - _extract_archive (#L726-L758): tar.gz/zip, flatten single-subdir (trừ .app),
//!   chmod +x, macOS xoá quarantine xattr (Gatekeeper).
//!
//! KHÔNG commit/redistribute binary trong repo (Binary License, docs/02/04) —
//! chỉ code tải lúc runtime. Progress báo qua callback để W3a emit
//! `binary://progress` {phase, pct}.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::config;
use crate::error::{AppError, Result};

/// Callback tiến độ: (phase, pct 0–100). Phases: "download" | "verify" | "extract" | "done".
pub type ProgressFn<'a> = &'a (dyn Fn(&str, u8) + Send + Sync);

/// Timeout kết nối (download.py#L63 connect=10s).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout fetch manifest nhỏ (download.py#L576 timeout=10s).
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);

fn binary_err(msg: impl Into<String>) -> AppError {
    AppError::Binary(msg.into())
}

fn emit(progress: Option<ProgressFn<'_>>, phase: &str, pct: u8) {
    if let Some(cb) = progress {
        cb(phase, pct);
    }
}

// ---------------------------------------------------------------------------
// Public API (expose cho W3a wiring)
// ---------------------------------------------------------------------------

/// Đảm bảo binary CloakBrowser sẵn sàng; tải + verify nếu thiếu. Trả đường dẫn
/// executable theo OS. Port ensure_binary (download.py#L131-L259, bỏ Pro/auto-update).
///
/// - `version`: pin phiên bản (None = version mặc định của platform).
/// - `progress`: callback (phase, pct) để W3a emit `binary://progress`.
///
/// Env: `CLOAKBROWSER_BINARY_PATH` → dùng bản local (bỏ tải);
/// `CLOAKBROWSER_CACHE_DIR`, `CLOAKBROWSER_DOWNLOAD_URL` xem config.rs.
pub async fn ensure_binary(
    version: Option<&str>,
    progress: Option<ProgressFn<'_>>,
) -> Result<PathBuf> {
    // 1) Local override (download.py#L146-L154): dùng bản build sẵn của user.
    if let Some(p) = config::get_local_binary_override() {
        let path = PathBuf::from(&p);
        if !path.exists() {
            return Err(binary_err(format!(
                "CLOAKBROWSER_BINARY_PATH set to '{p}' but file does not exist"
            )));
        }
        return Ok(path);
    }

    // 2) Fail fast nếu platform không có binary (download.py#L194).
    config::get_platform_tag()?;

    // 3) Cache hit.
    let binary_path = config::get_binary_path(version);
    if binary_path.exists() {
        return Ok(binary_path);
    }

    // 4) Tải + verify + giải nén.
    download_and_extract(version, progress).await?;

    let binary_path = config::get_binary_path(version);
    if !binary_path.exists() {
        return Err(binary_err(format!(
            "Download completed but binary not found at expected path: {}",
            binary_path.display()
        )));
    }
    emit(progress, "done", 100);
    Ok(binary_path)
}

// ---------------------------------------------------------------------------
// Download + extract (download.py#L262-L304)
// ---------------------------------------------------------------------------

async fn download_and_extract(
    version: Option<&str>,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    let primary_url = config::get_download_url(version)?;
    let fallback_url = config::get_fallback_download_url(version)?;
    let binary_dir = config::get_binary_dir(version);
    let binary_path = config::get_binary_path(version);

    if let Some(parent) = binary_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = temp_archive_path();

    let result = async {
        // Thử primary, fallback GitHub Releases (bỏ fallback nếu dùng custom URL).
        match download_file(&primary_url, &tmp_path, progress).await {
            Ok(()) => {}
            Err(primary_err) => {
                if config::has_custom_download_url() {
                    return Err(primary_err);
                }
                tracing::warn!(
                    "Primary download failed ({primary_err}); trying GitHub Releases..."
                );
                download_file(&fallback_url, &tmp_path, progress).await?;
            }
        }

        // Verify BẮT BUỘC (Ed25519 → version → SHA-256) trước khi giải nén.
        verify_download(&tmp_path, version, progress).await?;

        emit(progress, "extract", 0);
        extract_archive(&tmp_path, &binary_dir, &binary_path, std::env::consts::OS)?;
        emit(progress, "extract", 100);
        Ok(())
    }
    .await;

    let _ = std::fs::remove_file(&tmp_path);
    result
}

/// Đường dẫn file tạm cho archive (đuôi đúng theo OS).
fn temp_archive_path() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "browserx-cloak-{}-{}{}",
        std::process::id(),
        nanos,
        config::get_archive_ext()
    ))
}

/// Tải file với reqwest stream + progress logging (download.py#L689-L723).
async fn download_file(url: &str, dest: &Path, progress: Option<ProgressFn<'_>>) -> Result<()> {
    use std::io::Write;

    use futures::StreamExt;

    tracing::info!("Downloading from {url}");
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()?;
    let resp = client.get(url).send().await?.error_for_status()?;

    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_pct: i32 = -1;

    let mut file = std::fs::File::create(dest)?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(pct) = (downloaded * 100).checked_div(total) {
            let pct = pct as i32;
            if pct >= last_pct + 5 {
                last_pct = pct;
                emit(progress, "download", pct.clamp(0, 100) as u8);
            }
        }
    }
    file.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Verify (download.py#L474-L544): Ed25519 pinned pubkey → version → SHA-256
// ---------------------------------------------------------------------------

async fn verify_download(
    file_path: &Path,
    version: Option<&str>,
    progress: Option<ProgressFn<'_>>,
) -> Result<()> {
    emit(progress, "verify", 0);
    let tarball_name = config::get_archive_name()?;
    let requested = version
        .map(str::to_string)
        .unwrap_or_else(config::get_chromium_version);

    // Mirror tự host (CLOAKBROWSER_DOWNLOAD_URL): pinned key không áp dụng → dùng
    // SHA256SUMS same-origin, có thể bỏ qua bằng CLOAKBROWSER_SKIP_CHECKSUM.
    if config::has_custom_download_url() {
        if skip_checksum() {
            tracing::warn!("CLOAKBROWSER_SKIP_CHECKSUM set — skipping verification for custom URL");
            return Ok(());
        }
        match fetch_checksums(&requested).await {
            None => {
                tracing::warn!("SHA256SUMS not available from custom URL — skipping checksum");
                return Ok(());
            }
            Some(map) => match map.get(&tarball_name) {
                None => {
                    tracing::warn!("SHA256SUMS has no entry for {tarball_name} — skipping");
                    return Ok(());
                }
                Some(expected) => {
                    verify_checksum_file(file_path, expected)?;
                    emit(progress, "verify", 100);
                    return Ok(());
                }
            },
        }
    }

    // Kênh official: chữ ký là gốc tin cậy, KHÔNG bypass được.
    let (manifest_bytes, sig_bytes) = fetch_signed_manifest(&requested).await.ok_or_else(|| {
        binary_err(
            "Could not fetch a signed SHA256SUMS (SHA256SUMS + SHA256SUMS.sig) for this release \
             — refusing to use an unverified binary.",
        )
    })?;

    verify_signature(&manifest_bytes, &sig_bytes)?;
    let manifest_text = String::from_utf8_lossy(&manifest_bytes).into_owned();

    // Ràng version chống forced-downgrade (download.py#L525-L535).
    let declared = parse_manifest_version(&manifest_text);
    if declared.as_deref() != Some(requested.as_str()) {
        return Err(binary_err(format!(
            "Version mismatch in signed SHA256SUMS: requested {requested}, manifest declares {}. \
             Refusing (possible downgrade).",
            declared.as_deref().unwrap_or("none")
        )));
    }

    let expected = parse_checksums(&manifest_text).get(&tarball_name).cloned().ok_or_else(|| {
        binary_err(format!(
            "Signature-verified SHA256SUMS has no entry for {tarball_name} — cannot confirm integrity."
        ))
    })?;
    verify_checksum_file(file_path, &expected)?;
    emit(progress, "verify", 100);
    Ok(())
}

fn skip_checksum() -> bool {
    std::env::var("CLOAKBROWSER_SKIP_CHECKSUM")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Fetch (SHA256SUMS, SHA256SUMS.sig) raw bytes; primary rồi GitHub mirror.
async fn fetch_signed_manifest(version: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let bases = [
        format!("{}/chromium-v{version}", config::download_base_url()),
        format!("{}/chromium-v{version}", config::GITHUB_DOWNLOAD_BASE_URL),
    ];
    let client = reqwest::Client::builder()
        .timeout(MANIFEST_TIMEOUT)
        .build()
        .ok()?;
    for base in bases {
        let manifest = client.get(format!("{base}/SHA256SUMS")).send().await;
        let sig = client.get(format!("{base}/SHA256SUMS.sig")).send().await;
        if let (Ok(m), Ok(s)) = (manifest, sig) {
            if let (Ok(m), Ok(s)) = (m.error_for_status(), s.error_for_status()) {
                if let (Ok(mb), Ok(sb)) = (m.bytes().await, s.bytes().await) {
                    return Some((mb.to_vec(), sb.to_vec()));
                }
            }
        }
    }
    None
}

/// Fetch SHA256SUMS (không chữ ký) cho kênh custom mirror.
async fn fetch_checksums(version: &str) -> Option<HashMap<String, String>> {
    let url = format!(
        "{}/chromium-v{version}/SHA256SUMS",
        config::download_base_url()
    );
    let client = reqwest::Client::builder()
        .timeout(MANIFEST_TIMEOUT)
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?.error_for_status().ok()?;
    let text = resp.text().await.ok()?;
    Some(parse_checksums(&text))
}

/// Verify chữ ký Ed25519 detached với pinned pubkeys mặc định.
fn verify_signature(manifest_bytes: &[u8], sig_b64: &[u8]) -> Result<()> {
    verify_signature_with_keys(manifest_bytes, sig_b64, config::BINARY_SIGNING_PUBKEYS)
}

/// Verify chữ ký Ed25519 với danh sách pubkey base64 cho trước (tách để test).
/// Thành công nếu BẤT KỲ key nào validate; lỗi nếu chữ ký hỏng hoặc không key nào khớp.
fn verify_signature_with_keys(
    manifest_bytes: &[u8],
    sig_b64: &[u8],
    pubkeys: &[&str],
) -> Result<()> {
    let sig_str = std::str::from_utf8(sig_b64)
        .map_err(|e| binary_err(format!("Malformed SHA256SUMS.sig (not UTF-8): {e}")))?
        .trim();
    let sig_raw = B64
        .decode(sig_str)
        .map_err(|e| binary_err(format!("Malformed SHA256SUMS.sig (not valid base64): {e}")))?;
    let sig_arr: [u8; 64] = sig_raw
        .as_slice()
        .try_into()
        .map_err(|_| binary_err("SHA256SUMS.sig is not a 64-byte Ed25519 signature"))?;
    let signature = Signature::from_bytes(&sig_arr);

    for pk_b64 in pubkeys {
        let Ok(pk_raw) = B64.decode(pk_b64.trim()) else {
            continue;
        };
        let Ok(pk_arr): std::result::Result<[u8; 32], _> = pk_raw.as_slice().try_into() else {
            continue;
        };
        let Ok(vk) = VerifyingKey::from_bytes(&pk_arr) else {
            continue;
        };
        if vk.verify(manifest_bytes, &signature).is_ok() {
            return Ok(());
        }
    }
    Err(binary_err(
        "SHA256SUMS signature verification failed — no pinned key validated the manifest. \
         The binary's authenticity could not be confirmed.",
    ))
}

/// Đọc dòng `version=<v>` trong manifest đã ký (download.py#L547-L557).
fn parse_manifest_version(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version=") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Parse `<64-hex sha256>  filename` → map (download.py#L651-L668).
fn parse_checksums(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let mut it = line.trim().splitn(2, char::is_whitespace);
        let (Some(hash), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        let hash = hash.to_lowercase();
        let name = name.trim();
        if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        out.insert(name.trim_start_matches('*').to_string(), hash);
    }
    out
}

/// SHA-256 hex của bytes (helper cho unit test).
#[cfg(test)]
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Verify SHA-256 của bytes so với hash kỳ vọng (helper cho unit test).
#[cfg(test)]
fn verify_checksum_bytes(data: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_hex(data);
    if actual != expected.to_lowercase() {
        return Err(binary_err(format!(
            "Checksum verification failed! expected {expected}, got {actual}. \
             File may be corrupted or tampered with."
        )));
    }
    Ok(())
}

/// Verify SHA-256 của file (đọc theo chunk).
fn verify_checksum_file(path: &Path, expected: &str) -> Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected.to_lowercase() {
        return Err(binary_err(format!(
            "Checksum verification failed! expected {expected}, got {actual}. \
             File may be corrupted or tampered with."
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Extract (download.py#L726-L816)
// ---------------------------------------------------------------------------

fn extract_archive(
    archive_path: &Path,
    dest_dir: &Path,
    binary_path: &Path,
    os: &str,
) -> Result<()> {
    if dest_dir.exists() {
        std::fs::remove_dir_all(dest_dir)?;
    }
    std::fs::create_dir_all(dest_dir)?;

    if archive_path.extension().and_then(|s| s.to_str()) == Some("zip") {
        extract_zip(archive_path, dest_dir)?;
    } else {
        extract_tar(archive_path, dest_dir)?;
    }

    // Flatten single-subdir (trừ .app bundle) như download.py#L799-L815.
    flatten_single_subdir(dest_dir)?;

    if binary_path.exists() {
        make_executable(binary_path, os)?;
    }
    // macOS: xoá quarantine/provenance xattr tránh Gatekeeper (download.py#L753-L755).
    if os == "macos" {
        remove_quarantine(dest_dir);
    }
    Ok(())
}

/// Giải nén tar.gz với chống path traversal (download.py#L761-L784).
fn extract_tar(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let dest_canon = dest_dir
        .canonicalize()
        .unwrap_or_else(|_| dest_dir.to_path_buf());

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        // Symlink/hardlink: chỉ cho phép target tương đối, không '..' (macOS .app cần symlink).
        if let Some(link) = entry.link_name()? {
            if link.is_absolute() || link.components().any(|c| c.as_os_str() == "..") {
                tracing::warn!("Skipping suspicious link: {}", path.display());
                continue;
            }
        } else if path.is_absolute() || path.components().any(|c| c.as_os_str() == "..") {
            return Err(binary_err(format!(
                "Archive contains path traversal: {}",
                path.display()
            )));
        }
        entry.unpack_in(&dest_canon)?;
    }
    Ok(())
}

/// Giải nén zip với chống path traversal (download.py#L787-L796).
fn extract_zip(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    use std::io::copy;
    let file = std::fs::File::open(archive_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| binary_err(format!("zip open: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| binary_err(format!("zip entry: {e}")))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(binary_err(format!(
                "Archive contains path traversal: {}",
                entry.name()
            )));
        };
        let out = dest_dir.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut w = std::fs::File::create(&out)?;
            copy(&mut entry, &mut w)?;
        }
    }
    Ok(())
}

/// Nếu chỉ có 1 subdir (không phải .app), nâng nội dung lên (download.py#L799-L815).
fn flatten_single_subdir(dest_dir: &Path) -> Result<()> {
    let entries: Vec<_> = std::fs::read_dir(dest_dir)?.collect::<std::result::Result<_, _>>()?;
    if entries.len() != 1 {
        return Ok(());
    }
    let sub = entries[0].path();
    if !sub.is_dir() {
        return Ok(());
    }
    if sub
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".app"))
        .unwrap_or(false)
    {
        return Ok(());
    }
    for item in std::fs::read_dir(&sub)? {
        let item = item?;
        std::fs::rename(item.path(), dest_dir.join(item.file_name()))?;
    }
    std::fs::remove_dir(&sub)?;
    Ok(())
}

/// chmod +x (bỏ qua trên Windows — download.py#L823-L828).
fn make_executable(path: &Path, os: &str) -> Result<()> {
    if os == "windows" {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(path)?.permissions();
        perm.set_mode(perm.mode() | 0o111);
        std::fs::set_permissions(path, perm)?;
    }
    let _ = path;
    Ok(())
}

/// Xoá quarantine xattr trên macOS (best-effort — download.py#L831-L841).
fn remove_quarantine(path: &Path) {
    let _ = std::process::Command::new("xattr")
        .args(["-cr", &path.to_string_lossy()])
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::{Signer, SigningKey};

    fn test_keypair(seed: u8) -> (SigningKey, String) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let pk_b64 = B64.encode(sk.verifying_key().to_bytes());
        (sk, pk_b64)
    }

    fn sign_b64(sk: &SigningKey, msg: &[u8]) -> Vec<u8> {
        B64.encode(sk.sign(msg).to_bytes()).into_bytes()
    }

    #[test]
    fn ed25519_verify_happy_path() {
        let (sk, pk_b64) = test_keypair(7);
        let manifest = b"version=1.2.3.4\nabc  cloakbrowser-x.tar.gz\n";
        let sig = sign_b64(&sk, manifest);
        assert!(verify_signature_with_keys(manifest, &sig, &[pk_b64.as_str()]).is_ok());
    }

    #[test]
    fn ed25519_verify_fails_on_tampered_manifest() {
        let (sk, pk_b64) = test_keypair(9);
        let manifest = b"version=1.2.3.4\n";
        let sig = sign_b64(&sk, manifest);
        let tampered = b"version=9.9.9.9\n";
        let err = verify_signature_with_keys(tampered, &sig, &[pk_b64.as_str()]).unwrap_err();
        assert!(matches!(err, AppError::Binary(_)));
    }

    #[test]
    fn ed25519_verify_fails_on_wrong_key() {
        let (sk, _) = test_keypair(1);
        let (_, other_pk) = test_keypair(2);
        let manifest = b"payload";
        let sig = sign_b64(&sk, manifest);
        assert!(verify_signature_with_keys(manifest, &sig, &[other_pk.as_str()]).is_err());
    }

    #[test]
    fn ed25519_verify_rejects_malformed_signature() {
        let (_, pk_b64) = test_keypair(3);
        assert!(verify_signature_with_keys(b"m", b"not base64 !!!", &[pk_b64.as_str()]).is_err());
        // base64 hợp lệ nhưng sai độ dài (không phải 64 byte).
        let short = B64.encode([0u8; 10]);
        assert!(verify_signature_with_keys(b"m", short.as_bytes(), &[pk_b64.as_str()]).is_err());
    }

    #[test]
    fn pinned_pubkey_is_valid_base64_32_bytes() {
        for pk in config::BINARY_SIGNING_PUBKEYS {
            let raw = B64.decode(pk).expect("pinned key is base64");
            assert_eq!(raw.len(), 32, "pinned key must be 32 raw bytes");
            let arr: [u8; 32] = raw.as_slice().try_into().unwrap();
            assert!(VerifyingKey::from_bytes(&arr).is_ok());
        }
    }

    #[test]
    fn sha256_checksum_happy_and_mismatch() {
        let data = b"hello cloakbrowser";
        let expected = sha256_hex(data);
        assert_eq!(expected.len(), 64);
        assert!(verify_checksum_bytes(data, &expected).is_ok());
        // Uppercase vẫn khớp (case-insensitive).
        assert!(verify_checksum_bytes(data, &expected.to_uppercase()).is_ok());
        assert!(verify_checksum_bytes(data, &"0".repeat(64)).is_err());
    }

    #[test]
    fn parse_checksums_filters_junk_and_version_line() {
        let h_a = "a".repeat(64);
        let h_b = "B".repeat(64); // uppercase → phải được lowercased
        let text = format!(
            "version=1.2.3.4\n\
             {h_a}  cloakbrowser-linux-x64.tar.gz\n\
             {h_b}  *cloakbrowser-darwin-arm64.tar.gz\n\
             notahash  ignored.txt\n\
             \n"
        );
        let map = parse_checksums(&text);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("cloakbrowser-linux-x64.tar.gz").map(String::as_str),
            Some(h_a.as_str())
        );
        // '*' prefix trên filename bị strip; hash lowercased.
        assert_eq!(
            map.get("cloakbrowser-darwin-arm64.tar.gz")
                .map(String::as_str),
            Some("b".repeat(64).as_str())
        );
        assert!(!map.contains_key("ignored.txt"));
    }

    #[test]
    fn parse_manifest_version_reads_line() {
        assert_eq!(
            parse_manifest_version("version=1.2.3.4\nhash  f\n").as_deref(),
            Some("1.2.3.4")
        );
        assert_eq!(parse_manifest_version("hash  f\n"), None);
    }

    #[test]
    fn extract_tar_flattens_single_subdir() {
        use std::io::Write;

        let tmp = std::env::temp_dir().join(format!("browserx-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let archive = tmp.join("a.tar.gz");
        let dest = tmp.join("out");

        // Dựng tar.gz: wrap/chrome + wrap/lib.so
        {
            let f = std::fs::File::create(&archive).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            for (name, body) in [
                ("wrap/chrome", &b"#!/bin/sh\n"[..]),
                ("wrap/lib.so", &b"x"[..]),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, name, body).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }

        let binary_path = dest.join("chrome");
        extract_archive(&archive, &dest, &binary_path, "linux").unwrap();
        assert!(
            dest.join("chrome").exists(),
            "flatten should lift wrap/chrome to out/chrome"
        );
        assert!(dest.join("lib.so").exists());
        assert!(!dest.join("wrap").exists());

        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::io::stdout().flush();
    }
}
