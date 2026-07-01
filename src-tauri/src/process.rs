//! Process manager: registry các phiên đang chạy (profile_id → pid/cdp_port/child),
//! giới hạn concurrency bằng `tokio::Semaphore`, cấp cổng CDP tự do (bind-test),
//! theo dõi child chết (watchdog) và cleanup khi stop/crash.
//!
//! Nguyên tắc port (docs/03 + refs Manager): stop = kill(pid) TRỰC TIẾP (không pkill),
//! headful spawn qua `tokio::process::Command`. Emit event `profile://status` là việc
//! của W3a (cần AppHandle của Tauri) — module này giữ trạng thái nội bộ, decoupled & test được.
//!
//! Public cho W3a: `ProcessManager::new`, `allocate_cdp_port`, `spawn`, `stop`,
//! `list_running`, `is_running`, `start_watchdog`.
//!
//! Wave 2c.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;

use chrono::Utc;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

use crate::error::{AppError, Result};
use crate::models::RunningSession;

/// Default max concurrent khi settings chưa cung cấp. W3a sẽ override theo RAM.
/// (Không có API đọc RAM trong std; để hằng số bảo thủ + ghi chú cho W3a.)
pub const DEFAULT_MAX_CONCURRENT: usize = 8;

/// Một phiên đang chạy + tài nguyên gắn kèm (child process, permit semaphore).
struct Session {
    pid: u32,
    cdp_port: u16,
    started_at: String,
    child: Child,
    /// Giữ permit đến khi session bị xoá khỏi map → tự trả slot cho semaphore.
    _permit: OwnedSemaphorePermit,
}

impl Session {
    fn to_running(&self, profile_id: &str) -> RunningSession {
        RunningSession {
            profile_id: profile_id.to_string(),
            pid: self.pid,
            cdp_port: self.cdp_port,
            cdp_url: format!("http://127.0.0.1:{}", self.cdp_port),
            started_at: self.started_at.clone(),
        }
    }
}

