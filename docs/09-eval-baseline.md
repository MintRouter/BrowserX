# 09 — Baseline eval: version engine + stealth/fingerprint/data-integrity/proxy

> Tổng hợp **baseline đo-được** của BrowserX từ **integration test THẬT** (launch
> binary Chromium đã cache, attach CDP, đo qua `cdp::eval`). Mọi con số dưới đây
> đều **trích từ output test đã chạy** (2026-07-06, host macOS), kèm nguồn
> `path#Lstart-Lend` + commit. **Không bịa GPU/version.**
>
> ⚠️ Đây là baseline **đo tín hiệu cốt lõi qua CDP trên máy dev**, CHƯA phải điểm
> full suite online (`bot.sannysoft.com` / CreepJS / `browserleaks.com`). Các mục
> "chưa chạy online" được ghi rõ, không suy diễn.

## 1. Version engine

- **Binary chạy baseline:** `chromium-145.0.7632.109.2` (cache tại
  `~/.cloakbrowser/chromium-145.0.7632.109.2`), tức **Chromium 145** free-tier cho
  macOS (arm64/x64), **26 patch** (`refs/CloakBrowser/README.md#L845`).
- **Bảng version theo platform** (`refs/CloakBrowser/cloakbrowser/config.py#L20-L26`):
  `darwin-arm64`/`darwin-x64` = `145.0.7632.109.2`; `linux-x64`/`windows-x64` =
  `146.0.7680.177.5`; `linux-arm64` = `146.0.7680.177.3`.
- **`CHROMIUM_VERSION` hiển thị** = `146.0.7680.177.5` — là "mới nhất qua mọi
  platform" (`config.py#L15-L18`), KHÔNG phải version chạy trên macOS.
- **Hệ quả:** macOS free tier ở **145 (26 patch)**, chậm hơn Linux/Windows free
  **146 (58 patch)** (`refs/CloakBrowser/README.md#L841-L847`; docs/06 §6). Đây là
  lý do docs/06 khuyến nghị eval "Windows desktop sát thực" nên chạy trong Docker
  (Linux, nhánh 146).

## 2. Phương pháp đo

Real-binary integration test (`#[ignore]`, cần binary cache): launch 1 phiên
**headful thật** qua `launcher::build_args` + `process::ProcessManager`, attach CDP
rồi `cdp::eval` đo tín hiệu. Các test đo trên **HTTP origin THẬT** (local server
`std::net`, `127.0.0.1:<ephemeral>`) khi cần document origin thật (chrome.runtime,
permissions, localStorage) thay cho `about:blank` opaque.

| Test file | Nội dung | Lệnh chạy |
|---|---|---|
| `src-tauri/tests/stealth.rs` | 9 hard-assert + 4 log-only tín hiệu sannysoft/CreepJS | `cd src-tauri && cargo test --test stealth -- --ignored --nocapture` |
| `src-tauri/tests/fingerprint_seed.rs` | Canvas `getImageData` + WebGL renderer theo seed (W5b) | `cargo test --test fingerprint_seed -- --ignored --nocapture` |
| `src-tauri/tests/fingerprint_diff.rs` | Canvas + WebGL info + WebGL render hash theo seed (W5a) | `cargo test --test fingerprint_diff -- --ignored --nocapture` |
| `src-tauri/tests/cookie_roundtrip.rs` | Round-trip cookie qua CDP (W31b) | `cargo test --test cookie_roundtrip -- --ignored --nocapture` |
| `src-tauri/tests/storage_state_roundtrip.rs` | Round-trip full storage_state (cookie+localStorage, W33b) | `cargo test --test storage_state_roundtrip -- --ignored --nocapture` |
| `src-tauri/tests/proxy_health_accuracy.rs` | Độ chính xác phân loại proxy health-check, offline (W33a) | `cargo test --test proxy_health_accuracy -- --nocapture` |

> Các test fingerprint/cookie/storage cần `export BROWSERX_MASTER_KEY=$(head -c 32
> /dev/urandom | base64)`. Thiếu binary cache → test **SKIP** (không phải PASS).

## 3. Stealth baseline

Đo trên host macOS, profile với `platform` **mặc định = `windows`** (DB default,
`src-tauri/src/db.rs#L300`, `#L921`) → binary xuất fingerprint Windows. Nguồn assert:
`src-tauri/tests/stealth.rs`; commit `e065740` (khung W31) + `3f2d803`/`fcf65bd`
(siết `plugins >= 5`).

