//! CookieRobot (P3-4a): bot nuôi cookie MVP theo R2 — visit TUẦN TỰ list URL
//! trên MỘT profile (không bulk song song), dwell 20–40s mặc định (KHÔNG 3 phút),
//! optional click consent GDPR, optional đóng browser khi xong.
//!
//! An toàn:
//! - **Proxy-guard**: commands check proxy TRƯỚC khi start; giữa chừng robot
//!   re-check trước mỗi URL — proxy chết → kill phiên NGAY (chống leak IP).
//! - **Cancel**: `CancelToken` (tokio watch) trong `RobotRegistry` theo
//!   profile_id; dwell là sleep cancellable nên stop có hiệu lực tức thì.
//!   `RobotGuard` (RAII) gỡ đăng ký khi robot kết thúc theo MỌI đường.
//! - Phiên browser luôn nằm trong ProcessManager (launch qua flow chuẩn) →
//!   không zombie; robot dừng thì phiên vẫn được quản lý bình thường.
//!
//! Progress qua Tauri event `cookierobot://progress`
//! `{profileId, current, total, url, phase, error}`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;

use crate::error::{AppError, Result};
use crate::process::ProcessManager;
use crate::{cdp, proxy_check};

/// Tên event progress phát cho FE.
pub const PROGRESS_EVENT: &str = "cookierobot://progress";

/// Dwell mặc định (dwell_secs = 0): random mỗi site trong [20, 40] giây.
pub const DEFAULT_DWELL_MIN: u32 = 20;
pub const DEFAULT_DWELL_MAX: u32 = 40;
/// Dwell override: clamp [3, 120] giây (3s cho phép test nhanh, 120s là trần
/// hợp lý — list dài + dwell dài tăng rủi ro proxy đứt giữa chừng, xem R2 §1.3).
pub const MIN_DWELL: u32 = 3;
pub const MAX_DWELL: u32 = 120;
/// Chờ trang ổn định sau goto trước khi click consent (banner render trễ).
const SETTLE_SECS: u64 = 2;
/// Sau Browser.close chờ tối đa 20 lần × 500ms cho tiến trình tự thoát.
const CLOSE_WAIT_ROUNDS: u32 = 20;

// ---------------------------------------------------------------------------
// Pure helpers (unit-test được, không cần browser)
// ---------------------------------------------------------------------------

/// Dwell hiệu lực cho MỘT site: 0 → `roll(min, max)` (random 20–40s, kết quả
/// vẫn bị kẹp lại phòng roll hỏng); khác 0 → clamp [MIN_DWELL, MAX_DWELL].
pub fn resolve_dwell(requested: u32, roll: impl FnOnce(u32, u32) -> u32) -> u32 {
    if requested == 0 {
        roll(DEFAULT_DWELL_MIN, DEFAULT_DWELL_MAX).clamp(DEFAULT_DWELL_MIN, DEFAULT_DWELL_MAX)
    } else {
        requested.clamp(MIN_DWELL, MAX_DWELL)
    }
}

/// Chuẩn hoá list URL: trim, bỏ dòng rỗng, thiếu scheme → thêm `https://`,
/// chỉ giữ http/https (scheme khác bị loại — goto scheme lạ vô nghĩa).
pub fn normalize_urls(urls: &[String]) -> Vec<String> {
    urls.iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .map(|u| {
            if u.contains("://") {
                u.to_string()
            } else {
                format!("https://{u}")
            }
        })
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .collect()
}

/// Fisher–Yates với nguồn random tiêm được (`rand_below(n)` → [0, n)) để
/// unit-test deterministic. Production dùng `rand::random_range`.
pub fn shuffle_with(items: &mut [String], mut rand_below: impl FnMut(usize) -> usize) {
    for i in (1..items.len()).rev() {
        let j = rand_below(i + 1) % (i + 1);
        items.swap(i, j);
    }
}

