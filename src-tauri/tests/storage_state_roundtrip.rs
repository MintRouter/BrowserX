//! W33b: Bằng chứng round-trip FULL STORAGE_STATE (cookie + localStorage) qua
//! CDP thật (integration, `#[ignore]`).
//!
//! Kịch bản: start local HTTP server (std::net) → origin http THẬT. Launch
//! profile1 → set cookie xác định (Storage.setCookies) + localStorage xác định
//! (goto origin + setItem) → export storage_state (get_all_cookies +
//! get_local_storage → serialize_storage_state) = export1. parse_storage_state
//! (đúng đường import_storage_state dùng) → launch profile2 MỚI hoàn toàn →
//! set cookie + localStorage → đọc lại → export2. ASSERT: SHA-256 tập chuẩn
//! hoá (cookie sort name+domain+path, KHÔNG gồm expires — như cookie_roundtrip;
//! localStorage sort theo key) từ export1 == export2.
//!
//! Chạy: `cargo test --test storage_state_roundtrip -- --ignored --nocapture`
//! (cần binary Chromium đã cache; không có → SKIP kèm ghi chú môi trường).

use std::path::PathBuf;
use std::time::Duration;

use browserx_lib::cookies::{
    self, CookieItem, LocalStorageEntry, OriginState, StorageState,
};
use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, cdp};
use sha2::{Digest, Sha256};

/// Domain riêng cho cookie test — tách khỏi cookie browser tự sinh (nếu có).
const TEST_DOMAIN_SUFFIX: &str = "w33b-test.example";

/// Thư mục tạm duy nhất cho mỗi phiên (tự dọn sau khi đo xong).
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-ss-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Local HTTP server tối thiểu — CHỈ dùng `std::net` (như tests/stealth.rs).
/// Bind `127.0.0.1:0`, thread daemon phục vụ 1 HTML hợp lệ cho mọi GET; trả
/// cổng để test goto tới một http origin THẬT (localStorage cần origin thật —
/// `about:blank` là origin opaque, truy cập localStorage sẽ throw).
fn start_local_http() -> u16 {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("[ss] bind 127.0.0.1:0");
    let port = listener.local_addr().expect("[ss] local_addr").port();
    std::thread::spawn(move || {
        const BODY: &str =
            "<!doctype html><html><head><title>bx</title></head><body>ss</body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
            BODY.len()
        );
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
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
            eprintln!("[ss] SKIPPED: no binary cache — chưa có binary ({e})");
            None
        }
        Err(_) => {
            eprintln!("[ss] SKIPPED: no binary cache — ensure_binary timeout 120s");
            None
        }
    }
}

/// Tập CookieItem xác định phủ các field cần round-trip (như cookie_roundtrip).
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

