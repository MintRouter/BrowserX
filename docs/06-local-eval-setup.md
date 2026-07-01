# 06 — Đánh giá cục bộ bằng Docker trên macOS (Local Eval Setup)

> Hướng dẫn dựng và đánh giá **thực tế** hai repo đã clone trong `refs/` ngay trên
> máy dev macOS, chỉ bằng Docker. Mọi bước/đường dẫn/cổng đối chiếu **code thật**
> theo dạng `path#Lstart-Lend`. Không bịa.
>
> ⚠️ Đây là bước **eval** (đánh giá), không phải build sản phẩm BrowserX. Ta chạy
> nguyên hai repo tham chiếu để hiểu hành vi thật trước khi thiết kế.

## 0. Vì sao phải qua Docker trên macOS

- CloakBrowser-Manager là **Linux/Docker-only**: image cài KasmVNC + Xvnc + `xclip`
  (`refs/CloakBrowser-Manager/Dockerfile#L22`, `#L35-40`), entrypoint dọn tiến trình
  `Xvnc`/chromium bằng `pkill` (`refs/CloakBrowser-Manager/entrypoint.sh#L7-11`).
  Không có bản chạy native trên macOS.
- Chạy trong container Linux còn có lợi: engine sẽ mặc định **spoof Windows**
  (`platform="windows"` — `refs/CloakBrowser-Manager/backend/models.py#L16`), nên
  eval sát với ca dùng thật hơn so với chạy CloakBrowser native trên macOS (chạy như
  Mac thật — `refs/CloakBrowser/cloakbrowser/config.py#L57-76`).

## 1. Yêu cầu

Theo README repo Manager (`refs/CloakBrowser-Manager/README.md#L101-105`):

- **Docker 20.10+** (Docker Desktop for Mac). Hỗ trợ cả Apple Silicon (arm64) và
  Intel (x64): Dockerfile tự chọn gói KasmVNC theo `TARGETARCH`
  (`refs/CloakBrowser-Manager/Dockerfile#L36-40`), và engine có sẵn binary cho
  `linux-arm64` lẫn `linux-x64` (`refs/CloakBrowser/cloakbrowser/config.py#L20-26`).
- **~2 GB disk** (image + binary), **~512 MB RAM cho mỗi profile đang chạy**.
- (Khuyến nghị) một **residential proxy** để test các site anti-bot (không bắt buộc
  cho fingerprint cơ bản).

## 2. Dựng & chạy Manager

Trong thư mục repo Manager tham chiếu:

```bash
cd refs/CloakBrowser-Manager
docker compose up --build
```

Lệnh này khớp README (`refs/CloakBrowser-Manager/README.md#L34-38`) và
`docker-compose.yml`:

- Build từ `Dockerfile` local (`refs/CloakBrowser-Manager/docker-compose.yml#L2-3`).
- Bind cổng **`127.0.0.1:8080:8080`** — chỉ localhost
  (`docker-compose.yml#L4-5`); entrypoint chạy uvicorn trên `:8080`
  (`entrypoint.sh#L26`).
- Mount volume dữ liệu `~/.cloakbrowser-manager:/data`
  (`docker-compose.yml#L6-7`); profiles nằm ở `/data/profiles`
  (`entrypoint.sh#L5`).
- Trong lúc build, binary Chromium được **tải sẵn** để lần chạy đầu không phải chờ
  (`Dockerfile#L54-55`).

> Lần build đầu tải KasmVNC + binary (~200MB) nên hơi lâu; đây là bình thường.

Kiểm tra healthcheck (`Dockerfile#L59-60`) — endpoint không cần auth
(`backend/main.py#L570-579`):

```bash
curl -s http://localhost:8080/api/status
# {"running_count":0,"binary_version":"146.0.7680.177.5","profiles_total":0}
```

`binary_version` lấy từ `cloakbrowser.config.CHROMIUM_VERSION`
(`backend/main.py#L570-579`; `refs/CloakBrowser/cloakbrowser/config.py#L18`).

