//! Launcher: dựng CLI flags fingerprint từ `Profile` (port build_args từ
//! refs/CloakBrowser/cloakbrowser/browser.py + get_default_stealth_args trong config.py)
//! và spawn process Chromium (tokio::process) với CDP port riêng.
//!
//! Wave 2c implement.
