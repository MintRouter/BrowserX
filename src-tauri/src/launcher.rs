//! Launcher: dựng CLI flags fingerprint từ `Profile` (port build_args từ
//! refs/CloakBrowser/cloakbrowser/browser.py#L1028-L1087 + get_default_stealth_args
//! trong config.py#L54-L76) + proxy resolve (#L1305-L1352).
//!
//! Semantics quan trọng (giữ đúng port):
//! - Dedup theo KEY (phần trước dấu '='); update giữ nguyên vị trí (như dict Python).
//! - Ưu tiên: stealth default < user launch_args < tham số chuyên biệt (profile fields).
//! - Headful KHÔNG emulate viewport (screen width/height là fingerprint-screen, khác viewport).
//! - KHÔNG thêm --enable-automation / --enable-unsafe-swiftshader (IGNORE_DEFAULT_ARGS).
//!
//! Wave 2c. `build_args`/`resolve_proxy_args` là hàm public cho W3a wiring.

use crate::models::Profile;

/// Map bảo toàn thứ tự chèn; set lại key đã có sẽ cập nhật tại chỗ (giống dict Python).
struct OrderedArgs {
    keys: Vec<String>,
    vals: Vec<String>,
}

impl OrderedArgs {
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            vals: Vec::new(),
        }
    }

    /// Chèn/ghi đè theo key = phần trước dấu '=' đầu tiên của `flag`.
    fn set(&mut self, flag: impl Into<String>) {
        let flag = flag.into();
        let key = &flag[..flag.find('=').unwrap_or(flag.len())];
        if let Some(i) = self.keys.iter().position(|k| k == key) {
            self.vals[i] = flag;
        } else {
            self.keys.push(key.to_string());
            self.vals.push(flag);
        }
    }

    fn into_vec(self) -> Vec<String> {
        self.vals
    }
}

/// Platform fingerprint mặc định theo host khi profile.platform rỗng.
/// Port config.py: Darwin -> "macos", còn lại (Linux/Windows) -> "windows".
fn host_default_platform() -> &'static str {
    if std::env::consts::OS == "macos" {
        "macos"
    } else {
        "windows"
    }
}

/// Dựng argv đầy đủ để spawn Chromium cho `profile`.
///
/// `proxy_url` là URL proxy ĐÃ GIẢI MÃ (không đọc keychain ở đây) — nếu Some sẽ thành
/// `--proxy-server=<url>`. `cdp_port` là cổng remote-debugging đã cấp bởi process manager.
pub fn build_args(profile: &Profile, proxy_url: Option<&str>, cdp_port: u16) -> Vec<String> {
    let mut args = OrderedArgs::new();

    // 1) Stealth defaults (ưu tiên thấp nhất).
    args.set("--no-sandbox");
    args.set(format!("--fingerprint={}", profile.fingerprint_seed));
    let platform: &str = if profile.platform.trim().is_empty() {
        host_default_platform()
    } else {
        &profile.platform
    };
    args.set(format!("--fingerprint-platform={}", platform));

    // GPU blocklist bypass: chỉ khi headful (SwiftShader phục vụ WebGL). Port #L1047-L1055.
    if !profile.headless {
        args.set("--ignore-gpu-blocklist");
    }

    // 2) User launch_args (ghi đè stealth theo key). launch_args là JSON array chuỗi.
    if let Some(list) = profile.launch_args.as_array() {
        for v in list {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    args.set(s);
                }
            }
        }
    }

    // 3) Tham số chuyên biệt từ profile (ưu tiên cao nhất — luôn thắng user args).
    if let Some(tz) = profile.timezone.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--fingerprint-timezone={}", tz));
    }
    if let Some(loc) = profile.locale.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--lang={}", loc));
        args.set(format!("--fingerprint-locale={}", loc));
    }
    if let Some(v) = profile.gpu_vendor.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--fingerprint-gpu-vendor={}", v));
    }
    if let Some(r) = profile.gpu_renderer.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--fingerprint-gpu-renderer={}", r));
    }
    if profile.hardware_concurrency > 0 {
        args.set(format!(
            "--fingerprint-hardware-concurrency={}",
            profile.hardware_concurrency
        ));
    }
    // Screen fingerprint (KHÔNG phải viewport). Headful vẫn set fingerprint-screen an toàn.
    if profile.screen_width > 0 {
        args.set(format!(
            "--fingerprint-screen-width={}",
            profile.screen_width
        ));
    }
    if profile.screen_height > 0 {
        args.set(format!(
            "--fingerprint-screen-height={}",
            profile.screen_height
        ));
    }

    // Cờ vận hành bắt buộc — luôn từ tham số của ta.
    args.set(format!("--user-data-dir={}", profile.user_data_dir));
    args.set(format!("--remote-debugging-port={}", cdp_port));

    // Proxy (đã giải mã). Bao trùm cả SOCKS5 lẫn HTTP-cred-inline vì spawn binary trực tiếp.
    for flag in resolve_proxy_args(proxy_url) {
        args.set(flag);
    }

    args.into_vec()
}

