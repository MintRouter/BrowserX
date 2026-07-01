# 02 — Phản biện gay gắt & Phân tích rủi ro

> Tài liệu này phản biện thẳng thắn kiến trúc kế thừa từ hai repo tham chiếu
> `refs/CloakBrowser` (engine) và `refs/CloakBrowser-Manager` (manager), phục vụ
> quyết định triển khai BrowserX. Mọi khẳng định kỹ thuật đều đối chiếu **code
> thật** trong `refs/` theo dạng `path#Lstart-Lend`. Trích dẫn license là
> **nguyên văn**. Không bịa.

## 0. Bối cảnh & cách đọc

Các quyết định đã CHỐT (2026-07-01) — mọi phần "Giảm thiểu" bám theo đây:

- **Open source, KHÔNG bán SaaS** (self-host, cá nhân/cộng đồng, phi thương mại).
- **KHOÁ CỨNG CloakBrowser** — không làm lớp adapter đa-engine (đánh đổi có ý thức).
- **Thiết kế để chạy HÀNG NGHÌN profiles** (scale ngang).
- **Docs Tiếng Việt.**

Vì đã khoá cứng CloakBrowser và chọn open-source, một số rủi ro (SaaS-license,
bus-factor) được **chấp nhận có ý thức** thay vì loại bỏ; tài liệu vẫn ghi rõ để
người triển khai không quên.

Thang mức độ: **Critical** (chặn/đe doạ sống còn) · **High** (tác động lớn, cần
xử lý sớm) · **Medium** (cần theo dõi/giảm thiểu).

---

## 1. Risk Register (tổng hợp)

| # | Rủi ro | Mức độ | Likelihood | Impact | Trạng thái quyết định |
|---|--------|--------|-----------|--------|----------------------|
| R1 | License binary cấm SaaS / redistribute | Critical | Cao (nếu bán) | Nghiêm trọng (pháp lý) | Né bằng open-source self-host |
| R2 | Lệ thuộc 1 vendor binary đóng (bus-factor) | High | Trung bình | Nghiêm trọng (sản phẩm chết) | Chấp nhận có ý thức (locked) |
| R3 | Kiến trúc tham chiếu không scale | High | Cao (ở quy mô nghìn) | Cao (không đạt mục tiêu) | Phải thiết kế lại (docs 03) |
| R4 | Thiếu toàn bộ "phần Multilogin" | High | Chắc chắn | Cao (không parity) | Phải tự build (docs 07) |
| R5 | Fingerprint là hộp đen, không tự audit | High | Trung bình | Cao (bị phát hiện) | Chấp nhận + giám sát ngoài |
| R6 | Lỗ hổng bảo mật trong Manager | High | Cao (nếu expose) | Nghiêm trọng (RCE/leak) | Phải vá trước khi mở mạng |
| R7 | Pháp lý / ToS / thanh toán | Medium | Trung bình | Cao (tài khoản/pháp lý) | Disclaimer + acceptable-use |
| R8 | Giới hạn dev trên macOS | Medium | Chắc chắn (máy dev) | Trung bình (chỉ eval) | Dev qua Docker/Linux |

## 2. Ma trận Likelihood × Impact

```
Impact →      Thấp        Trung bình      Cao / Nghiêm trọng
Likelihood
  Cao        │            │ R8            │ R3, R4, R6
  Trung bình │            │               │ R2, R5, R7
  Thấp/ĐK*   │            │               │ R1 (*chỉ khi bán SaaS)
```

`*ĐK` = có điều kiện: R1 chỉ kích hoạt nếu chuyển sang mô hình browser-as-a-service.
Với quyết định open-source self-host hiện tại, R1 bị **vô hiệu hoá về mặt thực thi**
nhưng vẫn phải nêu disclaimer (xem R1 & docs 04).

---

## 3. Chi tiết từng rủi ro

### R1 — License binary cấm SaaS / redistribute — **Critical (có điều kiện)**

**Bằng chứng (nguyên văn license).** Trích từ `refs/CloakBrowser-Manager/BINARY-LICENSE.md`
(**v1.0 — Feb 2026**; mọi số dòng bên dưới theo bản này). Lưu ý: bản trong
`refs/CloakBrowser/BINARY-LICENSE.md` là **v1.1 — June 2026** — cùng các điều khoản
cấm nhưng **KHÔNG trùng số dòng** (v1.1 thêm mục "Version-Specific Terms" → mọi dòng
sau L18 lệch +6). Chi tiết khác biệt: xem **docs 04 §4b**.

