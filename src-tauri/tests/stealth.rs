//! W31a: Bằng chứng stealth QUA BrowserX (integration, `#[ignore]`).
//!
//! Launch 1 profile headful THẬT qua `launcher::build_args` + `ProcessManager`
//! (binary đã cache), attach CDP, goto tới một HTTP origin THẬT (local server
//! `std::net`, `127.0.0.1:<ephemeral>`) THAY CHO `about:blank` opaque, rồi
//! `cdp::eval` đo các tín hiệu sannysoft/CreepJS. Đo trên origin http thật để
//! `chrome.runtime`, `permissions`, `localStorage` có document origin thật —
//! `about:blank` là origin opaque nên làm sai lệch các tín hiệu này. In TỪNG giá
//! trị bằng `println!` (--nocapture). Phân loại:
//!
//! HARD-ASSERT (SAI → FAIL để phơi bày; binary CloakBrowser đảm bảo):
//!   - navigator.webdriver === false (undefined/true → FAIL, KHÔNG che).
//!   - navigator.userAgent KHÔNG chứa "Headless".
//!   - navigator.plugins.length > 0
//!   - navigator.plugins[0].name KHÔNG rỗng
//!   - navigator.mimeTypes.length > 0
//!   - navigator.languages.length > 0
//!   - typeof window.chrome === 'object'
//!   - navigator.hardwareConcurrency > 0
//!   - window.outerWidth > 0 (real-headless thường báo 0)
//!
//! LOG-ONLY (KHÔNG fail; đo + println để quan sát):
//!   - typeof window.chrome.runtime — CloakBrowser KHÔNG đảm bảo field này; hành
//!     vi phụ thuộc origin/version. `tests/test_launch.py` của họ CHỈ assert
//!     `window.chrome==='object'` (KHÔNG hề assert `chrome.runtime`), nên đây là
//!     LOG-ONLY: đo + in ra, KHÔNG hard-fail.
//!   - navigator.permissions.query({name:'notifications'}).state so với
//!     Notification.permission — chỉ log 2 giá trị; CHỈ cảnh báo (warn) nếu CẢ
//!     HAI resolve thành cặp thực sự mâu thuẫn; KHÔNG BAO GIỜ hard-fail. Nếu
//!     state trả về rỗng/không sẵn → log "<unavailable>", không fail.
//!   - WebGL UNMASKED_RENDERER ("SwiftShader"/"llvmpipe" → cảnh báo môi trường).
//!   - navigator.deviceMemory (log nếu có).
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

/// Local HTTP server tối thiểu — CHỈ dùng `std::net` (KHÔNG thêm crate nào).
/// Bind `127.0.0.1:0` (cổng ephemeral), spawn 1 `std::thread` daemon loop accept,
/// phục vụ 1 tài liệu HTML hợp lệ cho MỌI GET request (kèm header đầy đủ). Trả về
/// cổng đã chọn để `measure()` goto tới một http origin THẬT (thay cho origin
/// opaque `about:blank`). Loop phục vụ nhiều connection (navigate + favicon
/// fetch); thread chạy tới khi tiến trình thoát — không cần shutdown sạch.
fn start_local_http() -> u16 {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("[stealth] bind 127.0.0.1:0");
    let port = listener
        .local_addr()
        .expect("[stealth] local_addr")
        .port();
    std::thread::spawn(move || {
        const BODY: &str =
            "<!doctype html><html><head><title>bx</title></head><body>stealth</body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
            BODY.len()
        );
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            // Đọc request (đủ để không reset connection sớm), rồi trả HTML.
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
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

/// Eval 1 lần: trả JSON.stringify của object các tín hiệu stealth SYNC (string,
/// ổn định qua returnByValue). Parse ở Rust để assert từng field.
const STEALTH_JS: &str = r#"(() => {
  let webglRenderer = '';
  try {
    const c = document.createElement('canvas');
    const gl = c.getContext('webgl') || c.getContext('experimental-webgl');
    if (gl) {
      const ext = gl.getExtension('WEBGL_debug_renderer_info');
      if (ext) webglRenderer = String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) || '');
    }
  } catch (e) { webglRenderer = 'error:' + e.message; }
  return JSON.stringify({
    webdriver: navigator.webdriver,
    ua: navigator.userAgent,
    plugins: navigator.plugins.length,
    languages: navigator.languages.length,
    chrome: typeof window.chrome,
    chromeRuntime: (() => { try { return typeof window.chrome.runtime; } catch (e) { return 'no-chrome'; } })(),
    pluginName: navigator.plugins[0] ? navigator.plugins[0].name : '',
    mimeTypes: navigator.mimeTypes.length,
    hardwareConcurrency: navigator.hardwareConcurrency,
    outerWidth: window.outerWidth,
    notificationPermission: (typeof Notification !== 'undefined') ? Notification.permission : 'no-notification',
    deviceMemory: ('deviceMemory' in navigator) ? navigator.deviceMemory : null,
    webglRenderer: webglRenderer
  });
})()"#;