## 3. Mở UI, tạo & launch profile

1. Mở **http://localhost:8080** (`refs/CloakBrowser-Manager/README.md#L40`). Mặc định
   **không cần đăng nhập** vì chưa đặt `AUTH_TOKEN`
   (`backend/main.py#L48-54`; `docker-compose.yml#L8-9`).
2. **Create profile** → đặt tên; các trường mặc định: `platform=windows`,
   `screen 1920x1080`, seed fingerprint ngẫu nhiên nếu để trống
   (`backend/models.py#L10-33`). Có thể thêm proxy dạng
   `http://user:pass@host:port`, `host:port:user:pass`, hoặc `socks5://...`
   (`backend/browser_manager.py#L22-53`).
3. **Launch**. Backend cấp display Xvnc (`:100+`), cổng WS (`6100+`)
   (`backend/vnc_manager.py#L22-37`) và cổng CDP (`5100–5199`)
   (`backend/browser_manager.py#L145-146`, `#L364-377`), rồi mở persistent context
   (`backend/browser_manager.py#L217-234`).
4. **Xem trực tiếp** trong UI qua noVNC (WebSocket proxy tại
   `/api/profiles/{id}/vnc` — `backend/main.py#L677-707`).

> Trong container không có GPU nên engine dùng software GL
> (`--use-angle=swiftshader` — `backend/browser_manager.py#L384`). Đây là điều cần
> lưu ý khi đọc kết quả WebGL renderer.

## 4. Test fingerprint trên các site kiểm tra

Khi tạo profile, Manager tự tạo sẵn **bookmark các site kiểm tra**
(`backend/browser_manager.py#L84-113`). Mở trong browser đang chạy (qua VNC) để đánh
giá:

- **Bot detection**: `bot-detector.rebrowser.net`, `bot.sannysoft.com`,
  `bot.incolumitas.com`, `browserscan.net/bot-detection`,
  `demo.fingerprint.com/web-scraping`, `pixelscan.net/fingerprint-check`,
  `abrahamjuliot.github.io/creepjs` (`browser_manager.py#L85-93`).
- **Fingerprint**: `browserleaks.com/canvas|webgl|fonts|javascript`,
  `fingerprintjs.github.io/fingerprintjs`, `audiofingerprint.openwpm.com`
  (`browser_manager.py#L96-102`).
- **Headers & TLS**: `httpbin.org/headers|ip`, `tls.browserleaks.com`
  (`browser_manager.py#L104-107`).
- **reCAPTCHA/Turnstile**: demo v3 của Google/2captcha, `peet.ws/turnstile-test`
  (`browser_manager.py#L109-112`).

Cách đọc kết quả (tham chiếu kỳ vọng vendor — `refs/CloakBrowser/README.md#L207-227`,
tự kiểm chứng chứ không tin tuyệt đối): `navigator.webdriver=false`, `window.chrome`
là object, UA không lộ `HeadlessChrome`. Lưu ý các nhược điểm môi trường Docker/VNC:
software GL, và cần font cho một số site (§6).

## 5. (Tuỳ chọn) Eval engine CloakBrowser trực tiếp

Không qua Manager, chạy nhanh engine bằng image chính thức
(`refs/CloakBrowser/README.md#L50`, `#L862`):

```bash
docker run --rm cloakhq/cloakbrowser cloaktest
```

Hoặc bật CDP server để nối Playwright từ host
(`refs/CloakBrowser/README.md#L897-911`):

```bash
docker run -d --name cloak -p 127.0.0.1:9222:9222 cloakhq/cloakbrowser cloakserve
curl -s http://localhost:9222/json/version | jq -r .webSocketDebuggerUrl
docker stop cloak && docker rm cloak
```

`cloakserve` là CDP multiplexer: mỗi seed → 1 process Chrome riêng
(`refs/CloakBrowser/README.md#L977-1000`).

## 6. Giới hạn trên macOS (đọc kỹ)

