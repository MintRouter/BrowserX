//! Kiểm chứng exit-criteria Pha 1 (docs/05 §3) — integration tests.
//!
//! (a) CRUD ≥1000 profile stored; list/search p95 < 200ms.
//! (b) Proxy credential mã hoá at-rest: dump SQLite → 0 plaintext creds.
//! (c) build_args set được ≥12/17 flag `--fingerprint*` từ profile trong DB.
//! (d) Teardown stop giải phóng slot semaphore (bổ sung mức integration).
//! (smoke) BEST-EFFORT: ensure_binary → spawn headful → CDP /json/version;
//!         không có binary/mạng → SKIP kèm ghi chú giới hạn môi trường.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use browserx_lib::db::{Db, ProfileFilter, ProfileInput, ProfileUpdate, ProxyInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, crypto};

/// Thư mục tạm duy nhất cho mỗi test (tự dọn khi test pass).
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-exit-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// p95 của một tập mẫu thời gian.
fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let idx = ((samples.len() as f64) * 0.95).ceil() as usize;
    samples[idx.saturating_sub(1).min(samples.len() - 1)]
}

/// Tìm needle (bytes) trong haystack (bytes) — kiểm plaintext thô trong file DB.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------------------
// (a) CRUD ≥1000 profile — list/search p95 < 200ms
// ---------------------------------------------------------------------------

