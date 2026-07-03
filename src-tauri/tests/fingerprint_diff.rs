//! W5a: Vi sai fingerprint QUA đường thật BrowserX (integration, `#[ignore]`).
//!
//! Launch thật (`launcher::build_args` + `ProcessManager`, binary đã cache) rồi
//! `cdp::eval` đo 3 tín hiệu seed-based: (a) Canvas — SHA-256 pixel readback
//! (`getImageData`); (b) WebGL info — UNMASKED_VENDOR|RENDERER; (c) WebGL render
//! hash — SHA-256 của `readPixels` sau khi vẽ 1 scene WebGL xác định.
//!
//! Khẳng định: 2 profile seed KHÁC → cả 3 tín hiệu KHÁC; relaunch CÙNG 1 profile
//! → cả 3 tín hiệu GIỐNG (ổn định theo seed). Nếu tín hiệu KHÔNG khác giữa 2 seed
//! → phát hiện QUAN TRỌNG (binary không áp fingerprint) → FAIL để phơi bày.
//!
//! Chạy: `export BROWSERX_MASTER_KEY=$(head -c 32 /dev/urandom | base64)` rồi
//! `cargo test --test fingerprint_diff -- --ignored --nocapture`
//! (cần binary Chromium đã cache; không có → SKIP kèm ghi chú môi trường).

use std::path::PathBuf;
use std::time::Duration;

use browserx_lib::db::{Db, ProfileInput};
use browserx_lib::launcher::build_args;
use browserx_lib::models::Profile;
use browserx_lib::process::ProcessManager;
use browserx_lib::{binary, cdp};
use sha2::{Digest, Sha256};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("browserx-fpd-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sha256_hex(data: &str) -> String {
    let mut h = Sha256::new();
    h.update(data.as_bytes());
    hex::encode(h.finalize())
}

/// Canvas 2D → base64 pixel readback (`getImageData`), đường fingerprinter thật đọc.
const CANVAS_JS: &str = r#"(() => {
  const c = document.createElement('canvas');
  c.width = 320; c.height = 80;
  const ctx = c.getContext('2d');
  ctx.textBaseline = 'top';
  ctx.font = "18px 'Arial'";
  ctx.fillStyle = '#f60'; ctx.fillRect(8, 8, 240, 40);
  ctx.fillStyle = '#069'; ctx.fillText('BrowserX fingerprint \u{1F3A8}', 12, 14);
  ctx.strokeStyle = 'rgba(0,180,90,0.7)';
  ctx.beginPath(); ctx.arc(60, 40, 24, 0, Math.PI * 2); ctx.stroke();
  ctx.fillStyle = 'rgba(120,10,200,0.55)'; ctx.fillText('seed-probe', 30, 44);
  const data = ctx.getImageData(0, 0, c.width, c.height).data;
  let bin = '';
  for (let i = 0; i < data.length; i++) bin += String.fromCharCode(data[i]);
  return 'px:' + btoa(bin);
})()"#;

/// UNMASKED_VENDOR | UNMASKED_RENDERER (GPU model chọn theo seed).
const WEBGL_INFO_JS: &str = r#"(() => {
  const c = document.createElement('canvas');
  const gl = c.getContext('webgl') || c.getContext('experimental-webgl');
  if (!gl) return 'no-webgl';
  const ext = gl.getExtension('WEBGL_debug_renderer_info');
  if (!ext) return 'no-debug-ext';
  return gl.getParameter(ext.UNMASKED_VENDOR_WEBGL) + ' | ' +
         gl.getParameter(ext.UNMASKED_RENDERER_WEBGL);
})()"#;

/// Vẽ scene WebGL xác định → base64 `readPixels` (noise seed-based nằm ở readback).
const WEBGL_RENDER_JS: &str = r#"(() => {
  const c = document.createElement('canvas');
  c.width = 128; c.height = 128;
  const gl = c.getContext('webgl') || c.getContext('experimental-webgl');
  if (!gl) return 'no-webgl';
  const vs = gl.createShader(gl.VERTEX_SHADER);
  gl.shaderSource(vs, 'attribute vec2 p; varying vec2 v; void main(){v=p; gl_Position=vec4(p,0.0,1.0);}');
  gl.compileShader(vs);
  const fs = gl.createShader(gl.FRAGMENT_SHADER);
  gl.shaderSource(fs, 'precision mediump float; varying vec2 v; void main(){gl_FragColor=vec4(abs(v.x),abs(v.y),0.6,1.0);}');
  gl.compileShader(fs);
  const pr = gl.createProgram();
  gl.attachShader(pr, vs); gl.attachShader(pr, fs); gl.linkProgram(pr); gl.useProgram(pr);
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-0.8,-0.8, 0.8,-0.8, 0.0,0.9]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(pr, 'p');
  gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  gl.clearColor(0.1, 0.2, 0.3, 1.0); gl.clear(gl.COLOR_BUFFER_BIT);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  const px = new Uint8Array(c.width * c.height * 4);
  gl.readPixels(0, 0, c.width, c.height, gl.RGBA, gl.UNSIGNED_BYTE, px);
  let bin = '';
  for (let i = 0; i < px.length; i++) bin += String.fromCharCode(px[i]);
  return 'gl:' + btoa(bin);
})()"#;

struct Fp {
    canvas: String,
    webgl_info: String,
    webgl_render: String,
}

/// Định vị binary (cache có sẵn); timeout 120s. None → SKIPPED (không phải PASSED).
async fn ensure_bin() -> Option<PathBuf> {
    match tokio::time::timeout(Duration::from_secs(120), binary::ensure_binary(None, None)).await {
        Ok(Ok(p)) => Some(p),
        Ok(Err(e)) => {
            eprintln!("[fpd] SKIPPED: no binary cache — chưa có binary ({e})");
            None
        }
        Err(_) => {
            eprintln!("[fpd] SKIPPED: no binary cache — ensure_binary timeout 120s");
            None
        }
    }
}

