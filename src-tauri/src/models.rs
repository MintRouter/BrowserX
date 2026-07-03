//! Type dùng chung cho toàn bộ app — nguồn sự thật của Hợp đồng Tauri command.
//!
//! Wave 2/3 dùng các struct này; KHÔNG định nghĩa lại type trùng ở module khác.
//! Timestamp là chuỗi RFC3339 (UTC).

use serde::{Deserialize, Serialize};

/// Một browser profile với fingerprint riêng (map 1-1 với hàng trong bảng `profiles`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// UUID v4.
    pub id: String,
    pub name: String,
    /// Seed điều khiển fingerprint (`--fingerprint=<seed>`).
    pub fingerprint_seed: String,
    /// Target OS fingerprint: "windows" | "macos" | "linux" (chọn tự do, cảnh báo khi ≠ host).
    pub platform: String,
    /// IANA timezone, ví dụ "Asia/Ho_Chi_Minh". None = auto theo proxy/geoip.
    pub timezone: Option<String>,
    /// BCP-47 locale, ví dụ "vi-VN". None = auto.
    pub locale: Option<String>,
    pub screen_width: u32,
    pub screen_height: u32,
    pub gpu_vendor: Option<String>,
    pub gpu_renderer: Option<String>,
    pub hardware_concurrency: u32,
    /// Bật humanize input (CDP input patch).
    pub humanize: bool,
    pub human_preset: Option<String>,
    pub headless: bool,
    /// Auto-khớp timezone/locale/geo theo IP proxy.
    pub geoip: bool,
    /// "light" | "dark" | None (mặc định hệ thống).
    pub color_scheme: Option<String>,
    /// Mảng flag bổ sung, lưu dạng JSON (ví dụ `["--lang=vi"]`).
    pub launch_args: serde_json::Value,
    /// Thư mục user-data riêng của profile (tuyệt đối).
    pub user_data_dir: String,
    pub notes: Option<String>,
    /// FK → Folder.id (None = chưa thuộc thư mục nào).
    pub folder_id: Option<String>,
    /// Đánh dấu yêu thích (hiển thị ở mục Favorites trên UI).
    pub favorite: bool,
    /// Quick profile (dùng-xong-xoá, W18b): khi Stop, UI hỏi
    /// Save as regular (bỏ cờ) / Close & delete (purge data).
    pub is_quick: bool,
    /// FK → Proxy.id (None = không dùng proxy).
    pub proxy_id: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Lần launch thành công gần nhất (RFC3339 UTC). None = chưa từng chạy.
    pub last_start_at: Option<String>,
    /// Hành vi khởi động: "restore" (mở lại phiên trước) | "custom" (mở `startup_urls`).
    pub startup_behavior: String,
    /// Danh sách URL mở khi khởi động (JSON array chuỗi) — chỉ dùng khi
    /// `startup_behavior = "custom"`.
    pub startup_urls: serde_json::Value,
    /// (W19c) Bật noise injection (canvas/WebGL/audio/client-rects) — công tắc chung
    /// của binary. `false` → `--fingerprint-noise=false`. Mặc định true (bật).
    pub fp_noise: bool,
    /// (W19c) Chế độ WebRTC: "real" (không đụng) | "masked" (spoof ICE IP theo
    /// `webrtc_ip`). Binary chỉ hỗ trợ thay IP qua `--fingerprint-webrtc-ip`.
    pub webrtc_mode: String,
    /// (W19c) IP công khai để spoof WebRTC khi `webrtc_mode = "masked"`. None = bỏ qua.
    pub webrtc_ip: Option<String>,
    /// (W19c) Chế độ geolocation: "auto" (theo IP/hệ thống) | "manual" (toạ độ tự nhập).
    pub geolocation_mode: String,
    /// (W19c) Vĩ độ khi `geolocation_mode = "manual"` (chuỗi, ví dụ "52.5").
    pub geo_latitude: Option<String>,
    /// (W19c) Kinh độ khi `geolocation_mode = "manual"` (chuỗi, ví dụ "13.4").
    pub geo_longitude: Option<String>,
    /// (W20b) Lưu lịch sử duyệt web. `false` → xoá file History khi phiên dừng
    /// (binary không có flag disable — cơ chế là cleanup, xem `storage`).
    pub store_history: bool,
    /// (W20b) Lưu mật khẩu đã save. `false` → xoá Login Data khi phiên dừng.
    pub store_passwords: bool,
    /// (W20b) Giữ service-worker cache. `false` → xoá Default/Service Worker khi phiên dừng.
    pub store_sw_cache: bool,
    /// (W24b) Đường dẫn unpacked extension local (JSON array chuỗi) — emit
    /// `--load-extension` + `--disable-extensions-except` khi launch.
    pub extensions: serde_json::Value,
}

/// (W20b) Template cấu hình profile (bảng `profile_templates`). `config` là JSON
/// shape `ProfileInput` (db.rs) — field lạ bị bỏ qua khi tạo profile từ template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileTemplate {
    /// UUID v4.
    pub id: String,
    pub name: String,
    /// Cấu hình mặc định (os, proxy, fingerprint, startup, storage options…).
    pub config: serde_json::Value,
    pub created_at: String,
}

/// Thư mục nhóm profile (bảng `folders`), kèm số profile còn sống (không tính trash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    /// UUID v4.
    pub id: String,
    pub name: String,
    /// Số profile trong thư mục có `deleted_at IS NULL`.
    pub profile_count: i64,
    pub created_at: String,
}

/// Proxy dùng chung, gán được cho nhiều profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    /// UUID v4.
    pub id: String,
    pub name: String,
    /// "http" | "https" | "socks5".
    pub protocol: String,
    pub host: String,
    pub port: u16,
    /// (W5c) Username KHÔNG trả plaintext qua IPC — chỉ bản đã che
    /// (ký tự đầu + "***") để hiển thị; đổi thì nhập lại trong form
    /// (để trống = giữ nguyên).
    pub masked_username: Option<String>,
    /// Password KHÔNG bao giờ trả plaintext qua IPC — chỉ báo đã lưu hay chưa.
    /// Bản mã hoá at-rest (XChaCha20-Poly1305, khoá trong OS keychain) chỉ được
    /// giải mã trong backend lúc launch — xem `crypto`.
    pub has_password: bool,
    /// (W23b) Credential không giải mã được bằng master key hiện tại (key đã
    /// đổi/keychain reset) — FE hiện cảnh báo yêu cầu nhập lại password.
    #[serde(default)]
    pub credentials_invalid: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Một phiên browser đang chạy (trả về từ `launch_profile` / `list_running`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningSession {
    pub profile_id: String,
    pub pid: u32,
    pub cdp_port: u16,
    /// ví dụ "http://127.0.0.1:<cdp_port>".
    pub cdp_url: String,
    pub started_at: String,
}