- Cấm redistribute — `BINARY-LICENSE.md#L21-L27`:
  > "You may NOT: 1. **Redistribute** the Binary, in whole or in part, whether
  > modified or unmodified 2. **Resell, sublicense, or repackage** the Binary, or
  > include it in any product or service distributed to third parties ..."
- Điều khoản OEM/SaaS — `BINARY-LICENSE.md#L39`:
  > "**OEM/SaaS license required** — Bundling, embedding, or pre-installing the
  > Binary into a product, hosted service, or cloud artifact distributed to third
  > parties requires a separate OEM license. This includes running the Binary on
  > your infrastructure to serve third-party customers (e.g., browser-as-a-service).
  > Contact cloakhq@pm.me for OEM/SaaS licensing."

**Sắc thái quan trọng (đối chiếu chính xác).** License KHÔNG cấm dùng nội bộ/thương
mại nội bộ, và cho phép list làm dependency:

- Dùng nội bộ được phép — `BINARY-LICENSE.md#L33`:
  > "**Internal use** — You may store and run the unmodified Binary within internal
  > infrastructure, including Docker images, VM templates, CI runners, container
  > registries, and artifact repositories ... solely for your organization's
  > internal operational purposes."
- Liệt kê dependency KHÔNG phải redistribute — `BINARY-LICENSE.md#L35`:
  > "**Dependency listing** — Listing CloakBrowser as a dependency ... is not
  > redistribution, as end users download the Binary directly from official CloakHQ
  > channels."
- Dùng cho business của chính mình là miễn phí — `BINARY-LICENSE.md#L37`:
  > "**Using CloakBrowser for your own business is free** — no license beyond this
  > one is needed, regardless of company size or revenue."

**Tác động.** Multilogin/GoLogin/AdsPower CHÍNH LÀ browser-as-a-service → nếu
BrowserX phục vụ khách hàng bên thứ ba mà không có OEM deal thì **vi phạm license**
(termination tự động — `BINARY-LICENSE.md#L84-L86`).

**Giảm thiểu (khớp quyết định).**
1. Giữ mô hình **open-source self-host**: người dùng tự tải binary từ kênh CloakHQ
   lúc runtime (đúng như wrapper đang làm) — repo BrowserX **chỉ ship code + script
   tải**, KHÔNG commit binary (né L21-L27, L41-L43).
2. Nêu **disclaimer** rõ trong docs 04 + README: open-source KHÔNG cấp quyền chạy
   SaaS; ai muốn thương mại hoá phải tự lo OEM license với CloakHQ.
3. Không bao giờ đóng gói binary vào Docker image công khai để phân phối.

### R2 — Lệ thuộc 1 vendor binary đóng (bus-factor) — **High (chấp nhận)**

**Bằng chứng.**
- Toàn bộ giá trị stealth nằm trong binary đóng; wrapper chỉ truyền CLI flags:
  `refs/CloakBrowser/cloakbrowser/config.py#L51-L76` (`get_default_stealth_args`
  chỉ ghép `--fingerprint=<seed>`, `--fingerprint-platform=...`).
- Cấm reverse-engineer/modify → không thể tự vá: `BINARY-LICENSE.md#L21-L27`
  (mục 3 "Reverse engineer..." và mục 4 "Modify").
- Vendor không có nghĩa vụ cập nhật/hỗ trợ: `BINARY-LICENSE.md#L80-L82`
  > "CloakHQ is under no obligation to provide updates, patches, new versions, or
  > support for the Binary."
- Phân phối chỉ qua kênh CloakHQ (single-vendor): `BINARY-LICENSE.md#L41-L43`.
- Free tier chậm hơn Pro về Chromium: free pin `CHROMIUM_VERSION = "146..."`
  (`refs/CloakBrowser/cloakbrowser/config.py#L18`), trong khi Pro là Chromium
  148 và "v146 stays free forever" (`refs/CloakBrowser/README.md#L153-L155`,
  `#L191`). Pro cần license key trả phí (`refs/CloakBrowser/cloakbrowser/license.py#L23-L37`,
  download Pro qua key: `refs/CloakBrowser/cloakbrowser/download.py#L372-L389`).