/// Launch `profile` (giữ nguyên user_data_dir), đo 3 tín hiệu, teardown, trả kết quả.
async fn launch_measure(profile: &Profile, bin: &str, tag: &str) -> Fp {
    let pm = ProcessManager::new(1);
    let port = pm.allocate_cdp_port().unwrap();
    let args = build_args(profile, None, port, &[]);
    let sess = pm
        .spawn(&profile.id, bin, args, port)
        .await
        .unwrap_or_else(|e| panic!("[fpd] spawn thất bại ({tag}): {e}"));
    println!("[fpd] {tag}: pid {} cdp {}", sess.pid, port);

    cdp::attach(port)
        .await
        .unwrap_or_else(|e| panic!("[fpd] attach CDP thất bại ({tag}): {e}"));
    let _ = cdp::goto(port, "about:blank").await;

    let canvas = cdp::eval(port, CANVAS_JS)
        .await
        .unwrap_or_else(|e| panic!("[fpd] canvas eval thất bại ({tag}): {e}"));
    let webgl_info = cdp::eval(port, WEBGL_INFO_JS)
        .await
        .unwrap_or_else(|e| panic!("[fpd] webgl-info eval thất bại ({tag}): {e}"));
    let webgl_render = cdp::eval(port, WEBGL_RENDER_JS)
        .await
        .unwrap_or_else(|e| panic!("[fpd] webgl-render eval thất bại ({tag}): {e}"));

    let _ = pm.stop(&profile.id).await;

    let cv = canvas.as_str().unwrap_or_default();
    let gr = webgl_render.as_str().unwrap_or_default();
    assert!(
        cv.starts_with("px:") && cv.len() > 100,
        "[fpd] canvas readback không hợp lệ ({tag}): {}",
        &cv.chars().take(40).collect::<String>()
    );
    assert!(
        gr.starts_with("gl:") && gr.len() > 100,
        "[fpd] webgl readback không hợp lệ ({tag}): {}",
        &gr.chars().take(40).collect::<String>()
    );
    Fp {
        canvas: sha256_hex(cv),
        webgl_info: webgl_info.as_str().unwrap_or("<null>").to_string(),
        webgl_render: sha256_hex(gr),
    }
}

/// 2 profile seed khác → 3 tín hiệu khác; relaunch CÙNG profile → 3 tín hiệu giống.
#[tokio::test]
#[ignore = "cần binary Chromium đã cache; chạy với --ignored"]
async fn fingerprint_differs_across_seeds_stable_within_profile() {
    let bin = match ensure_bin().await {
        Some(p) => p.to_string_lossy().into_owned(),
        None => {
            // Skip thật: test KHÔNG chạy — đừng đọc "ok" của libtest là bằng chứng PASSED.
            eprintln!("[fpd] TEST SKIPPED (not passed): thiếu binary cache, không đo gì cả");
            return;
        }
    };

    let dir_a = temp_dir("A");
    let dir_b = temp_dir("B");
    let db_a = Db::open_at_dir(&dir_a).unwrap();
    let db_b = Db::open_at_dir(&dir_b).unwrap();
    let profile_a = db_a
        .create_profile(ProfileInput {
            name: "seedA".into(),
            fingerprint_seed: Some("111111".into()),
            user_data_dir: Some(dir_a.join("udd").to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();
    let profile_b = db_b
        .create_profile(ProfileInput {
            name: "seedB".into(),
            fingerprint_seed: Some("222222".into()),
            user_data_dir: Some(dir_b.join("udd").to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();

    // Profile A launch 2 lần (cùng seed + user-data-dir) → chứng minh ổn định.
    let a1 = launch_measure(&profile_a, &bin, "A#1").await;
    let a2 = launch_measure(&profile_a, &bin, "A#2").await;
    // Profile B (seed khác) → chứng minh vi sai.
    let b = launch_measure(&profile_b, &bin, "B").await;

    let short = |h: &str| h.chars().take(16).collect::<String>();
    println!("[fpd] A#1 seed 111111: canvas={} render={} info={}", short(&a1.canvas), short(&a1.webgl_render), a1.webgl_info);
    println!("[fpd] A#2 seed 111111: canvas={} render={} info={}", short(&a2.canvas), short(&a2.webgl_render), a2.webgl_info);
    println!("[fpd] B   seed 222222: canvas={} render={} info={}", short(&b.canvas), short(&b.webgl_render), b.webgl_info);

    // Ổn định theo seed: relaunch cùng profile cho kết quả GIỐNG.
    assert_eq!(a1.canvas, a2.canvas, "canvas PHẢI ổn định khi relaunch cùng profile (seed 111111)");
    assert_eq!(a1.webgl_render, a2.webgl_render, "WebGL render hash PHẢI ổn định khi relaunch cùng profile");
    assert_eq!(a1.webgl_info, a2.webgl_info, "WebGL vendor|renderer PHẢI ổn định khi relaunch cùng profile");

    // Vi sai theo seed: 2 profile seed khác cho kết quả KHÁC.
    assert_ne!(a1.canvas, b.canvas, "canvas readback PHẢI khác giữa 2 seed — nếu giống, canvas không seed-based (phát hiện quan trọng)");
    assert_ne!(a1.webgl_render, b.webgl_render, "WebGL render hash PHẢI khác giữa 2 seed — nếu giống, WebGL render không seed-based");
    assert_ne!(a1.webgl_info, b.webgl_info, "WebGL vendor|renderer PHẢI khác giữa 2 seed");

    drop(db_a);
    drop(db_b);
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}
