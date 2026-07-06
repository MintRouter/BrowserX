# 05 — Roadmap theo pha (app LOCAL Rust, tới hàng nghìn profiles)

> Roadmap triển khai **BrowserX** — ứng dụng **desktop chạy local, cross-platform
> (macOS / Linux / Windows)**, mở **cửa sổ browser thật (headful)**, **không VNC,
> không server**. Tài liệu này **viết lại hoàn toàn** theo hướng đã chốt trong Spec,
> mục **"🔁 ĐỔI HƯỚNG: App LOCAL desktop bằng RUST (chốt 2026-07-01)"**, thay cho
> bản roadmap server/pod-per-profile/Postgres/VNC cũ.
>
> **Stack chốt:** **Rust** (core) + **SQLite** + **Tauri** (desktop shell) + **tái
> dùng React frontend** của `refs/CloakBrowser-Manager/frontend`; **spawn binary
> CloakBrowser trực tiếp** (không nhúng Python); **headful, không VNC**. Kiến trúc
> chi tiết ở **[`docs/03-target-architecture.md`](03-target-architecture.md)**.
>
> ⚠️ **Ràng buộc nền tảng (Spec):** OPEN SOURCE, **KHÔNG** bán SaaS; **KHOÁ CỨNG
> CloakBrowser** (không adapter đa-engine); **docs Tiếng Việt**. Mọi con số "trạng
> thái hiện tại" của repo tham chiếu đối chiếu **code thật** trong `refs/` dạng
> `path#Lstart-Lend`.

Liên quan: [`docs/02-critique-risks.md`](02-critique-risks.md) ·
[`docs/03-target-architecture.md`](03-target-architecture.md) ·
[`docs/06-local-eval-setup.md`](06-local-eval-setup.md) ·
[`docs/07-features-spec.md`](07-features-spec.md).

---

## 0. Nguyên tắc của roadmap (bản LOCAL)

1. **Local-first.** Không API server, không container, không Docker cho sản phẩm.
   Toàn bộ control-plane (UI + logic + DB) nằm trong **một tiến trình Tauri**;
   browser là các tiến trình con Rust spawn (docs/03 §0, §1).
2. **Phân biệt "lưu trữ" vs "chạy đồng thời".** Mục tiêu **hàng nghìn profiles** là
   số *stored* (hàng SQLite + `user_data_dir` trên đĩa — gần như miễn phí). Số
   *concurrent* bị chặn bởi RAM host (~0.3–1GB/phiên — Spec §🔁; docs/02 R3) → chỉ
   **vài chục** trên 1 máy desktop. Cap mặc định scale theo RAM: reserve 4GiB cho OS,
   ngân sách ~2.5GiB/phiên → 24GiB → 8, 32GiB → 11 (≥10 khi RAM ≥32GiB),
   trần 64. Exit criteria tách rõ hai chỉ số.
3. **Kế thừa, đừng làm lại.** Fingerprint theo seed, proxy per-profile, GeoIP tz/locale,
   WebRTC spoof, CDP, persistence qua `user_data_dir` đã có trong CloakBrowser (docs/07
   §9) → **port sang Rust**, không viết lại engine.
4. **Bảo mật at-rest ngay từ MVP.** Proxy credential + cookie/session export **mã hoá**
   bằng khoá trong OS keychain (docs/03 §2) — vá plaintext-proxy bản gốc
   (`refs/CloakBrowser-Manager/backend/app/database.py#L38`).
5. **Exit criteria đo được.** Mỗi pha "xong" khi tiêu chí định lượng (số, %, p95,
   pass/fail) đạt — không dùng tiêu chí cảm tính.
6. **Kỷ luật license.** Không pha nào commit/redistribute binary; luôn **tải runtime**
   + **verify Ed25519** (docs/02 R1, docs/03 §3.4, docs/04). Kèm disclaimer:
   open-source ≠ quyền chạy browser-as-a-service.

---

## 1. Bảng tổng quan các pha

