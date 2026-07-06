//! W31a: Bằng chứng stealth cốt lõi QUA BrowserX (integration, `#[ignore]`).
//!
//! Launch 1 profile headful THẬT qua `launcher::build_args` + `ProcessManager`
//! (binary đã cache), attach CDP, goto("about:blank") rồi `cdp::eval` một lần
//! trả JSON object đo các tín hiệu sannysoft cốt lõi, ASSERT TỪNG tín hiệu:
//!   - navigator.webdriver === false (undefined/true → FAIL để phơi bày).
//!   - navigator.userAgent KHÔNG chứa "Headless".
//!   - navigator.plugins.length > 0
//!   - navigator.languages.length > 0
//!   - typeof window.chrome === 'object'
//!
//! Chạy: `cargo test --test stealth -- --ignored --nocapture`
//! (cần binary Chromium đã cache; không có → SKIP kèm ghi chú môi trường).

use std::path::PathBuf;
use std::time::Duration;

use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, cdp};

/// Thư mục tạm duy nhất cho mỗi phiên (tự dọn sau khi đo xong).
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-st-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Định vị binary (cache có sẵn); timeout 120s. None → SKIPPED (không phải PASSED).
async fn ensure_bin() -> Option<PathBuf> {
    match tokio::time::timeout(Duration::from_secs(120), binary::ensure_binary(None, None)).await {
        Ok(Ok(p)) => Some(p),
        Ok(Err(e)) => {
            eprintln!("[stealth] SKIPPED: no binary cache — chưa có binary ({e})");
            None
        }
        Err(_) => {
            eprintln!("[stealth] SKIPPED: no binary cache — ensure_binary timeout 120s");
            None
        }
    }
}

/// Eval 1 lần: trả JSON.stringify của object các tín hiệu stealth (string, ổn
/// định qua returnByValue). Parse ở Rust để assert từng field.
const STEALTH_JS: &str = r#"(() => {
  return JSON.stringify({
    webdriver: navigator.webdriver,
    ua: navigator.userAgent,
    plugins: navigator.plugins.length,
    languages: navigator.languages.length,
    chrome: typeof window.chrome
  });
})()"#;

/// Launch 1 phiên headful thật, attach CDP, goto about:blank, đo tín hiệu stealth,
/// teardown sạch (kill process + dọn temp), trả JSON string đã eval.
async fn measure(bin: &str, tag: &str) -> String {
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
    let args = build_args(&profile, None, port, &[]);
    let sess = pm
        .spawn(&profile.id, bin, args, port)
        .await
        .unwrap_or_else(|e| panic!("[stealth] spawn thất bại ({tag}): {e}"));
    println!("[stealth] {tag}: pid {} cdp {}", sess.pid, port);

    cdp::attach(port)
        .await
        .unwrap_or_else(|e| panic!("[stealth] attach CDP thất bại ({tag}): {e}"));
    let _ = cdp::goto(port, "about:blank").await;

    let val = cdp::eval(port, STEALTH_JS).await;

    // Teardown TRƯỚC khi unwrap để không để zombie khi eval lỗi.
    let _ = pm.stop(&profile.id).await;
    let _ = std::fs::remove_dir_all(&dir);

    let val = val.unwrap_or_else(|e| panic!("[stealth] eval thất bại ({tag}): {e}"));
    val.as_str()
        .unwrap_or_else(|| panic!("[stealth] eval không trả string JSON ({tag}): {val}"))
        .to_string()
}

/// Assert các tín hiệu stealth cốt lõi trên 1 phiên headful thật.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn core_stealth_signals_present() {
    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => {
            // Skip thật: test KHÔNG chạy — đừng đọc "ok" của libtest là bằng chứng PASSED.
            eprintln!("[stealth] TEST SKIPPED (not passed): thiếu binary cache, không đo gì cả");
            return;
        }
    };

    let raw = measure(&bin, "profile1").await;
    let m: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("[stealth] parse JSON đo được: {e} — raw={raw}"));

    let webdriver = &m["webdriver"];
    let ua = m["ua"].as_str().unwrap_or("<null>");
    let plugins = m["plugins"].as_u64().unwrap_or(0);
    let languages = m["languages"].as_u64().unwrap_or(0);
    let chrome = m["chrome"].as_str().unwrap_or("<null>");

    println!("[stealth] navigator.webdriver = {webdriver}");
    println!("[stealth] navigator.userAgent = {ua}");
    println!("[stealth] navigator.plugins.length = {plugins}");
    println!("[stealth] navigator.languages.length = {languages}");
    println!("[stealth] typeof window.chrome = {chrome}");

    // navigator.webdriver PHẢI === false (undefined/true → FAIL, KHÔNG che).
    assert_eq!(
        webdriver,
        &serde_json::Value::Bool(false),
        "[stealth] navigator.webdriver PHẢI là false, đo được {webdriver} (undefined/true = lộ automation)"
    );
    // UA KHÔNG được lộ Headless (bắt cả "Headless" lẫn "HeadlessChrome").
    assert!(
        !ua.contains("Headless"),
        "[stealth] navigator.userAgent chứa \"Headless\" — lộ headless: {ua}"
    );
    assert!(plugins > 0, "[stealth] navigator.plugins.length PHẢI > 0, đo được {plugins}");
    assert!(languages > 0, "[stealth] navigator.languages.length PHẢI > 0, đo được {languages}");
    assert_eq!(chrome, "object", "[stealth] typeof window.chrome PHẢI là 'object', đo được '{chrome}'");

    println!("[stealth] PASS: tất cả tín hiệu stealth cốt lõi hợp lệ");
}
