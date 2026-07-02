//! Dọn & nén profile (W16): đo dung lượng user_data_dir + dọn cache an toàn.
//!
//! Chỉ xoá các thư mục cache Chromium tự tạo lại được (`CACHE_DIRS`) — TUYỆT ĐỐI
//! không đụng Cookies/Local Storage/IndexedDB/History/Bookmarks để giữ session
//! đăng nhập. REFUSE dọn khi profile đang chạy (check `ProcessManager`).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::process::ProcessManager;

/// Key setting bật tự dọn cache khi phiên dừng (default: tắt — chưa có key).
pub const AUTO_CLEAR_SETTING: &str = "auto_clear_cache_on_stop";

/// Thư mục cache an toàn để xoá trong user_data_dir (Chromium tự tạo lại).
pub const CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "ShaderCache",
    "GrShaderCache",
    "DawnCache",
    "Default/Cache",
    "Default/Code Cache",
    "Default/GPUCache",
    "Default/Service Worker/CacheStorage",
    "Default/Service Worker/ScriptCache",
    "Crashpad",
    "Safe Browsing",
];

/// (W20b) File/dir lịch sử duyệt web — xoá khi phiên dừng nếu profile tắt
/// `store_history`. Chromium tự tạo lại; cookies/session KHÔNG bị đụng.
pub const HISTORY_PATHS: &[&str] = &[
    "Default/History",
    "Default/History-journal",
    "Default/Visited Links",
    "Default/Top Sites",
    "Default/Top Sites-journal",
    "Default/Shortcuts",
    "Default/Shortcuts-journal",
    "History",
    "History-journal",
    "Visited Links",
];

/// (W20b) File mật khẩu đã lưu — xoá khi phiên dừng nếu profile tắt `store_passwords`.
pub const PASSWORD_PATHS: &[&str] = &[
    "Default/Login Data",
    "Default/Login Data-journal",
    "Default/Login Data For Account",
    "Default/Login Data For Account-journal",
    "Login Data",
    "Login Data-journal",
];

/// (W20b) Thư mục service worker (registration + CacheStorage + ScriptCache) —
/// xoá khi phiên dừng nếu profile tắt `store_sw_cache`.
pub const SW_CACHE_PATHS: &[&str] = &["Default/Service Worker", "Service Worker"];

/// Dung lượng đĩa của 1 profile — khớp command `profile_storage_sizes`.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileStorageSize {
    pub profile_id: String,
    pub bytes: u64,
}

/// Kết quả dọn cache 1 profile — khớp command `clear_profile_cache`.
#[derive(Debug, Clone, Serialize)]
pub struct ClearCacheResult {
    pub profile_id: String,
    pub freed_bytes: u64,
    /// Có giá trị khi bị từ chối (profile đang chạy) hoặc xoá lỗi.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Tổng bytes của `path` (walk đệ quy). KHÔNG theo symlink; path không tồn tại
/// hoặc đọc lỗi = 0 (best-effort, không fail cả phép đo).
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| dir_size(&e.path())).sum()
}

/// Xoá các `CACHE_DIRS` trong `user_data_dir`, trả về số bytes giải phóng.
/// Dir con không tồn tại hoặc là symlink → bỏ qua; `user_data_dir` chưa tồn tại
/// (profile chưa từng launch) → `Ok(0)`.
pub fn clear_cache(user_data_dir: &Path) -> Result<u64> {
    if !user_data_dir.is_dir() {
        return Ok(0);
    }
    let mut freed = 0u64;
    for rel in CACHE_DIRS {
        let dir = user_data_dir.join(rel);
        let Ok(meta) = fs::symlink_metadata(&dir) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let size = dir_size(&dir);
        fs::remove_dir_all(&dir)?;
        freed += size;
    }
    Ok(freed)
}