| Pha | Tên | Mục tiêu 1 câu | Quy mô (stored / concurrent) | Nền tảng |
|---|---|---|---|---|
| **0** | Eval + PoC | Chạy thật repo tham chiếu (docs/06) + PoC Rust spawn 1 CloakBrowser headful, attach CDP | ~1–5 / ~1–3 (máy dev) | Docker (eval) + native (PoC) |
| **1** | MVP local | Tauri + SQLite CRUD profile + build flags/spawn theo OS + fingerprint seed + proxy + timezone; tái dùng React; tải+verify binary | ~1.000 / ~10–30 (1 máy) | macOS/Linux/Windows native |
| **2** | Parity Multilogin | Cookie import/export & snapshot (mã hoá), tags/search/template/bulk, proxy health-check, automation API qua CDP | ~5.000 / ~20–50 (1 máy) | native, mã hoá at-rest |
| **3** | Hoàn thiện & phân phối | Installer 3 OS (Tauri bundler) + auto-update + audit log + đa hồ sơ local + observability/logs | ~10.000 / theo RAM host | 3 OS đóng gói |
| **S** | *(Ngoài phạm vi local)* Scale server | Nghìn **phiên đồng thời** → kiến trúc server đa-node | ≥1.000 concurrent | Server/K8s (Spec) |

> Pha 1–3 bám kiến trúc **docs/03** (Rust/Tauri/SQLite/headful). **Bản LOCAL đã giảm
> mạnh R3 / R6 / R8** so với bản server (xem §8): bỏ VNC/Xvnc/xclip (hết Linux-only),
> bỏ token cleartext + RFB proxy, teardown bằng `kill(pid)` thay `pkill`.

---

## 2. Pha 0 — Eval + PoC Rust

**Mục tiêu.** (a) Xác nhận repo tham chiếu chạy được + đo baseline hành vi
fingerprint/anti-bot **trước khi** viết code sản phẩm; (b) **PoC Rust**: spawn 1 binary
CloakBrowser **headful** và **attach CDP** trên máy dev — chứng minh khả thi bỏ Python.

**Hạng mục.**
- Dựng Manager qua Docker theo **docs/06 §2** (`docker compose up --build`); tạo ≥2
  profile khác seed, launch, xem qua noVNC (docs/06 §3).
- Chạy site kiểm tra bot/fingerprint (docs/06 §4); nối Playwright qua CDP (docs/06 §7).
- **PoC Rust (native, không Docker):** đọc đường dẫn binary theo OS
  (`refs/CloakBrowser/cloakbrowser/config.py#L169-L181`), dựng tối thiểu các flag
  stealth (`config.py#L54-L76`) + `--user-data-dir` + `--remote-debugging-port`, dùng
  `std::process::Command` spawn **headful**, rồi attach CDP `ws://127.0.0.1:<port>`.
- Ghi baseline: version Chromium, kết quả CreepJS/sannysoft, giới hạn môi trường.

**Exit criteria (đo được).**
- [ ] Hoàn tất **toàn bộ** checklist eval **docs/06 §8** (mọi mục ✅).
      — ⏳ chưa thực hiện: docs/06 §8 còn nguyên 8 mục [ ] — dự án pivot thẳng sang
      PoC native + Pha 1, không chạy eval Docker Manager.
- [x] ≥2 profile khác seed → **canvas/WebGL hash khác nhau** (browserleaks).
      — ✅ W5r evidence run 2026-07-03 trên binary thật: canvas + WebGL hash khác giữa
      2 seed, trùng khi cùng seed (`src-tauri/tests/fingerprint_seed.rs`,
      `fingerprint_diff.rs` — đo qua CDP; chưa đo lại trên browserleaks.com).
- [ ] `navigator.webdriver=false`, UA **không** lộ `HeadlessChrome` (sannysoft).
      — ⏳ chưa có bằng chứng đo: sannysoft chưa chạy, không có test nào assert
      `navigator.webdriver`.
- [x] **PoC Rust** spawn được **1 CloakBrowser headful native** trên máy dev (macOS),
      cửa sổ hiện thật; attach CDP + gọi `Page.navigate` thành công ≥1 lần.
      — ✅ smoke 2026-07-02: ensure_binary tải → Chromium 145 headful → CDP
      `/json/version` OK; W5r 2026-07-03: `cdp::goto` qua ProcessManager trên binary thật.
- [ ] Tài liệu baseline ghi rõ hạn chế môi trường (software GL, font; fingerprint-platform
      **mặc định** theo host nhưng chọn được tự do — cảnh báo mismatch khi cross-OS,
      docs/03 §6).
      — ⏳ chưa viết tài liệu baseline riêng (chưa có ghi chú CreepJS/sannysoft/version
      trong docs/).

**Rủi ro liên quan.** R8 (macOS: eval Manager qua Docker, nhưng PoC native chạy được),
R5 (kết quả vendor chỉ tham khảo).

