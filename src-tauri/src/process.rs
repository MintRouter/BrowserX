//! Process manager: registry các phiên đang chạy (profile_id → pid/cdp_port/child),
//! giới hạn concurrency bằng `tokio::Semaphore`, cấp cổng CDP tự do (bind-test),
//! theo dõi child chết (watchdog) và cleanup khi stop/crash.
//!
//! Nguyên tắc port (docs/03 + refs Manager): stop = kill(pid) TRỰC TIẾP (không pkill),
//! headful spawn qua `tokio::process::Command`. Liveness dựa trên `Child::try_wait()`
//! (KHÔNG kill(pid,0)/ps — cả hai coi zombie là "sống"); mọi đường đọc trạng thái
//! (`spawn`/`is_running`/`list_running`) đều reap child đã thoát để không còn zombie
//! và trả slot semaphore ngay. `start_watchdog` nhận callback `(profile_id, clean)`
//! để W3a emit `profile://status` (stopped/crashed).
//!
//! Public cho W3a: `ProcessManager::new`, `recommended_max_concurrent`,
//! `allocate_cdp_port`, `spawn`, `stop`, `list_running`, `is_running`, `start_watchdog`.
//! W25b thêm `begin_maintenance` (cờ chặn spawn trong lúc backup/restore).
//!
//! Wave 2c; W6 fix zombie reaping + cap theo RAM.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

use crate::error::{AppError, Result};
use crate::models::RunningSession;

/// Default max concurrent — CHỈ dùng làm fallback khi settings chưa cung cấp
/// VÀ không đọc được RAM host. KHÔNG còn là trần tuyệt đối:
/// `recommended_max_concurrent` scale theo RAM (xem `cap_from_ram_gib`).
pub const DEFAULT_MAX_CONCURRENT: usize = 8;

