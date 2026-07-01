# 00 — Tổng quan, Tầm nhìn & Phạm vi (Overview & Scope)

> Tài liệu mở đầu bộ docs BrowserX. Mọi khẳng định kỹ thuật đối chiếu **code
> thật** trong `refs/` theo dạng `path#Lstart-Lend`. Trích dẫn nguyên văn khi
> dẫn README/license. Không bịa.

## 0. Tóm tắt 30 giây

**BrowserX** là ứng dụng quản lý antidetect browser kiểu Multilogin/GoLogin/AdsPower,
**open-source, self-host, KHÔNG bán SaaS**, kế thừa nhanh từ hai repo tham chiếu:

- `refs/CloakBrowser` — **engine**: wrapper mỏng (Python/JS/.NET) quanh một binary
  Chromium đóng đã patch fingerprint ở mức C++ source.
- `refs/CloakBrowser-Manager` — **manager**: FastAPI + React + SQLite + KasmVNC,
  CRUD profile + launch + xem trực tiếp qua noVNC + tự động hoá qua CDP.

Kiến trúc BrowserX (chốt 2026-07-01 — xem Spec §"🔁 ĐỔI HƯỚNG"): **app desktop
LOCAL** viết bằng **Rust + SQLite + Tauri**, **tái dùng React frontend** của
`refs/CloakBrowser-Manager/frontend` (thay backend FastAPI Python). Mở **cửa sổ
browser thật (headful) trên máy user, KHÔNG dùng VNC**; automation qua CDP.
Cross-platform **Mac/Linux/Windows**. Khoá cứng CloakBrowser làm engine duy nhất,
spawn binary trực tiếp từ Rust (không nhúng Python).

## 1. Quyết định đã CHỐT (2026-07-01)

Các quyết định nền tảng (chi tiết ở Spec và docs 02/04):

- **Q1 — Open source, KHÔNG bán SaaS** (self-host cá nhân/cộng đồng, phi thương mại).
- **Q2 — KHOÁ CỨNG CloakBrowser**: không làm lớp adapter đa-engine (đánh đổi có ý thức,
  chấp nhận rủi ro bus-factor).
- **Q3 (cập nhật 2026-07-01) — App LOCAL desktop bằng Rust**: thay kiến trúc
  server/K8s/VNC bằng Rust + SQLite + Tauri, headful không VNC, cross-platform
  Mac/Linux/Windows. Bản local nhắm **lưu hàng nghìn profile / chạy đồng thời vài
  chục phiên**; nghìn phiên đồng thời để dành cho phương án server tương lai.
- **Q4 — Docs Tiếng Việt.**

## 2. Tầm nhìn sản phẩm

CloakBrowser-Manager tự mô tả là *"Free, self-hosted alternative to Multilogin,
GoLogin, and AdsPower"* (`refs/CloakBrowser-Manager/README.md#L9`). BrowserX kế thừa
tầm nhìn đó nhưng đẩy xa hơn về **quy mô** và **parity tính năng**:

- Mỗi profile = một danh tính thiết bị độc lập (fingerprint + proxy + cookies +
  session riêng), *"Everything runs in one Docker container"*
  (`refs/CloakBrowser-Manager/README.md#L26`).
- Giá trị stealth nằm ở binary Chromium patch C++ — *"58 source-level C++ patches"*
  (`refs/CloakBrowser/README.md#L39`), phiên bản mới nhất công bố 59 patch
  (`refs/CloakBrowser/README.md#L153`).
- BrowserX bổ sung phần "quản lý đội/quy mô" mà repo tham chiếu còn thiếu (xem §5).

## 3. Kiến trúc kế thừa (tóm tắt, có dẫn chứng)

**Engine (CloakBrowser).** Fingerprint điều khiển qua CLI flags: seed ngẫu nhiên
mỗi lần chạy `--fingerprint=<seed>` và `--fingerprint-platform`
(`refs/CloakBrowser/cloakbrowser/config.py#L54-76`); các flag mở rộng GPU/screen/
hardware/timezone/locale (`refs/CloakBrowser/README.md#L707-731`). Binary tải tự
động từ `cloakbrowser.dev`/GitHub Releases (`config.py#L253-283`) và **verify chữ ký
Ed25519** trước khi tin (`config.py#L28-39`).

