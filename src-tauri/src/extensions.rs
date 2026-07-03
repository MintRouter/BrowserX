//! (P3-1a) Kho extension trung tâm: validate folder unpacked + tải CRX từ
//! Chrome Web Store → strip header CRX2/CRX3 → unzip vào `<data_dir>/extensions/<id>/`.
//!
//! Endpoint tải CRX (chuẩn của Chrome updater):
//! `https://clients2.google.com/service/update2/crx?response=redirect&prodversion=<ver>&acceptformat=crx2,crx3&x=id%3D<EXT_ID>%26uc`
//! Extension id là 32 ký tự [a-p] (mã hoá hex a→p của SHA-256 public key).

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// prodversion khai với endpoint update2/crx (chỉ cần đủ mới để server trả CRX3).
const CHROME_PRODVERSION: &str = "138.0.0.0";

/// Thư mục kho extension tải từ store: `<data_dir>/extensions`.
pub fn extensions_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("extensions")
}

/// Tách extension id 32 ký tự [a-p] từ URL Chrome Web Store (hoặc chuỗi chứa id).
/// Ví dụ `https://chromewebstore.google.com/detail/ublock-origin/cjpalhdlnbpafiamejdnhcphjbkeiagm`.
pub fn parse_store_extension_id(url: &str) -> Result<String> {
    url.split(|c: char| !c.is_ascii_lowercase() || c > 'p')
        .find(|s| s.len() == 32)
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::InvalidInput(
                "cannot find a 32-character extension id in the Web Store URL".into(),
            )
        })
}

/// URL tải CRX từ endpoint update2/crx (redirect → file .crx).
pub fn crx_download_url(ext_id: &str) -> String {
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={CHROME_PRODVERSION}&acceptformat=crx2,crx3&x=id%3D{ext_id}%26uc"
    )
}

/// Strip header CRX2/CRX3 → phần ZIP payload.
/// CRX3: magic "Cr24" + version(3) LE + header_len LE u32 → ZIP tại 12+header_len.
/// CRX2: magic + version(2) + pubkey_len + sig_len → ZIP tại 16+pk+sig.
pub fn crx_to_zip(bytes: &[u8]) -> Result<&[u8]> {
    let err = |msg: &str| AppError::InvalidInput(format!("invalid CRX file: {msg}"));
    if bytes.len() < 16 || &bytes[0..4] != b"Cr24" {
        return Err(err("missing Cr24 magic"));
    }
    let u32le = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
    let zip_start = match u32le(4) {
        2 => 16 + u32le(8) + u32le(12),
        3 => 12 + u32le(8),
        v => return Err(err(&format!("unsupported CRX version {v}"))),
    };
    if zip_start + 2 > bytes.len() || &bytes[zip_start..zip_start + 2] != b"PK" {
        return Err(err("ZIP payload not found after header"));
    }
    Ok(&bytes[zip_start..])
}

/// Tải file CRX của `ext_id` từ Chrome Web Store. Lỗi mạng/HTTP → message rõ
/// ràng (offline-friendly), không để nguyên lỗi reqwest khó hiểu.
pub async fn download_crx(ext_id: &str) -> Result<Vec<u8>> {
    let url = crx_download_url(ext_id);
    let offline = |e: reqwest::Error| {
        AppError::InvalidInput(format!(
            "cannot download extension {ext_id} from Chrome Web Store (check your internet connection): {e}"
        ))
    };
    let resp = reqwest::get(&url).await.map_err(offline)?;
    if !resp.status().is_success() {
        return Err(AppError::InvalidInput(format!(
            "Chrome Web Store returned HTTP {} for extension {ext_id} — check the extension id/URL",
            resp.status()
        )));
    }
    let bytes = resp.bytes().await.map_err(offline)?;
    Ok(bytes.to_vec())
}

/// Kết quả cài extension từ store: id + tên (từ manifest) + thư mục unpacked.
pub struct StoreInstall {
    pub ext_id: String,
    pub name: String,
    pub unpacked_path: PathBuf,
}