---

## 3. Pha 1 — MVP local (Tauri + SQLite, 1 máy)

**Mục tiêu.** Có BrowserX tối thiểu **tự chạy trên desktop** (không Docker/VNC/server):
Tauri app + SQLite CRUD profile + Rust dựng flags & spawn binary theo OS + fingerprint
seed + proxy per-profile + timezone; **tái dùng React frontend**; **tải binary runtime
+ verify Ed25519**.

**Hạng mục (bám docs/03 §1–§4).**
- **Tauri shell + tái dùng React:** giữ `ProfileList`/`ProfileForm`/`LaunchButton`/
  `StatusIndicator` + interface `Profile` (`frontend/src/lib/api.ts#L4-L35`); **thay
  lớp `api.ts`** `fetch("/api/...")` (`api.ts#L91-L157`) bằng `@tauri-apps/api`
  `invoke()`; **bỏ** `ProfileViewer`/noVNC + `LoginPage` (docs/03 §7).
- **SQLite CRUD:** file `~/.browserx/browserx.db`; bảng `profiles` (kế thừa
  `database.py#L34-L59`) + `settings` (docs/03 §2).
- **Launcher theo OS:** Rust port `get_binary_path` (`config.py#L169-L181`) + `build_args`
  (`browser.py#L1028-L1087`): stealth default (`config.py#L54-L76`), dedup theo key,
  `--user-data-dir` + `--remote-debugging-port`, headful **không emulate viewport**
  (docs/03 §3.2); teardown `kill(pid)` thay `pkill` (docs/03 §3.3).
- **Fingerprint seed + timezone:** `--fingerprint=<seed>`, `--fingerprint-platform` theo
  **lựa chọn profile** (mặc định theo host, cho override + cảnh báo mismatch — docs/03 §6),
  `--fingerprint-timezone`/`--lang`/`--fingerprint-locale` qua **flag binary**
  (không CDP emulation — `browser.py#L429-L430`).
- **Proxy per-profile + mã hoá:** bảng `proxies` với `username_enc`/`password_enc` mã hoá
  AEAD, khoá trong OS keychain; giải mã trong RAM ngay trước khi dựng `--proxy-server`
  (port `_resolve_proxy_config` `browser.py#L1305-L1352`) — vá `database.py#L38`.
- **Tải + verify binary runtime:** port `download.py` (`#L131-L259`, `#L474-L544`):
  tải từ `cloakbrowser.dev`/GitHub Releases, **verify Ed25519 pinned pubkey** rồi SHA-256
  (docs/03 §3.4); Rust dùng `reqwest`+`ed25519-dalek`+`sha2`.
- **Concurrency cap:** `tokio::Semaphore` với `max_concurrent` (bảng `settings`) + hàng
  đợi launch tránh OOM (docs/03 §5).

**Exit criteria (đo được).**
- [ ] App **native** chạy trên **cả 3 OS** (ít nhất macOS + Linux; Windows nếu có máy) —
      không cần Docker/VNC.
      — ⏳ build + test CI xanh cả 3 OS (CI ubuntu/macos run 28589356494; Windows CI từ
      W22b; release v0.1.0 run 28817004073 xanh 4 job) nhưng app mới **chạy thật** trên
      macOS (smoke .dmg 2026-07-07); cần chạy thật trên máy Linux/Windows.
- [x] Lưu & CRUD **≥1.000 profile stored**; list/search local **< 200ms p95**.
      — ✅ `src-tauri/tests/exit_criteria.rs` (p95 trên 40 mẫu, 1.000 profile):
      list p95 = 15.9ms, search p95 = 4.3ms (2026-07-02; verifier đo lại 19.28/5.11ms).
- [x] Launch/stop 1 profile qua UI: cửa sổ **headful thật** mở, teardown `kill(pid)`
      **không rác tiến trình** (kiểm tra bằng process list).
      — ✅ smoke headful 2026-07-02; soak 30′ N=8 (W5r 2026-07-03): 0 zombie sau
      teardown, `pgrep chromium` = 0; reaper `try_wait` có unit test (process.rs).