**Manager (CloakBrowser-Manager).** Stack: FastAPI (Python) + React/Tailwind +
noVNC + SQLite + engine CloakBrowser (`refs/CloakBrowser-Manager/README.md#L68-74`).
Mỗi lần launch: cấp một display Xvnc (base `:100`) + cổng WS (base `6100`)
(`backend/vnc_manager.py#L22-37`) và một cổng CDP trong dải `5100–5199`
(`backend/browser_manager.py#L145-146`), rồi gọi `launch_persistent_context_async`
(`backend/browser_manager.py#L217-234`). Xem trực tiếp qua WebSocket VNC proxy và tự
động hoá qua CDP proxy (`backend/main.py#L677-1016`).

**Xác thực (hiện trạng repo).** Chỉ một `AUTH_TOKEN` dùng chung, mặc định mở toang
khi chạy local (`backend/main.py#L48-54`); token truyền cleartext qua HTTP
(`refs/CloakBrowser-Manager/README.md#L178`).

**Kiến trúc BrowserX (bản local).** Thay backend FastAPI + KasmVNC/Xvnc bằng lõi
**Rust** làm control-plane + **SQLite** lưu profile/proxy/settings + **Tauri** làm vỏ
desktop, **tái dùng React frontend** của Manager. Không có tầng VNC/RFB proxy: Rust
spawn binary CloakBrowser **headful** trực tiếp trên máy user (dựng CLI flags, không
nhúng Python), port từ `build_args` (`refs/CloakBrowser/cloakbrowser/browser.py#L1028`)
và `refs/CloakBrowser/cloakbrowser/config.py`. Đường dẫn binary theo OS
(`config.py#L169-181`: macOS `Chromium.app/.../Chromium`, Windows `chrome.exe`, Linux
`chrome`) và danh sách nền tảng có sẵn binary (`config.py#L20-27`, `#L91-98`) cho phép
chạy cross-platform Mac/Linux/Windows. Bỏ VNC giải quyết phần lớn rủi ro bảo mật RFB
proxy và phụ thuộc Linux-only (xem docs 02/03).

## 4. So sánh: Multilogin/GoLogin/AdsPower vs. BrowserX

| Tiêu chí | Multilogin/GoLogin/AdsPower (SaaS thương mại) | Repo tham chiếu (as-is) | **BrowserX (định hướng)** |
|---|---|---|---|
| Mô hình | SaaS trả phí, cloud | Open-source, self-host | **Open-source, app desktop LOCAL** |
| Engine trình duyệt | Chrome (Mimic) **+ Firefox** (Stealthfox/Mimic) | Chỉ Chromium (CloakBrowser) `refs/CloakBrowser/README.md#L265` | **Chỉ Chromium (khoá cứng)** |
| Fingerprint | Canvas/WebGL/audio/GPU/fonts/screen/WebRTC | Có, qua flags C++ `README.md#L39,#L707-731` | Kế thừa nguyên | 
| Proxy per-profile | Có, kèm marketplace proxy | Có (HTTP/SOCKS5 + auth) `backend/browser_manager.py#L22-53` | **Kế thừa + health-check** |
| Multi-user / team / RBAC | Có | **Không** (1 token) `backend/main.py#L48-54` | **Phải tự build** (docs 07) |
| Cookie import/export & sync mã hoá | Có | **Không** | **Phải tự build** (docs 07) |
| Bulk / template / tags | Có | Chỉ tags cơ bản `backend/models.py#L60-67` | **Mở rộng** (docs 07) |
| Automation API | CDP/Selenium/Local API | CDP/Playwright `README.md#L119-144` | Kế thừa CDP/Playwright |
| Mobile fingerprint (Android/iOS) | Có | **Không** | Ngoài phạm vi giai đoạn này |
| Quy mô | Hàng nghìn+ (cloud) | 1 container, ~100 cổng CDP `browser_manager.py#L146` | **Local: lưu nghìn profile / chạy vài chục phiên** (docs 03; nghìn phiên đồng thời → server tương lai) |
| Chi phí | Trả phí theo profile | Miễn phí | Miễn phí (tự chịu hạ tầng) |

