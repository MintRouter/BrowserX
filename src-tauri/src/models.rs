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
    /// FK → Proxy.id (None = không dùng proxy).
    pub proxy_id: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
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
    pub username: Option<String>,
    /// Mã hoá at-rest (XChaCha20-Poly1305, khoá trong OS keychain) — xem `crypto`.
    pub password: Option<String>,
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
