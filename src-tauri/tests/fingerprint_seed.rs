//! W5b: Bằng chứng fingerprint seed QUA BrowserX (integration, `#[ignore]`).
//!
//! Launch thật qua `launcher::build_args` + `ProcessManager` (binary đã cache),
//! rồi `cdp::eval` chạy JS đo 2 tín hiệu fingerprint seed-based: (a) Canvas —
//! hash SHA-256 của pixel readback (`getImageData`), đường mà script fingerprint
//! thật đọc canvas; (b) WebGL — UNMASKED_RENDERER (GPU model chọn theo seed).
//! Khẳng định: 2 seed KHÁC → cả hai tín hiệu KHÁC; phiên 3 cùng seed phiên 1 →
//! cả hai tín hiệu GIỐNG phiên 1.
//!
//! Chạy: `cargo test --test fingerprint_seed -- --ignored --nocapture`
//! (cần binary Chromium đã cache; không có → SKIP kèm ghi chú môi trường).
//!
//! PHÁT HIỆN (W5b): trong build này `canvas.toDataURL()` KHÔNG bị noise theo seed
//! (PNG giống hệt giữa các seed), nhưng `getImageData()` CÓ noise seed-based
//! (khác 1–2 đơn vị mỗi pixel). Vì fingerprinter đọc qua `getImageData`, test dùng
//! chính đường readback này. Nếu hash KHÔNG khác giữa 2 seed → phát hiện QUAN TRỌNG
//! (canvas không seed-based) → test FAIL để phơi bày, không giấu.

use std::path::PathBuf;
use std::time::Duration;

use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, cdp};
use sha2::{Digest, Sha256};

/// Thư mục tạm duy nhất cho mỗi phiên (tự dọn sau khi đo xong).
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-fp-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sha256_hex(data: &str) -> String {
    let mut h = Sha256::new();
    h.update(data.as_bytes());
    hex::encode(h.finalize())
}

/// Vẽ hình xác định rồi trả **base64 của pixel readback** (`getImageData`) —
/// đường fingerprinter thật dùng; noise seed-based nằm ở đây (không ở toDataURL).
const CANVAS_JS: &str = r#"(() => {
  const c = document.createElement('canvas');
  c.width = 320; c.height = 80;
  const ctx = c.getContext('2d');
  ctx.textBaseline = 'top';
  ctx.font = "18px 'Arial'";
  ctx.fillStyle = '#f60';
  ctx.fillRect(8, 8, 240, 40);
  ctx.fillStyle = '#069';
  ctx.fillText('BrowserX fingerprint \u{1F3A8}', 12, 14);
  ctx.strokeStyle = 'rgba(0,180,90,0.7)';
  ctx.beginPath();
  ctx.arc(60, 40, 24, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = 'rgba(120,10,200,0.55)';
  ctx.fillText('seed-probe', 30, 44);
  const data = ctx.getImageData(0, 0, c.width, c.height).data;
  let bin = '';
  for (let i = 0; i < data.length; i++) bin += String.fromCharCode(data[i]);
  return 'px:' + btoa(bin);
})()"#;

/// Đọc UNMASKED_RENDERER + UNMASKED_VENDOR qua WEBGL_debug_renderer_info.
const WEBGL_JS: &str = r#"(() => {
  const c = document.createElement('canvas');
  const gl = c.getContext('webgl') || c.getContext('experimental-webgl');
  if (!gl) return 'no-webgl';
  const ext = gl.getExtension('WEBGL_debug_renderer_info');
  if (!ext) return 'no-debug-ext';
  const r = gl.getParameter(ext.UNMASKED_RENDERER_WEBGL);
  const v = gl.getParameter(ext.UNMASKED_VENDOR_WEBGL);
  return v + ' | ' + r;
})()"#;

struct Fp {
    canvas_hash: String,
    webgl: String,
}

/// Định vị binary (cache có sẵn); timeout 120s. None → SKIPPED (không phải PASSED).
async fn ensure_bin() -> Option<PathBuf> {
    match tokio::time::timeout(Duration::from_secs(120), binary::ensure_binary(None, None)).await {
        Ok(Ok(p)) => Some(p),
        Ok(Err(e)) => {
            eprintln!("[fp] SKIPPED: no binary cache — chưa có binary ({e})");
            None
        }
        Err(_) => {
            eprintln!("[fp] SKIPPED: no binary cache — ensure_binary timeout 120s");
            None
        }
    }
}

