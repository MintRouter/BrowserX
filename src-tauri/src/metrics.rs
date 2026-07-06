//! (W26b) Launch metrics in-memory cho Observability panel: counter
//! success/fail (AtomicU64) + ring buffer duration ms (cap 100) để tính p95.
//!
//! Cố ý KHÔNG persist: audit chỉ ghi `profile.launch` khi THÀNH CÔNG (không
//! có duration, không có bản ghi lỗi) nên số liệu trung thực nhất là đếm
//! in-memory "since app start" — reset khi mở lại app, panel ghi rõ hạn chế.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Số mẫu duration giữ lại để tính p95 (mẫu cũ nhất bị đẩy ra).
const DURATION_CAP: usize = 100;

/// Bộ đếm launch từ lúc mở app. Field `AppState.metrics` (commands.rs).
#[derive(Default)]
pub struct LaunchMetrics {
    success: AtomicU64,
    fail: AtomicU64,
    /// Duration ms của các launch THÀNH CÔNG gần nhất (p95 chỉ tính launch ok).
    durations_ms: Mutex<VecDeque<u64>>,
}

impl LaunchMetrics {
    /// Ghi 1 launch thành công + duration của nó vào ring buffer.
    pub fn record_success(&self, duration_ms: u64) {
        self.success.fetch_add(1, Ordering::Relaxed);
        let mut buf = self.durations_ms.lock().unwrap_or_else(|e| e.into_inner());
        if buf.len() >= DURATION_CAP {
            buf.pop_front();
        }
        buf.push_back(duration_ms);
    }

    /// Ghi 1 launch thất bại (mọi lỗi trong `launch_profile`, kể cả CDP attach).
    pub fn record_fail(&self) {
        self.fail.fetch_add(1, Ordering::Relaxed);
    }

    /// `(success, fail, p95_ms)` — p95 `None` khi chưa có mẫu nào.
    pub fn snapshot(&self) -> (u64, u64, Option<u64>) {
        let buf = self.durations_ms.lock().unwrap_or_else(|e| e.into_inner());
        let samples: Vec<u64> = buf.iter().copied().collect();
        (
            self.success.load(Ordering::Relaxed),
            self.fail.load(Ordering::Relaxed),
            p95(&samples),
        )
    }
}

/// p95 theo nearest-rank: sort tăng dần, lấy phần tử thứ `ceil(0.95·n)`.
/// 0 mẫu → `None`; 1 mẫu → chính mẫu đó.
pub fn p95(samples: &[u64]) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * 0.95).ceil() as usize - 1;
    Some(sorted[idx.min(sorted.len() - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Edge: 0 mẫu → None (UI hiển thị N/A, không fake 0ms).
    #[test]
    fn p95_empty_is_none() {
        assert_eq!(p95(&[]), None);
    }

    /// Edge: 1 mẫu → chính mẫu đó.
    #[test]
    fn p95_single_sample() {
        assert_eq!(p95(&[1234]), Some(1234));
    }

    /// 100 mẫu 1..=100 → nearest-rank thứ 95; thứ tự đầu vào không quan trọng.
    #[test]
    fn p95_hundred_samples_nearest_rank() {
        let mut v: Vec<u64> = (1..=100).collect();
        v.reverse();
        assert_eq!(p95(&v), Some(95));
        assert_eq!(p95(&[10, 20]), Some(20));
        assert_eq!(p95(&[5, 5, 5]), Some(5));
    }

    /// Counter success/fail độc lập; snapshot trả đúng cả ba giá trị.
    #[test]
    fn counters_and_snapshot() {
        let m = LaunchMetrics::default();
        assert_eq!(m.snapshot(), (0, 0, None));
        m.record_success(100);
        m.record_success(300);
        m.record_fail();
        let (ok, fail, p) = m.snapshot();
        assert_eq!((ok, fail), (2, 1));
        assert_eq!(p, Some(300));
    }

    /// Ring buffer giữ tối đa 100 mẫu MỚI NHẤT — mẫu cũ bị đẩy ra khỏi p95.
    #[test]
    fn ring_buffer_caps_at_100() {
        let m = LaunchMetrics::default();
        m.record_success(999_999); // sẽ bị đẩy ra sau 100 mẫu mới
        for _ in 0..100 {
            m.record_success(50);
        }
        let (ok, _, p) = m.snapshot();
        assert_eq!(ok, 101);
        assert_eq!(p, Some(50));
    }
}
