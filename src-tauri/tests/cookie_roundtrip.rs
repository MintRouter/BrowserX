//! W31b: Bằng chứng round-trip COOKIE qua CDP thật (integration, `#[ignore]`).
//!
//! TRUNG THỰC về phạm vi: test này CHỈ chứng minh round-trip **cookie** qua
//! đường CDP (`Storage.setCookies`/`Storage.getCookies` + `cookies::serialize`/
//! `cookies::parse`) — KHÔNG bao gồm localStorage hay full storage_state, vì
//! tính năng export/import của BrowserX chủ đích chỉ xử lý cookie qua CDP.
//!
//! Kịch bản: launch profile1 → set tập CookieItem xác định → get_all_cookies →
//! serialize = export1. parse(export1) → launch profile2 MỚI → set_cookies →
//! get_all_cookies → serialize = export2. ASSERT: SHA-256 của tập cookie test
//! CHUẨN HOÁ (sort theo name+domain+path; so name/value/domain/path/secure/
//! httpOnly/sameSite) từ export1 == export2; khác → FAIL log rõ field mất.
//!
//! Chạy: `cargo test --test cookie_roundtrip -- --ignored --nocapture`
//! (cần binary Chromium đã cache; không có → SKIP kèm ghi chú môi trường).

use std::path::PathBuf;
use std::time::Duration;

use browserx_lib::cookies::{self, CookieItem, Format};
use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, cdp};
use sha2::{Digest, Sha256};

/// Domain riêng cho cookie test — tách khỏi cookie browser tự sinh (nếu có).
const TEST_DOMAIN_SUFFIX: &str = "w31b-test.example";

/// Thư mục tạm duy nhất cho mỗi phiên (tự dọn sau khi đo xong).
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-ck-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Định vị binary (cache có sẵn); timeout 120s. None → SKIPPED (không phải PASSED).
async fn ensure_bin() -> Option<PathBuf> {
    match tokio::time::timeout(Duration::from_secs(120), binary::ensure_binary(None, None)).await {
        Ok(Ok(p)) => Some(p),
        Ok(Err(e)) => {
            eprintln!("[ck] SKIPPED: no binary cache — chưa có binary ({e})");
            None
        }
        Err(_) => {
            eprintln!("[ck] SKIPPED: no binary cache — ensure_binary timeout 120s");
            None
        }
    }
}

/// Tập CookieItem xác định (deterministic) phủ các field cần round-trip:
/// httpOnly, secure, sameSite Strict/Lax/None, session cookie (expires None).
fn test_cookies() -> Vec<CookieItem> {
    vec![
        CookieItem {
            name: "sid".into(),
            value: "abc-123-xyz".into(),
            domain: format!(".{TEST_DOMAIN_SUFFIX}"),
            path: "/".into(),
            expires: Some(1_900_000_000.0),
            http_only: true,
            secure: true,
            same_site: Some("Lax".into()),
        },
        CookieItem {
            name: "theme".into(),
            value: "dark".into(),
            domain: format!("app.{TEST_DOMAIN_SUFFIX}"),
            path: "/settings".into(),
            expires: None, // session cookie
            http_only: false,
            secure: false,
            same_site: None,
        },
        CookieItem {
            name: "csrf".into(),
            value: "t0k3n%3D%3B|%20-42".into(), // ký tự đặc biệt percent-encoded (';'/'=' thô bị Chromium từ chối)
            domain: format!("app.{TEST_DOMAIN_SUFFIX}"),
            path: "/".into(),
            expires: Some(1_900_000_000.0),
            http_only: false,
            secure: true,
            same_site: Some("Strict".into()),
        },
        CookieItem {
            name: "tracker".into(),
            value: "cross-site".into(),
            domain: format!(".{TEST_DOMAIN_SUFFIX}"),
            path: "/ads".into(),
            expires: Some(1_900_000_000.0),
            http_only: true,
            secure: true, // SameSite=None bắt buộc secure (Chromium)
            same_site: Some("None".into()),
        },
    ]
}

/// Chuẩn hoá 1 cookie thành dòng key ổn định (KHÔNG gồm expires — CDP có thể
/// làm tròn double; phạm vi so sánh theo task: name/value/domain/path/secure/
/// httpOnly/sameSite).
fn normalize_line(c: &CookieItem) -> String {
    format!(
        "name={} value={} domain={} path={} secure={} httpOnly={} sameSite={}",
        c.name,
        c.value,
        c.domain,
        c.path,
        c.secure,
        c.http_only,
        c.same_site.as_deref().unwrap_or("<unset>")
    )
}

/// Lọc cookie test (theo domain suffix), sort theo name+domain+path, trả về
/// (sha256 hex, các dòng chuẩn hoá để log diff khi FAIL).
fn normalized_hash(items: &[CookieItem]) -> (String, Vec<String>) {
    let mut ours: Vec<&CookieItem> = items
        .iter()
        .filter(|c| c.domain.ends_with(TEST_DOMAIN_SUFFIX))
        .collect();
    ours.sort_by(|a, b| {
        (&a.name, &a.domain, &a.path).cmp(&(&b.name, &b.domain, &b.path))
    });
    let lines: Vec<String> = ours.iter().map(|c| normalize_line(c)).collect();
    let mut h = Sha256::new();
    for l in &lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    (hex::encode(h.finalize()), lines)
}