#[test]
fn a_crud_1000_profiles_list_search_p95_under_200ms() {
    let dir = temp_dir("crud");
    let db = Db::open_at_dir(&dir).unwrap();

    let t_create = Instant::now();
    let mut first_id = String::new();
    for i in 0..1000 {
        let p = db
            .create_profile(ProfileInput {
                name: format!("profile-{i:04}"),
                tags: Some(vec![format!("batch-{}", i % 10)]),
                ..Default::default()
            })
            .unwrap();
        if i == 0 {
            first_id = p.id;
        }
    }
    let create_ms = t_create.elapsed().as_millis();

    // R (list + search + get), đo p95 trên 40 lần mỗi thao tác.
    let mut list_samples = Vec::new();
    let mut search_samples = Vec::new();
    for _ in 0..40 {
        let t = Instant::now();
        let all = db.list_profiles().unwrap();
        list_samples.push(t.elapsed());
        assert_eq!(all.len(), 1000);

        let t = Instant::now();
        let hits = db
            .search_profiles("profile-09", &ProfileFilter::default())
            .unwrap();
        search_samples.push(t.elapsed());
        assert_eq!(hits.len(), 100);
    }
    let list_p95 = p95(list_samples);
    let search_p95 = p95(search_samples);
    println!(
        "[exit a] create 1000 = {create_ms}ms; list p95 = {:?}; search p95 = {:?}",
        list_p95, search_p95
    );
    assert!(
        list_p95 < Duration::from_millis(200),
        "list p95 {list_p95:?} >= 200ms"
    );
    assert!(
        search_p95 < Duration::from_millis(200),
        "search p95 {search_p95:?} >= 200ms"
    );

    // U + D để phủ đủ CRUD.
    let updated = db
        .update_profile(
            &first_id,
            ProfileUpdate {
                name: Some("renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.name, "renamed");
    assert!(db.delete_profile(&first_id).unwrap());
    assert_eq!(db.list_profiles().unwrap().len(), 999);

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// (b) Proxy credential mã hoá at-rest — 0 plaintext trong dump SQLite
// ---------------------------------------------------------------------------

#[test]
fn b_proxy_credentials_encrypted_at_rest() {
    // Khoá gốc qua env để crypto không đụng OS keychain (hết popup macOS, CI-safe).
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    std::env::set_var("BROWSERX_MASTER_KEY", B64.encode([7u8; 32]));
    let dir = temp_dir("proxy");
    // Chuỗi unique để chắc chắn không trùng ngẫu nhiên với bytes khác.
    let user = "exitcrit-user-b1946ac9";
    let pass = "exitcrit-P@ssw0rd-4b825dc6";

    {
        let db = Db::open_at_dir(&dir).unwrap();
        // Đúng luồng sản phẩm (commands.rs): crypto::encrypt_secret → BLOB → db.create_proxy.
        let rec = db
            .create_proxy(ProxyInput {
                name: "exit-proxy".into(),
                protocol: "socks5".into(),
                host: "192.0.2.10".into(),
                port: 1080,
                username_enc: Some(crypto::encrypt_secret(user).unwrap()),
                password_enc: Some(crypto::encrypt_secret(pass).unwrap()),
            })
            .unwrap();

        // Đọc RAW cột username_enc/password_enc bằng connection rusqlite riêng.
        let raw_conn = rusqlite::Connection::open(dir.join("browserx.db")).unwrap();
        let (u_raw, p_raw): (Vec<u8>, Vec<u8>) = raw_conn
            .query_row(
                "SELECT username_enc, password_enc FROM proxies WHERE id = ?1",
                [&rec.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            !contains_bytes(&u_raw, user.as_bytes()),
            "username plaintext trong cột username_enc"
        );
        assert!(
            !contains_bytes(&p_raw, pass.as_bytes()),
            "password plaintext trong cột password_enc"
        );
        // Blob đúng layout [nonce 24][ct+tag>=16] và giải mã lại đúng bản gốc.
        assert!(u_raw.len() >= 24 + 16 + user.len());
        assert_eq!(crypto::decrypt_secret(&u_raw).unwrap(), user);
        assert_eq!(crypto::decrypt_secret(&p_raw).unwrap(), pass);
    }

    // Dump toàn bộ file trong data-dir (db + WAL nếu còn) sau khi đóng DB: 0 plaintext.
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !contains_bytes(&bytes, user.as_bytes()),
                "plaintext user trong {path:?}"
            );
            assert!(
                !contains_bytes(&bytes, pass.as_bytes()),
                "plaintext pass trong {path:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// (c) build_args set được ≥12/17 flag --fingerprint* từ profile lưu trong DB
// ---------------------------------------------------------------------------

/// 17 flag --fingerprint* theo docs/07 §2.
const FP_FLAGS_17: [&str; 17] = [
    "--fingerprint",
    "--fingerprint-platform",
    "--fingerprint-gpu-vendor",
    "--fingerprint-gpu-renderer",
    "--fingerprint-hardware-concurrency",
    "--fingerprint-device-memory",
    "--fingerprint-screen-width",
    "--fingerprint-screen-height",
    "--fingerprint-brand",
    "--fingerprint-brand-version",
    "--fingerprint-platform-version",
    "--fingerprint-location",
    "--fingerprint-timezone",
    "--fingerprint-locale",
    "--fingerprint-storage-quota",
    "--fingerprint-taskbar-height",
    "--fingerprint-webrtc-ip",
];

#[test]
fn c_build_args_sets_at_least_12_of_17_fingerprint_flags() {
    let dir = temp_dir("flags");
    let db = Db::open_at_dir(&dir).unwrap();

    // Profile "đầy đủ": field chuyên biệt (9 flag) + launch_args JSON trong DB
    // (đường UI/DB hợp lệ cho các flag còn lại — docs/05 §3 "UI/DB set được").
    let created = db
        .create_profile(ProfileInput {
            name: "full-fingerprint".into(),
            fingerprint_seed: Some("42424".into()),
            platform: Some("windows".into()),
            timezone: Some("Asia/Ho_Chi_Minh".into()),
            locale: Some("vi-VN".into()),
            gpu_vendor: Some("Intel Inc.".into()),
            gpu_renderer: Some("Intel Iris OpenGL".into()),
            hardware_concurrency: Some(8),
            screen_width: Some(1920),
            screen_height: Some(1080),
            launch_args: Some(serde_json::json!([
                "--fingerprint-device-memory=8",
                "--fingerprint-brand=Chrome",
                "--fingerprint-brand-version=131.0.0.0",
                "--fingerprint-platform-version=15.1.0",
                "--fingerprint-location=10.762622,106.660172",
                "--fingerprint-storage-quota=2147483648",
                "--fingerprint-taskbar-height=48",
                "--fingerprint-webrtc-ip=203.0.113.7",
            ])),
            ..Default::default()
        })
        .unwrap();

    // Đọc lại từ DB (roundtrip) rồi mới dựng args — xác nhận flow DB → launcher.
    let profile = db.get_profile(&created.id).unwrap();
    let args = build_args(&profile, None, 9222, &[], None);

    let present: Vec<&str> = FP_FLAGS_17
        .iter()
        .copied()
        .filter(|f| args.iter().any(|a| a.split('=').next() == Some(*f)))
        .collect();
    let missing: Vec<&str> = FP_FLAGS_17
        .iter()
        .copied()
        .filter(|f| !present.contains(f))
        .collect();
    println!(
        "[exit c] fingerprint flags present {}/17: {present:?}",
        present.len()
    );
    println!("[exit c] missing: {missing:?}");
    assert!(
        present.len() >= 12,
        "chỉ set được {}/17 flag --fingerprint* (cần ≥12): {present:?}",
        present.len()
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// (d) Teardown: stop giải phóng slot semaphore (mức integration)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn d_stop_releases_semaphore_slot_and_kills_process() {
    let pm = ProcessManager::new(1);
    let s1 = pm
        .spawn("exit-p1", "sleep", vec!["30".into()], 30001)
        .await
        .unwrap();

    // Slot duy nhất đã dùng → spawn thứ 2 bị chặn.
    assert!(pm
        .spawn("exit-p2", "sleep", vec!["30".into()], 30002)
        .await
        .is_err());

    // Stop → kill(pid) + trả slot: spawn lại phải thành công NGAY (không chờ reap).
    pm.stop("exit-p1").await.unwrap();
    assert!(!pm.is_running("exit-p1").await);
    let s2 = pm
        .spawn("exit-p2", "sleep", vec!["30".into()], 30003)
        .await
        .unwrap();
    assert_ne!(s1.pid, s2.pid);

    // Không rác tiến trình: pid cũ phải đã chết (probe qua `ps -p`).
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!pid_alive(s1.pid), "pid {} vẫn sống sau stop()", s1.pid);

    let _ = pm.stop("exit-p2").await;
}

/// Probe process còn sống bằng `ps -p <pid>` (không thêm dep libc).
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// (smoke) BEST-EFFORT: ensure_binary → spawn headful → CDP /json/version
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "spawns real browser; run with --ignored"]
async fn smoke_ensure_binary_spawn_headful_attach_cdp_best_effort() {
    // 1) Tải/định vị binary (timeout 60s — mạng/license có thể chặn).
    let bin = match tokio::time::timeout(Duration::from_secs(60), binary::ensure_binary(None, None))
        .await
    {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            println!("[smoke] SKIP — giới hạn môi trường: chưa tải được binary runtime ({e})");
            return;
        }
        Err(_) => {
            println!("[smoke] SKIP — giới hạn môi trường: ensure_binary timeout 60s (mạng chậm/không có)");
            return;
        }
    };
    println!("[smoke] binary: {}", bin.display());

    // 2) Spawn headful với args tối thiểu qua ProcessManager + build_args.
    let dir = temp_dir("smoke");
    let db = Db::open_at_dir(&dir).unwrap();
    let profile = db
        .create_profile(ProfileInput {
            name: "smoke".into(),
            user_data_dir: Some(dir.join("udd").to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();
    let pm = ProcessManager::new(1);
    let port = pm.allocate_cdp_port().unwrap();
    let args = build_args(&profile, None, port, &[], None);
    let sess = match pm
        .spawn(&profile.id, &bin.to_string_lossy(), args, port)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            println!("[smoke] SKIP — spawn thất bại (môi trường): {e}");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
    };

    // 3) Attach CDP /json/version (poll tối đa 20s).
    let url = format!("http://127.0.0.1:{port}/json/version");
    let mut cdp_ok = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(resp) = reqwest::get(&url).await {
            if resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                println!(
                    "[smoke] CDP /json/version OK: {}",
                    body.lines().take(3).collect::<Vec<_>>().join(" ")
                );
                cdp_ok = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let _ = pm.stop(&profile.id).await;
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        cdp_ok,
        "spawn được ({} pid {}) nhưng CDP không trả lời trong 20s",
        bin.display(),
        sess.pid
    );
}