/// Cài extension từ URL Web Store: parse id → tải CRX → strip header → unzip
/// vào `<data_dir>/extensions/<id>/` (ghi đè bản cũ) → validate manifest.
pub async fn install_from_store_url(url: &str, data_dir: &Path) -> Result<StoreInstall> {
    let ext_id = parse_store_extension_id(url)?;
    let crx = download_crx(&ext_id).await?;
    let zip_bytes = crx_to_zip(&crx)?;
    let dest = extensions_dir(data_dir).join(&ext_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    unzip_to(zip_bytes, &dest)?;
    let name = validate_unpacked_dir(&dest)?;
    Ok(StoreInstall {
        ext_id,
        name,
        unpacked_path: dest,
    })
}

/// Giải nén ZIP payload vào `dest` (tạo thư mục nếu chưa có). Chống zip-slip
/// bằng `enclosed_name()` — entry có đường dẫn thoát ra ngoài bị từ chối.
pub fn unzip_to(zip_bytes: &[u8], dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| AppError::InvalidInput(format!("invalid CRX zip payload: {e}")))?;
    std::fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AppError::InvalidInput(format!("corrupt zip entry: {e}")))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(AppError::InvalidInput(format!(
                "zip entry escapes destination: {}",
                entry.name()
            )));
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&out, buf)?;
    }
    Ok(())
}

/// Validate 1 thư mục unpacked extension: tồn tại + có `manifest.json` parse
/// được. Trả tên extension (resolve `__MSG_…__` qua `_locales/<default_locale>`
/// nếu được; fallback tên thư mục).
pub fn validate_unpacked_dir(dir: &Path) -> Result<String> {
    if !dir.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "extension folder not found: {}",
            dir.display()
        )));
    }
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "not an unpacked extension (missing manifest.json): {}",
            dir.display()
        )));
    }
    let manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        &manifest_path,
    )?)
    .map_err(|e| AppError::InvalidInput(format!("invalid manifest.json: {e}")))?;
    let fallback = || {
        dir.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "extension".into())
    };
    let name = match manifest.get("name").and_then(|v| v.as_str()) {
        Some(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                fallback()
            } else if let Some(key) = raw
                .strip_prefix("__MSG_")
                .and_then(|s| s.strip_suffix("__"))
            {
                resolve_localized_name(dir, &manifest, key).unwrap_or_else(fallback)
            } else {
                raw.to_string()
            }
        }
        None => fallback(),
    };
    Ok(name)
}