/// Quản lý toàn bộ phiên browser đang chạy.
#[derive(Clone)]
pub struct ProcessManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl ProcessManager {
    pub fn new(max_concurrent: usize) -> Self {
        let max = max_concurrent.max(1);
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(max)),
            max_concurrent: max,
        }
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Cấp một cổng CDP tự do bằng cách bind thử `127.0.0.1:0` rồi nhả ra.
    /// Có race nhỏ (TOCTOU) — chấp nhận theo yêu cầu bind-test của task.
    pub fn allocate_cdp_port(&self) -> Result<u16> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AppError::Launch(format!("không cấp được cổng CDP: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Launch(format!("không đọc được cổng CDP: {e}")))?
            .port();
        Ok(port)
    }

    /// Spawn headful một phiên cho `profile_id`. `program` là đường dẫn binary Chromium,
    /// `args` là argv từ `launcher::build_args` (đã chứa --remote-debugging-port=<cdp_port>).
    /// Trả lỗi `Launch` khi đã đạt giới hạn concurrency hoặc profile đang chạy.
    pub async fn spawn(
        &self,
        profile_id: &str,
        program: &str,
        args: Vec<String>,
        cdp_port: u16,
    ) -> Result<RunningSession> {
        {
            let map = self.sessions.lock().await;
            if map.contains_key(profile_id) {
                return Err(AppError::Launch(format!(
                    "profile {profile_id} đang chạy"
                )));
            }
        }

        let permit = Arc::clone(&self.semaphore)
            .try_acquire_owned()
            .map_err(|_| {
                AppError::Launch(format!(
                    "đã đạt giới hạn {} phiên đồng thời",
                    self.max_concurrent
                ))
            })?;

        let mut child = Command::new(program)
            .args(&args)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Launch(format!("spawn thất bại: {e}")))?;

        let pid = child.id().ok_or_else(|| {
            let _ = child.start_kill();
            AppError::Launch("không lấy được PID sau khi spawn".into())
        })?;

        let session = Session {
            pid,
            cdp_port,
            started_at: Utc::now().to_rfc3339(),
            child,
            _permit: permit,
        };
        let running = session.to_running(profile_id);
        self.sessions
            .lock()
            .await
            .insert(profile_id.to_string(), session);
        Ok(running)
    }

    /// Dừng phiên: kill(pid) TRỰC TIẾP qua child handle (KHÔNG pkill), xoá khỏi registry
    /// (drop child + permit). Trả `NotFound` nếu profile không chạy.
    pub async fn stop(&self, profile_id: &str) -> Result<()> {
        let mut session = {
            let mut map = self.sessions.lock().await;
            map.remove(profile_id)
                .ok_or_else(|| AppError::NotFound(format!("profile {profile_id} không chạy")))?
        };
        session
            .child
            .kill()
            .await
            .map_err(|e| AppError::Launch(format!("kill thất bại: {e}")))?;
        Ok(())
    }

    /// Danh sách phiên đang chạy.
    pub async fn list_running(&self) -> Vec<RunningSession> {
        let map = self.sessions.lock().await;
        map.iter().map(|(id, s)| s.to_running(id)).collect()
    }

    pub async fn is_running(&self, profile_id: &str) -> bool {
        self.sessions.lock().await.contains_key(profile_id)
    }

    /// Quét định kỳ, phát hiện child đã chết (crash/đóng thủ công) và xoá khỏi registry
    /// (giải phóng permit). Trả về danh sách profile_id vừa bị dọn để W3a emit sự kiện.
    pub async fn reap_dead(&self) -> Vec<String> {
        let mut dead = Vec::new();
        let mut map = self.sessions.lock().await;
        let ids: Vec<String> = map.keys().cloned().collect();
        for id in ids {
            let exited = match map.get_mut(&id) {
                Some(s) => matches!(s.child.try_wait(), Ok(Some(_)) | Err(_)),
                None => false,
            };
            if exited {
                map.remove(&id);
                dead.push(id);
            }
        }
        dead
    }

    /// Watchdog nền: mỗi `interval_ms` gọi `reap_dead`. Trả về handle để W3a có thể abort.
    pub fn start_watchdog(&self, interval_ms: u64) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_millis(interval_ms.max(50)));
            loop {
                ticker.tick().await;
                let _ = this.reap_dead().await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lệnh dài để làm phiên "giả" (không cần browser thật) — dùng `sleep` trên unix.
    fn long_running() -> (&'static str, Vec<String>) {
        ("sleep", vec!["30".to_string()])
    }

    #[test]
    fn allocate_ports_are_valid_and_distinct() {
        let pm = ProcessManager::new(4);
        let a = pm.allocate_cdp_port().unwrap();
        let b = pm.allocate_cdp_port().unwrap();
        assert!(a >= 1024);
        assert!(b >= 1024);
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn semaphore_limits_concurrency() {
        let pm = ProcessManager::new(2);
        let (prog, args) = long_running();
        pm.spawn("p1", prog, args.clone(), 1).await.unwrap();
        pm.spawn("p2", prog, args.clone(), 2).await.unwrap();
        // Slot thứ 3 phải bị chặn bởi semaphore.
        let third = pm.spawn("p3", prog, args.clone(), 3).await;
        assert!(third.is_err());
        assert_eq!(pm.list_running().await.len(), 2);

        // Stop 1 phiên → giải phóng slot → spawn lại được.
        pm.stop("p1").await.unwrap();
        assert!(!pm.is_running("p1").await);
        let again = pm.spawn("p3", prog, args, 3).await;
        assert!(again.is_ok());

        // Dọn dẹp.
        let _ = pm.stop("p2").await;
        let _ = pm.stop("p3").await;
    }

    #[tokio::test]
    async fn duplicate_profile_rejected() {
        let pm = ProcessManager::new(4);
        let (prog, args) = long_running();
        pm.spawn("dup", prog, args.clone(), 10).await.unwrap();
        let second = pm.spawn("dup", prog, args, 11).await;
        assert!(second.is_err());
        let _ = pm.stop("dup").await;
    }

    #[tokio::test]
    async fn reap_dead_removes_exited_and_frees_slot() {
        let pm = ProcessManager::new(1);
        // Lệnh kết thúc ngay để mô phỏng child chết.
        pm.spawn("quick", "true", vec![], 20).await.unwrap();
        // Chờ process thoát.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let dead = pm.reap_dead().await;
        assert_eq!(dead, vec!["quick".to_string()]);
        assert!(!pm.is_running("quick").await);
        // Slot đã được giải phóng → spawn mới thành công dù max_concurrent=1.
        let (prog, args) = long_running();
        assert!(pm.spawn("next", prog, args, 21).await.is_ok());
        let _ = pm.stop("next").await;
    }
}