/// Bước 1 của đo permissions: `navigator.permissions.query` TRẢ Promise, nhưng
/// `cdp::eval` (chromiumoxide `page.evaluate` → `Runtime.evaluate`) KHÔNG set
/// `awaitPromise` nên Promise KHÔNG được resolve tự động. Vì vậy dùng 2 bước:
/// script này kick-off query rồi gán state ĐÃ resolve vào `window.__perm` qua
/// `.then(...)`, trả về ngay "set".
const PERM_SET_JS: &str = r#"(() => {
  window.__perm = 'pending';
  try {
    navigator.permissions.query({ name: 'notifications' })
      .then((r) => { window.__perm = r.state; })
      .catch((e) => { window.__perm = 'error:' + e.message; });
  } catch (e) { window.__perm = 'throw:' + e.message; }
  return 'set';
})()"#;

/// Bước 2: sau khi sleep ~200ms cho microtask `.then` chạy xong, đọc lại giá trị
/// đã resolve từ `window.__perm` (cùng page/tab nên biến vẫn còn).
const PERM_GET_JS: &str = "window.__perm";

/// Launch 1 phiên headful thật, attach CDP, goto một http origin THẬT
/// (`http://127.0.0.1:<ephemeral>/` do server `std::net` trong test phục vụ), đo tín hiệu stealth
/// (sync + permissions 2 bước), teardown sạch (kill process + dọn temp), trả về
/// object JSON đã parse gồm mọi field stealth + field `permState`.
async fn measure(bin: &str, tag: &str) -> serde_json::Value {
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
    // Navigate tới HTTP origin THẬT (local server std::net) thay cho about:blank
    // opaque — để chrome.runtime/permissions/localStorage có document origin thật.
    let http_port = start_local_http();
    let _ = cdp::goto(port, &format!("http://127.0.0.1:{http_port}/")).await;
    // Cho document (+ favicon) tải xong & JS context ổn định trước khi đo.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // (1) Đo tín hiệu sync một lần (JSON.stringify → string ổn định).
    let stealth = cdp::eval(port, STEALTH_JS).await;

    // (2) Permissions: kick-off Promise → sleep cho microtask chạy → đọc lại.
    // Cần 2 bước vì `cdp::eval` không awaitPromise (xem doc PERM_SET_JS).
    let _ = cdp::eval(port, PERM_SET_JS).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let perm = cdp::eval(port, PERM_GET_JS).await;

    // Teardown TRƯỚC khi unwrap để không để zombie khi eval lỗi.
    let _ = pm.stop(&profile.id).await;
    let _ = std::fs::remove_dir_all(&dir);

    let stealth = stealth.unwrap_or_else(|e| panic!("[stealth] eval stealth thất bại ({tag}): {e}"));
    let raw = stealth
        .as_str()
        .unwrap_or_else(|| panic!("[stealth] eval stealth không trả string JSON ({tag}): {stealth}"));
    let mut obj: serde_json::Value = serde_json::from_str(raw)
        .unwrap_or_else(|e| panic!("[stealth] parse JSON stealth ({tag}): {e} — raw={raw}"));

    // permState là host-dependent về mặt đo (Promise có thể chưa resolve kịp);
    // gắn vào object để test đọc và assert phần mâu thuẫn.
    let perm_state = perm
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "<unavailable>".to_string());
    obj["permState"] = serde_json::Value::String(perm_state);
    obj
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

    let m = measure(&bin, "profile1").await;

    // ---- Đọc từng tín hiệu ----
    let webdriver = &m["webdriver"];
    let ua = m["ua"].as_str().unwrap_or("<null>");
    let plugins = m["plugins"].as_u64().unwrap_or(0);
    let languages = m["languages"].as_u64().unwrap_or(0);
    let chrome = m["chrome"].as_str().unwrap_or("<null>");
    let chrome_runtime = m["chromeRuntime"].as_str().unwrap_or("<null>");
    let plugin_name = m["pluginName"].as_str().unwrap_or("");
    let mime_types = m["mimeTypes"].as_u64().unwrap_or(0);
    let hardware_concurrency = m["hardwareConcurrency"].as_u64().unwrap_or(0);
    let outer_width = m["outerWidth"].as_u64().unwrap_or(0);
    let notif = m["notificationPermission"].as_str().unwrap_or("<null>");
    let perm_state = m["permState"].as_str().unwrap_or("<null>");
    let webgl = m["webglRenderer"].as_str().unwrap_or("<null>");
    let device_memory = &m["deviceMemory"];

    // ---- LOG từng giá trị (--nocapture hiển thị) ----
    println!("[stealth] navigator.webdriver = {webdriver}");
    println!("[stealth] navigator.userAgent = {ua}");
    println!("[stealth] navigator.plugins.length = {plugins}");
    println!("[stealth] navigator.languages.length = {languages}");
    println!("[stealth] typeof window.chrome = {chrome}");
    println!("[stealth] typeof window.chrome.runtime = {chrome_runtime}");
    println!("[stealth] navigator.plugins[0].name = {plugin_name:?}");
    println!("[stealth] navigator.mimeTypes.length = {mime_types}");
    println!("[stealth] navigator.hardwareConcurrency = {hardware_concurrency}");
    println!("[stealth] window.outerWidth = {outer_width}");
    println!("[stealth] Notification.permission = {notif}");
    println!("[stealth] permissions.query(notifications).state = {perm_state}");
    println!("[stealth] WebGL UNMASKED_RENDERER = {webgl:?}");
    println!("[stealth] navigator.deviceMemory = {device_memory}");

    // ---- HARD asserts: 5 tín hiệu cốt lõi (GIỮ NGUYÊN) ----
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
    assert!(plugins >= 5, "[stealth] navigator.plugins.length PHẢI >= 5 (baseline test CloakBrowser test_launch.py/test_stealth.py), đo được {plugins}");
    assert!(languages > 0, "[stealth] navigator.languages.length PHẢI > 0, đo được {languages}");
    assert_eq!(chrome, "object", "[stealth] typeof window.chrome PHẢI là 'object', đo được '{chrome}'");

    // ---- HARD asserts: tín hiệu MỚI (binary đảm bảo → SAI thì FAIL, không nới) ----
    assert!(
        !plugin_name.is_empty(),
        "[stealth] navigator.plugins[0].name PHẢI khác rỗng, đo được {plugin_name:?}"
    );
    assert!(mime_types > 0, "[stealth] navigator.mimeTypes.length PHẢI > 0, đo được {mime_types}");
    assert!(
        hardware_concurrency > 0,
        "[stealth] navigator.hardwareConcurrency PHẢI > 0, đo được {hardware_concurrency}"
    );
    assert!(
        outer_width > 0,
        "[stealth] window.outerWidth PHẢI > 0 (real-headless báo 0), đo được {outer_width}"
    );

    // ---- LOG-ONLY: KHÔNG fail — chỉ đo + println (và warn khi hợp lý) ----
    // typeof window.chrome.runtime: CloakBrowser KHÔNG đảm bảo field này (hành vi
    // phụ thuộc origin/version). test_launch.py của họ CHỈ assert
    // window.chrome==='object', KHÔNG hề assert chrome.runtime → LOG-ONLY, không
    // hard-fail dù đo trên origin http thật.
    if chrome_runtime == "undefined" || chrome_runtime == "no-chrome" {
        println!(
            "[stealth] LOG-ONLY: typeof window.chrome.runtime = '{chrome_runtime}' \
             (CloakBrowser không đảm bảo; test_launch.py chỉ assert window.chrome==='object')"
        );
    } else {
        println!("[stealth] LOG-ONLY: typeof window.chrome.runtime = '{chrome_runtime}' (present)");
    }

    // permissions.query(notifications).state vs Notification.permission: chỉ log 2
    // giá trị; CHỈ warn nếu CẢ HAI resolve thành cặp thực sự mâu thuẫn (denied vs
    // prompt); state rỗng/không sẵn → "<unavailable>". KHÔNG BAO GIỜ hard-fail.
    if perm_state == "<unavailable>" || perm_state.is_empty() {
        println!("[stealth] LOG-ONLY: permissions.query(notifications).state = <unavailable>");
    } else {
        println!(
            "[stealth] LOG-ONLY: Notification.permission='{notif}' vs \
             permissions.query(notifications).state='{perm_state}'"
        );
        if notif == "denied" && perm_state == "prompt" {
            eprintln!(
                "[stealth] CẢNH BÁO (KHÔNG fail): permissions mâu thuẫn — \
                 Notification.permission='{notif}' nhưng query.state='{perm_state}'"
            );
        }
    }

    // WebGL UNMASKED_RENDERER + deviceMemory: phụ thuộc host/GPU, log-only.
    println!("[stealth] LOG-ONLY: WebGL UNMASKED_RENDERER = {webgl:?}");
    if webgl.contains("SwiftShader") || webgl.contains("llvmpipe") {
        eprintln!(
            "[stealth] CẢNH BÁO MÔI TRƯỜNG (KHÔNG fail): WebGL renderer là software \
             ({webgl:?}) — máy không có GPU thật; trên host thật giá trị này sẽ khác"
        );
    }
    println!("[stealth] LOG-ONLY: navigator.deviceMemory = {device_memory}");
    if device_memory.is_null() {
        println!("[stealth] navigator.deviceMemory không hiện diện (log-only, KHÔNG fail)");
    }

    println!(
        "[stealth] PASS: tất cả tín hiệu stealth hard-assert hợp lệ \
         (chrome.runtime/permissions/webgl/deviceMemory log-only)"
    );
}
