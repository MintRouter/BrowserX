//! W5b/W6: Soak harness (integration, `#[ignore]`).
//!
//! Spawn `SOAK_N` phiên concurrent (env, default = cap theo RAM host —
//! `process::recommended_max_concurrent`) qua `launcher::build_args` +
//! `ProcessManager`, giữ chạy `SOAK_SECS` giây (env, default 1800), poll CDP
//! `/json/version` mỗi 30s, watchdog reap chạy nền. Yêu cầu:
//! - launch ≥ 99%;
//! - alive ≥ 99% ở MỌI mốc poll;
//! - sau teardown KHÔNG còn zombie (ps stat 'Z') trong các pid đã track.
//!
//! Chạy bản ngắn để chứng minh harness:
//!   `SOAK_SECS=60 cargo test --test soak -- --ignored --nocapture`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::{recommended_max_concurrent, ProcessManager};
use browserx_lib::{binary, cdp};

/// Số phiên concurrent: env `SOAK_N` → default cap theo RAM host.
fn soak_n() -> usize {
    std::env::var("SOAK_N")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(recommended_max_concurrent)
        .max(1)
}

/// Đếm pid đang ở trạng thái zombie (`ps -o stat=` bắt đầu bằng 'Z').
fn zombie_count(pids: &[u32]) -> usize {
    if pids.is_empty() {
        return 0;
    }
    let args: Vec<String> = std::iter::once("-o".to_string())
        .chain(std::iter::once("stat=".to_string()))
        .chain(pids.iter().flat_map(|p| ["-p".to_string(), p.to_string()]))
        .collect();
    std::process::Command::new("ps")
        .args(&args)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.trim_start().starts_with('Z'))
                .count()
        })
        .unwrap_or(0)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-soak-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Định vị binary (cache có sẵn); timeout 120s. None → SKIPPED (không phải PASSED).
async fn ensure_bin() -> Option<PathBuf> {
    match tokio::time::timeout(Duration::from_secs(120), binary::ensure_binary(None, None)).await {
        Ok(Ok(p)) => Some(p),
        Ok(Err(e)) => {
            eprintln!("[soak] SKIPPED: no binary cache — chưa có binary ({e})");
            None
        }
        Err(_) => {
            eprintln!("[soak] SKIPPED: no binary cache — ensure_binary timeout 120s");
            None
        }
    }
}

/// Poll CDP `/json/version` (timeout ngắn) → true nếu phiên còn phản hồi.
async fn cdp_alive(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/json/version");
    matches!(
        tokio::time::timeout(Duration::from_secs(5), reqwest::get(&url)).await,
        Ok(Ok(r)) if r.status().is_success()
    )
}

/// Tổng RSS (MB) của các pid qua `ps -o rss=` (KB) — best-effort, 0 nếu lỗi.
fn total_rss_mb(pids: &[u32]) -> u64 {
    if pids.is_empty() {
        return 0;
    }
    let args: Vec<String> = std::iter::once("-o".to_string())
        .chain(std::iter::once("rss=".to_string()))
        .chain(pids.iter().flat_map(|p| ["-p".to_string(), p.to_string()]))
        .collect();
    std::process::Command::new("ps")
        .args(&args)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .filter_map(|s| s.parse::<u64>().ok())
                .sum::<u64>()
                / 1024
        })
        .unwrap_or(0)
}

struct Session {
    profile_id: String,
    port: u16,
    pid: u32,
    dir: PathBuf,
    db: Db,
}