- **Manager không chạy native trên macOS** — bắt buộc Docker (Linux) vì phụ thuộc
  KasmVNC/Xvnc/xclip (`refs/CloakBrowser-Manager/Dockerfile#L22,#L35-40`).
- **Software GL trong container**: WebGL dùng SwiftShader
  (`backend/browser_manager.py#L384`) → renderer khác máy thật; đừng đánh giá GPU
  fingerprint như trên host.
- **Chạy engine native trên macOS = Mac thật (mặc định)**: wrapper **mặc định** bỏ qua
  spoof Windows trên Darwin (`refs/CloakBrowser/cloakbrowser/config.py#L57-76`) — đây là
  default, **không phải khoá cứng**; vẫn có thể ép `--fingerprint-platform=windows` nhưng
  chất lượng giảm do mismatch (docs/03 §6). Để eval "Windows desktop" **sát thực tế nhất**
  nên chạy trong container Linux (mặc định của Manager).
- **Free tier chậm hơn Chromium mới**: macOS free ở Chromium 145 với 26 patch, trong
  khi Linux/Windows free là 146/58 patch (`refs/CloakBrowser/README.md#L841-847`;
  `config.py#L20-26`). Trong Docker (Linux) ta dùng nhánh 146 → sát bản mới hơn.
- **Font Windows cho CreepJS**: một số site chấm điểm font cần cài thêm font; image
  Manager đã có `ttf-mscorefonts-installer` (`Dockerfile#L28-33`) nhưng đây là font
  XP-era, chưa đủ cho điểm CreepJS cao (`refs/CloakBrowser/README.md#L746`).
- **Đồng thời (concurrency)**: giới hạn thực tế bởi RAM (~512MB/profile) và dải cổng
  CDP chỉ 100 (`refs/CloakBrowser-Manager/README.md#L105`;
  `backend/browser_manager.py#L145-146`). Không phù hợp cho quy mô lớn — đó là lý do
  cần kiến trúc scale riêng (docs 03).

## 7. Automation qua CDP (kiểm chứng API)

Mỗi profile đang chạy expose endpoint CDP `/api/profiles/{id}/cdp`
(`refs/CloakBrowser-Manager/README.md#L119-144`; `backend/main.py#L845-1016`):

```python
from playwright.async_api import async_playwright
async with async_playwright() as pw:
    browser = await pw.chromium.connect_over_cdp(
        "http://localhost:8080/api/profiles/<profile-id>/cdp")
    page = browser.contexts[0].pages[0]
    await page.goto("https://example.com")
```

## 8. Checklist eval (chạy được)

- [ ] `cd refs/CloakBrowser-Manager && docker compose up --build` khởi động không lỗi.
- [ ] `curl -s http://localhost:8080/api/status` trả JSON có `binary_version`.
- [ ] Mở `http://localhost:8080`, tạo 1 profile (platform=windows, seed để trống).
- [ ] Launch profile; thấy màn hình browser qua noVNC trong UI.
- [ ] Mở `bot.sannysoft.com` + `browserleaks.com/canvas`: `navigator.webdriver=false`,
      không lộ `HeadlessChrome`.
- [ ] Tạo profile thứ 2 khác seed → canvas/WebGL hash khác profile 1.
- [ ] (Tuỳ chọn) Nối Playwright qua `/api/profiles/{id}/cdp` và `goto` thành công.
- [ ] (Tuỳ chọn) `docker run --rm cloakhq/cloakbrowser cloaktest` chạy được.

## 9. Dọn dẹp

```bash
# Dừng Manager
docker compose down            # trong refs/CloakBrowser-Manager
# Xoá dữ liệu profiles đã eval (tuỳ chọn — nằm ngoài repo)
rm -rf ~/.cloakbrowser-manager
```

Volume dữ liệu ở `~/.cloakbrowser-manager` (`docker-compose.yml#L6-7`); binary engine
cache mặc định ở `~/.cloakbrowser` (`refs/CloakBrowser/cloakbrowser/config.py#L150-159`).
Không đụng gì vào `refs/`.