/// Keyword nhận diện nút chấp nhận cookie (lowercase, so khớp `includes`).
const CONSENT_KEYWORDS: &[&str] = &[
    "accept",
    "agree",
    "allow",
    "consent",
    "got it",
    "đồng ý",
    "chấp nhận",
    "cho phép",
    "akzeptieren",
    "accepter",
    "aceptar",
    "aceitar",
    "同意",
];

/// Selector của các consent-manager phổ biến (OneTrust, Funding Choices,
/// Google) — thử trước khi quét text để click đúng nút "Accept".
const CONSENT_SELECTORS: &[&str] = &[
    "#onetrust-accept-btn-handler",
    ".fc-cta-consent",
    "#L2AGLb",
    "button[aria-label*='ccept']",
];

/// Dựng JS click nút consent: thử selector known-manager trước, rồi quét
/// button/[role=button]/input theo keyword (label ≤ 40 ký tự tránh khớp nhầm
/// đoạn văn chứa "accept"). Trả `true` nếu đã click. Keyword/selector nhúng
/// qua serde_json để escape an toàn.
pub fn build_consent_js() -> String {
    let keywords = serde_json::to_string(CONSENT_KEYWORDS).expect("static array serializes");
    let selectors = serde_json::to_string(CONSENT_SELECTORS).expect("static array serializes");
    format!(
        r#"(() => {{
  const KEYWORDS = {keywords};
  const SELECTORS = {selectors};
  const norm = (t) => (t || "").replace(/\s+/g, " ").trim().toLowerCase();
  const matches = (t) => {{
    const s = norm(t);
    return s.length > 0 && s.length <= 40 && KEYWORDS.some((k) => s.includes(k));
  }};
  for (const sel of SELECTORS) {{
    const el = document.querySelector(sel);
    if (el) {{ el.click(); return true; }}
  }}
  const candidates = document.querySelectorAll(
    'button, [role="button"], input[type="button"], input[type="submit"]'
  );
  for (const el of candidates) {{
    const label = el.tagName === "INPUT" ? el.value : el.innerText;
    if (matches(label)) {{ el.click(); return true; }}
  }}
  return false;
}})()"#
    )
}

// ---------------------------------------------------------------------------
// Cancel token + registry
// ---------------------------------------------------------------------------

/// Token huỷ dựa trên `tokio::sync::watch`: `cancel()` đánh thức MỌI sleep
/// đang chờ ngay lập tức (watch không có race mất wakeup như Notify thô).
pub struct CancelToken {
    tx: watch::Sender<bool>,
}

impl CancelToken {
    fn new() -> Self {
        let (tx, _rx) = watch::channel(false);
        Self { tx }
    }

    pub fn cancel(&self) {
        self.tx.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.tx.borrow()
    }

    /// Ngủ `dur` nhưng dậy NGAY khi bị cancel. `true` = ngủ đủ, `false` = bị huỷ.
    pub async fn sleep(&self, dur: Duration) -> bool {
        if self.is_cancelled() {
            return false;
        }
        let mut rx = self.tx.subscribe();
        tokio::select! {
            _ = tokio::time::sleep(dur) => true,
            _ = rx.wait_for(|c| *c) => false,
        }
    }
}

/// Registry robot đang chạy theo profile_id — nằm trong `AppState`.
/// Chỉ cho phép MỘT robot mỗi profile (và bản chất MVP là 1 robot toàn cục
/// chạy tuần tự, không bulk).
#[derive(Clone, Default)]
pub struct RobotRegistry {
    inner: Arc<Mutex<HashMap<String, Arc<CancelToken>>>>,
}

impl RobotRegistry {
    /// Đăng ký robot cho profile — Err nếu profile này đã có robot chạy.
    /// Guard trả về gỡ đăng ký khi drop (mọi đường kết thúc của robot).
    pub fn begin(&self, profile_id: &str) -> Result<RobotGuard> {
        let mut map = self.inner.lock().expect("robot registry lock");
        if map.contains_key(profile_id) {
            return Err(AppError::InvalidInput(format!(
                "cookie robot đang chạy cho profile {profile_id}"
            )));
        }
        let token = Arc::new(CancelToken::new());
        map.insert(profile_id.to_string(), Arc::clone(&token));
        Ok(RobotGuard {
            registry: self.clone(),
            profile_id: profile_id.to_string(),
            token,
        })
    }