/// Launch 1 phiên với `seed`, đo canvas hash + WebGL, teardown sạch rồi trả kết quả.
async fn measure(bin: &str, seed: &str, tag: &str) -> Fp {
    let dir = temp_dir(tag);
    let db = Db::open_at_dir(&dir).unwrap();
    let profile = db
        .create_profile(ProfileInput {
            name: tag.into(),
            fingerprint_seed: Some(seed.into()),
            user_data_dir: Some(dir.join("udd").to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();

    let pm = ProcessManager::new(1);
    let port = pm.allocate_cdp_port().unwrap();
    let args = build_args(&profile, None, port, &[]);
    let sess = pm
        .spawn(&profile.id, bin, args, port)
        .await
        .unwrap_or_else(|e| panic!("[fp] spawn thất bại (seed {seed}): {e}"));
    println!("[fp] {tag}: pid {} cdp {}", sess.pid, port);

    cdp::attach(port)
        .await
        .unwrap_or_else(|e| panic!("[fp] attach CDP thất bại (seed {seed}): {e}"));
    // Điều hướng tới trang scriptable (tránh newtab đặc quyền) trước khi eval.
    let _ = cdp::goto(port, "about:blank").await;

    let canvas = cdp::eval(port, CANVAS_JS)
        .await
        .unwrap_or_else(|e| panic!("[fp] canvas eval thất bại (seed {seed}): {e}"));
    let webgl = cdp::eval(port, WEBGL_JS)
        .await
        .unwrap_or_else(|e| panic!("[fp] webgl eval thất bại (seed {seed}): {e}"));

    let _ = pm.stop(&profile.id).await;
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);

    let readback = canvas.as_str().unwrap_or_default();
    assert!(
        readback.starts_with("px:") && readback.len() > 100,
        "[fp] getImageData readback không hợp lệ (seed {seed}): {}",
        &readback.chars().take(40).collect::<String>()
    );
    Fp {
        canvas_hash: sha256_hex(readback),
        webgl: webgl.as_str().unwrap_or("<null>").to_string(),
    }
}

/// 2 seed khác → canvas hash khác; phiên 3 cùng seed phiên 1 → hash giống.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn canvas_webgl_seed_determinism() {
    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => {
            // Skip thật: test KHÔNG chạy — đừng đọc "ok" của libtest là bằng chứng PASSED.
            eprintln!("[fp] TEST SKIPPED (not passed): thiếu binary cache, không đo gì cả");
            return;
        }
    };

    let a = measure(&bin, "111111", "seedA").await;
    let b = measure(&bin, "222222", "seedB").await;
    // Phiên 3: cùng seed với phiên 1 (profile + user-data-dir mới hoàn toàn).
    let c = measure(&bin, "111111", "seedC").await;

    let short = |h: &str| h.chars().take(16).collect::<String>();
    println!("[fp] seed 111111 (A): canvas={}  webgl={}", short(&a.canvas_hash), a.webgl);
    println!("[fp] seed 222222 (B): canvas={}  webgl={}", short(&b.canvas_hash), b.webgl);
    println!("[fp] seed 111111 (C): canvas={}  webgl={}", short(&c.canvas_hash), c.webgl);

    // Canvas pixel-readback: khác giữa 2 seed, ổn định khi cùng seed.
    assert_ne!(
        a.canvas_hash, b.canvas_hash,
        "canvas readback hash PHẢI khác giữa 2 seed khác nhau — nếu giống, canvas không seed-based (phát hiện quan trọng)"
    );
    assert_eq!(
        a.canvas_hash, c.canvas_hash,
        "canvas readback hash PHẢI giống khi cùng seed (111111) — nếu khác, noise không ổn định theo seed"
    );

    // WebGL UNMASKED_RENDERER: cùng tính chất seed-based (bằng chứng độc lập).
    assert_ne!(
        a.webgl, b.webgl,
        "WebGL renderer PHẢI khác giữa 2 seed khác nhau"
    );
    assert_eq!(
        a.webgl, c.webgl,
        "WebGL renderer PHẢI giống khi cùng seed (111111)"
    );
}