> Ghi chú: cột giữa mô tả **đúng hiện trạng** repo tham chiếu; cột phải là **định
> hướng BrowserX**, không phải tính năng đã có.
>
> BrowserX theo mô hình **desktop client cài trên máy user — giống AdsPower/GoLogin
> client** (mỗi máy tự chạy & quản lý profile local), không phải mô hình cloud/SaaS
> tập trung.

## 5. Phạm vi MVP vs. Non-goals

### 5.1. Trong phạm vi MVP (app local Rust)

- Quản lý profile (CRUD) trên SQLite: fingerprint seed, proxy, timezone/locale, screen,
  platform, user-agent (mô hình dữ liệu tham chiếu `backend/models.py#L10-33`).
- Fingerprint **theo profile** (canvas/webgl/audio/gpu/screen/hardware/webrtc) qua CLI
  flags của CloakBrowser (`refs/CloakBrowser/cloakbrowser/browser.py#L1028`).
- Proxy per-profile (HTTP/SOCKS + auth) và timezone/geo/locale auto-khớp proxy.
- **Launch headful** một profile trên máy user (spawn binary trực tiếp từ Rust),
  điều khiển qua CDP — **KHÔNG VNC**.
- Chạy cross-platform Mac/Linux/Windows qua vỏ Tauri.

### 5.2. Định hướng sau MVP (parity Multilogin — docs 07)

Theo Spec: fingerprint theo profile đầy đủ, proxy per-profile + health-check,
timezone/geo/locale auto-khớp proxy, cookie/localStorage import-export & sync (mã
hoá), quản lý profile (tags/search/template/bulk), **team + RBAC**, automation API,
audit log; và kiến trúc scale hàng nghìn profiles (docs 03) + roadmap theo pha (docs 05).

### 5.3. Non-goals

- **Bản local KHÔNG nhắm nghìn phiên chạy đồng thời / multi-node / orchestration**
  (giới hạn RAM/CPU máy user ~0.3–1GB/phiên) — để dành **phương án server tương lai**.
- **Không** dùng VNC/KasmVNC/Xvnc và **không** nhúng Python runtime.
- **Không** làm lớp adapter đa-engine (đã khoá cứng CloakBrowser).
- **Không** mobile fingerprint (Android/iOS) và **không** engine Firefox.
- **Không** sửa `refs/` (chỉ đọc tham chiếu) và **không** tư vấn pháp lý chính thức
  (chỉ disclaimer, xem docs 04).

## 6. Giới hạn nền tảng cần biết sớm

Bản local Rust bỏ VNC nên **không còn phụ thuộc Linux/Docker-only** của Manager tham
chiếu (KasmVNC/Xvnc/xclip — `refs/CloakBrowser-Manager/Dockerfile#L22,#L35-40`); app
chạy trực tiếp Mac/Linux/Windows. Về **fingerprint OS**: `--fingerprint-platform` chỉ có
**mặc định theo host** (macOS→`macos`, Linux/Win→`windows`), **không bị khoá** —
`refs/CloakBrowser/cloakbrowser/config.py#L54-76` không validate giá trị theo host. Vì
BrowserX spawn binary trực tiếp từ Rust, user được **chọn target OS fingerprint tự do**;
khi target OS ≠ host OS thì **hiện cảnh báo mismatch** (fonts/GPU/WebGL renderer thật lộ
ra, chất lượng ngụy trang giảm) — mô hình cảnh báo giống Multilogin, không chặn cứng.
Muốn profile Windows chất lượng cao hàng loạt thì **nên** chạy host Linux/Windows
(khuyến nghị, không bắt buộc). Chi tiết eval & giới hạn: xem `06-local-eval-setup.md`.

## 7. Bản đồ bộ docs

- `00-overview-scope.md` — tài liệu này.
- `01-cloakbrowser-research.md` — deep-dive kiến trúc CloakBrowser + Manager.
- `02-critique-risks.md` — phản biện & risk register.
- `03-*` — kiến trúc mục tiêu (bản local Rust + SQLite + Tauri, headful không VNC).
- `04-licensing-legal-decision.md` — license & pháp lý.
- `05-*` — roadmap theo pha.
- `06-local-eval-setup.md` — hướng dẫn eval cục bộ bằng Docker.
- `07-*` — đặc tả tính năng parity Multilogin.
