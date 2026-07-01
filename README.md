# BrowserX

**BrowserX** là ứng dụng desktop **local, open-source** quản lý antidetect browser (kiểu Multilogin/GoLogin/AdsPower): quản lý profile với fingerprint riêng, proxy per-profile, timezone/locale, và automation qua CDP.

- **Stack:** Rust + Tauri v2 (shell) · SQLite (rusqlite) · React + Vite + TypeScript + Tailwind (UI).
- **Engine:** spawn trực tiếp binary CloakBrowser (Chromium đã patch stealth) — headful, không VNC.
- **Cross-platform:** macOS (arm64/x64), Linux (x64/arm64), Windows (x64).
- **Tài liệu:** xem thư mục [`docs/`](docs/) (kiến trúc `docs/03`, roadmap `docs/05`, UI spec `docs/08`).

## Trạng thái

Pha 1 — MVP local (đang phát triển). Cấu trúc: frontend ở root (pnpm + Vite), core Rust ở `src-tauri/`.

```bash
pnpm install
pnpm tauri dev
```

## ⚠️ Binary CloakBrowser KHÔNG kèm trong repo

Repo này **chỉ chứa mã nguồn** (MIT). Binary Chromium của CloakBrowser là **proprietary** ("Binary License") — **không được redistribute**. BrowserX **tải binary lúc runtime** từ nguồn phân phối chính thức của CloakHQ và verify chữ ký Ed25519. Việc sử dụng binary tuân theo license riêng của CloakHQ.

## Acceptable-use disclaimer

- Dự án open-source này **không** cấp cho bạn quyền chạy **browser-as-a-service** (SaaS) trên binary CloakBrowser. Theo Binary License của CloakHQ, chạy binary trên hạ tầng của bạn để phục vụ khách hàng bên thứ ba **cần OEM/SaaS license riêng** với CloakHQ. Self-host cho mục đích **cá nhân/nội bộ** là hợp lệ.
- **Nghiêm cấm** dùng BrowserX cho fraud, credential-stuffing, tạo tài khoản tự động trái phép, hoặc bất kỳ hành vi vi phạm pháp luật/ToS nền tảng. Bạn tự chịu trách nhiệm về cách sử dụng.

## License

Mã nguồn BrowserX: MIT. Binary CloakBrowser: proprietary (license riêng của CloakHQ, xem `docs/04-licensing-legal-decision.md`).