/// Một phiên browser thật: profile riêng + user-data-dir tạm + ProcessManager.
struct Session {
    pm: ProcessManager,
    profile_id: String,
    dir: PathBuf,
    port: u16,
}

/// Launch 1 phiên mới hoàn toàn (profile + user-data-dir mới), attach CDP,
/// điều hướng về about:blank (tránh newtab đặc quyền).
async fn launch(bin: &str, tag: &str) -> Session {
    let dir = temp_dir(tag);
    let db = Db::open_at_dir(&dir).unwrap();
    let profile = db
        .create_profile(ProfileInput {
            name: tag.into(),
            user_data_dir: Some(dir.join("udd").to_string_lossy().into_owned()),
            startup_behavior: Some("custom".into()),
            startup_urls: Some(serde_json::json!([])),
            ..Default::default()
        })
        .unwrap();
    drop(db);

    let pm = ProcessManager::new(1);
    let port = pm.allocate_cdp_port().unwrap();
    let args = build_args(&profile, None, port, &[], None);
    let sess = pm
        .spawn(&profile.id, bin, args, port)
        .await
        .unwrap_or_else(|e| panic!("[ck] spawn thất bại ({tag}): {e}"));
    println!("[ck] {tag}: pid {} cdp {}", sess.pid, port);

    cdp::attach(port)
        .await
        .unwrap_or_else(|e| panic!("[ck] attach CDP thất bại ({tag}): {e}"));
    let _ = cdp::goto(port, "about:blank").await;

    Session {
        pm,
        profile_id: profile.id,
        dir,
        port,
    }
}

/// Teardown: stop process (0 zombie) rồi dọn thư mục tạm.
async fn teardown(s: Session) {
    let _ = s.pm.stop(&s.profile_id).await;
    let _ = std::fs::remove_dir_all(&s.dir);
}

/// Ghi `items` qua Storage.setCookies rồi đọc lại toàn bộ + serialize JSON.
/// Trả (export JSON, tổng số cookie fetch được). Lỗi → Err để caller teardown
/// sạch trước khi panic.
async fn set_then_export(port: u16, items: &[CookieItem]) -> Result<(String, usize), String> {
    let set = cdp::set_cookies(port, items)
        .await
        .map_err(|e| format!("set_cookies: {e}"))?;
    if set != items.len() {
        return Err(format!("set_cookies gửi {set}/{} cookie", items.len()));
    }
    let all = cdp::get_all_cookies(port)
        .await
        .map_err(|e| format!("get_all_cookies: {e}"))?;
    let data = cookies::serialize(&all, Format::Json).map_err(|e| format!("serialize: {e}"))?;
    Ok((data, all.len()))
}

/// export → import (parse) → re-export: hash tập cookie test chuẩn hoá phải bằng nhau.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn cookie_roundtrip_via_cdp_preserves_normalized_set() {
    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => {
            // Skip thật: test KHÔNG chạy — đừng đọc "ok" của libtest là bằng chứng PASSED.
            eprintln!("[ck] TEST SKIPPED (not passed): thiếu binary cache, không đo gì cả");
            return;
        }
    };
    let seed = test_cookies();

    // Phiên 1: set cookie xác định → export1.
    let s1 = launch(&bin, "profile1").await;
    let r1 = set_then_export(s1.port, &seed).await;
    teardown(s1).await;
    let (export1, total1) = r1.unwrap_or_else(|e| panic!("[ck] profile1: {e}"));

    // Import: parse(export1) — đúng đường import_cookies dùng (auto-detect JSON).
    let imported = cookies::parse(&export1).unwrap_or_else(|e| panic!("[ck] parse export1: {e}"));

    // Phiên 2: profile + user-data-dir MỚI hoàn toàn → set cookie đã parse → export2.
    let s2 = launch(&bin, "profile2").await;
    let r2 = set_then_export(s2.port, &imported).await;
    teardown(s2).await;
    let (export2, total2) = r2.unwrap_or_else(|e| panic!("[ck] profile2: {e}"));

    let (hash1, lines1) = normalized_hash(&cookies::parse(&export1).unwrap());
    let (hash2, lines2) = normalized_hash(&cookies::parse(&export2).unwrap());

    println!("[ck] seed cookies: {}", seed.len());
    println!("[ck] export1: {total1} cookie fetch, {} cookie test — hash {hash1}", lines1.len());
    println!("[ck] export2: {total2} cookie fetch, {} cookie test — hash {hash2}", lines2.len());

    // Cookie test phải hiện diện đủ ngay từ export1 (nếu Chromium từ chối cookie
    // nào thì phơi bày tại đây, không để hash rỗng == hash rỗng che mất).
    assert_eq!(
        lines1.len(),
        seed.len(),
        "[ck] export1 thiếu cookie test: chỉ thấy {:#?} (seed {} cookie)",
        lines1,
        seed.len()
    );

    if hash1 != hash2 {
        // Log rõ field/cookie bị mất trước khi FAIL.
        for l in &lines1 {
            if !lines2.contains(l) {
                eprintln!("[ck] MẤT sau round-trip: {l}");
            }
        }
        for l in &lines2 {
            if !lines1.contains(l) {
                eprintln!("[ck] THÊM sau round-trip: {l}");
            }
        }
        panic!("[ck] FAIL: hash chuẩn hoá khác nhau — export1 {hash1} != export2 {hash2}");
    }
    println!("[ck] PASS: round-trip cookie qua CDP giữ nguyên tập chuẩn hoá ({hash1})");
}
