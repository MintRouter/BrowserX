# 04 — Licensing & quyết định pháp lý

> **Disclaimer (bắt buộc đọc):** Tài liệu này **KHÔNG phải tư vấn pháp lý chính thức**.
> Đây là bản diễn giải kỹ thuật do đội kỹ sư đọc trực tiếp các file license trong `refs/`
> để phục vụ ra quyết định nội bộ. Trước khi thương mại hoá hoặc phân phối rộng, hãy nhờ
> luật sư có chuyên môn về IP/phần mềm rà soát. Mọi trích dẫn nguyên văn giữ tiếng Anh để
> tránh sai lệch; phần dịch tiếng Việt chỉ mang tính tham khảo.

Liên quan: [`docs/02-critique-risks.md`](02-critique-risks.md) (rủi ro #1, #7),
Spec (mục "Quyết định cuối 2026-07-01"), `refs/CloakBrowser/BINARY-LICENSE.md`.

---

## 1. TL;DR — quyết định đã chốt

- Dự án của ta (**BrowserX**): **OPEN SOURCE, self-host, KHÔNG bán SaaS** (Spec — Q1).
- **Code wrapper/manager** của CloakBrowser là **MIT** → ta được tự do dùng/sửa/phân phối
  (`refs/CloakBrowser/LICENSE#L1-L22`).
- **Binary Chromium** của CloakBrowser là **proprietary** ("Binary License") →
  ta **KHÔNG được commit/redistribute binary** vào repo. Cách hợp lệ: **tải lúc runtime**
  (đúng như CloakBrowser đang làm — xem §5).
- Open-source hoá dự án **không** tự cấp cho người khác quyền chạy **browser-as-a-service**:
  ai muốn làm SaaS thương mại vẫn **phải tự lo OEM license** với CloakHQ (§4, §6).
- Kết luận: đi theo **kịch bản A (cá nhân/nội bộ hợp lệ, miễn phí)**; ghi disclaimer OEM cho
  người tái sử dụng; giữ cửa **kịch bản C (tự build engine)** như lối thoát lệ thuộc.

---

## 2. Bản đồ license theo thành phần

| Thành phần | License | Ta được phép? | Nguồn |
|---|---|---|---|
| Wrapper Python/JS/.NET (repo CloakBrowser) | **MIT** | Dùng/sửa/phân phối tự do (giữ notice) | `refs/CloakBrowser/LICENSE#L1-L22` |
| Manager (FastAPI/React) | **MIT** | Như trên | `refs/CloakBrowser-Manager/LICENSE` (MIT) |
| **Binary Chromium đã patch** | **Proprietary "Binary License"** | Chỉ **dùng**, cấm redistribute/sửa/reverse | `refs/CloakBrowser/BINARY-LICENSE.md#L5-L13` |
| Chromium/ungoogled-chromium (thượng nguồn) | BSD-3-Clause (không bị hạn chế bởi Binary License) | Theo license gốc | `refs/CloakBrowser/BINARY-LICENSE.md#L13` |
| Widevine CDM | Proprietary Google, **không redistribute** | Người dùng tự tải (opt-in) | `refs/CloakBrowser/README.md#L448` |

> Wrapper MIT và Binary proprietary là **hai license tách biệt** — Binary License nói rõ nó
> "does **not** apply to the wrapper source code ... which is licensed under the MIT License"
> (`refs/CloakBrowser/BINARY-LICENSE.md#L7`).

---

## 3. Ranh giới: Wrapper (MIT) vs Binary (proprietary)

**Wrapper = MIT (thoải mái).** Nguyên văn README: *"The wrapper (Python + JS) is MIT and free
forever"* (`refs/CloakBrowser/README.md#L1324`, `#L1372`). Ta có thể fork, đổi tên, tích hợp vào
BrowserX open-source mà không ràng buộc ngoài việc giữ copyright notice (MIT — `LICENSE#L12-L13`).

**Binary = proprietary (nhiều ràng buộc).** Trích nguyên văn phần *Grant of Use*:

> "You are granted a non-exclusive, non-transferable, royalty-free license to use the Binary
> for personal or commercial purposes, subject to the Version-Specific Terms below."
> — `refs/CloakBrowser/BINARY-LICENSE.md#L15-L17`

Tức là: **dùng** cho cá nhân/thương mại **miễn phí** (kể cả cho doanh nghiệp của bạn —
`#L43`: *"Using CloakBrowser for your own business is free ... regardless of company size or
revenue"*), **nhưng** kèm loạt **Restrictions** — bạn **KHÔNG được** (`#L25-L35`):

1. **Redistribute** binary (dù sửa hay không) — `#L29`
2. **Resell, sublicense, hoặc repackage** binary, hoặc nhúng vào sản phẩm/dịch vụ phân phối cho bên thứ ba — `#L30`
3. **Reverse engineer / decompile / disassemble** — `#L31`
4. **Modify** hoặc tạo derivative works — `#L32`
5. **Gỡ/sửa** copyright notice/attribution — `#L33`

> Lưu ý quan trọng cho ta: *"Normal use of the Binary with command-line flags, browser
> extensions, managed policies, custom profiles, or user data directories does **not** constitute
> modification"* (`#L35`). → Toàn bộ cách CloakBrowser-Manager điều khiển fingerprint qua **CLI
> flags** (`--fingerprint=<seed>` …) là **hợp lệ**, không bị coi là "modify".

---

## 4. Điều khoản quyết định: SaaS cần OEM

Đây là điều khoản **chặn lớn nhất** (đối chiếu rủi ro #1 trong `docs/02`):

> **OEM/SaaS license required** — "Bundling, embedding, or pre-installing the Binary into a
> product, hosted service, or cloud artifact distributed to third parties requires a separate OEM
> license. **This includes running the Binary on your infrastructure to serve third-party
> customers (e.g., browser-as-a-service).** Contact cloakhq@pm.me for OEM/SaaS licensing."
> — `refs/CloakBrowser/BINARY-LICENSE.md#L45`

Multilogin/GoLogin/AdsPower **chính là** browser-as-a-service. → **Bán SaaS mà không có OEM deal =
vi phạm license.** Ngược lại, **nội bộ thì được phép** (`#L37-L43`):

> **Internal use** — "You may store and run the unmodified Binary within internal infrastructure,
> including Docker images, VM templates, CI runners, container registries, and artifact
> repositories ... solely for your organization's internal operational purposes." — `#L39`

> **Dependency listing** — liệt kê CloakBrowser như dependency (trong `requirements.txt`,
> `package.json`, docs) **không** phải redistribution vì end user tự tải binary từ kênh chính
> thức của CloakHQ. — `#L41`

**Official distribution** (`#L47-L49`): binary phải lấy gốc từ GitHub Releases /
cloakbrowser.dev; mirror nội bộ tổ chức được phép theo mục Internal use.

---

## 4b. ⚠️ Cảnh báo: hai bản BINARY-LICENSE khác version trong `refs/`

Hai repo clone **KHÔNG cùng phiên bản** Binary License — phải chọn bản mới nhất làm chuẩn:

| File | Version | Điểm khác |
|---|---|---|
| `refs/CloakBrowser/BINARY-LICENSE.md#L3` | **v1.1 — June 2026** | Có mục **"Version-Specific Terms"** (bắt Pro từ Chromium 148) |
| `refs/CloakBrowser-Manager/BINARY-LICENSE.md#L3` | **v1.0 — Feb 2026** | Ghi *"No fees are required"*, **chưa** có điều khoản Pro/version |

**Version-Specific Terms** (bản v1.1, `refs/CloakBrowser/BINARY-LICENSE.md#L19-L23`):

> "Starting with Chromium 148, downloading the **latest major** Binary version requires an active
> CloakBrowser Pro subscription. Previous **major** versions (v146 and earlier) remain available
> at no cost ... Each time a new **major** Chromium version is released, the **prior major
> version** becomes available for free download."

Hệ quả (khớp rủi ro #2 trong `docs/02`): **free tier luôn chậm 1 major Chromium so với Pro**
(README `#L1374-L1375`: v146 free, v148+ cần Pro). Với mô hình open-source phi thương mại của ta,
mặc định dùng **binary free (v146)**; nếu cần bản mới nhất phải có **license key Pro trả phí**
(cơ chế validate: `refs/CloakBrowser/cloakbrowser/license.py#L23-L27`, `#L54-L96`).

> **Khuyến nghị:** khi triển khai, luôn đối chiếu **BINARY-LICENSE.md bản mới nhất trên GitHub**
> của CloakHQ tại thời điểm dùng, vì điều khoản có thể thay đổi (license tự cho phép CloakHQ cập
> nhật — không có cam kết ổn định).

---

## 5. Hệ quả kỹ thuật cho dự án open-source của ta

Vì binary là proprietary + cấm redistribute, repo BrowserX **KHÔNG được chứa binary**. Cách an
toàn duy nhất: **tải runtime**, chỉ ship **code + script tải** (đúng thiết kế gốc CloakBrowser).

- Hàm tải: `ensure_binary()` — `refs/CloakBrowser/cloakbrowser/download.py#L131`
  (README: *"On first run, the ... binary is automatically downloaded (~200MB, cached locally)"*
  — `refs/CloakBrowser/README.md#L127`).
- Xác thực bắt buộc: chữ ký **Ed25519 non-bypassable** trên SHA256SUMS trước khi giải nén
  (`download.py#L295-L298`, `#L474-L513`; README `#L281`). `CLOAKBROWSER_SKIP_CHECKSUM` **chỉ**
  áp dụng cho URL mirror tuỳ chỉnh, **không** bỏ qua được chữ ký kênh chính thức
  (`refs/CloakBrowser/README.md#L663`).
- **Việc cần làm trong repo ta:**
  - [ ] Thêm `refs/` và mọi thư mục cache binary (`~/.browserx/engine/` — hoặc
        `~/.cloakbrowser/` cũ trước W58e — `*.tar`, `*.zip` chromium) vào `.gitignore`.
  - [ ] KHÔNG bake binary vào Docker image công khai phân phối cho bên thứ ba (đó là
        redistribution — `BINARY-LICENSE.md#L30`). Nếu build image **nội bộ**, được phép
        (`#L39`), nhưng **không push image kèm binary lên registry công khai**.
  - [ ] README của ta: liệt kê CloakBrowser như **dependency** + hướng dẫn người dùng tự tải
        (hợp lệ theo `#L41`). Có thể để notice *"Powered by CloakBrowser"* (tuỳ chọn, `#L55-L57`).
  - [ ] Widevine CDM: không bundle; để opt-in tự tải như CloakBrowser (README `#L448`, `#L475`).

---

## 6. Ba kịch bản dùng + Decision Matrix

**Kịch bản A — Cá nhân/nội bộ hợp lệ (đã chọn).** Self-host cho bản thân/tổ chức/cộng đồng, phi
thương mại; mỗi người tự tải binary. → **Miễn phí, hợp lệ** theo Grant of Use + Internal use
(`BINARY-LICENSE.md#L15-L17`, `#L39-L43`).

**Kịch bản B — SaaS thương mại (browser-as-a-service).** Chạy binary trên hạ tầng của ta để phục
vụ khách hàng bên thứ ba. → **Bắt buộc OEM license** với CloakHQ (`#L45`). Không có deal = vi
phạm. **Ngoài phạm vi dự án hiện tại.**

**Kịch bản C — Tự build engine (thoát lệ thuộc).** Thay CloakBrowser bằng Camoufox/Firefox
stealth, Patchright, hoặc tự patch Chromium. → Thoát ràng buộc Binary License, nhưng chi phí kỹ
thuật lớn (đối chiếu rủi ro #2, #5 trong `docs/02`). Lưu ý Spec **Q2 đã khoá cứng CloakBrowser** →
kịch bản C là **lối thoát dự phòng**, không phải kế hoạch hiện tại.

| Tiêu chí | A. Cá nhân/nội bộ | B. SaaS thương mại | C. Tự build engine |
|---|---|---|---|
| Hợp lệ theo Binary License | ✅ Có | ⚠️ Chỉ khi có OEM | ✅ Không áp dụng |
| Chi phí license | Free (v146) / Pro (v148+) | Phí OEM (thương lượng) | 0 (nhưng chi phí dev cao) |
| Được redistribute binary | ❌ Không | Theo OEM | ✅ (engine của bạn) |
| Phù hợp mục tiêu open-source của ta | ✅ **Đã chọn** | ❌ Ngoài scope | 🔶 Dự phòng |
| Rủi ro bus-factor (`docs/02` #2) | Cao (lệ thuộc CloakHQ) | Cao | Thấp |
| Nỗ lực triển khai | Thấp | Trung bình + pháp lý | Rất cao |

---

## 7. Acceptable Use — ranh giới cấm (áp dụng cho MỌI kịch bản)

Binary License cấm rõ (`BINARY-LICENSE.md#L59-L68`), bất kể free hay Pro:

> "You agree NOT to use the Binary for any activity that violates applicable laws ... the following
> uses are expressly prohibited: Unauthorized access to financial, banking, healthcare, or
> government authentication systems; **Credential stuffing, brute-force login attempts, or
> automated account creation**; Circumventing authentication on systems you do not own ...; Any
> activity that constitutes fraud, identity theft, or unauthorized data collection."

- **Indemnification** (`#L70-L72`): người dùng phải bồi thường cho CloakHQ nếu dùng trái luật.
- **Disclaimer / "AS IS"** (`#L74-L76`) và **Limitation of Liability** (`#L78-L80`): trách nhiệm
  tối đa của CloakHQ **US$100**.
- **Termination** (`#L90-L92`): license **tự động chấm dứt** nếu vi phạm bất kỳ điều khoản nào,
  phải huỷ mọi bản copy binary.

Trong README/docs của BrowserX **nên có Acceptable Use riêng** nhắc lại các cấm này + nêu rõ dự án
không endorse dùng sai mục đích (đối chiếu rủi ro #7 trong `docs/02`).

---

## 8. Checklist pháp lý / ToS / payment trước khi thương mại hoá

> Chỉ cần khi ai đó muốn chuyển sang **kịch bản B**. Với kịch bản A hiện tại, chỉ cần mục [ ] đầu.

- [ ] **(A + B)** Thêm `LICENSE` (MIT cho code ta) + `NOTICE` ghi rõ binary CloakBrowser là
      proprietary, người dùng tự tải & tự chịu trách nhiệm ToS.
- [ ] **(A + B)** Trang Acceptable Use / Terms: cấm fraud, credential stuffing, automated account
      creation (nhắc lại `BINARY-LICENSE.md#L59-L68`).
- [ ] **(B)** Liên hệ CloakHQ xin **OEM/SaaS license** (email §9) — có văn bản trước khi phục vụ
      khách bên thứ ba.
- [ ] **(B)** Rà soát **ToS nền tảng đích** (Google/Meta/…): multi-account thường vi phạm ToS của
      họ — quyết định có ý thức về rủi ro (rủi ro #7 `docs/02`).
- [ ] **(B)** **Payment processor**: Stripe/PayPal thường xếp ngành antidetect vào high-risk/cấm →
      xác nhận policy trước, dự phòng cổng thanh toán thay thế.
- [ ] **(B)** Chính sách dữ liệu/PII: cookie/proxy credential lưu **mã hoá** (rủi ro #6 `docs/02`);
      cân nhắc GDPR/CCPA nếu có người dùng EU/US.
- [ ] **(B)** Luật sư IP rà soát toàn bộ trước khi ký OEM & mở bán.

---

## 9. Template email hỏi OEM CloakHQ

> Gửi tới **cloakhq@pm.me** (`BINARY-LICENSE.md#L45`, `#L120`). Chỉ dùng khi cân nhắc kịch bản B.

\`\`\`
To: cloakhq@pm.me
Subject: OEM/SaaS licensing inquiry — CloakBrowser Binary

Hi CloakHQ team,

We are building a self-hosted browser profile manager on top of the CloakBrowser
wrapper (MIT) and the CloakBrowser Chromium Binary. We have read BINARY-LICENSE.md
and understand that running the Binary on our infrastructure to serve third-party
customers (browser-as-a-service) requires a separate OEM license (Section
"OEM/SaaS license required").

We would like to discuss:
1. OEM/SaaS licensing terms and pricing for browser-as-a-service usage.
2. Expected concurrency/scale: up to ~[N] stored profiles, ~[M] concurrent live
   sessions across multiple worker nodes.
3. Access to the latest major (Pro) Binary versions under an OEM agreement, and
   redistribution scope inside our container images.
4. Any brand/attribution requirements ("Powered by CloakBrowser") you'd like.

Company: [tên]  |  Website: [url]  |  Contact: [tên, email]

Thanks,
[Your name]
\`\`\`

---

## 10. Các bước cần chốt (action items)

1. [ ] Xác nhận **kịch bản A** là hướng chính thức (đã có trong Spec) → ghi vào README dự án.
2. [ ] Thêm `refs/`, cache binary vào `.gitignore`; thiết lập pipeline **tải binary runtime**.
3. [ ] Soạn `LICENSE` (MIT) + `NOTICE` + trang **Acceptable Use** cho BrowserX.
4. [ ] Ghi disclaimer OEM trong docs người dùng: self-host cá nhân/nội bộ OK; SaaS phải tự lo OEM.
5. [ ] Theo dõi thay đổi BINARY-LICENSE trên GitHub CloakHQ (điều khoản có thể đổi bất kỳ lúc nào).
6. [ ] (Dự phòng) Giữ tài liệu đánh giá **kịch bản C** phòng khi CloakHQ đổi điều khoản/biến mất.

---

## 11. Tham chiếu (đối chiếu code/license thật trong `refs/`)

- MIT wrapper: `refs/CloakBrowser/LICENSE#L1-L22`; README `#L1370-L1376`.
- Binary License (chuẩn, v1.1): `refs/CloakBrowser/BINARY-LICENSE.md`
  — IP `#L11-L13`, Grant `#L15-L17`, Version-Specific Terms `#L19-L23`, Restrictions `#L25-L35`,
  Cloud/Internal `#L37-L43`, OEM/SaaS `#L45`, Official distribution `#L47-L49`,
  Trademark `#L51-L53`, Attribution `#L55-L57`, Acceptable Use `#L59-L68`,
  Indemnification `#L70-L72`, Disclaimer `#L74-L76`, Liability `#L78-L80`,
  Termination `#L90-L92`, Contact `#L118-L121`.
- Binary License (bản cũ, v1.0): `refs/CloakBrowser-Manager/BINARY-LICENSE.md#L3`, `#L17-L18`.
- Runtime download + verify: `refs/CloakBrowser/cloakbrowser/download.py#L131`, `#L295-L298`,
  `#L474-L513`; README `#L127`, `#L281`, `#L663`.
- Pro license validate: `refs/CloakBrowser/cloakbrowser/license.py#L23-L27`, `#L54-L96`.
- Widevine CDM proprietary: `refs/CloakBrowser/README.md#L448`, `#L475`.