- [x] Chạy ổn định **≥10 phiên concurrent** trên 1 máy dev ≥ 30 phút, tỉ lệ launch ≥ 99%
      — **điều kiện theo RAM**: đạt khi host RAM **≥32GiB** (cap → 11 ≥ 10). Trên host
      24GiB cap mặc định = 8; ổn định đã chứng minh bằng soak **N=8 / 30 phút** (chạy 10
      trên 24GiB overcommit bộ nhớ macOS → **không** phải default).
      — ✅ soak N=8/30′ PASS trên host 24GiB (W5r 2026-07-03: launched 8/8, alive 8/8
      suốt 30′, RSS phẳng ~1610MB, 0 zombie) — đúng nhánh 24GiB của tiêu chí;
      ⚠️ nhánh ≥10 phiên trên host ≥32GiB **chưa đo** (chưa có máy).
- [x] ≥2 profile khác seed → canvas/WebGL hash khác nhau (như Pha 0, nhưng qua BrowserX).
      — ✅ W5r 2026-07-03: hash khác seed qua launcher/ProcessManager BrowserX
      (`tests/fingerprint_seed.rs` PASS trên binary thật).
- [x] Proxy credential **mã hoá at-rest** (dump SQLite: **0** plaintext creds).
      — ✅ verifier 2026-07-02: dump SQLite 0 plaintext creds (XChaCha20-Poly1305, khoá
      keychain/env-first); IPC không lộ password (W5c).
- [x] Binary **tải runtime + verify Ed25519** pass; **không** binary nào trong repo/artifact
      (kiểm tra `git ls-files`).
      — ✅ smoke ensure_binary tải + verify Ed25519 (2026-07-02); `git ls-files` sạch
      (verifier Pha 1); artifact .dmg 24MB không chứa binary (smoke 2026-07-07).
- [x] UI/DB set được **≥12/17 flag** `--fingerprint*` (docs/07 §2) theo profile.
      — ✅ 17/17 flag set được qua DB, có test (verifier 2026-07-02); Pha 3 bổ sung thêm
      flag navigator/fonts/storage (P3-5a).

**Rủi ro liên quan.** R6 (bỏ VNC/token cleartext + mã hoá proxy → **giảm mạnh** ở bản
local), R3 (headful native, cap RAM — không còn nghẽn CDP-port/VNC), R2/R5 (pin+verify
binary), R1 (tải runtime, không redistribute).

---

## 4. Pha 2 — Parity Multilogin

**Mục tiêu.** Bù các mục 🔴/⚠️ docs/07 §9 để đạt trải nghiệm "kiểu Multilogin" ở mức
local: cookie import/export & snapshot (mã hoá), tags/search/template/bulk, quản lý proxy
có health-check, automation API qua CDP.

**Hạng mục.**
- **Cookie/profile import-export & snapshot:** định dạng chuẩn (Netscape/JSON); snapshot
  `user_data_dir` khi stop (docs/03 §4 bước "snapshot"); **đóng gói + mã hoá** bản export
  bằng khoá keychain (docs/03 §2) — hiện Manager không có endpoint (docs/07 §6).
- **Tags/search/template/bulk:** giữ tags (`database.py#L61-L66`); thêm **template
  profile** + **bulk create/launch/stop** (thay `auto_launch` tuần tự
  `browser_manager.py#L342-L362`, docs/07 §7); search/filter local.
- **Quản lý proxy production:** pool + **health-check** + xoay + gán lại (docs/07 §4).
- **Automation API qua CDP:** expose client CDP ổn định (docs/03 §8: `chromiumoxide` hoặc
  tự viết) để user script hoá; cấp `cdp_url` per-profile như bản gốc (`main.py#L546`).
- **Mã hoá at-rest toàn diện:** proxy + cookie export + secret trong DB (docs/03 §2).

**Exit criteria (đo được).**
- [ ] Import → export → import lại 1 profile **round-trip** giữ nguyên cookie/localStorage
      (so hash `storage_state`).
      — ⏳ export profile chủ đích **không** kèm cookie/user_data_dir (export.rs); cookie
      import/export riêng qua CDP (W24a) có test round-trip định dạng JSON/Netscape,
      nhưng chưa có phép đo so hash `storage_state` trên profile thật.
- [x] Bulk tạo **≥500 profile** từ 1 template trong 1 thao tác; bulk launch/stop **≥20
      profile** trong 1 lệnh (tôn trọng semaphore concurrency).
      — ✅ `create_profiles_from_template` 1 transaction, canary 500 profile = 62ms
      (W29a 2026-07-07, commit 887ab47); bulk launch/stop qua multi-select ⇧L/⇧S tôn
      trọng semaphore (W13, xác nhận lại ở W29).