/// Đọc `_locales/<default_locale>/messages.json` để resolve tên `__MSG_<key>__`
/// (key của Chrome i18n là case-insensitive). Best-effort — None nếu thiếu file/key.
fn resolve_localized_name(dir: &Path, manifest: &serde_json::Value, key: &str) -> Option<String> {
    let locale = manifest.get("default_locale")?.as_str()?;
    let raw = std::fs::read_to_string(dir.join("_locales").join(locale).join("messages.json"))
        .ok()?;
    let messages: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let obj = messages.as_object()?;
    let entry = obj
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)?;
    let msg = entry.get("message")?.as_str()?.trim();
    (!msg.is_empty()).then(|| msg.to_string())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir() -> TempDir {
        let dir = std::env::temp_dir().join(format!("browserx-ext-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn zip_with_manifest(manifest: &str) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("manifest.json", opts).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        w.add_directory("assets", opts).unwrap();
        w.start_file("assets/icon.txt", opts).unwrap();
        w.write_all(b"icon").unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn parse_store_id_from_url_and_raw() {
        let id = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
        let url =
            format!("https://chromewebstore.google.com/detail/ublock-origin/{id}?hl=vi&pli=1");
        assert_eq!(parse_store_extension_id(&url).unwrap(), id);
        assert_eq!(parse_store_extension_id(id).unwrap(), id);
        // Không có id 32 ký tự [a-p] → InvalidInput.
        assert!(matches!(
            parse_store_extension_id("https://chromewebstore.google.com/detail/foo"),
            Err(AppError::InvalidInput(_))
        ));
        // URL trong query của endpoint chuẩn.
        assert!(crx_download_url(id).contains(&format!("x=id%3D{id}%26uc")));
    }

    #[test]
    fn crx3_and_crx2_headers_are_stripped_to_zip() {
        let zip_bytes = zip_with_manifest(r#"{"name":"T","version":"1"}"#);

        // CRX3: Cr24 + ver 3 + header_len + header giả.
        let header = vec![0xAAu8; 20];
        let mut crx3 = Vec::new();
        crx3.extend_from_slice(b"Cr24");
        crx3.extend_from_slice(&3u32.to_le_bytes());
        crx3.extend_from_slice(&(header.len() as u32).to_le_bytes());
        crx3.extend_from_slice(&header);
        crx3.extend_from_slice(&zip_bytes);
        assert_eq!(crx_to_zip(&crx3).unwrap(), &zip_bytes[..]);

        // CRX2: Cr24 + ver 2 + pk_len + sig_len + pk + sig.
        let (pk, sig) = (vec![1u8; 8], vec![2u8; 4]);
        let mut crx2 = Vec::new();
        crx2.extend_from_slice(b"Cr24");
        crx2.extend_from_slice(&2u32.to_le_bytes());
        crx2.extend_from_slice(&(pk.len() as u32).to_le_bytes());
        crx2.extend_from_slice(&(sig.len() as u32).to_le_bytes());
        crx2.extend_from_slice(&pk);
        crx2.extend_from_slice(&sig);
        crx2.extend_from_slice(&zip_bytes);
        assert_eq!(crx_to_zip(&crx2).unwrap(), &zip_bytes[..]);

        // Hỏng: thiếu magic / version lạ / payload không phải ZIP.
        assert!(crx_to_zip(b"PK\x03\x04junk").is_err());
        let mut bad_ver = crx3.clone();
        bad_ver[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert!(crx_to_zip(&bad_ver).is_err());
        let mut no_zip = crx3.clone();
        no_zip.truncate(12 + 20);
        assert!(crx_to_zip(&no_zip).is_err());
    }

    #[test]
    fn unzip_writes_files_and_validate_reads_name() {
        let guard = temp_dir();
        let dest = guard.0.join("ext");
        let zip_bytes = zip_with_manifest(r#"{"name":"My Ext","version":"1"}"#);
        unzip_to(&zip_bytes, &dest).unwrap();
        assert!(dest.join("manifest.json").is_file());
        assert!(dest.join("assets/icon.txt").is_file());
        assert_eq!(validate_unpacked_dir(&dest).unwrap(), "My Ext");
    }

    #[test]
    fn validate_rejects_missing_dir_or_manifest() {
        let guard = temp_dir();
        // Thư mục không tồn tại.
        assert!(matches!(
            validate_unpacked_dir(&guard.0.join("nope")),
            Err(AppError::InvalidInput(_))
        ));
        // Folder có file nhưng thiếu manifest.json.
        let dir = guard.0.join("not-ext");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.txt"), "hi").unwrap();
        assert!(matches!(
            validate_unpacked_dir(&dir),
            Err(AppError::InvalidInput(_))
        ));
        // manifest.json không parse được.
        std::fs::write(dir.join("manifest.json"), "{oops").unwrap();
        assert!(matches!(
            validate_unpacked_dir(&dir),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn validate_resolves_localized_msg_name_with_fallback() {
        let guard = temp_dir();
        let dir = guard.0.join("localized");
        std::fs::create_dir_all(dir.join("_locales/en")).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"name":"__MSG_appName__","default_locale":"en","version":"1"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("_locales/en/messages.json"),
            r#"{"appname":{"message":"Localized Name"}}"#,
        )
        .unwrap();
        assert_eq!(validate_unpacked_dir(&dir).unwrap(), "Localized Name");

        // Không resolve được (thiếu key) → fallback tên thư mục.
        let dir2 = guard.0.join("fallback");
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(
            dir2.join("manifest.json"),
            r#"{"name":"__MSG_missing__","version":"1"}"#,
        )
        .unwrap();
        assert_eq!(validate_unpacked_dir(&dir2).unwrap(), "fallback");
    }
}
