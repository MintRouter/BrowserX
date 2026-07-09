//! (W56a) Dọn stale Chromium SingletonLock trước khi launch.
//!
//! Port từ VFlowX `internal/gologin/profile_lock.go`: Chromium giữ single-instance
//! lock bằng 3 file `SingletonLock` / `SingletonCookie` / `SingletonSocket` trong
//! user-data-dir. Phiên bị kill -9 để lại lock → lần launch sau Chromium tưởng
//! instance cũ còn sống và abort. Trên macOS/Linux `SingletonLock` là symlink có
//! target dạng `<hostname>-<pid>` — đọc PID từ TARGET symlink (KHÔNG đọc nội dung file):
//! - PID còn sống (và != PID mình) → chặn launch, KHÔNG xoá lock (phiên thật đang chạy,
//!   xoá sẽ dẫn tới 2 Chromium ghi cùng user-data-dir → hỏng profile).
//! - PID chết / lock malformed / không đọc được / Windows (lock là file thường,
//!   không encode PID) → sweep best-effort cả 3 file, launch tiếp tục.

use std::path::Path;

/// 3 file single-instance của Chromium có thể sót lại sau khi phiên bị kill.
const SINGLETON_FILES: [&str; 3] = ["SingletonLock", "SingletonCookie", "SingletonSocket"];

/// Lỗi typed: SingletonLock thuộc về PID còn sống → phiên trước còn chạy thật.
/// Caller chặn launch với message user-facing này; lock được GIỮ NGUYÊN.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("phiên trước của profile {profile_id} còn chạy (pid={pid}) — hãy đóng phiên đó trước khi mở lại")]
pub struct PrevSessionAlive {
    pub profile_id: String,
    pub pid: i32,
}

/// Kiểm tra + dọn lock trong `user_data_dir` trước khi spawn.
/// `Ok(())` = không có lock / đã dọn xong → launch tiếp;
/// `Err(PrevSessionAlive)` = PID trong lock còn sống → caller chặn launch.
pub fn cleanup_stale_profile_lock(
    user_data_dir: &Path,
    profile_id: &str,
) -> Result<(), PrevSessionAlive> {
    let lock_path = user_data_dir.join("SingletonLock");
    let Some(pid) = read_singleton_lock_pid(&lock_path) else {
        // Không có lock / target không đọc được / Windows → sweep best-effort
        // để lock malformed không chặn launch (Chromium tự tạo lock mới).
        remove_stale_singleton_files(user_data_dir, profile_id);
        return Ok(());
    };
    // PID trùng chính app này (PID wraparound hiếm gặp) → coi là stale.
    if process_alive(pid) && pid as u32 != std::process::id() {
        tracing::warn!(
            "profile_lock {profile_id}: SingletonLock thuộc PID {pid} còn sống — chặn launch, giữ nguyên lock"
        );
        return Err(PrevSessionAlive {
            profile_id: profile_id.to_string(),
            pid,
        });
    }
    tracing::info!("profile_lock {profile_id}: dọn SingletonLock stale từ phiên trước (pid={pid})");
    remove_stale_singleton_files(user_data_dir, profile_id);
    Ok(())
}

/// Đọc PID từ TARGET của symlink SingletonLock (dạng `<hostname>-<pid>`).
/// `None` khi: không có lock, không phải symlink, target không đúng dạng, hoặc
/// đang chạy Windows (lock là file thường) — caller rơi xuống nhánh sweep.
fn read_singleton_lock_pid(lock_path: &Path) -> Option<i32> {
    if cfg!(windows) {
        return None;
    }
    let target = std::fs::read_link(lock_path).ok()?;
    let target = target.to_str()?.trim().to_string();
    let (_, pid_str) = target.rsplit_once('-')?;
    let pid: i32 = pid_str.parse().ok()?;
    (pid > 0).then_some(pid)
}

/// PID còn signal được không (kill 0 — probe no-op). EPERM (process của user
/// khác — không thể là Chromium do app này spawn cùng user) coi như chết →
/// dọn lock thay vì chặn launch oan, giống port Go (Signal(0) err → false).
#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

/// Windows không tới nhánh này (read_singleton_lock_pid trả None) — liveness
/// qua PID không tin cậy nên luôn coi là stale, khớp port Go.
#[cfg(not(unix))]
fn process_alive(_pid: i32) -> bool {
    false
}