/// (W20b) Áp storage options của profile SAU khi phiên dừng: xoá dữ liệu của
/// các loại bị tắt (history / passwords / service-worker cache), trả bytes
/// giải phóng. Cơ chế là CLEANUP — binary CloakBrowser không có flag disable
/// các loại này (đã kiểm refs/CloakBrowser: chỉ có họ flag --fingerprint-*).
/// File/dir không tồn tại hoặc symlink → bỏ qua; user_data_dir chưa có → `Ok(0)`.
pub fn clear_storage_options(
    user_data_dir: &Path,
    store_history: bool,
    store_passwords: bool,
    store_sw_cache: bool,
) -> Result<u64> {
    if !user_data_dir.is_dir() {
        return Ok(0);
    }
    let mut targets: Vec<&str> = Vec::new();
    if !store_history {
        targets.extend_from_slice(HISTORY_PATHS);
    }
    if !store_passwords {
        targets.extend_from_slice(PASSWORD_PATHS);
    }
    if !store_sw_cache {
        targets.extend_from_slice(SW_CACHE_PATHS);
    }
    let mut freed = 0u64;
    for rel in targets {
        let path = user_data_dir.join(rel);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let size = dir_size(&path);
            fs::remove_dir_all(&path)?;
            freed += size;
        } else if meta.is_file() {
            freed += meta.len();
            fs::remove_file(&path)?;
        }
    }
    Ok(freed)
}

