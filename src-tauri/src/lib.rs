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
//! - `storage`  — đo dung lượng + dọn cache profile (W16)

pub mod backup;
pub mod binary;
pub mod cdp;
pub mod commands;
pub mod config;
pub mod cookierobot;
pub mod cookies;
pub mod crypto;
pub mod db;
pub mod error;
pub mod export;
pub mod extensions;
pub mod launcher;
pub mod logging;
pub mod models;
pub mod process;
pub mod proxy_check;
pub mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tauri::Builder::default()
        .setup(|app| {
            use std::sync::Arc;
            use tauri::Manager;

            let db = Arc::new(db::Db::open_default()?);
            let max_concurrent = db
                .get_setting("max_concurrent")?
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or_else(process::recommended_max_concurrent);
            let procs = process::ProcessManager::new(max_concurrent);

            let watchdog = procs.clone();
            let watchdog_db = db.clone();
            let status_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _handle = watchdog.start_watchdog(2000, move |profile_id, clean| {
                    let status = if clean { "stopped" } else { "crashed" };
                    commands::emit_status(&status_app, profile_id, status, None, None);
                    let _ = commands::auto_clear_cache_if_enabled(&watchdog_db, profile_id);
                    let _ = commands::apply_storage_options_on_stop(&watchdog_db, profile_id);
                });
            });

            app.manage(commands::AppState {
                db,
                procs,
                quitting: std::sync::atomic::AtomicBool::new(false),
                robots: cookierobot::RobotRegistry::default(),
            });
            Ok(())
        })
        // (W23a) Đóng cửa sổ khi còn phiên chạy → giữ cửa sổ lại (để hiện modal)
        // + báo FE mở dialog "Stop all & quit".
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use tauri::Manager;
                let state = window.state::<commands::AppState>();
                if state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let count = tauri::async_runtime::block_on(state.procs.list_running()).len();
                if count > 0 {
                    api.prevent_close();
                    commands::emit_exit_requested(window.app_handle(), count);
                }
            }
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
            commands::check_proxy,
            commands::list_proxy_templates,
            commands::create_proxy_template,
            commands::update_proxy_template,
            commands::delete_proxy_template,
            commands::create_proxy_from_template,
            commands::master_key_status,
            commands::launch_profile,
            commands::stop_profile,
            commands::list_running,
            commands::bring_to_front,
            commands::get_cdp_ws_url,
            commands::ensure_binary,
            commands::get_settings,
            commands::set_setting,
            commands::list_tags,
            commands::set_profile_tags,
            commands::list_folders,
            commands::create_folder,
            commands::rename_folder,
            commands::delete_folder,
            commands::set_favorite,
            commands::move_profiles_to_folder,
            commands::trash_profiles,
            commands::restore_profiles,
            commands::purge_profiles,
            commands::list_trash,
            commands::convert_quick_profile,
            commands::delete_quick_profile,
            commands::profile_storage_sizes,
            commands::clear_profile_cache,
            commands::list_templates,
            commands::save_as_template,
            commands::update_template,
            commands::delete_template,
            commands::create_profile_from_template,
            commands::export_profile,
            commands::import_profile,
            commands::export_cookies,
            commands::import_cookies,
            commands::start_cookie_robot,
            commands::stop_cookie_robot,
            commands::open_logs_folder,
            commands::stop_all_and_quit,
            commands::create_backup,
            commands::restore_backup,
            commands::restart_app,
            commands::list_extensions,
            commands::add_extension_from_folder,
            commands::add_extension_from_store_url,
            commands::remove_extension,
            commands::set_extension_enabled,
            commands::assign_extensions,
            commands::get_profile_extensions,
            commands::list_audit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // (W23a) Quit khi còn phiên chạy (vd Cmd+Q): chặn thoát + báo FE mở
        // dialog; thoát sạch (không phiên, hoặc sau stop_all_and_quit) thì
        // checkpoint WAL trước khi app tắt.
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = &event {
                use tauri::Manager;
                let state = app.state::<commands::AppState>();
                if state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
                    let _ = state.db.wal_checkpoint_truncate();
                    return;
                }
                let count = tauri::async_runtime::block_on(state.procs.list_running()).len();
                if count > 0 {
                    api.prevent_exit();
                    commands::emit_exit_requested(app, count);
                } else {
                    let _ = state.db.wal_checkpoint_truncate();
                }
            }
        });
}