**Tác động.** CloakHQ đổi ý / gỡ release / ngừng phát triển ⇒ không tự nâng
fingerprint, kẹt ở Chromium cũ (dễ bị phát hiện hơn theo thời gian). Repo ở giai
đoạn sớm (`_version.py` = `0.4.5`).

**Giảm thiểu (khớp quyết định "khoá cứng").**
1. Chấp nhận rủi ro có ý thức (Q2) để đi nhanh — ghi vào decision log.
2. **Pin & cache binary version** đã verify, mirror nội bộ hợp lệ theo mục
   Internal use (`BINARY-LICENSE.md#L33`) để không phụ thuộc uptime kênh tải.
3. Theo dõi phát hành CloakHQ; giữ script tải + checksum để tái lập môi trường.
4. Cô lập điểm phụ thuộc sau một interface mỏng trong code BrowserX (không phải
   adapter đa-engine, chỉ là ranh giới để dễ thay thế nếu buộc phải) — tuỳ docs 03.

### R3 — Kiến trúc tham chiếu không scale tới hàng nghìn profiles — **High**

**Bằng chứng.**
- **Store là SQLite + state in-memory:** `refs/CloakBrowser-Manager/backend/database.py#L1`
  (`"""SQLite database operations..."""`), kết nối trực tiếp file
  (`database.py#L19-L21`). Không có Postgres/multi-node.
- **Cấp phát display bằng bộ đếm tăng dần in-memory:**
  `refs/CloakBrowser-Manager/backend/vnc_manager.py#L22-L37` — `BASE_DISPLAY = 100`,
  vòng `while display in self._allocated: display += 1`, lưu trong dict
  `self._allocated` (mất khi restart process).
- **Cổng CDP giới hạn ~100 concurrent:**
  `refs/CloakBrowser-Manager/backend/browser_manager.py#L145-L146` — `BASE_CDP_PORT
  = 5100`, `CDP_PORT_RANGE = 100  # cycle through 5100-5199`; hết cổng thì raise
  (`browser_manager.py#L364-L378`).
- **Mỗi profile = 1 Xvnc + 1 Chromium** (nặng RAM/CPU):
  `browser_manager.py#L176-L233` (allocate display → start Xvnc → `launch_persistent_context_async`),
  buộc `--use-angle=swiftshader` (software GL, không GPU trong container):
  `browser_manager.py#L383-L385`.
- **Dọn rác crash bằng `pkill`:** `vnc_manager.py#L120-L129`
  (`["pkill", "-f", r"Xvnc :[0-9]"]`) — không có orchestration/queue/health-checker.
- **Chạy 1 container, bind localhost:** `refs/CloakBrowser-Manager/docker-compose.yml#L4-L9`
  (`"127.0.0.1:8080:8080"`, 1 service `manager`).

**Tác động.** Trần thực tế ~100 phiên đồng thời/1 node, không multi-node, state
mất khi restart, cleanup mong manh → **không đạt mục tiêu Q3 (hàng nghìn profiles)**.

**Giảm thiểu (sau pivot LOCAL).** Bản BrowserX đã đổi hướng sang **app local Rust**
(docs 03): lưu **nghìn profile** trong SQLite nhưng chỉ **chạy đồng thời vài chục
phiên** + hàng đợi (semaphore). **Nghìn phiên đồng thời** cần **kiến trúc server
tương lai** (Spec §Q3; docs 05 Pha S): tách control-plane/data-plane, DB Postgres,
hàng đợi launch, worker đa-node, container/pod-per-profile, VNC gateway scale. Phân
biệt số profile *lưu trữ* (nghìn) vs. số *chạy đồng thời* (giới hạn RAM/CPU, mỗi phiên
live ≈ 0.5–1GB).

### R4 — Thiếu toàn bộ "phần Multilogin" — **High**

**Bằng chứng.**
- **Chỉ 1 token dùng chung, không user/team/RBAC:**
  `refs/CloakBrowser-Manager/backend/main.py#L51`
  (`AUTH_TOKEN: str | None = os.environ.get("AUTH_TOKEN") or None`) — không có
  bảng users/teams. Schema DB chỉ có `profiles` và `profile_tags`:
  `refs/CloakBrowser-Manager/backend/database.py#L34` và `#L61`.