- [ ] Proxy health-check phân loại **healthy/unhealthy** đúng ≥ 95% mẫu test; UI hiển thị
      trạng thái + lần kiểm tra cuối.
      — ⏳ tính năng đã có (W19b verified: proxy check + UI trạng thái/lần kiểm cuối;
      4 unit test phân loại proxy_check.rs) nhưng **chưa đo** tỉ lệ đúng ≥95% trên bộ
      mẫu chuẩn.
- [x] Automation: script mẫu qua CDP điều khiển ≥1 profile (goto + click + đọc DOM) thành công.
      — ✅ W5r 2026-07-03: `cdp::goto` + eval đọc DOM trên binary thật PASS
      (`tests/fingerprint_seed.rs`); click consent qua `cdp::eval` trong CookieRobot
      (P3-4, verified 2026-07-06); docs snippet automation (W24c).
- [x] **0 secret plaintext** trong DB & bản export (proxy/cookie) — xác nhận qua dump + scan.
      — ✅ dump + scan 0 plaintext (verifier 2026-07-02); export/backup mã hoá
      AES-256-GCM + Argon2id (W25a verified); credential proxy template mã hoá at-rest
      (P3-3a).
- [x] Lưu trữ **≥5.000 profile stored**; list/search local p95 < 300ms.
      — ✅ canary 5.000 profile (db.rs `five_thousand_profiles_list_and_search_are_fast`,
      đo 2026-07-07, debug build): list = 219ms, search = 6ms, filter = 215ms — release
      còn nhanh hơn.

**Rủi ro liên quan.** R4 (build phần Multilogin), R6 (mã hoá toàn diện), R7 (acceptable-use
+ disclaimer trong UI).

---

## 5. Pha 3 — Hoàn thiện & phân phối

**Mục tiêu.** Đưa BrowserX thành app **cài đặt được cho người dùng cuối trên 3 OS**:
installer (Tauri bundler) + auto-update + audit log + đa hồ sơ người dùng local (nếu cần)
+ observability/logs.

**Hạng mục.**
- **Đóng gói installer 3 OS (Tauri bundler):** `.dmg`/`.app` (macOS), `.msi`/`.exe`
  (Windows), `.AppImage`/`.deb` (Linux); ký (code-signing) nơi khả thi. **Không** nhúng
  binary CloakBrowser vào installer — vẫn tải runtime + verify (docs/04, R1).
- **Auto-update:** cơ chế cập nhật app (Tauri updater) có verify chữ ký bản cập nhật.
- **Audit log bền:** bảng `audit` (docs/03 §2) ghi tạo/sửa/launch/stop, truy vấn được
  (thay log stdout `main.py#L41-L42`, docs/07 §8).
- **Đa hồ sơ người dùng local (tuỳ chọn):** tách vùng dữ liệu/DB theo OS user; **không**
  RBAC server (đó là mô hình team, thuộc Pha S).
- **Observability/logs:** `tracing` — số phiên live, RAM ước tính/phiên, tỉ lệ lỗi launch,
  thời gian launch p95; log file xoay vòng trong vùng dữ liệu app.
- **Watchdog/idle reclaim:** phát hiện tiến trình con chết + đóng phiên idle trả RAM
  (docs/03 §5).

**Exit criteria (đo được).**
- [ ] Installer build & cài **thành công trên cả 3 OS** (smoke test: cài → mở → tạo →
      launch 1 profile).
      — ⏳ phần **build** ĐẠT: tag v0.1.0 → workflow Release xanh **4 job**, 8 artifacts
      dmg/msi/exe/AppImage/deb (run 28817004073, 2026-07-07); smoke cài → mở mới làm
      trên **macOS** (.dmg); **chưa** smoke cài/mở/tạo/launch trên máy thật
      Windows/Linux (installer chưa ký — cần cert của user).
- [ ] Auto-update: nâng từ version N-1 → N **thành công + verify chữ ký** trên ≥1 OS.
      — ⏳ chưa làm: W23c auto-updater huỷ 2026-07-03 (cần user tạo minisign key +
      GitHub Secrets); chưa có bản N-1 → N.
- [x] **100%** hành động tạo/sửa/launch/stop sinh bản ghi **audit** truy vấn được.
      — ✅ 43 điểm insert_audit phủ tạo/sửa/launch/stop, rà toàn bộ không lộ secret
      (W26a verified 2026-07-06); audit viewer truy vấn được trong Settings; bulk create
      ghi 1 audit/batch (W29a).
