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
        .setup(|app| {
            use std::sync::Arc;
            use tauri::Manager;

            let db = Arc::new(db::Db::open_default()?);
            let max_concurrent = db
                .get_setting("max_concurrent")?
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(process::DEFAULT_MAX_CONCURRENT);
            let procs = process::ProcessManager::new(max_concurrent);

            let watchdog = procs.clone();
            tauri::async_runtime::spawn(async move {
                let _handle = watchdog.start_watchdog(2000);
            });

            app.manage(commands::AppState { db, procs });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::get_profile,
            commands::create_profile,
            commands::update_profile,
            commands::delete_profile,
            commands::search_profiles,
            commands::list_proxies,
            commands::create_proxy,
            commands::update_proxy,
            commands::delete_proxy,
            commands::assign_proxy,
            commands::launch_profile,
            commands::stop_profile,
            commands::list_running,
            commands::ensure_binary,
            commands::get_settings,
            commands::set_setting,
            commands::list_tags,
            commands::set_profile_tags,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