/// Xoá best-effort cả 3 singleton files (xoá chính symlink, không theo target).
/// File không tồn tại / dir không tồn tại → bỏ qua; lỗi khác chỉ warn,
/// KHÔNG BAO GIỜ chặn launch. Idempotent.
fn remove_stale_singleton_files(user_data_dir: &Path, profile_id: &str) {
    for name in SINGLETON_FILES {
        let path = user_data_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!("profile_lock {profile_id}: đã xoá {name} stale"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!("profile_lock {profile_id}: không xoá được {name}: {e}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_udd() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "browserx-profile-lock-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    // (1) user-data-dir không tồn tại → no-op, không lỗi.
    #[test]
    fn missing_dir_is_noop() {
        let dir = std::env::temp_dir().join(format!("browserx-plock-missing-{}", uuid::Uuid::new_v4()));
        assert_eq!(cleanup_stale_profile_lock(&dir, "p1"), Ok(()));
        assert!(!dir.exists());
    }

    // (2) Dir tồn tại nhưng không có lock → no-op.
    #[test]
    fn no_lock_is_noop() {
        let udd = temp_udd();
        assert_eq!(cleanup_stale_profile_lock(&udd.0, "p1"), Ok(()));
    }

    // (3) Lock của PID chết → cả 3 singleton files bị xoá, launch tiếp.
    #[cfg(unix)]
    #[test]
    fn stale_dead_pid_lock_removed() {
        let udd = temp_udd();
        // PID 99999999 vượt PID_MAX của macOS/Linux → chắc chắn chết (ESRCH).
        std::os::unix::fs::symlink("myhost-99999999", udd.0.join("SingletonLock")).unwrap();
        std::os::unix::fs::symlink("myhost-99999999", udd.0.join("SingletonCookie")).unwrap();
        std::fs::write(udd.0.join("SingletonSocket"), b"").unwrap();

        assert_eq!(cleanup_stale_profile_lock(&udd.0, "p1"), Ok(()));
        for name in SINGLETON_FILES {
            assert!(
                std::fs::symlink_metadata(udd.0.join(name)).is_err(),
                "{name} phải bị xoá"
            );
        }
    }

    // (4) Lock của PID còn sống (khác PID mình) → Err typed, lock GIỮ NGUYÊN.
    #[cfg(unix)]
    #[test]
    fn live_pid_lock_blocks_launch() {
        let udd = temp_udd();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        std::os::unix::fs::symlink(format!("myhost-{pid}"), udd.0.join("SingletonLock")).unwrap();

        let res = cleanup_stale_profile_lock(&udd.0, "p1");
        let keep = std::fs::symlink_metadata(udd.0.join("SingletonLock")).is_ok();
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            res,
            Err(PrevSessionAlive { profile_id: "p1".into(), pid })
        );
        assert!(keep, "lock của phiên sống KHÔNG được xoá");
        assert!(res.unwrap_err().to_string().contains(&format!("pid={pid}")));
    }

    // (5) Lock malformed (target không có PID) → không chặn, sweep sạch.
    #[cfg(unix)]
    #[test]
    fn malformed_lock_swept_not_blocking() {
        let udd = temp_udd();
        std::os::unix::fs::symlink("khong-co-pid-x", udd.0.join("SingletonLock")).unwrap();
        std::fs::write(udd.0.join("SingletonCookie"), b"junk").unwrap();

        assert_eq!(cleanup_stale_profile_lock(&udd.0, "p1"), Ok(()));
        assert!(std::fs::symlink_metadata(udd.0.join("SingletonLock")).is_err());
        assert!(std::fs::symlink_metadata(udd.0.join("SingletonCookie")).is_err());
    }

    // (6) Lock là file thường (kiểu Windows) → không đọc PID được → sweep, không chặn.
    #[test]
    fn regular_file_lock_swept() {
        let udd = temp_udd();
        std::fs::write(udd.0.join("SingletonLock"), b"whatever").unwrap();

        assert_eq!(cleanup_stale_profile_lock(&udd.0, "p1"), Ok(()));
        assert!(std::fs::symlink_metadata(udd.0.join("SingletonLock")).is_err());
    }

    // (7) Lock trỏ về chính PID app (wraparound) → coi là stale, dọn được.
    #[cfg(unix)]
    #[test]
    fn own_pid_lock_treated_stale() {
        let udd = temp_udd();
        let own = std::process::id();
        std::os::unix::fs::symlink(format!("myhost-{own}"), udd.0.join("SingletonLock")).unwrap();

        assert_eq!(cleanup_stale_profile_lock(&udd.0, "p1"), Ok(()));
        assert!(std::fs::symlink_metadata(udd.0.join("SingletonLock")).is_err());
    }

    // (8) Parse PID: hostname chứa '-' vẫn lấy đúng PID cuối.
    #[cfg(unix)]
    #[test]
    fn pid_parse_hostname_with_dashes() {
        let udd = temp_udd();
        std::os::unix::fs::symlink("my-mac-mini-99999999", udd.0.join("SingletonLock")).unwrap();
        assert_eq!(
            read_singleton_lock_pid(&udd.0.join("SingletonLock")),
            Some(99999999)
        );
    }
}