/// Dọn cache nhiều profile — REFUSE profile đang chạy (giữ nguyên dữ liệu,
/// báo qua `error`). Mỗi phần tử `targets` = (profile_id, user_data_dir).
/// Phần FS chạy trên `spawn_blocking` để không nghẽn executor.
pub async fn clear_profiles_cache(
    procs: &ProcessManager,
    targets: Vec<(String, PathBuf)>,
) -> Vec<ClearCacheResult> {
    let mut out = Vec::with_capacity(targets.len());
    for (profile_id, dir) in targets {
        if procs.is_running(&profile_id).await {
            out.push(ClearCacheResult {
                profile_id,
                freed_bytes: 0,
                error: Some("profile đang chạy — dừng phiên trước khi dọn cache".into()),
            });
            continue;
        }
        let cleared = tokio::task::spawn_blocking(move || clear_cache(&dir))
            .await
            .unwrap_or_else(|e| Err(AppError::Other(anyhow::anyhow!("dọn cache panic: {e}"))));
        out.push(match cleared {
            Ok(freed) => ClearCacheResult {
                profile_id,
                freed_bytes: freed,
                error: None,
            },
            Err(e) => ClearCacheResult {
                profile_id,
                freed_bytes: 0,
                error: Some(e.to_string()),
            },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("browserx-storage-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, bytes: usize) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, vec![b'x'; bytes]).unwrap();
    }

    /// Cấu trúc user_data_dir giả kiểu Chromium: cache (xoá được) + dữ liệu
    /// nhạy cảm (phải giữ nguyên). Trả (cache_bytes, keep_bytes).
    fn fake_profile(root: &Path) -> (u64, u64) {
        write(root, "Cache/Cache_Data/f_0001", 100);
        write(root, "Code Cache/js/index", 50);
        write(root, "GPUCache/data_0", 10);
        write(root, "Default/Cache/Cache_Data/f_0002", 200);
        write(root, "Default/Code Cache/wasm/index", 30);
        write(root, "Default/Service Worker/CacheStorage/abc/index", 40);
        write(root, "Default/Service Worker/ScriptCache/index", 20);
        write(root, "Crashpad/pending/dump.dmp", 60);
        write(root, "Safe Browsing/store", 15);
        let cache = 100 + 50 + 10 + 200 + 30 + 40 + 20 + 60 + 15;

        write(root, "Default/Cookies", 500);
        write(root, "Default/Local Storage/leveldb/000003.log", 300);
        write(root, "Default/IndexedDB/https_example.com_0/1.sqlite", 250);
        write(root, "Default/History", 120);
        write(root, "Default/Bookmarks", 80);
        write(root, "Local State", 45);
        let keep = 500 + 300 + 250 + 120 + 80 + 45;
        (cache, keep)
    }

    /// Danh sách file nhạy cảm phải còn nguyên sau clear (giữ session đăng nhập).
    const SENSITIVE: &[&str] = &[
        "Default/Cookies",
        "Default/Local Storage/leveldb/000003.log",
        "Default/IndexedDB/https_example.com_0/1.sqlite",
        "Default/History",
        "Default/Bookmarks",
        "Local State",
    ];

    #[test]
    fn dir_size_walks_recursively_and_skips_symlinks() {
        let root = tmp_root();
        write(&root, "a/b/c.bin", 100);
        write(&root, "a/d.bin", 20);
        write(&root, "top.bin", 5);
        // Symlink trỏ ra file to bên ngoài — KHÔNG được tính vào tổng.
        let outside =
            std::env::temp_dir().join(format!("browserx-storage-out-{}", uuid::Uuid::new_v4()));
        fs::write(&outside, vec![0u8; 10_000]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("a/link.bin")).unwrap();

        assert_eq!(dir_size(&root), 125);
        assert_eq!(dir_size(&root.join("missing")), 0);
        fs::remove_dir_all(&root).unwrap();
        fs::remove_file(&outside).unwrap();
    }

    #[test]
    fn clear_cache_removes_only_cache_dirs_and_reports_freed() {
        let root = tmp_root();
        let (cache, keep) = fake_profile(&root);
        assert_eq!(dir_size(&root), cache + keep);

        let freed = clear_cache(&root).unwrap();
        assert_eq!(freed, cache);
        for rel in CACHE_DIRS {
            assert!(!root.join(rel).exists(), "{rel} phải bị xoá");
        }
        for rel in SENSITIVE {
            assert!(root.join(rel).exists(), "{rel} phải còn nguyên");
        }
        assert_eq!(dir_size(&root), keep);
        // Idempotent: chạy lại giải phóng 0; dir không tồn tại → 0.
        assert_eq!(clear_cache(&root).unwrap(), 0);
        assert_eq!(clear_cache(&root.join("missing")).unwrap(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn clear_storage_options_removes_only_disabled_kinds() {
        let root = tmp_root();
        write(&root, "Default/History", 120);
        write(&root, "Default/History-journal", 10);
        write(&root, "Default/Visited Links", 30);
        write(&root, "Default/Login Data", 200);
        write(&root, "Default/Login Data For Account", 50);
        write(&root, "Default/Service Worker/CacheStorage/abc/index", 400);
        write(&root, "Default/Service Worker/Database/db", 40);
        write(&root, "Default/Cookies", 500);
        write(&root, "Default/Bookmarks", 80);

        // Mọi thứ bật → không xoá gì.
        assert_eq!(clear_storage_options(&root, true, true, true).unwrap(), 0);
        assert!(root.join("Default/History").exists());

        // Tắt history → chỉ history biến mất.
        let freed = clear_storage_options(&root, false, true, true).unwrap();
        assert_eq!(freed, 120 + 10 + 30);
        assert!(!root.join("Default/History").exists());
        assert!(root.join("Default/Login Data").exists());
        assert!(root.join("Default/Service Worker").exists());

        // Tắt passwords + sw cache → xoá nốt, cookies/bookmarks còn nguyên.
        let freed = clear_storage_options(&root, true, false, false).unwrap();
        assert_eq!(freed, 200 + 50 + 400 + 40);
        assert!(!root.join("Default/Login Data").exists());
        assert!(!root.join("Default/Service Worker").exists());
        assert!(root.join("Default/Cookies").exists());
        assert!(root.join("Default/Bookmarks").exists());

        // Idempotent + dir không tồn tại → 0.
        assert_eq!(clear_storage_options(&root, false, false, false).unwrap(), 0);
        assert_eq!(
            clear_storage_options(&root.join("missing"), false, false, false).unwrap(),
            0
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn clear_refused_while_running_then_ok_after_stop() {
        let pm = ProcessManager::new(2);
        pm.spawn("p1", "sleep", vec!["30".to_string()], 1)
            .await
            .unwrap();
        let root = tmp_root();
        let (cache, _keep) = fake_profile(&root);

        let res = clear_profiles_cache(&pm, vec![("p1".to_string(), root.clone())]).await;
        assert_eq!(res.len(), 1);
        assert!(res[0].error.is_some(), "phải refuse khi profile đang chạy");
        assert_eq!(res[0].freed_bytes, 0);
        assert!(root.join("Cache").exists(), "không được đụng dữ liệu");

        pm.stop("p1").await.unwrap();
        let res = clear_profiles_cache(&pm, vec![("p1".to_string(), root.clone())]).await;
        assert!(res[0].error.is_none());
        assert_eq!(res[0].freed_bytes, cache);
        assert!(!root.join("Cache").exists());
        for rel in SENSITIVE {
            assert!(root.join(rel).exists(), "{rel} phải còn nguyên");
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