- **Không có engine Firefox / mobile fingerprint:** grep `firefox|gecko|android|ios|mobile`
  trong `refs/CloakBrowser/cloakbrowser/` **không có kết quả** — engine chỉ Chromium
  desktop; flags fingerprint chỉ platform desktop
  (`browser_manager.py#L379-L414` — platform/gpu/hardware/screen).
- **Không có audit log / quota / billing** trong schema (`database.py#L34-L82`).

**Tác động.** So với Multilogin (Mimic-Chrome + Stealthfox-Firefox, team/RBAC,
sync cookie mã hoá, template/bulk, audit) thì đây mới là "profile launcher" 1 người
dùng. Cần build phần lớn tính năng parity.

**Giảm thiểu.** Đặc tả đầy đủ ở **docs 07** (parity Multilogin): RBAC/team,
import-export & sync cookie mã hoá, template/bulk, tags/search, audit log,
automation API. Chấp nhận **chỉ Chromium** (hệ quả của khoá cứng — không Firefox/mobile).

### R5 — Fingerprint là hộp đen, không tự audit — **High**

**Bằng chứng.** Patch nằm trong binary đóng, cấm reverse-engineer
(`BINARY-LICENSE.md#L21-L27` mục 3). Con số "59 source-level patches" và các kết
quả test ("reCAPTCHA v3 0.9", "Cloudflare Turnstile 3 live tests") đều do **vendor
tự công bố** (`refs/CloakBrowser/README.md#L153-L155`; bảng Test Results
`#L207-L213`; `#L28-L29`). Wrapper chỉ bật patch qua flags, không thấy được nội dung
(`config.py#L52-L53` ghi rõ flags "activate source-level fingerprint patches
compiled into the binary").

**Tác động.** Ship một hộp đen: không audit được chất lượng chống CreepJS/
FingerprintJS, không tự vá khi bị phát hiện. Free tier còn chậm 1 major Chromium
(R2) → rủi ro phát hiện tăng theo thời gian.

**Giảm thiểu.** (1) Xây **harness kiểm chứng ngoài** (CreepJS, browserleaks,
Cloudflare/reCAPTCHA test) chạy định kỳ trên profile mẫu để đo drift độc lập với
tuyên bố vendor. (2) Ghi nhận version Chromium đang chạy (`config.py#L18-L25`,
`get_chromium_version()`), cảnh báo khi tụt hậu so với stock Chrome. (3) Coi kết
quả vendor là tham khảo, không phải bảo chứng.

### R6 — Lỗ hổng bảo mật trong Manager — **High**

**Bằng chứng.**
- **1 shared token, có token là điều khiển mọi profile qua CDP:**
  `main.py#L51`; endpoint launch trả `cdp_url=f"/api/profiles/{profile_id}/cdp"`
  (`main.py#L546`) → ai qua được auth là có toàn quyền CDP (đọc/ghi trang, JS).
- **Sandbox tắt:** `refs/CloakBrowser/cloakbrowser/config.py#L64-L65` base args gồm
  `"--no-sandbox"`; Manager cũng thêm `--test-type` để nuốt cảnh báo
  `--no-sandbox` (`browser_manager.py#L383`). Web không tin cậy chạy `--no-sandbox`
  chung 1 container ⇒ nguy cơ container-escape cao hơn.
- **Proxy credential lưu plaintext trong SQLite:** cột `proxy TEXT`
  (`database.py#L38`) lưu nguyên `http://user:pass@host:port`
  (`refs/CloakBrowser-Manager/backend/models.py#L13`, `#L38`); create ghi thẳng
  giá trị (`database.py#L101-L109`). Không mã hoá.
- **VNC proxy là hack parse byte RFB thủ công:** `main.py#L227-L300`
  (`_RFB_MSG_SIZE`, `_rfb_msg_length`, `_rewrite_set_encodings`) — tự bóc tách
  message boundary để tương thích noVNC 1.4 ↔ KasmVNC 1.3.3; comment thừa nhận
  "KasmVNC 1.3.3 crashes on unsupported types" và "we had wrong numbers"
  (`main.py#L231-L232`, `#L254`) ⇒ dễ vỡ khi nâng version.

**Sắc thái (công bằng với code).** Manager KHÔNG hoàn toàn cẩu thả: dùng
`hmac.compare_digest` chống timing attack (`main.py#L65`, `#L76`), cookie
`httponly` + `secure` khi HTTPS (`main.py#L414-L420`), có chống CSWSH bằng
kiểm origin WebSocket (`main.py#L89-L135`), và docker-compose bind `127.0.0.1`
(`docker-compose.yml#L5`). Rủi ro "cleartext" chủ yếu xảy ra khi **tự expose qua
HTTP không TLS** ra mạng.

**Tác động.** Nếu mở ra mạng mà thiếu TLS/kiểm soát: lộ token ⇒ RCE-cấp-CDP trên
mọi profile, lộ proxy credential, và bề mặt container-escape.

**Giảm thiểu (bắt buộc trước khi mở mạng).** (1) TLS bắt buộc + reverse proxy;
(2) thay shared token bằng **auth per-user** (gắn với R4/RBAC); (3) **mã hoá
proxy credential at-rest**; (4) cô lập mỗi profile (container/pod-per-profile,
network policy) — cũng phục vụ scale (R3); (5) thay VNC RFB hack thủ công bằng
gateway ổn định hơn khi thiết kế lại (docs 03).

### R7 — Pháp lý / ToS / thanh toán — **Medium**

**Bằng chứng.** License tự cấm dùng cho gian lận/tạo tài khoản tự động:
`BINARY-LICENSE.md#L53-L62`:
> "the following uses are expressly prohibited: - Unauthorized access to
> financial, banking, healthcare, or government authentication systems -
> Credential stuffing, brute-force login attempts, or automated account creation ..."

Kèm điều khoản indemnification (`#L64-L66`) và giới hạn trách nhiệm tối đa **US $100**
(`#L72-L74`).

**Tác động.** Ngành antidetect/multi-account thường vi phạm ToS nền tảng; payment
processor hay từ chối. Với open-source phi thương mại, rủi ro thanh toán giảm,
nhưng rủi ro lạm dụng của người dùng cuối vẫn còn.

**Giảm thiểu.** (1) **Acceptable-use policy** + disclaimer trong README/docs 04,
lặp lại nguyên văn ranh giới cấm của license; (2) không quảng bá use-case gian
lận; (3) không nhận vai trò tư vấn pháp lý (Non-goal). Đây KHÔNG phải tư vấn
pháp lý chính thức.

### R8 — Giới hạn dev trên macOS — **Medium**

**Bằng chứng.**
- Manager phụ thuộc Linux (Xvnc/KasmVNC, `pkill`, xclip):
  `vnc_manager.py#L46-L57` (spawn `Xvnc`), `#L120-L129` (`pkill`),
  `main.py#L586-L602` (xclip theo display) → Docker/Linux-only.
- CloakBrowser trên macOS **mặc định** chạy như Mac thật
  (`--fingerprint-platform=macos`), nhưng đây chỉ là **default của wrapper, KHÔNG bị
  khoá**: `refs/CloakBrowser/cloakbrowser/config.py#L57-L76` — comment "On macOS, skips
  platform/GPU spoofing — runs as a native Mac browser. Spoofing Windows on Mac creates
  detectable mismatches (fonts, GPU, etc.)". Tức ép `--fingerprint-platform=windows`
  trên Mac **vẫn chạy được**, chỉ **giảm chất lượng** do mismatch — KHÔNG phải bất khả
  thi (chi tiết docs 03 §6). macOS còn pin Chromium thấp hơn
  (`darwin-*: "145..."` vs linux/windows `146...`, `config.py#L20-L25`).

**Tác động.** Máy dev hiện tại là macOS ⇒ chỉ eval/chạy đầy đủ qua **Docker/Linux**.
Về cross-OS fingerprint: ép Windows trên macOS host **làm được nhưng chất lượng ngụy
trang giảm** (mismatch fonts/GPU/WebGL renderer thật của Mac) → đây là **cảnh báo có
kiểm soát**, không phải rào chặn cứng.

**Giảm thiểu.** Chuẩn hoá môi trường eval bằng Docker (khớp docs 00/01); coi macOS chỉ
để phát triển code manager, còn chạy engine + VNC luôn qua container Linux. Với cross-OS
fingerprint: BrowserX **cho chọn target OS tự do + UI cảnh báo mismatch rõ ràng** và
**khuyến nghị host phù hợp** (profile Windows chất lượng cao → host Linux/Windows), thay
vì chặn cứng (docs 03 §6).

---

## 4. Trích nguyên văn các điều khoản chặn (BINARY-LICENSE.md)

Để tra cứu nhanh, dưới đây là các điều khoản quyết định, trích nguyên văn từ
`refs/CloakBrowser-Manager/BINARY-LICENSE.md` (**v1.0**; số dòng theo bản này). Bản
`refs/CloakBrowser/BINARY-LICENSE.md` (**v1.1**) có cùng các điều khoản cấm nhưng số
dòng lệch — xem **docs 04 §4b**:

- **Restrictions** — `#L21-L29`:
  > "You may NOT: 1. **Redistribute** the Binary, in whole or in part, whether
  > modified or unmodified 2. **Resell, sublicense, or repackage** the Binary, or
  > include it in any product or service distributed to third parties 3. **Reverse
  > engineer, decompile, or disassemble** the Binary ... 4. **Modify** the Binary
  > or create derivative works based on it 5. **Remove or alter** any copyright
  > notices ... Normal use of the Binary with command-line flags, browser
  > extensions, managed policies, custom profiles, or user data directories does
  > not constitute modification or creation of derivative works."
- **Grant of Use** — `#L17`:
  > "You are granted a non-exclusive, non-transferable, royalty-free license to use
  > the Binary for personal or commercial purposes. No fees are required."
- **OEM/SaaS license required** — `#L39` (đã trích ở R1).
- **Official Distribution** — `#L41-L43`:
  > "The Binary must originally be obtained from official CloakHQ distribution
  > channels, including GitHub Releases (github.com/CloakHQ/CloakBrowser) and
  > cloakbrowser.dev. Internal organizational mirrors permitted under the Cloud,
  > Container & Integration Use section are not considered unauthorized sources."
- **Acceptable Use (cấm)** — `#L53-L62` (đã trích ở R7).
- **Limitation of Liability** — `#L72-L74`:
  > "CLOAKHQ'S TOTAL AGGREGATE LIABILITY SHALL NOT EXCEED ONE HUNDRED US DOLLARS
  > (US $100)."
- **Termination** — `#L84-L86`:
  > "This license terminates automatically if you violate any of its terms. Upon
  > termination, you must destroy all copies of the Binary in your possession. ..."

> Phân tích license đầy đủ + quyết định pháp lý: xem **docs 04 (Licensing & legal
> decision)**. Tài liệu này chỉ trích để chứng minh các rủi ro ở trên.

---

## 5. Khuyến nghị tổng hợp

1. **R1/R7 — Kỷ luật phân phối:** không bao giờ commit/redistribute binary; tải
   runtime từ kênh CloakHQ; kèm disclaimer + acceptable-use rõ ràng (docs 04).
2. **R3 — Bản LOCAL (docs 03) giảm mạnh R3:** vài chục phiên đồng thời + hàng đợi.
   Muốn **nghìn phiên đồng thời** mới cần scale server (Postgres, queue, worker
   đa-node, pod-per-profile, VNC gateway) — **phương án tương lai** (docs 05 Pha S).
3. **R6 — Vá bảo mật trước khi mở mạng:** TLS, auth per-user, mã hoá proxy
   credential, cô lập profile. Không expose bản tham chiếu nguyên trạng.
4. **R4 — Build phần Multilogin** theo docs 07; chấp nhận chỉ-Chromium.
5. **R2/R5 — Quản trị rủi ro hộp đen:** pin+mirror binary hợp lệ, harness kiểm
   chứng fingerprint độc lập, theo dõi version Chromium.
6. **R8 — Chuẩn hoá eval qua Docker/Linux.**

**Kết luận thẳng thắn:** hai repo tham chiếu là điểm khởi đầu tốt cho một
*launcher 1-người-dùng*, nhưng còn cách **rất xa** một manager kiểu Multilogin
chạy hàng nghìn profiles. Phần lớn giá trị (scale, bảo mật, tính năng team) phải
**tự xây**; phần stealth thì **lệ thuộc hoàn toàn** vào một binary đóng đã được
chấp nhận có ý thức. Đi tiếp là hợp lý cho mục tiêu open-source self-host, với
điều kiện thực thi đầy đủ các giảm thiểu ở docs 03/04/07.