    /// Huỷ robot đang chạy. `false` nếu không có robot nào cho profile.
    pub fn cancel(&self, profile_id: &str) -> bool {
        match self.inner.lock().expect("robot registry lock").get(profile_id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Có robot đang đăng ký cho profile không.
    pub fn is_active(&self, profile_id: &str) -> bool {
        self.inner
            .lock()
            .expect("robot registry lock")
            .contains_key(profile_id)
    }
}

/// RAII: giữ suốt vòng đời robot; drop = gỡ khỏi registry (kể cả panic/return sớm).
pub struct RobotGuard {
    registry: RobotRegistry,
    profile_id: String,
    token: Arc<CancelToken>,
}

impl RobotGuard {
    pub fn token(&self) -> Arc<CancelToken> {
        Arc::clone(&self.token)
    }
}

impl Drop for RobotGuard {
    fn drop(&mut self) {
        self.registry
            .inner
            .lock()
            .expect("robot registry lock")
            .remove(&self.profile_id);
    }
}

// ---------------------------------------------------------------------------
// Progress event + run loop
// ---------------------------------------------------------------------------

/// Payload event `cookierobot://progress` (camelCase cho FE).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieRobotProgress {
    pub profile_id: String,
    /// Site đang xử lý (1-based); 0 ở phase "starting".
    pub current: usize,
    pub total: usize,
    /// URL đang xử lý; rỗng ở các phase toàn cục (starting/done/…).
    pub url: String,
    /// starting | proxy_check | goto | consent | dwell | closing | done |
    /// cancelled | error
    pub phase: String,
    pub error: Option<String>,
}

/// Job cho một lần chạy robot — dựng trong `start_cookie_robot` (commands.rs).
/// `proxy_url` chứa credential plaintext → KHÔNG log, chỉ dùng cho check.
pub struct RobotJob {
    pub profile_id: String,
    /// Đã normalize (+ shuffle nếu random_order) trong command.
    pub urls: Vec<String>,
    pub proxy_url: Option<String>,
    pub cdp_port: u16,
    /// 0 = random 20–40s mỗi site; khác 0 = clamp [3, 120].
    pub dwell_secs: u32,
    pub process_consent: bool,
    pub close_when_done: bool,
}

/// Vòng lặp robot — chạy trong task nền (spawn từ `start_cookie_robot`).
/// Với mỗi URL: proxy-guard → goto → settle → consent → dwell. Cancel có hiệu
/// lực ở mọi điểm chờ; proxy chết → kill phiên ngay + emit lỗi.
pub async fn run(app: AppHandle, procs: ProcessManager, guard: RobotGuard, job: RobotJob) {
    let token = guard.token();
    let total = job.urls.len();
    let consent_js = job.process_consent.then(build_consent_js);
    let emit = |current: usize, url: &str, phase: &str, error: Option<String>| {
        let _ = app.emit(
            PROGRESS_EVENT,
            CookieRobotProgress {
                profile_id: job.profile_id.clone(),
                current,
                total,
                url: url.to_string(),
                phase: phase.to_string(),
                error,
            },
        );
    };

    emit(0, "", "starting", None);
    for (idx, url) in job.urls.iter().enumerate() {
        let current = idx + 1;
        if token.is_cancelled() {
            emit(current, url, "cancelled", None);
            return;
        }
        // Phiên chết giữa chừng (user đóng browser/crash) → dừng robot sạch.
        if !procs.is_running(&job.profile_id).await {
            emit(current, url, "error", Some("browser session ended".into()));
            return;
        }
        // Proxy-guard giữa chừng: check trước MỖI URL; chết → kill phiên NGAY
        // (chống leak IP thật qua traffic tiếp theo).
        if let Some(purl) = &job.proxy_url {
            emit(current, url, "proxy_check", None);
            let check = proxy_check::check_proxy_url(purl).await;
            if !check.ok {
                let _ = procs.stop(&job.profile_id).await;
                crate::commands::emit_status(&app, &job.profile_id, "stopped", None, None);
                emit(
                    current,
                    url,
                    "error",
                    Some(format!(
                        "proxy chết giữa chừng — đã dừng phiên: {}",
                        check.error.unwrap_or_else(|| "unknown".into())
                    )),
                );
                return;
            }
            if token.is_cancelled() {
                emit(current, url, "cancelled", None);
                return;
            }
        }

        emit(current, url, "goto", None);
        if let Err(e) = cdp::goto(job.cdp_port, url).await {
            // Site hỏng/timeout → qua site kế, không chết cả run.
            tracing::warn!("cookierobot {}: goto {url} lỗi: {e}", job.profile_id);
            continue;
        }

        let dwell = u64::from(resolve_dwell(job.dwell_secs, |lo, hi| {
            rand::random_range(lo..=hi)
        }));
        let settle = SETTLE_SECS.min(dwell);
        if !token.sleep(Duration::from_secs(settle)).await {
            emit(current, url, "cancelled", None);
            return;
        }
        if let Some(js) = &consent_js {
            emit(current, url, "consent", None);
            if let Err(e) = cdp::eval(job.cdp_port, js).await {
                tracing::debug!("cookierobot {}: consent eval lỗi: {e}", job.profile_id);
            }
        }
        emit(current, url, "dwell", None);
        if !token.sleep(Duration::from_secs(dwell - settle)).await {
            emit(current, url, "cancelled", None);
            return;
        }
    }

    if job.close_when_done && !token.is_cancelled() {
        emit(total, "", "closing", None);
        // Shutdown MỀM (flush cookie xuống đĩa) rồi chờ tiến trình tự thoát;
        // còn sống thì stop cứng. Cuối cùng emit profile://status để UI cập
        // nhật (is_running đã reap nên watchdog không thấy nữa).
        if cdp::close_browser(job.cdp_port).await.is_ok() {
            for _ in 0..CLOSE_WAIT_ROUNDS {
                if !procs.is_running(&job.profile_id).await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        if procs.is_running(&job.profile_id).await {
            let _ = procs.stop(&job.profile_id).await;
        }
        crate::commands::emit_status(&app, &job.profile_id, "stopped", None, None);
    }
    emit(total, "", "done", None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolve_dwell_zero_rolls_default_range() {
        // roll được gọi với đúng biên mặc định.
        assert_eq!(
            resolve_dwell(0, |lo, hi| {
                assert_eq!((lo, hi), (DEFAULT_DWELL_MIN, DEFAULT_DWELL_MAX));
                33
            }),
            33
        );
        // Roll hỏng (ngoài biên) vẫn bị kẹp lại.
        assert_eq!(resolve_dwell(0, |_, _| 999), DEFAULT_DWELL_MAX);
        assert_eq!(resolve_dwell(0, |_, _| 1), DEFAULT_DWELL_MIN);
    }

    #[test]
    fn resolve_dwell_override_is_clamped() {
        let roll = |_: u32, _: u32| panic!("roll must not be called for override");
        assert_eq!(resolve_dwell(1, roll), MIN_DWELL);
        assert_eq!(resolve_dwell(3, roll), 3);
        assert_eq!(resolve_dwell(30, roll), 30);
        assert_eq!(resolve_dwell(120, roll), 120);
        assert_eq!(resolve_dwell(9999, roll), MAX_DWELL);
    }

    #[test]
    fn normalize_urls_trims_schemes_and_filters() {
        let input = strs(&[
            "  example.com  ",
            "http://a.com",
            "https://b.com/x?y=1",
            "",
            "   ",
            "ftp://bad.com",
            "www.c.com",
        ]);
        assert_eq!(
            normalize_urls(&input),
            strs(&[
                "https://example.com",
                "http://a.com",
                "https://b.com/x?y=1",
                "https://www.c.com",
            ])
        );
        assert!(normalize_urls(&[]).is_empty());
    }

    #[test]
    fn shuffle_with_is_deterministic_and_permutes() {
        // rand_below trả 0 → phần tử cuối dồn dần lên đầu.
        let mut v = strs(&["a", "b", "c"]);
        shuffle_with(&mut v, |_| 0);
        assert_eq!(v, strs(&["b", "c", "a"]));
        // rand_below = n-1 (giữ nguyên vị trí) → identity.
        let mut v = strs(&["a", "b", "c", "d"]);
        shuffle_with(&mut v, |n| n - 1);
        assert_eq!(v, strs(&["a", "b", "c", "d"]));
        // Luôn là hoán vị (không mất/không nhân đôi phần tử).
        let mut v = strs(&["1", "2", "3", "4", "5"]);
        shuffle_with(&mut v, |n| n / 2);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, strs(&["1", "2", "3", "4", "5"]));
        // Không panic với list rỗng / 1 phần tử.
        shuffle_with(&mut Vec::<String>::new(), |_| 0);
        let mut one = strs(&["x"]);
        shuffle_with(&mut one, |_| 0);
        assert_eq!(one, strs(&["x"]));
    }

    #[test]
    fn consent_js_embeds_keywords_and_selectors() {
        let js = build_consent_js();
        assert!(js.starts_with("(() =>"));
        assert!(js.ends_with("})()"));
        // Keyword/selector nhúng dạng JSON hợp lệ (escape an toàn).
        assert!(js.contains(&serde_json::to_string(CONSENT_KEYWORDS).unwrap()));
        assert!(js.contains(&serde_json::to_string(CONSENT_SELECTORS).unwrap()));
        assert!(js.contains("#onetrust-accept-btn-handler"));
        assert!(js.contains("\"accept\""));
        assert!(js.contains("el.click()"));
    }

    #[test]
    fn registry_rejects_duplicate_until_guard_dropped() {
        let reg = RobotRegistry::default();
        let guard = reg.begin("p1").unwrap();
        assert!(reg.is_active("p1"));
        assert!(reg.begin("p1").is_err());
        // Profile khác vẫn begin được (registry theo profile_id).
        let other = reg.begin("p2").unwrap();
        drop(other);
        // Guard drop → gỡ đăng ký → begin lại OK.
        drop(guard);
        assert!(!reg.is_active("p1"));
        assert!(reg.begin("p1").is_ok());
    }

    #[test]
    fn cancel_unknown_profile_returns_false() {
        let reg = RobotRegistry::default();
        assert!(!reg.cancel("ghost"));
    }

    #[tokio::test]
    async fn cancel_wakes_sleep_immediately() {
        let reg = RobotRegistry::default();
        let guard = reg.begin("p").unwrap();
        let token = guard.token();
        let sleeper = tokio::spawn(async move { token.sleep(Duration::from_secs(60)).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(reg.cancel("p"));
        // Sleep 60s phải dậy ngay (trong ≤2s) và báo "bị huỷ".
        let completed = tokio::time::timeout(Duration::from_secs(2), sleeper)
            .await
            .expect("sleep must wake right after cancel")
            .unwrap();
        assert!(!completed);
        assert!(guard.token().is_cancelled());
    }

    #[tokio::test]
    async fn sleep_completes_when_not_cancelled() {
        let reg = RobotRegistry::default();
        let guard = reg.begin("p").unwrap();
        assert!(guard.token().sleep(Duration::from_millis(10)).await);
    }

    #[tokio::test]
    async fn sleep_after_cancel_returns_false_instantly() {
        let reg = RobotRegistry::default();
        let guard = reg.begin("p").unwrap();
        guard.token().cancel();
        let started = std::time::Instant::now();
        assert!(!guard.token().sleep(Duration::from_secs(60)).await);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
