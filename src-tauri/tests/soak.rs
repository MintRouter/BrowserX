//! W5b: Soak harness (integration, `#[ignore]`).
//!
//! Spawn 10 phiên concurrent qua `launcher::build_args` + `ProcessManager`,
//! giữ chạy trong `SOAK_SECS` giây (env, default 1800 = 30 phút), poll CDP
//! `/json/version` mỗi 30s cho từng phiên, cuối cùng in:
//!   launched N/10, alive_end M/10 — cùng ghi chú RSS tổng (best-effort).
//! Yêu cầu tỉ lệ launch ≥ 99%. Teardown kill hết + dọn thư mục tạm.
//!
//! Chạy bản ngắn để chứng minh harness:
//!   `SOAK_SECS=60 cargo test --test soak -- --ignored --nocapture`
//! Coordinator chạy bản 30 phút với default (không set SOAK_SECS).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, cdp};

const N: usize = 10;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-soak-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Định vị binary (cache có sẵn); timeout 120s. None → SKIP.
async fn ensure_bin() -> Option<PathBuf> {
    match tokio::time::timeout(Duration::from_secs(120), binary::ensure_binary(None, None)).await {
        Ok(Ok(p)) => Some(p),
        Ok(Err(e)) => {
            println!("[soak] SKIP — giới hạn môi trường: chưa có binary ({e})");
            None
        }
        Err(_) => {
            println!("[soak] SKIP — giới hạn môi trường: ensure_binary timeout 120s");
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

/// 10 phiên concurrent giữ SOAK_SECS giây, poll 30s/lần, ≥99% launch.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn soak_ten_concurrent_sessions() {
    let secs: u64 = std::env::var("SOAK_SECS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1800);
    println!("[soak] SOAK_SECS={secs}s, N={N}");

    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => return,
    };

    let pm = ProcessManager::new(N);
    let mut sessions: Vec<Session> = Vec::with_capacity(N);
    let mut launched = 0usize;

    for i in 0..N {
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
    println!("[soak] launched {launched}/{N}");

    // Poll mỗi 30s (hoặc phần còn lại) cho tới hết SOAK_SECS.
    let start = Instant::now();
    let deadline = start + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::sleep(remaining.min(Duration::from_secs(30))).await;
        let mut alive = 0usize;
        for s in &sessions {
            if cdp_alive(s.port).await {
                alive += 1;
            }
        }
        println!(
            "[soak] t={}s alive {}/{}",
            start.elapsed().as_secs(),
            alive,
            N
        );
    }

    // Ảnh chụp cuối trước khi teardown.
    let mut alive_end = 0usize;
    for s in &sessions {
        if cdp_alive(s.port).await {
            alive_end += 1;
        }
    }
    let pids: Vec<u32> = sessions.iter().map(|s| s.pid).collect();
    let rss_mb = total_rss_mb(&pids);

    // Teardown sạch: kill từng pid + dọn thư mục tạm.
    for s in sessions {
        let _ = pm.stop(&s.profile_id).await;
        drop(s.db);
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    println!(
        "[soak] KẾT QUẢ: launched {launched}/{N}, alive_end {alive_end}/{N}, RSS≈{rss_mb} MB"
    );

    let ratio = launched as f64 / N as f64;
    assert!(
        ratio >= 0.99,
        "tỉ lệ launch {launched}/{N} ({:.0}%) < 99%",
        ratio * 100.0
    );
}