| Tín hiệu | Giá trị đo | Loại | Nguồn assert |
|---|---|---|---|
| `navigator.webdriver` | `false` | HARD | `stealth.rs#L268-L272` |
| `navigator.userAgent` | `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36` (KHÔNG chứa "Headless") | HARD | `stealth.rs#L274-L277` |
| `navigator.plugins.length` | `5` | HARD (≥5) | `stealth.rs#L278` |
| `navigator.plugins[0].name` | `"PDF Viewer"` | HARD (≠ rỗng) | `stealth.rs#L283-L286` |
| `navigator.mimeTypes.length` | `2` | HARD (>0) | `stealth.rs#L287` |
| `navigator.languages.length` | `2` | HARD (>0) | `stealth.rs#L279` |
| `typeof window.chrome` | `object` | HARD | `stealth.rs#L280` |
| `navigator.hardwareConcurrency` | `8` | HARD (>0) | `stealth.rs#L288-L291` |
| `window.outerWidth` | `1200` | HARD (>0) | `stealth.rs#L292-L295` |
| `typeof window.chrome.runtime` | `undefined` | log-only | `stealth.rs#L302-L309` |
| `Notification.permission` | `denied` | log-only | `stealth.rs#L317-L320` |
| `permissions.query(notifications).state` | `<unavailable>` | log-only | `stealth.rs#L314-L316` |
| WebGL `UNMASKED_RENDERER` | `ANGLE (NVIDIA, NVIDIA GeForce RTX 5080 Laptop GPU (0x00002C59) Direct3D11 vs_5_0 ps_5_0, D3D11)` | log-only | `stealth.rs#L330-L336` |
| `navigator.deviceMemory` | `null` (không hiện diện) | log-only | `stealth.rs#L337-L340` |

⚠️ **Phạm vi:** mới **assert tín hiệu cốt lõi qua CDP**, CHƯA chạy full suite
`bot.sannysoft.com` / CreepJS online. `chrome.runtime`/`permissions`/`deviceMemory`
là **log-only** (CloakBrowser không đảm bảo; `test_launch.py` gốc chỉ assert
`window.chrome==='object'`) → KHÔNG hard-fail.

## 4. Fingerprint baseline (seed-based)

Canvas hash = SHA-256 của **`getImageData` pixel readback** (đường fingerprinter
thật đọc). WebGL render hash = SHA-256 của `readPixels` sau khi vẽ scene xác định.
Số dưới là **prefix 16 hex** như test in ra (hash đầy đủ không print). Chạy
2026-07-06; commit khung W5 (bằng chứng cũ ghi trong docs/05 dòng 88-90).