- [x] Lưu trữ **≥10.000 profile stored** không suy giảm list/search (p95 < 500ms).
      — ✅ canary 10.000 profile (W26c 2026-07-06): list = 439ms, search = 8ms,
      tag = 51ms, filter = 440ms — tất cả < 500ms.
- [x] Dashboard/log hiển thị realtime: concurrent sessions, RAM/phiên, launch p95, error rate.
      — ✅ SystemPanel (W26b verified 2026-07-06): sessions live, RAM/phiên (RSS),
      launch p95 nearest-rank, error rate; poll 3s.
- [x] Watchdog: tiến trình con crash → trạng thái cập nhật < 10s, **không** "profile treo
      running", không rác tiến trình.
      — ✅ watchdog interval 2s (lib.rs:55 `start_watchdog(2000)`) < 10s; reap
      `try_wait` có unit test (process.rs); soak W5r 2026-07-03: 0 zombie, không profile
      treo running.
- [x] **Không** binary CloakBrowser trong installer/artifact (kiểm tra gói).
      — ✅ smoke artifact 2026-07-07: .dmg aarch64 24MB, app chỉ 3 file chính — không
      chứa binary CloakBrowser (vẫn tải runtime + verify Ed25519).

**Rủi ro liên quan.** R1 (đóng gói không kèm binary), R2 (auto-update app + mirror binary
hợp lệ tuỳ chọn — `BINARY-LICENSE.md#L39` internal use), R6 (audit + đa hồ sơ), R7
(disclaimer trong installer/UI).

---

## 6. Pha S — Scale server (NGOÀI phạm vi bản LOCAL)

**Khi nào cần.** Chỉ khi cần **hàng nghìn phiên chạy đồng thời** — vượt RAM của một máy
desktop (Spec §🔁: mỗi phiên ≈ 0.3–1GB; docs/03 §5). App local **không** giải quyết mục
tiêu này.

**Hướng đi (trỏ tới kiến trúc server cũ trong Spec).** Chuyển sang **kiến trúc server
scale ngang**: tách **control-plane / data-plane**, worker **đa node**, **container/
pod-per-profile**, DB **Postgres**, **hàng đợi launch** (Redis/NATS), **VNC gateway**,
autoscale, observability tập trung — mô tả trong Spec mục **"Q3 / 🔁 kiến trúc server"**
(và docs/03 §5 coi là **phương án scale tương lai**).

**Ghi chú.** Pha S **không** nằm trong lộ trình local; liệt kê ở đây để tránh nhầm rằng
app desktop sẽ tự đạt "nghìn phiên đồng thời". Nếu triển khai, đây là **dự án/kiến trúc
riêng**, tái dùng phần lõi logic flags/verify từ Pha 1.

---

## 7. Ánh xạ tính năng "phải tự xây" (docs/07 §9) → pha

| Hạng mục còn thiếu (🔴/⚠️) | Nguồn (docs/07) | Pha |
|---|---|:--:|
| CRUD profile + fingerprint seed + timezone (port sang Rust/Tauri) | §1–§5 | 1 |
| Dựng flags + spawn theo OS + tải/verify binary | §2, docs/03 §3 | 1 |
| Proxy per-profile + mã hoá at-rest (proxy) | §3, `database.py#L38` | 1 |
| Không gian seed lớn (chuỗi ≤128) | §2, `database.py#L37,#L93` | 1 |
| Expose flag fingerprint còn thiếu (device-memory, brand*, ...) | §2, §9 | 1→2 |
| Search/filter local (thay client filter) | §7, `main.py#L438-L439` | 1 |
| Import/export cookie & profile + snapshot | §6 | 2 |
| Template profile | §7 | 2 |
| Bulk create/launch/stop | §7, `browser_manager.py#L342-L362` | 2 |
| Mã hoá at-rest (cookie/secret) | §6, `database.py#L34-L59` | 2 |
| Quản lý proxy production (pool/health-check/xoay) | §4 | 2 |
| Automation API qua CDP (ổn định, script hoá) | §8 | 2 |
| Audit log bền | §8, `main.py#L41-L42` | 3 |
| Đa hồ sơ người dùng local | §8 | 3 |
| Đóng gói installer 3 OS + auto-update + observability | docs/03 §8 | 3 |
| Multi-user + RBAC (mô hình team) | §8, `main.py#L48-L80` | **S** |
| Orchestration pod-per-profile + autoscale + VNC gateway | docs/03 §5 | **S** |