/// Dựng arg proxy từ URL đã giải mã. Port `_resolve_proxy_config` (#L1305-L1352):
/// spawn binary trực tiếp nên mọi loại proxy đều qua `--proxy-server` (không dùng Playwright dict).
pub fn resolve_proxy_args(proxy_url: Option<&str>) -> Vec<String> {
    match proxy_url {
        Some(url) if !url.trim().is_empty() => vec![format!("--proxy-server={}", url.trim())],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_profile() -> Profile {
        Profile {
            id: "p1".into(),
            name: "test".into(),
            fingerprint_seed: "42".into(),
            platform: "windows".into(),
            timezone: None,
            locale: None,
            screen_width: 0,
            screen_height: 0,
            gpu_vendor: None,
            gpu_renderer: None,
            hardware_concurrency: 0,
            humanize: false,
            human_preset: None,
            headless: true,
            geoip: false,
            color_scheme: None,
            launch_args: serde_json::json!([]),
            user_data_dir: "/tmp/udd".into(),
            notes: None,
            proxy_id: None,
            tags: vec![],
            created_at: "2026-07-01T00:00:00Z".into(),
            updated_at: "2026-07-01T00:00:00Z".into(),
        }
    }

    fn value_of<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
        args.iter()
            .find(|a| a.split('=').next() == Some(key))
            .map(|a| a.split_once('=').map_or("", |x| x.1))
    }

    fn count_key(args: &[String], key: &str) -> usize {
        args.iter()
            .filter(|a| a.split('=').next() == Some(key))
            .count()
    }

    #[test]
    fn seed_platform_and_port_present() {
        let p = base_profile();
        let args = build_args(&p, None, 5100);
        assert!(args.iter().any(|a| a == "--no-sandbox"));
        assert_eq!(value_of(&args, "--fingerprint"), Some("42"));
        assert_eq!(value_of(&args, "--fingerprint-platform"), Some("windows"));
        assert_eq!(value_of(&args, "--remote-debugging-port"), Some("5100"));
        assert_eq!(value_of(&args, "--user-data-dir"), Some("/tmp/udd"));
    }

    #[test]
    fn headless_omits_gpu_blocklist_headful_includes() {
        let mut p = base_profile();
        p.headless = true;
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--ignore-gpu-blocklist"),
            0
        );
        p.headless = false;
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--ignore-gpu-blocklist"),
            1
        );
    }

    #[test]
    fn timezone_locale_gpu_hardware_screen_flags() {
        let mut p = base_profile();
        p.timezone = Some("Asia/Ho_Chi_Minh".into());
        p.locale = Some("vi-VN".into());
        p.gpu_vendor = Some("Intel Inc.".into());
        p.gpu_renderer = Some("Intel Iris".into());
        p.hardware_concurrency = 8;
        p.screen_width = 1920;
        p.screen_height = 1080;
        let args = build_args(&p, None, 1);
        assert_eq!(
            value_of(&args, "--fingerprint-timezone"),
            Some("Asia/Ho_Chi_Minh")
        );
        assert_eq!(value_of(&args, "--lang"), Some("vi-VN"));
        assert_eq!(value_of(&args, "--fingerprint-locale"), Some("vi-VN"));
        assert_eq!(
            value_of(&args, "--fingerprint-gpu-vendor"),
            Some("Intel Inc.")
        );
        assert_eq!(
            value_of(&args, "--fingerprint-gpu-renderer"),
            Some("Intel Iris")
        );
        assert_eq!(
            value_of(&args, "--fingerprint-hardware-concurrency"),
            Some("8")
        );
        assert_eq!(value_of(&args, "--fingerprint-screen-width"), Some("1920"));
        assert_eq!(value_of(&args, "--fingerprint-screen-height"), Some("1080"));
    }

    #[test]
    fn proxy_flag_from_decoded_url() {
        let p = base_profile();
        let args = build_args(&p, Some("socks5://user:pass@1.2.3.4:1080"), 1);
        assert_eq!(count_key(&args, "--proxy-server"), 1);
        assert_eq!(
            value_of(&args, "--proxy-server"),
            Some("socks5://user:pass@1.2.3.4:1080")
        );
        // None => không có flag proxy.
        assert_eq!(count_key(&build_args(&p, None, 1), "--proxy-server"), 0);
        assert!(resolve_proxy_args(Some("  ")).is_empty());
    }

    #[test]
    fn dedup_priority_user_overrides_stealth_dedicated_overrides_user() {
        let mut p = base_profile();
        // user override --fingerprint-platform + đặt --lang, cùng flag không key khác.
        p.launch_args = serde_json::json!([
            "--fingerprint-platform=macos",
            "--lang=en-US",
            "--window-size=800,600"
        ]);
        // dedicated locale phải thắng user --lang.
        p.locale = Some("vi-VN".into());
        let args = build_args(&p, None, 1);
        // user override thắng stealth platform.
        assert_eq!(value_of(&args, "--fingerprint-platform"), Some("macos"));
        assert_eq!(count_key(&args, "--fingerprint-platform"), 1);
        // dedicated locale thắng user --lang.
        assert_eq!(value_of(&args, "--lang"), Some("vi-VN"));
        assert_eq!(count_key(&args, "--lang"), 1);
        // flag không đụng key vẫn giữ.
        assert_eq!(value_of(&args, "--window-size"), Some("800,600"));
    }

    #[test]
    fn empty_platform_falls_back_to_host_default() {
        let mut p = base_profile();
        p.platform = "".into();
        let args = build_args(&p, None, 1);
        let v = value_of(&args, "--fingerprint-platform").unwrap();
        assert!(v == "macos" || v == "windows");
    }
}