/// SOAK_N phiên concurrent giữ SOAK_SECS giây, poll 30s/lần:
/// ≥99% launch, ≥99% alive mọi mốc, 0 zombie sau teardown.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn soak_ten_concurrent_sessions() {
    let secs: u64 = std::env::var("SOAK_SECS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1800);
    let n = soak_n();
    println!("[soak] SOAK_SECS={secs}s, N={n} (cap RAM host = {})", recommended_max_concurrent());

    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => {
            // Skip thật: test KHÔNG chạy — đừng đọc "ok" của libtest là bằng chứng PASSED.
            eprintln!("[soak] TEST SKIPPED (not passed): thiếu binary cache, không đo gì cả");
            return;
        }
    };

    let pm = ProcessManager::new(n);
    // Watchdog reap nền như trong app thật (lib.rs): zombie bị thu hồi ≤2s.
    let watchdog = pm.start_watchdog(2000, |id, clean| {
        println!("[soak] watchdog reap: {id} (clean={clean})");
    });
    let mut sessions: Vec<Session> = Vec::with_capacity(n);
    let mut launched = 0usize;

    for i in 0..n {
        let dir = temp_dir(&format!("s{i}"));
        let db = Db::open_at_dir(&dir).unwrap();
        let profile = db
            .create_profile(ProfileInput {
                name: format!("soak-{i}"),
                user_data_dir: Some(dir.join("udd").to_string_lossy().into_owned()),
                ..Default::default()
            })
            .unwrap();
        let port = pm.allocate_cdp_port().unwrap();
        let args = build_args(&profile, None, port);
        match pm.spawn(&profile.id, &bin, args, port).await {
            Ok(sess) => {
                match cdp::attach(port).await {
                    Ok(_) => {
                        launched += 1;
                        println!("[soak] #{i} launched pid {} cdp {}", sess.pid, port);
                    }
                    Err(e) => println!("[soak] #{i} spawn OK nhưng CDP attach lỗi: {e}"),
                }
                sessions.push(Session {
                    profile_id: profile.id,
                    port,
                    pid: sess.pid,
                    dir,
                    db,
                });
            }
            Err(e) => {
                println!("[soak] #{i} spawn thất bại: {e}");
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
    println!("[soak] launched {launched}/{n}");
    let pids: Vec<u32> = sessions.iter().map(|s| s.pid).collect();

    // Poll mỗi 30s (hoặc phần còn lại) cho tới hết SOAK_SECS.
    let start = Instant::now();
    let deadline = start + Duration::from_secs(secs);
    let mut min_alive = launched;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(remaining.min(Duration::from_secs(30))).await;
        let mut alive = 0usize;
        for s in &sessions {
            if cdp_alive(s.port).await {
                alive += 1;
            }
        }
        min_alive = min_alive.min(alive);
        println!(
            "[soak] t={}s alive {}/{} zombie={} tracked={} RSS≈{} MB",
            start.elapsed().as_secs(),
            alive,
            n,
            zombie_count(&pids),
            pm.list_running().await.len(),
            total_rss_mb(&pids)
        );
    }

    // Ảnh chụp cuối trước khi teardown.
    let mut alive_end = 0usize;
    for s in &sessions {
        if cdp_alive(s.port).await {
            alive_end += 1;
        }
    }
    let rss_mb = total_rss_mb(&pids);

    // Teardown sạch: kill từng pid + dọn thư mục tạm.
    for s in sessions {
        let _ = pm.stop(&s.profile_id).await;
        drop(s.db);
        let _ = std::fs::remove_dir_all(&s.dir);
    }
    watchdog.abort();

    let zombies_after = zombie_count(&pids);
    println!(
        "[soak] KẾT QUẢ: launched {launched}/{n}, min_alive {min_alive}/{launched}, \
         alive_end {alive_end}/{n}, RSS≈{rss_mb} MB, zombie sau teardown = {zombies_after}"
    );

    let ratio = launched as f64 / n as f64;
    assert!(
        ratio >= 0.99,
        "tỉ lệ launch {launched}/{n} ({:.0}%) < 99%",
        ratio * 100.0
    );
    let alive_ratio = if launched == 0 {
        1.0
    } else {
        min_alive as f64 / launched as f64
    };
    assert!(
        alive_ratio >= 0.99,
        "alive tối thiểu {min_alive}/{launched} ({:.0}%) < 99%",
        alive_ratio * 100.0
    );
    assert_eq!(
        zombies_after, 0,
        "còn {zombies_after} zombie sau teardown trong pids {pids:?}"
    );
}