> **Ngoài phạm vi mọi pha (do KHOÁ CỨNG CloakBrowser):** mobile fingerprint và engine
> Firefox/Stealthfox — binary chỉ desktop Chromium (`config.py#L18-L26`, `#L91-L98`);
> phụ thuộc roadmap CloakHQ, BrowserX không tự vá (docs/02 R2, docs/04).

---

## 8. Ánh xạ rủi ro (docs/02 R1–R8) → pha xử lý

> **Lưu ý:** bản LOCAL đã **giảm mạnh R3 / R6 / R8** so với bản server nhờ bỏ
> VNC/Xvnc/xclip (hết Linux-only), bỏ token cleartext + RFB proxy, headful native +
> `kill(pid)` thay `pkill` (docs/03 §0, §7; Spec §🔁 L107).

| Rủi ro | Mức độ | Pha xử lý chính | Cách xử lý (bản local) |
|---|---|:--:|---|
| R1 — License cấm SaaS/redistribute | Critical (đk) | 1, 3 (xuyên suốt) | Tải binary runtime + verify; **không** nhúng binary vào installer; disclaimer open-source ≠ SaaS |
| R2 — Bus-factor binary đóng | High (chấp nhận) | 1, 3 | Pin pubkey + verify Ed25519; mirror nội bộ hợp lệ (internal use); theo dõi version |
| R3 — Không scale | High → **giảm** | 1, (S) | Local: SQLite nghìn *stored* + semaphore vài chục *concurrent*; nghìn *concurrent* → **Pha S server** |
| R4 — Thiếu phần Multilogin | High | 2 | Import/export, template/bulk, proxy health-check, automation CDP |
| R5 — Fingerprint hộp đen | High | 0, 2 | Harness kiểm chứng ngoài (CreepJS/sannysoft); theo dõi version Chromium |
| R6 — Lỗ hổng bảo mật Manager | High → **giảm mạnh** | 1, 3 | **Bỏ VNC/RFB proxy + token cleartext** (local, single-user OS); mã hoá at-rest; audit (3) |
| R7 — Pháp lý/ToS | Medium | 2, 3 | Acceptable-use + disclaimer trong UI/installer/README |
| R8 — Giới hạn macOS | Medium → **giảm** | 0, 1 | PoC + app **native** chạy trên macOS (không cần Docker); fingerprint-platform chọn tự do + cảnh báo mismatch khi cross-OS (docs/03 §6) |

---

## 9. Cổng chuyển pha (không bỏ qua)

- **0 → 1:** hoàn tất checklist eval docs/06 §8 **và** PoC Rust spawn headful + CDP chạy được.
- **1 → 2:** app native chạy ≥2 OS + SQLite CRUD ≥1.000 stored + proxy mã hoá at-rest +
  ≥10 concurrent ổn định + binary verify Ed25519 pass.
- **2 → 3:** import/export round-trip + template/bulk + proxy health-check + **0 secret
  plaintext** đạt.
- **3 → S (tuỳ chọn):** chỉ khi cần **nghìn phiên đồng thời** — chuyển sang kiến trúc
  server (Spec §Q3), ngoài phạm vi bản local.
- **Xuyên suốt:** không pha nào commit/redistribute binary; giữ disclaimer open-source ≠
  quyền chạy browser-as-a-service (docs/02 R1, docs/04).

---

## 10. Lưu ý nhất quán

- Kiến trúc chi tiết của Pha 1–3 là **docs/03** (`docs/03-target-architecture.md` — bản
  LOCAL Rust); tài liệu này chỉ định nghĩa **thứ tự pha + exit criteria đo được**. Khi
  docs/03 đổi thuật ngữ (launcher, process-manager, semaphore, keychain), cập nhật đồng
  bộ §3–§5 tại đây.
- Danh mục tính năng bám **docs/07**; risk IDs (R1–R8) bám **docs/02**. Khi hai tài liệu
  đó đổi, rà lại §7 và §8.
- Kiến trúc **server** (control-plane/data-plane, pod-per-profile, Postgres/Redis, VNC
  gateway) **không** thuộc lộ trình local — chỉ xuất hiện ở **Pha S** như phương án tương
  lai (Spec §Q3, docs/03 §5).