| Seed | Canvas hash (16-hex) | WebGL render hash (16-hex) | WebGL renderer |
|---|---|---|---|
| `111111` (A/A#1) | `8039998e4ca25fdd` | `fde5aa4adcda6658` | `ANGLE (NVIDIA ... RTX 5090 Laptop GPU (0x00002C58) Direct3D11..., D3D11)` |
| `111111` relaunch (A#2/C) | `8039998e4ca25fdd` (trùng A) | `fde5aa4adcda6658` (trùng A) | trùng A |
| `222222` (B) | `7f5eb7e3c658fe06` | `0628be0d4bd85a15` | `ANGLE (NVIDIA ... RTX 4090 (0x00002684) Direct3D11..., D3D11)` |

- **Vi sai theo seed:** 2 seed khác → canvas + WebGL render hash + renderer **KHÁC**
  (`fingerprint_diff.rs#L213-L216`, `fingerprint_seed.rs#L172-L189`).
- **Ổn định theo seed:** cùng seed (kể cả relaunch profile mới) → **GIỐNG**
  (`fingerprint_diff.rs#L208-L211`).
- **Phát hiện W5b** (`fingerprint_seed.rs#L13-L17`): trong build này
  `canvas.toDataURL()` **KHÔNG** bị noise theo seed (PNG giống hệt giữa các seed),
  nhưng **`getImageData()` CÓ** noise seed-based (khác 1–2 đơn vị mỗi pixel) → test
  dùng chính đường `getImageData` mà fingerprinter đọc.
- ⚠️ Đo **qua CDP**, CHƯA đo lại trên `browserleaks.com/canvas|webgl` online.

## 5. Data-integrity baseline (round-trip)

Chứng minh export → import → re-export **không mất dữ liệu**: hash tập chuẩn hoá
(sort ổn định, KHÔNG gồm `expires` vì CDP làm tròn double) từ export1 == export2.

- **Cookie round-trip** (`cookie_roundtrip.rs`, W31b, commit `e065740`): 4 cookie
  test (phủ httpOnly/secure/sameSite Strict/Lax/None + session cookie), export1 ==
  export2, hash =
  `38eb8ec37f0d28298decc1ab859e0d9981e37290141fd114e4d5fcbefd277819`.
- **Full storage_state round-trip** (`storage_state_roundtrip.rs`, W33b, commit
  `bf5ecf8`): **3 cookie + 4 localStorage key** (value có space/`=`/`;`/JSON/
  unicode), export1 == export2. Hash là **run-specific** vì origin gồm **port
  ephemeral** (`http://127.0.0.1:<port>/`) đưa vào chuỗi localStorage → run
  2026-07-06 cho hash =
  `3af2a790371d7c14f89f6269b371062b04af31397531668be80556779ef6212c`; điều bất biến
  là **export1 == export2**, KHÔNG phải giá trị hash cố định giữa các run.
- **Phạm vi trung thực:** cookie round-trip chỉ chứng minh **cookie** qua CDP
  (`Storage.setCookies`/`getCookies`); full storage_state bổ sung **localStorage**
  theo origin. KHÔNG bao gồm IndexedDB/ServiceWorker cache.

## 6. Proxy health-check baseline

`proxy_health_accuracy.rs` (W33a, commit `82d2f14` + fix `585be39`): corpus
**OFFLINE tất định** (mock IP-echo + mock forward proxy chỉ bằng `std::net`), **24
kịch bản có nhãn = 9 healthy + 15 unhealthy**. Kết quả run 2026-07-06:

```
Độ chính xác phân loại: 24/24 = 100.00% (ngưỡng ≥95%)
```

- Case healthy đi qua **đúng tầng proxy** (`reqwest::Proxy::all` → mock forward
  proxy: absolute-form GET + CONNECT tunnel), không shortcut vào ip-echo.
- Case unhealthy phủ: cổng đóng, timeout im lặng/treo, captive-portal trả HTML,
  407 auth, body rỗng, JSON thiếu key `ip`, token rác, HTTP 500. Case #24 (JSON lỗi
  không whitespace) đóng nhờ validate token bằng `IpAddr` (`fix 585be39`).
- **Caveat:** đo trên **HTTP-proxy transport** (mock forward proxy HTTP); chưa đo
  SOCKS5 hay proxy provider thật online.

## 7. HẠN CHẾ MÔI TRƯỜNG (đọc kỹ — đây là lý do exit-criteria này tồn tại)

Baseline trên đo trên **1 host macOS cụ thể**; đừng đọc như điểm phổ quát. Các giới
hạn cần ghi rõ khi diễn giải:

- **WebGL/GPU renderer là giá trị FABRICATED, không phải GPU thật của host, cũng
  không phải SwiftShader.** Run này (profile `platform=windows` — DB default) cho
  renderer **Windows D3D11 NVIDIA** (RTX 5090/5080/4090), khác nhau theo seed — do
  fingerprint patch của binary tạo ra, KHÔNG phải GPU macOS/Metal thật của máy.
  Trong **Docker (Linux)** thì ngược lại: không GPU → engine dùng **software GL
  SwiftShader** (`--use-angle=swiftshader`,
  `refs/CloakBrowser-Manager/backend/browser_manager.py#L384`; docs/06 §6) → renderer
  khác hẳn máy thật. Cả hai đều KHÔNG phải GPU host thật.
- **Font Windows chưa đủ cho CreepJS.** Image Manager chỉ có
  `ttf-mscorefonts-installer` (font XP-era), chưa đủ để đạt điểm CreepJS cao
  (`refs/CloakBrowser/README.md#L746`; docs/06 §6). Chưa cài bộ font Windows đầy đủ.
- **`fingerprint-platform` MẶC ĐỊNH theo host, KHÔNG khoá cứng.** Wrapper default:
  Darwin → `macos` (bỏ spoof Windows), Linux/Windows → `windows`
  (`config.py#L57-L76`; `src-tauri/src/launcher.rs` `host_default_platform`). Nhưng
  BrowserX cho **override tự do theo profile** (`profile.platform`, DB default
  `windows` — `db.rs#L300`): **ép được** `windows` trên máy Mac (chính là run
  baseline này) — vẫn chạy, nhưng **chất lượng giảm do mismatch** (UA Windows nhưng
  GPU/font/WebGL host thật của Mac có thể lộ). Đây là **mô hình cảnh báo**, không
  chặn cứng (docs/03 §6).
- **Cảnh báo cross-OS mismatch:** khi target OS ≠ host OS, tín hiệu host thật dễ lộ
  lệch (docs/03 §6, `#L267-L294`). Muốn danh mục profile Windows chất lượng cao hàng
  loạt thì **nên** chạy trên host Linux/Windows.
- **Chưa chạy full suite online:** `bot.sannysoft.com`, CreepJS,
  `browserleaks.com/canvas|webgl|fonts` **chưa được chạy/ghi điểm**. Baseline này
  chỉ là **assert tín hiệu cốt lõi qua CDP** trên máy dev.

---

**Nguồn commit chính:** `e065740` (W31 stealth+cookie), `3f2d803`/`fcf65bd` (siết
stealth), `bf5ecf8` (W33b storage_state), `82d2f14`+`585be39` (W33a proxy 24/24),
`26154b2` (docs roadmap W33). Số liệu đo lại 2026-07-06 trên
`chromium-145.0.7632.109.2`.