/// Tập localStorage xác định — phủ value có space/`=`/`;`/JSON/unicode.
fn test_local_storage() -> Vec<(String, String)> {
    vec![
        ("alpha".into(), "1".into()),
        ("beta".into(), "x y=z;&%20".into()),
        ("gamma".into(), r#"{"json":true,"n":42}"#.into()),
        ("delta".into(), "tiếng Việt ✓".into()),
    ]
}

/// Chuẩn hoá 1 cookie thành dòng key ổn định (KHÔNG gồm expires — CDP có thể
/// làm tròn double; cùng chuẩn với tests/cookie_roundtrip.rs).
fn normalize_cookie_line(c: &CookieItem) -> String {
    format!(
        "cookie name={} value={} domain={} path={} secure={} httpOnly={} sameSite={}",
        c.name,
        c.value,
        c.domain,
        c.path,
        c.secure,
        c.http_only,
        c.same_site.as_deref().unwrap_or("<unset>")
    )
}

/// Chuẩn hoá full storage_state → (sha256 hex, dòng chuẩn hoá, #cookie, #lsKey):
/// cookie test (lọc theo domain suffix) sort name+domain+path; localStorage
/// sort theo origin rồi key.
fn normalized_hash(state: &StorageState) -> (String, Vec<String>, usize, usize) {
    let mut ours: Vec<&CookieItem> = state
        .cookies
        .iter()
        .filter(|c| c.domain.ends_with(TEST_DOMAIN_SUFFIX))
        .collect();
    ours.sort_by(|a, b| (&a.name, &a.domain, &a.path).cmp(&(&b.name, &b.domain, &b.path)));
    let mut lines: Vec<String> = ours.iter().map(|c| normalize_cookie_line(c)).collect();
    let cookie_count = lines.len();

    let mut origins: Vec<&OriginState> = state.origins.iter().collect();
    origins.sort_by(|a, b| a.origin.cmp(&b.origin));
    let mut ls_count = 0;
    for o in origins {
        let mut entries: Vec<&LocalStorageEntry> = o.local_storage.iter().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        ls_count += entries.len();
        for e in entries {
            lines.push(format!("ls origin={} {}={}", o.origin, e.name, e.value));
        }
    }

    let mut h = Sha256::new();
    for l in &lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    (hex::encode(h.finalize()), lines, cookie_count, ls_count)
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
        .unwrap_or_else(|e| panic!("[ss] spawn thất bại ({tag}): {e}"));
    println!("[ss] {tag}: pid {} cdp {}", sess.pid, port);

    cdp::attach(port)
        .await
        .unwrap_or_else(|e| panic!("[ss] attach CDP thất bại ({tag}): {e}"));
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

/// Đọc lại full storage_state hiện tại (get_all_cookies + get_local_storage
/// theo origin) rồi serialize — đúng đường export_storage_state dùng.
async fn read_state(port: u16, origin: &str) -> Result<String, String> {
    let cookies = cdp::get_all_cookies(port)
        .await
        .map_err(|e| format!("get_all_cookies: {e}"))?;
    let entries = cdp::get_local_storage(port, origin)
        .await
        .map_err(|e| format!("get_local_storage: {e}"))?;
    let state = StorageState {
        cookies,
        origins: vec![OriginState {
            origin: origin.to_string(),
            local_storage: entries
                .into_iter()
                .map(|(name, value)| LocalStorageEntry { name, value })
                .collect(),
        }],
    };
    cookies::serialize_storage_state(&state).map_err(|e| format!("serialize_storage_state: {e}"))
}

/// Ghi cookie + localStorage rồi đọc lại full storage_state. Lỗi → Err để
/// caller teardown sạch trước khi panic.
async fn write_then_read(
    port: u16,
    origin: &str,
    cookie_items: &[CookieItem],
    ls_items: &[(String, String)],
) -> Result<String, String> {
    if !cookie_items.is_empty() {
        let set = cdp::set_cookies(port, cookie_items)
            .await
            .map_err(|e| format!("set_cookies: {e}"))?;
        if set != cookie_items.len() {
            return Err(format!("set_cookies gửi {set}/{} cookie", cookie_items.len()));
        }
    }
    if !ls_items.is_empty() {
        let set = cdp::set_local_storage(port, origin, ls_items)
            .await
            .map_err(|e| format!("set_local_storage: {e}"))?;
        if set != ls_items.len() {
            return Err(format!("set_local_storage ghi {set}/{} key", ls_items.len()));
        }
    }
    read_state(port, origin).await
}

/// export → parse_storage_state (đường import) → ghi vào profile MỚI →
/// re-export: hash tập chuẩn hoá (cookie + localStorage) phải bằng nhau.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn storage_state_roundtrip_via_cdp_preserves_normalized_set() {
    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => {
            // Skip thật: test KHÔNG chạy — đừng đọc "ok" của libtest là bằng chứng PASSED.
            eprintln!("[ss] TEST SKIPPED (not passed): thiếu binary cache, không đo gì cả");
            return;
        }
    };
    // MỘT server cho cả 2 phiên → cùng origin (localStorage scoped theo origin).
    let http_port = start_local_http();
    let origin = format!("http://127.0.0.1:{http_port}/");
    println!("[ss] origin: {origin}");

    let seed_cookies = test_cookies();
    let seed_ls = test_local_storage();

    // Phiên 1: set cookie + localStorage xác định → export1.
    let s1 = launch(&bin, "profile1").await;
    let r1 = write_then_read(s1.port, &origin, &seed_cookies, &seed_ls).await;
    teardown(s1).await;
    let export1 = r1.unwrap_or_else(|e| panic!("[ss] profile1: {e}"));

    // Import: parse_storage_state(export1) — đúng đường import_storage_state dùng.
    let imported = cookies::parse_storage_state(&export1)
        .unwrap_or_else(|e| panic!("[ss] parse export1: {e}"));
    let imported_ls: Vec<(String, String)> = imported
        .origins
        .iter()
        .flat_map(|o| o.local_storage.iter().map(|e| (e.name.clone(), e.value.clone())))
        .collect();

    // Phiên 2: profile + user-data-dir MỚI hoàn toàn → ghi storage_state đã
    // parse → đọc lại → export2.
    let s2 = launch(&bin, "profile2").await;
    let r2 = write_then_read(s2.port, &origin, &imported.cookies, &imported_ls).await;
    teardown(s2).await;
    let export2 = r2.unwrap_or_else(|e| panic!("[ss] profile2: {e}"));

    let state1 = cookies::parse_storage_state(&export1).unwrap();
    let state2 = cookies::parse_storage_state(&export2).unwrap();
    let (hash1, lines1, ck1, ls1) = normalized_hash(&state1);
    let (hash2, lines2, ck2, ls2) = normalized_hash(&state2);

    println!("[ss] seed: {} cookie, {} localStorage key", seed_cookies.len(), seed_ls.len());
    println!("[ss] export1: {ck1} cookie test, {ls1} ls key — hash {hash1}");
    println!("[ss] export2: {ck2} cookie test, {ls2} ls key — hash {hash2}");

    // Dữ liệu test phải hiện diện đủ ngay từ export1 (Chromium từ chối phần
    // nào thì phơi bày tại đây, không để hash rỗng == hash rỗng che mất).
    assert_eq!(
        ck1,
        seed_cookies.len(),
        "[ss] export1 thiếu cookie test: chỉ thấy {lines1:#?} (seed {} cookie)",
        seed_cookies.len()
    );
    assert_eq!(
        ls1,
        seed_ls.len(),
        "[ss] export1 thiếu localStorage key: chỉ thấy {lines1:#?} (seed {} key)",
        seed_ls.len()
    );

    if hash1 != hash2 {
        // Log rõ dòng bị mất/thêm trước khi FAIL.
        for l in &lines1 {
            if !lines2.contains(l) {
                eprintln!("[ss] MẤT sau round-trip: {l}");
            }
        }
        for l in &lines2 {
            if !lines1.contains(l) {
                eprintln!("[ss] THÊM sau round-trip: {l}");
            }
        }
        panic!("[ss] FAIL: hash chuẩn hoá khác nhau — export1 {hash1} != export2 {hash2}");
    }
    println!(
        "[ss] PASS: full storage_state round-trip giữ nguyên tập chuẩn hoá \
         ({} cookie + {} localStorage key, hash {hash1})",
        ck1, ls1
    );
}
