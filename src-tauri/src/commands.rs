//! Tauri commands (invoke handlers) — theo Hợp đồng command trong spec:
//! - Profiles: list_profiles, get_profile, create_profile, update_profile, delete_profile, search_profiles
//! - Proxies: list_proxies, create_proxy, update_proxy, delete_proxy, assign_proxy
//! - Session: launch_profile, stop_profile, list_running
//! - Binary: ensure_binary (emit `binary://progress`)
//! - Settings/tags: get_settings, set_setting, list_tags, set_profile_tags
//!
//! Wave 3a implement + đăng ký vào `tauri::Builder` trong lib.rs.
