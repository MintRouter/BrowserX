//! BrowserX — local antidetect browser manager (Tauri v2 shell).
//!
//! Module layout (xem docs/03-target-architecture.md):
//! - `models`   — struct dùng chung (Profile, Proxy, RunningSession) theo Hợp đồng Tauri command
//! - `error`    — AppError (thiserror) + Result dùng chung
//! - `db`       — SQLite (rusqlite): schema, migrations, CRUD (Wave 2a)
//! - `config`   — đường dẫn app-data, settings, binary path theo OS (Wave 2b)
//! - `binary`   — tải + verify (Ed25519/SHA-256) binary CloakBrowser lúc runtime (Wave 2b)
//! - `launcher` — dựng CLI flags fingerprint + spawn Chromium (Wave 2c)
//! - `process`  — quản lý process/phiên đang chạy, cleanup (Wave 2c)
//! - `crypto`   — XChaCha20-Poly1305 + OS keychain (Wave 2d)
//! - `cdp`      — CDP client (chromiumoxide) attach/automation (Wave 3a)
//! - `commands` — Tauri commands (invoke handlers) (Wave 3a)

pub mod binary;
pub mod cdp;
pub mod commands;
pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod launcher;
pub mod models;
pub mod process;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