/// RAM vật lý của host (bytes), best-effort. macOS: `sysctl -n hw.memsize`;
/// Linux: `/proc/meminfo`. None nếu không đọc được.
fn host_ram_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = s
            .lines()
            .find(|l| l.starts_with("MemTotal:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        Some(kb * 1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Lõi công thức cap theo RAM (thuần, test được): chừa 4 GiB cho OS/app,
/// ngân sách ~2.5 GiB mỗi phiên Chromium headful (soak thực tế trên macOS 24 GiB:
/// N=10 mất ổn định, N=8 ổn định 30 phút), kẹp trong [1, 64].
/// Ví dụ: 8 GiB → 1; 16 GiB → 4; 24 GiB → 8; 32 GiB → 11; 64 GiB → 24.
fn cap_from_ram_gib(ram_gib: u64) -> usize {
    ((ram_gib.saturating_sub(4) * 2 / 5) as usize).clamp(1, 64)
}

/// Cap concurrency an toàn, tỉ lệ theo RAM host (không còn trần cứng 8):
/// map `host_ram_bytes()` qua `cap_from_ram_gib`. Khi không đọc được RAM
/// → fallback `DEFAULT_MAX_CONCURRENT`.
pub fn recommended_max_concurrent() -> usize {
    match host_ram_bytes() {
        Some(bytes) => cap_from_ram_gib(bytes >> 30),
        None => DEFAULT_MAX_CONCURRENT,
    }
}

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

/// Thông báo lỗi khi spawn bị cờ maintenance chặn (W25b).
const MAINTENANCE_MSG: &str = "backup/restore đang chạy — đợi hoàn tất rồi mở lại profile";

/// (W25b) RAII guard của chế độ bảo trì backup/restore: drop là clear cờ,
/// nên mọi error path/early-return đều không kẹt cờ chặn launch mãi mãi.
pub struct MaintenanceGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for MaintenanceGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

/// Quản lý toàn bộ phiên browser đang chạy.
#[derive(Clone)]
pub struct ProcessManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
    /// (W25b) true khi backup/restore đang đụng filesystem → spawn bị chặn.
    /// Set/check DƯỚI lock `sessions` để đóng race với check "không phiên chạy".
    maintenance: Arc<AtomicBool>,
}

impl ProcessManager {
    pub fn new(max_concurrent: usize) -> Self {
        let max = max_concurrent.max(1);
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(max)),
            max_concurrent: max,
            maintenance: Arc::new(AtomicBool::new(false)),
        }
    }

    /// (W25b) Bật chế độ bảo trì cho backup/restore: verify "không phiên chạy"
    /// VÀ set cờ dưới CÙNG lock registry — đóng race TOCTOU giữa check
    /// `list_running()` của commands và thao tác FS. Fail nếu còn phiên chạy
    /// hoặc một backup/restore khác đang giữ cờ.
    pub async fn begin_maintenance(&self) -> Result<MaintenanceGuard> {
        let mut map = self.sessions.lock().await;
        Self::reap_locked(&mut map);
        if !map.is_empty() {
            return Err(AppError::InvalidInput(
                "stop all running profiles before backup/restore".into(),
            ));
        }
        if self
            .maintenance
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(AppError::InvalidInput(
                "another backup/restore is already in progress".into(),
            ));
        }
        Ok(MaintenanceGuard {
            flag: Arc::clone(&self.maintenance),
        })
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
        // (W25c) Giữ lock registry XUYÊN SUỐT check → spawn → insert:
        // begin_maintenance set cờ dưới CÙNG lock này, nên hoặc spawn thấy cờ
        // TRƯỚC khi spawn (từ chối sạch), hoặc backup thấy session đã đăng ký
        // (begin fail) — không còn cửa sổ "spawn rồi kill" giữa hai lần check.
        let mut map = self.sessions.lock().await;
        // Reap trước khi kiểm tra: phiên đã chết không được chặn relaunch
        // hay giữ slot semaphore.
        Self::reap_locked(&mut map);
        if self.maintenance.load(Ordering::SeqCst) {
            return Err(AppError::Launch(MAINTENANCE_MSG.into()));
        }
        if map.contains_key(profile_id) {
            return Err(AppError::Launch(format!("profile {profile_id} đang chạy")));
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
        map.insert(profile_id.to_string(), session);
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

    /// Danh sách phiên đang chạy — reap child đã thoát trước để KHÔNG trả về zombie.
    pub async fn list_running(&self) -> Vec<RunningSession> {
        let mut map = self.sessions.lock().await;
        Self::reap_locked(&mut map);
        map.iter().map(|(id, s)| s.to_running(id)).collect()
    }

    /// Phiên có ĐANG SỐNG THẬT không: `try_wait()` trên child handle — zombie
    /// (đã thoát nhưng chưa reap) bị reap tại chỗ và trả `false`.
    /// KHÔNG dùng kill(pid,0)/ps: cả hai coi zombie là "còn sống".
    pub async fn is_running(&self, profile_id: &str) -> bool {
        let mut map = self.sessions.lock().await;
        match map.get_mut(profile_id) {
            Some(s) => {
                if matches!(s.child.try_wait(), Ok(Some(_)) | Err(_)) {
                    map.remove(profile_id);
                    false
                } else {
                    true
                }
            }
            None => false,
        }
    }

    /// Reap mọi child đã thoát trong `map` (khi ĐANG GIỮ lock): `try_wait()` thu hồi
    /// zombie, remove khỏi registry → drop `_permit` trả slot semaphore.
    /// Trả `(profile_id, clean)` — `clean=true` nếu exit code 0.
    fn reap_locked(map: &mut HashMap<String, Session>) -> Vec<(String, bool)> {
        let dead: Vec<(String, bool)> = map
            .iter_mut()
            .filter_map(|(id, s)| match s.child.try_wait() {
                Ok(Some(status)) => Some((id.clone(), status.success())),
                Err(_) => Some((id.clone(), false)),
                Ok(None) => None,
            })
            .collect();
        for (id, _) in &dead {
            map.remove(id);
        }
        dead
    }

    /// Quét một lượt, reap child đã chết (crash/đóng thủ công) và xoá khỏi registry
    /// (giải phóng permit). Trả `(profile_id, clean)` để caller emit sự kiện.
    pub async fn reap_dead(&self) -> Vec<(String, bool)> {
        let mut map = self.sessions.lock().await;
        Self::reap_locked(&mut map)
    }

    /// Watchdog nền: mỗi `interval_ms` gọi `reap_dead`, gọi `on_dead(profile_id, clean)`
    /// cho từng phiên vừa bị dọn (để emit `profile://status`). Trả handle để abort.
    pub fn start_watchdog<F>(&self, interval_ms: u64, on_dead: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn(&str, bool) + Send + 'static,
    {
        let this = self.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_millis(interval_ms.max(50)));
            loop {
                ticker.tick().await;
                for (id, clean) in this.reap_dead().await {
                    on_dead(&id, clean);
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lệnh dài để làm phiên "giả" (không cần browser thật): `sleep` trên unix,
    /// `ping -n` trên Windows (runner không chắc có `sleep.exe` trong PATH).
    #[cfg(unix)]
    fn long_running() -> (&'static str, Vec<String>) {
        ("sleep", vec!["30".to_string()])
    }
    #[cfg(windows)]
    fn long_running() -> (&'static str, Vec<String>) {
        ("ping", vec!["-n".into(), "31".into(), "127.0.0.1".into()])
    }

    /// Lệnh thoát ngay để mô phỏng child chết: `true` trên unix, `cmd /C exit 0`
    /// trên Windows.
    #[cfg(unix)]
    fn quick_exit() -> (&'static str, Vec<String>) {
        ("true", vec![])
    }
    #[cfg(windows)]
    fn quick_exit() -> (&'static str, Vec<String>) {
        ("cmd", vec!["/C".into(), "exit 0".into()])
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
        let (qprog, qargs) = quick_exit();
        pm.spawn("quick", qprog, qargs, 20).await.unwrap();
        // Chờ process thoát.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let dead = pm.reap_dead().await;
        assert_eq!(dead, vec![("quick".to_string(), true)]);
        assert!(!pm.is_running("quick").await);
        // Slot đã được giải phóng → spawn mới thành công dù max_concurrent=1.
        let (prog, args) = long_running();
        assert!(pm.spawn("next", prog, args, 21).await.is_ok());
        let _ = pm.stop("next").await;
    }

    /// Zombie (child thoát nhưng chưa wait) KHÔNG được coi là "đang chạy":
    /// is_running/list_running phải reap tại chỗ và phản ánh sống thật.
    #[tokio::test]
    async fn zombie_not_reported_as_running() {
        let pm = ProcessManager::new(2);
        let (qprog, qargs) = quick_exit();
        pm.spawn("z", qprog, qargs, 30).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        // Child là zombie tại đây (chưa ai gọi wait). kill(pid,0)/ps vẫn "thấy" nó,
        // nhưng is_running phải trả false nhờ try_wait.
        assert!(!pm.is_running("z").await);
        assert!(pm.list_running().await.is_empty());
        // Spawn lại cùng profile_id phải được (không bị "đang chạy" chặn).
        let (prog, args) = long_running();
        assert!(pm.spawn("z", prog, args, 31).await.is_ok());
        let _ = pm.stop("z").await;
    }

    /// Watchdog reap child thoát trong ≤ vài giây, báo callback + trả slot.
    #[tokio::test]
    async fn watchdog_reaps_and_reports() {
        let pm = ProcessManager::new(1);
        let reaped = Arc::new(std::sync::Mutex::new(Vec::<(String, bool)>::new()));
        let sink = Arc::clone(&reaped);
        let handle = pm.start_watchdog(50, move |id, clean| {
            sink.lock().unwrap().push((id.to_string(), clean));
        });

        let (qprog, qargs) = quick_exit();
        pm.spawn("w", qprog, qargs, 40).await.unwrap();
        // Chờ child thoát + watchdog tick.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        assert_eq!(*reaped.lock().unwrap(), vec![("w".to_string(), true)]);
        assert!(!pm.is_running("w").await);
        // Permit đã trả → spawn mới OK dù max=1.
        let (prog, args) = long_running();
        assert!(pm.spawn("next", prog, args, 41).await.is_ok());
        let _ = pm.stop("next").await;
        handle.abort();
    }

    /// (W25b) Cờ maintenance chặn spawn trong lúc backup/restore; guard drop
    /// là launch lại bình thường. Double-begin hoặc còn phiên chạy → begin fail.
    #[tokio::test]
    async fn maintenance_blocks_spawn_until_guard_dropped() {
        let pm = ProcessManager::new(2);
        let (prog, args) = long_running();

        let guard = pm.begin_maintenance().await.unwrap();
        // Đang giữ cờ: begin lần 2 bị từ chối, spawn bị chặn với lỗi rõ ràng.
        assert!(pm.begin_maintenance().await.is_err());
        let err = pm.spawn("m", prog, args.clone(), 50).await.unwrap_err();
        assert!(
            err.to_string().contains("backup/restore"),
            "unexpected error: {err}"
        );
        assert!(pm.list_running().await.is_empty());
        drop(guard);

        // Guard drop (kể cả error path nhờ RAII) → spawn lại bình thường;
        // begin khi còn phiên chạy phải fail.
        assert!(pm.spawn("m", prog, args, 51).await.is_ok());
        assert!(pm.begin_maintenance().await.is_err());
        let _ = pm.stop("m").await;
        assert!(pm.begin_maintenance().await.is_ok());
    }

    /// Cap theo RAM luôn nằm trong [1, 64] (RAM đọc được) hoặc = fallback.
    #[test]
    fn recommended_cap_in_bounds() {
        let cap = recommended_max_concurrent();
        assert!((1..=64).contains(&cap) || cap == DEFAULT_MAX_CONCURRENT);
    }

    /// Công thức cap theo RAM: reserve 4 GiB OS, ~2.5 GiB/phiên, clamp [1, 64].
    /// Kiểm biên (clamp min/max) + các mốc RAM phổ biến + monotonic.
    #[test]
    fn cap_from_ram_gib_formula() {
        assert_eq!(cap_from_ram_gib(4), 1); // clamp min
        assert_eq!(cap_from_ram_gib(6), 1);
        assert_eq!(cap_from_ram_gib(8), 1);
        assert_eq!(cap_from_ram_gib(16), 4);
        assert_eq!(cap_from_ram_gib(24), 8);
        assert_eq!(cap_from_ram_gib(32), 11);
        assert_eq!(cap_from_ram_gib(64), 24);
        assert_eq!(cap_from_ram_gib(200), 64); // clamp max
        // Monotonic không giảm theo RAM.
        let mut prev = 0;
        for gib in 0..=256 {
            let cap = cap_from_ram_gib(gib);
            assert!(cap >= prev);
            prev = cap;
        }
    }
}
