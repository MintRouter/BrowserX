# 07 — Đặc tả tính năng (parity Multilogin)

> **Mục đích:** Ánh xạ từng tính năng "kiểu Multilogin" vào khả năng **thật** của
> CloakBrowser (engine) và CloakBrowser-Manager (UI/backend), dựa trên **code trong
> `refs/`** (không đọc README làm chuẩn trừ khi README là nơi duy nhất liệt kê flag của
> binary đóng). Mọi khẳng định đều kèm `path#Lstart-Lend`. Đây là tài liệu để **thiết kế
> BrowserX** — dự án OPEN SOURCE, KHOÁ CỨNG CloakBrowser, hướng scale hàng nghìn profiles
> (xem Spec, mục "Quyết định cuối 2026-07-01").

Liên quan: [`docs/02-critique-risks.md`](02-critique-risks.md) (thiếu "phần Multilogin" — rủi ro #4),
[`docs/04-licensing-legal-decision.md`](04-licensing-legal-decision.md) (binary đóng, cấm reverse).

> **UI parity:** đặc tả giao diện clone layout Multilogin X (top bar/search, sidebar, profile
> table, wizard, running dashboard, dark mode + i18n) và map hành động UI → Tauri command nằm ở
> [`docs/08-ui-spec.md`](08-ui-spec.md).

---

## 0. Quy ước trạng thái (legend)

| Ký hiệu | Ý nghĩa |
|---|---|
| ✅ **Engine** | CloakBrowser binary/wrapper hỗ trợ sẵn (chỉ cần truyền flag/tham số) |
| 🟡 **Manager** | CloakBrowser-Manager đã tự xây một phần (UI/DB/endpoint) |
| 🔴 **Thiếu** | Không tồn tại trong cả 2 repo → **BrowserX phải tự xây** |
| ⚠️ **Giới hạn** | Có nhưng ràng buộc/không production-grade (ghi rõ ở mục chi tiết) |
| **Cột BrowserX** | Trạng thái **code BrowserX hiện tại** (2026-07-07, grep `src/` + `src-tauri/src/`): ✅ đã có, 🟡 một phần, 🔴 chưa làm (kèm lý do ngắn) |

> Lưu ý phương pháp: các thuộc tính fingerprint (canvas/WebGL/audio/GPU/screen/fonts) được
> patch **ở source-level trong binary C++ đóng** → ta **không đọc được implementation**, chỉ
> đọc được **giao diện điều khiển** (CLI flags) do wrapper/README công bố. Vì vậy phần
> "cách hoạt động bên trong" là **hộp đen** (rủi ro #5 ở docs/02).

---

## 1. Bảng tổng quan parity (feature × trạng thái)

| Tính năng Multilogin | Engine | Manager | BrowserX | Nguồn chính |
|---|:--:|:--:|:--|---|
| Fingerprint theo seed (canvas/WebGL/audio/fonts/rects) | ✅ | 🟡 | ✅ `launcher.rs` (`--fingerprint=seed`) | `refs/CloakBrowser/README.md#L698-L703`; `refs/CloakBrowser-Manager/backend/browser_manager.py#L387-L389` |
| GPU vendor/renderer override | ✅ | 🟡 | ✅ `launcher.rs` | `README.md#L713-L714`; `browser_manager.py#L396-L402` |
| Hardware concurrency / device memory | ✅ | ⚠️ | ✅ `launcher.rs` (cả 2 flag) | `README.md#L715-L716`; `browser_manager.py#L404-L406` |
| Screen width/height | ✅ | 🟡 | ✅ `launcher.rs` | `README.md#L717-L718`; `browser_manager.py#L408-L413` |
| Chọn OS fingerprint (Windows/macOS/Linux) — user chọn tự do + cảnh báo mismatch | ✅ | 🟡 | ✅ `launcher.rs` + cảnh báo mismatch trong UI (`i18n` `platformMismatch`) | `refs/CloakBrowser/cloakbrowser/config.py#L54-L76`; `browser_manager.py#L391-L394` |
| Brand / brand-version / platform-version (UA-CH) | ✅ | 🔴 | ✅ `launcher.rs` (P3-5a) | `README.md#L719-L721` |
| WebRTC IP spoof theo proxy | ✅ | ⚠️ | ✅ `launcher.rs` (`webrtc_mode` masked/auto/real) | `refs/CloakBrowser/cloakbrowser/browser.py#L201-L204`, `#L987-L1025` |
| Noise on/off (chống ML tampering) | ✅ | 🔴 | ✅ `launcher.rs` (`fp_noise`, W19c) | `README.md#L730` |
| Fonts theo platform (fonts-dir / font-metrics) | ✅ | 🔴 | ✅ `launcher.rs` (`fonts_dir`, `windows_font_metrics`) | `README.md#L727-L728` |
| Proxy per-profile HTTP/HTTPS/SOCKS5 + auth | ✅ | 🟡 | ✅ `db.rs` (http/https/socks5) + `launcher.rs` `--proxy-server` | `browser.py#L1305-L1351`; `browser_manager.py#L22-L53` |
| Proxy health-check / rotation / marketplace | 🔴 | 🔴 | 🟡 health-check ✅ (`proxy_check.rs`, W19b); rotation ✅ (`db.rs` `rotate_proxy` + `commands.rs` `rotate_proxy`/`rotate_proxies`, W36 commit 69509a4 — round-robin bỏ qua proxy unhealthy, wrap-around, ghi audit; UI: toolbar + menu ⋮ — W39 8aa163d, W40 2d2828f; notice "áp dụng ở lần launch sau" khi profile đang chạy, single + bulk — W41 0c8b3b4; rotate-on-launch per-profile — schema v12 cột `rotate_on_launch`, tự xoay best-effort trong `launch_profile_inner`, toggle ProxyTab, audit `profile.rotate_on_launch` — W42 11c3007); marketplace 🔴 chưa làm | (không có — xem §4) |
| Timezone/Geo/Locale auto khớp IP (GeoIP) | ✅ | 🟡 | ✅ auto khớp IP thoát proxy (`geoip.rs`, W35 commit 69509a4 — resolve timezone/locale/geolocation qua ipapi.co **qua proxy**, không dùng mmdb offline; wire `launcher::build_args`); best-effort tổng ≤10s (W37 96eb4a4), set thủ công thắng GeoIP, `geoip=false` bỏ qua | `refs/CloakBrowser/cloakbrowser/geoip.py#L54-L109`; `browser_manager.py#L222-L226` |
| Geolocation coords override | ✅ | 🔴 | ✅ `launcher.rs` (`geolocation_mode` manual → `--fingerprint-location`) | `README.md#L722` |
| Cookie/localStorage persistence (user_data_dir) | ✅ | 🟡 | ✅ `launcher.rs` (`--user-data-dir`) | `browser.py#L347-L471`; `browser_manager.py#L217-L218` |
| Cookie/profile import-export (định dạng) | ⚠️ | 🔴 | ✅ cookie JSON+Netscape qua CDP (`cookies.rs`, W24a); profile `.bxprofile` (`export.rs`, W19a) | `browser.py#L745`, `README.md#L755-L765` (chỉ `storage_state`); Manager: không endpoint |
| Mã hoá cookie/proxy khi lưu (encryption at rest) | 🔴 | 🔴 | 🟡 proxy credential mã hoá XChaCha20-Poly1305 (`crypto.rs`, `username_enc/password_enc`); backup mã hoá (W25a); cookie vẫn trong `user_data_dir` của Chromium | `refs/CloakBrowser-Manager/backend/database.py#L34-L59` (plaintext) |
| Tags | — | 🟡 | ✅ `commands.rs` (`list_tags`, `set_profile_tags`) | `database.py#L61-L66`; `models.py#L60-L67` |
| Search / filter profile | — | ⚠️ | ✅ server-side SQL (`db.rs` `search_profiles` + `ProfileFilter` os/proxy/tag/folder; canary 10k) | `refs/CloakBrowser-Manager/frontend/src/components/ProfileList.tsx#L14-L17` (client-side, theo tên) |
| Template profile | — | 🔴 | ✅ profile + proxy template (`commands.rs` W20b: `list_templates`, `create_profile_from_template`, `create_proxy_from_template`) | (không có) |
| Bulk create / bulk launch | — | ⚠️ | ✅ bulk create từ template (`create_profiles_from_template`, W29a) + bulk launch/stop qua selection (Shift+L/S, tuần tự) | chỉ `auto_launch` khi khởi động: `browser_manager.py#L342-L362` |
| Notes theo profile | — | 🟡 | ✅ cột `notes` (`db.rs`, `models.rs`) | `database.py#L55`; `models.py#L31` |
| Team / multi-user / RBAC | 🔴 | ⚠️ | 🔴 chưa làm — app desktop local 1 user, chưa có team/RBAC | `refs/CloakBrowser-Manager/backend/main.py#L48-L80` (1 token chung) |
| Automation API (CDP/Playwright) | ✅ | 🟡 | ✅ `get_cdp_ws_url` (`commands.rs`, `cdp.rs`, W24c — xem §10) | `main.py#L845-L879`; `browser_manager.py#L217` |
| Audit log (bền, truy vấn được) | 🔴 | 🔴 | ✅ `db.rs` (`insert_audit`/`list_audit`, filter + cursor) + viewer/metrics (W26, `metrics.rs`, `get_metrics`) | `main.py#L42` (chỉ log stdout) |
| Mobile fingerprint (Android/iOS) | 🔴 | 🔴 | 🔴 chưa làm — binary CloakBrowser chỉ desktop, khoá cứng nên phụ thuộc CloakHQ | `config.py#L91-L98` (chỉ desktop) |
| Engine Firefox (Stealthfox) | 🔴 | 🔴 | 🔴 chưa làm — chỉ Chromium, khoá cứng CloakBrowser | chỉ Chromium (`config.py#L18-L26`) |

> Cột "Engine —" nghĩa là tính năng thuộc tầng quản lý (không phải nhiệm vụ của binary).

> **Chọn OS fingerprint (parity Multilogin):** `--fingerprint-platform=<macos|windows|...>`
> chỉ có **mặc định theo host**, **không bị khoá** (`config.py#L54-L76` không validate giá
> trị theo host). Vì BrowserX spawn binary trực tiếp từ Rust, user được **chọn target OS
> tự do** (ví dụ profile Windows trên máy Mac). Khi target OS ≠ host OS, BrowserX **hiện
> cảnh báo mismatch** (fonts/GPU/WebGL renderer thật của host lộ ra → chất lượng ngụy
> trang giảm) — đúng mô hình **cảnh báo của Multilogin**, không chặn cứng. Khuyến nghị
> (không bắt buộc): profile Windows chất lượng cao hàng loạt → chạy host Linux/Windows.
> Chi tiết: docs 03 §6.

---

## 2. Fingerprint: ánh xạ flag `--fingerprint*` → thuộc tính

Binary nhận **một seed chủ** rồi tự sinh toàn bộ danh tính; flag riêng lẻ chỉ để **ghi đè**
từng giá trị (`README.md#L679-L705`). Danh sách flag đầy đủ (nguồn duy nhất là README của
binary đóng — xem cảnh báo hộp đen ở §0):

| Flag | Thuộc tính JS/hệ thống bị chi phối | Nguồn |
|---|---|---|
| `--fingerprint=<seed>` | Seed chủ: canvas, WebGL, audio, fonts, client rects | `README.md#L700` |
| `--fingerprint-platform` | `navigator.platform`, UA OS, chọn pool GPU | `README.md#L701` |
| `--fingerprint-gpu-vendor` | `UNMASKED_VENDOR_WEBGL` | `README.md#L713` |
| `--fingerprint-gpu-renderer` | `UNMASKED_RENDERER_WEBGL` | `README.md#L714` |
| `--fingerprint-hardware-concurrency` | `navigator.hardwareConcurrency` | `README.md#L715` |
| `--fingerprint-device-memory` | `navigator.deviceMemory` (GB) | `README.md#L716` |
| `--fingerprint-screen-width/height` | Kích thước màn hình | `README.md#L717-L718` |
| `--fingerprint-brand` / `-brand-version` | Brand + version (UA + Client Hints) | `README.md#L719-L720` |
| `--fingerprint-platform-version` | Client Hints platform version | `README.md#L721` |
| `--fingerprint-location` | Toạ độ geolocation | `README.md#L722` |
| `--fingerprint-timezone` / `-locale` | Timezone / locale | `README.md#L723-L724` |
| `--fingerprint-storage-quota` | `storage.estimate()`, storageBuckets | `README.md#L725` |
| `--fingerprint-taskbar-height` | Chiều cao taskbar | `README.md#L726` |
| `--fingerprint-fonts-dir` | Thư mục font theo platform | `README.md#L727` |
| `--fingerprint-windows-font-metrics` | Căn font-metrics kiểu Windows (binary 148+) | `README.md#L728` |
| `--fingerprint-webrtc-ip` | IP trong ICE candidate của WebRTC | `README.md#L729` |
| `--fingerprint-noise=false` | Tắt noise (canvas/WebGL/audio/rects), giữ seed | `README.md#L730` |

**Cách sinh & lưu seed (thực tế trong Manager):**
- Seed là **integer**, cột `fingerprint_seed INTEGER NOT NULL` trong SQLite
  (`database.py#L37`).
- Khi tạo profile không truyền seed → `random.randint(10000, 99999)`
  (`database.py#L93`) — cùng dải mặc định binary công bố (`README.md#L700`).
- Manager dựng arg từ profile trong `_build_fingerprint_args`
  (`browser_manager.py#L379-L415`): chỉ map `--fingerprint`, `-platform`,
  `-gpu-vendor`, `-gpu-renderer`, `-hardware-concurrency`, `-screen-width/height`.
- **Khoảng trống Manager (BrowserX phải tự bổ sung UI/DB):** `device-memory`, `brand*`,
  `platform-version`, `location`, `storage-quota`, `taskbar-height`, `fonts-dir`,
  `windows-font-metrics`, `noise` — **không** có cột DB lẫn arg builder
  (đối chiếu `database.py#L34-L59` và `browser_manager.py#L379-L415`).

> Với scale hàng nghìn profiles: seed integer 5 chữ số chỉ có ~90k giá trị → **nguy cơ
> trùng seed** khi vượt vài chục nghìn profiles. BrowserX nên mở rộng không gian seed
> (binary chấp nhận chuỗi `[A-Za-z0-9_-]{1,128}`, xem `cloakserve` `SAFE_SEED_RE`).

---

## 3. Proxy per-profile

- **Định dạng chấp nhận (Manager):** `http/https/socks5://`, `host:port`,
  `host:port:user:pass` — chuẩn hoá về `http://user:pass@host:port`
  (`browser_manager.py#L22-L39`), rồi validate scheme/host/port
  (`browser_manager.py#L41-L53`).
- **Áp vào launch:** normalize + validate ngay trước khi mở context
  (`browser_manager.py#L209-L213`).
- **Engine phân giải proxy** theo version binary (credentials inline vs Playwright dict)
  trong `_resolve_proxy_config` (`browser.py#L1305` trở đi).
- **Giới hạn:** Manager **không** health-check/xoay proxy/marketplace; chỉ dùng exit IP
  của proxy cho GeoIP (§5). Không mã hoá proxy khi lưu — cột `proxy TEXT` plaintext
  (`database.py#L38`).

---

## 4. Proxy nâng cao — khoảng trống lớn

Cả engine lẫn Manager **không** có: kiểm tra sức khoẻ proxy, xoay/pool proxy, tích hợp
marketplace, hay gán proxy theo nhóm. Manager chỉ lưu 1 chuỗi proxy/profile
(`database.py#L38`) và dùng exit IP cho GeoIP. → **BrowserX tự xây** nếu cần vận hành scale
(pool, health-check định kỳ, gán lại khi proxy chết).

---

## 5. Timezone / Geolocation / Locale (GeoIP)

- Engine tải **GeoLite2-City.mmdb (~70MB)** từ mirror P3TERX lần đầu, cache cục bộ
  (`geoip.py#L1-L30`).
- `resolve_proxy_geo(proxy_url)` trả `(timezone, locale)` suy từ IP thoát của proxy;
  `resolve_proxy_geo_with_ip` trả thêm exit IP để spoof WebRTC
  (`geoip.py#L54-L106`).
- Country → locale qua bảng `COUNTRY_LOCALE_MAP` (`geoip.py#L35`).
- Manager bật/tắt qua cờ `geoip` (`database.py#L52`), truyền vào launch
  (`browser_manager.py#L222-L226`); timezone/locale cũng có thể set thủ công
  (`database.py#L39-L40`).
- **Giới hạn:** `--fingerprint-location` (toạ độ) **không** được Manager expose → chỉ khớp
  timezone/locale, chưa khớp toạ độ geolocation.

---

## 6. Cookie / Storage / Persistence

- Persistence chuẩn qua **`user_data_dir`** (profile Chromium thật) bằng
  `launch_persistent_context` (`browser.py#L347-L471`); Manager luôn dùng cơ chế này
  (`browser_manager.py#L217-L218`, cột `user_data_dir` bắt buộc `database.py#L56`).
- **Import/export cookie có cấu trúc:** engine chỉ hỗ trợ Playwright `storage_state`
  (JSON cookie + localStorage) qua kwargs của context (`README.md#L755-L765`) — **không**
  có định dạng cookie chuyên dụng kiểu Multilogin (Netscape/JSON chuẩn).
- **Manager: không có endpoint import/export** cookie/profile (không route nào trong
  `main.py`). → **BrowserX tự xây** nếu cần chuyển profile giữa máy/đội.
- **Không mã hoá at-rest:** cookie nằm trong `user_data_dir` và proxy plaintext trong DB
  (`database.py#L34-L59`).

---

## 7. Quản lý profile (list / search / tag / bulk / template / notes)

- **CRUD + tag + notes:** đầy đủ ở DB/model (`database.py#L34-L66`, `models.py#L25-L67`);
  tag lưu bảng `profile_tags` (`database.py#L61-L66`).
- **Search:** chỉ **client-side theo tên**
  (`frontend/src/components/ProfileList.tsx#L14-L17`); `GET /api/profiles` trả toàn bộ
  (`main.py#L438-L439`) → không phân trang/lọc phía server → **không scale** hàng nghìn.
- **Bulk:** không có bulk create/launch; chỉ `auto_launch` tuần tự lúc khởi động
  (`browser_manager.py#L342-L362`).
- **Template profile:** không có.
- **Human-like automation:** có cờ `humanize` + `human_preset` (default/careful) truyền
  xuống engine (`browser_manager.py#L224-L225`, `models.py` field `human_preset`).

---

## 8. Team / RBAC / Automation / Audit

- **Auth:** một **AUTH_TOKEN chung** (env), so khớp qua header hoặc cookie
  (`main.py#L48-L80`). **Không** user/role/RBAC, không phân quyền theo profile/đội.
- **Automation:** engine expose CDP; Manager cấp `cdp_url` per-profile
  (`main.py#L546`) và endpoint CDP/JSON version để `connect_over_cdp`
  (`main.py#L845-L879`). VNC/noVNC qua websocket (`main.py#L677-L678`).
- **Audit log bền:** không có. Chỉ log stdout qua `logging.basicConfig`
  (`main.py#L41-L42`) — không lưu DB, không truy vấn được.

---

## 9. Kết luận cho thiết kế BrowserX

**Kế thừa nguyên trạng từ CloakBrowser (đừng làm lại):** fingerprint theo seed + toàn bộ
flag `--fingerprint*`, proxy per-profile, GeoIP timezone/locale, WebRTC IP spoof,
persistence qua `user_data_dir`, CDP automation. (Lưu ý: BrowserX mở browser **headful**
trực tiếp, **KHÔNG** dùng VNC/noVNC của Manager tham chiếu — xem pivot ở docs/03 §0.)

**Phải tự xây (🔴/⚠️) — ưu tiên cho scale hàng nghìn:**
1. **Search/filter/phân trang phía server** + bulk create/launch/stop (thay
   `main.py#L438-L439` + client filter).
2. **Không gian seed lớn** (chuỗi tới 128 ký tự) tránh trùng khi vượt ~90k profiles.
3. **Expose các flag còn thiếu:** device-memory, brand*, platform-version, location,
   storage-quota, fonts-dir, windows-font-metrics, noise.
4. **Quản lý proxy production:** pool + health-check + xoay + gán lại.
5. **Import/export cookie & profile** (định dạng chuẩn) để di chuyển giữa máy/đội.
6. **Mã hoá at-rest** cho proxy/cookie/secret trong DB.
7. **Multi-user + RBAC + audit log bền** (thay 1 token chung).
8. **Template profile** để tạo hàng loạt nhất quán.

**Ngoài phạm vi hiện tại của engine (không phải chỉ Manager):** mobile fingerprint
(chỉ desktop — `config.py#L91-L98`) và engine Firefox/Stealthfox (chỉ Chromium —
`config.py#L18-L26`). Do **KHOÁ CỨNG CloakBrowser**, các mục này phụ thuộc roadmap của
CloakHQ, BrowserX không tự vá được (binary đóng — xem docs/04).

---

## 10. Automation với BrowserX — Copy CDP URL (W24c)

Mỗi profile **đang chạy** expose một endpoint CDP local. Trong bảng Profiles, mở menu
⋮ của profile đang chạy → **Copy CDP URL** để copy `webSocketDebuggerUrl` dạng
`ws://127.0.0.1:{port}/devtools/browser/{id}` vào clipboard (backend lấy port từ
process manager rồi đọc `GET /json/version` — command `get_cdp_ws_url`,
`src-tauri/src/commands.rs`; helper `cdp::ws_url`, `src-tauri/src/cdp.rs`).

Dán URL vào script automation:

**Playwright** (`connectOverCDP` nhận cả URL `ws://` lẫn `http://127.0.0.1:{port}`):

```ts
import { chromium } from "playwright";

const browser = await chromium.connectOverCDP(
  "ws://127.0.0.1:PORT/devtools/browser/XXXX", // Copy CDP URL
);
const page = browser.contexts()[0].pages()[0];
await page.goto("https://example.com");
await browser.close(); // chỉ ngắt kết nối, KHÔNG đóng browser
```

**Puppeteer** (`puppeteer.connect` với `browserWSEndpoint`):

```ts
import puppeteer from "puppeteer-core";

const browser = await puppeteer.connect({
  browserWSEndpoint: "ws://127.0.0.1:PORT/devtools/browser/XXXX", // Copy CDP URL
});
const [page] = await browser.pages();
await page.goto("https://example.com");
await browser.disconnect();
```

Lưu ý: endpoint chỉ bind `127.0.0.1` (localhost) và **đổi port mỗi lần launch**
(port cấp động — `ProcessManager::allocate_cdp_port`, `src-tauri/src/process.rs`),
nên copy lại URL sau mỗi lần khởi chạy profile.
