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
/// `assigned_extensions` (P3-1a) là paths unpacked từ kho trung tâm
/// (`db.profile_extension_paths`) — merge + dedup với legacy `profile.extensions`.
pub fn build_args(
    profile: &Profile,
    proxy_url: Option<&str>,
    cdp_port: u16,
    assigned_extensions: &[String],
) -> Vec<String> {
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

    // (P3-5a) Fingerprint controls sâu — flag `--fingerprint-*` THẬT của binary
    // (xác minh trong refs/CloakBrowser/README.md flag table). Chỉ emit khi field
    // có giá trị → profile cũ (mọi field None/false) KHÔNG đổi hành vi.
    if let Some(b) = profile.nav_brand.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--fingerprint-brand={}", b));
    }
    if let Some(v) = profile.nav_brand_version.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--fingerprint-brand-version={}", v));
    }
    if let Some(v) = profile
        .platform_version
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        args.set(format!("--fingerprint-platform-version={}", v));
    }
    if let Some(m) = profile.device_memory.filter(|m| *m > 0) {
        args.set(format!("--fingerprint-device-memory={}", m));
    }
    if let Some(d) = profile.fonts_dir.as_deref().filter(|s| !s.is_empty()) {
        args.set(format!("--fingerprint-fonts-dir={}", d));
    }
    if profile.windows_font_metrics {
        args.set("--fingerprint-windows-font-metrics");
    }
    if let Some(q) = profile.storage_quota.filter(|q| *q > 0) {
        args.set(format!("--fingerprint-storage-quota={}", q));
    }

    // (W19c) Fingerprint controls nâng cao — flag thật của CloakBrowser binary.
    // Noise injection (canvas/WebGL/audio/client-rects) bật mặc định trong binary;
    // chỉ cần emit khi TẮT.
    if !profile.fp_noise {
        args.set("--fingerprint-noise=false");
    }
    // WebRTC: binary chỉ hỗ trợ spoof ICE public IP. "masked" → set IP (nếu trống
    // → "auto": binary tự lấy IP công khai từ proxy/mạng). "real" → không đụng.
    if profile.webrtc_mode == "masked" {
        let ip = profile
            .webrtc_ip
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("auto");
        args.set(format!("--fingerprint-webrtc-ip={}", ip));
    }
    // Geolocation: "manual" + đủ lat/lon → --fingerprint-location=lat,lon.
    if profile.geolocation_mode == "manual" {
        if let (Some(lat), Some(lon)) = (
            profile
                .geo_latitude
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            profile
                .geo_longitude
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            args.set(format!("--fingerprint-location={},{}", lat, lon));
        }
    }

    // (W24b + P3-1a) Unpacked extensions: gộp paths gán từ kho trung tâm
    // (assigned_extensions, đứng trước) với legacy profile.extensions (JSON),
    // trim + bỏ rỗng + dedup giữ thứ tự → comma-join thành --load-extension +
    // --disable-extensions-except (chặn extension ngoài danh sách,
    // semantics giống extension_paths trong browser.py#L1078-L1086).
    let legacy = profile.extensions.as_array();
    let mut ext_paths: Vec<&str> = Vec::new();
    for s in assigned_extensions
        .iter()
        .map(|s| s.trim())
        .chain(
            legacy
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .map(str::trim),
        )
        .filter(|s| !s.is_empty())
    {
        if !ext_paths.contains(&s) {
            ext_paths.push(s);
        }
    }
    if !ext_paths.is_empty() {
        let joined = ext_paths.join(",");
        args.set(format!("--load-extension={}", joined));
        args.set(format!("--disable-extensions-except={}", joined));
    }

    // Cờ vận hành bắt buộc — luôn từ tham số của ta.
    args.set(format!("--user-data-dir={}", profile.user_data_dir));
    args.set(format!("--remote-debugging-port={}", cdp_port));

    // Proxy (đã giải mã). Bao trùm cả SOCKS5 lẫn HTTP-cred-inline vì spawn binary trực tiếp.
    for flag in resolve_proxy_args(proxy_url) {
        args.set(flag);
    }

    // Startup behavior (W18c): "custom" → URLs làm positional args (append SAU mọi
    // flag, không dedup theo key); mặc định/"restore" → mở lại phiên trước.
    let mut startup_urls: Vec<String> = Vec::new();
    if profile.startup_behavior == "custom" {
        if let Some(list) = profile.startup_urls.as_array() {
            for v in list {
                if let Some(s) = v.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        startup_urls.push(s.to_string());
                    }
                }
            }
        }
    } else {
        args.set("--restore-last-session");
    }

    let mut argv = args.into_vec();
    argv.extend(startup_urls);
    argv
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
            folder_id: None,
            favorite: false,
            is_quick: false,
            proxy_id: None,
            tags: vec![],
            created_at: "2026-07-01T00:00:00Z".into(),
            updated_at: "2026-07-01T00:00:00Z".into(),
            last_start_at: None,
            startup_behavior: "restore".into(),
            startup_urls: serde_json::json!([]),
            fp_noise: true,
            webrtc_mode: "real".into(),
            webrtc_ip: None,
            geolocation_mode: "auto".into(),
            geo_latitude: None,
            geo_longitude: None,
            store_history: true,
            store_passwords: true,
            store_sw_cache: true,
            extensions: serde_json::json!([]),
            nav_brand: None,
            nav_brand_version: None,
            platform_version: None,
            device_memory: None,
            fonts_dir: None,
            windows_font_metrics: false,
            storage_quota: None,
        }
    }

    /// Shadow glob-import: đa số test không quan tâm extension gán từ kho
    /// trung tâm → gọi bản 3 tham số, truyền `&[]` cho assigned_extensions.
    fn build_args(profile: &Profile, proxy_url: Option<&str>, cdp_port: u16) -> Vec<String> {
        super::build_args(profile, proxy_url, cdp_port, &[])
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
    fn extensions_emit_load_and_disable_except_flags() {
        let mut p = base_profile();
        // Rỗng → không có flag nào.
        let args = build_args(&p, None, 1);
        assert_eq!(count_key(&args, "--load-extension"), 0);
        assert_eq!(count_key(&args, "--disable-extensions-except"), 0);
        // Trim + bỏ rỗng, comma-join theo semantics browser.py.
        p.extensions = serde_json::json!(["/data/ext/ublock", " /data/ext/dark ", ""]);
        let args = build_args(&p, None, 1);
        assert_eq!(
            value_of(&args, "--load-extension"),
            Some("/data/ext/ublock,/data/ext/dark")
        );
        assert_eq!(
            value_of(&args, "--disable-extensions-except"),
            Some("/data/ext/ublock,/data/ext/dark")
        );
    }

    #[test]
    fn assigned_extensions_merge_and_dedup_with_legacy() {
        let mut p = base_profile();
        // Chỉ assigned (kho trung tâm) — legacy rỗng.
        let assigned = vec!["/data/ext/store-a".to_string(), " /data/ext/store-b ".into()];
        let args = super::build_args(&p, None, 1, &assigned);
        assert_eq!(
            value_of(&args, "--load-extension"),
            Some("/data/ext/store-a,/data/ext/store-b")
        );

        // Merge: assigned đứng trước legacy; trùng path (sau trim) chỉ giữ 1.
        p.extensions = serde_json::json!(["/data/ext/legacy", "/data/ext/store-a", ""]);
        let args = super::build_args(&p, None, 1, &assigned);
        assert_eq!(
            value_of(&args, "--load-extension"),
            Some("/data/ext/store-a,/data/ext/store-b,/data/ext/legacy")
        );
        assert_eq!(
            value_of(&args, "--disable-extensions-except"),
            Some("/data/ext/store-a,/data/ext/store-b,/data/ext/legacy")
        );
    }

    #[test]
    fn startup_restore_adds_flag_no_positional() {
        let p = base_profile();
        let args = build_args(&p, None, 1);
        assert!(args.iter().any(|a| a == "--restore-last-session"));
        // Không có positional arg nào (mọi phần tử đều là flag).
        assert!(args.iter().all(|a| a.starts_with("--")));
    }

    #[test]
    fn startup_custom_appends_urls_as_positional_after_flags() {
        let mut p = base_profile();
        p.startup_behavior = "custom".into();
        p.startup_urls =
            serde_json::json!(["https://a.example", " https://b.example ", ""]);
        let args = build_args(&p, None, 1);
        assert!(!args.iter().any(|a| a == "--restore-last-session"));
        // URLs (trim, bỏ rỗng) nằm CUỐI argv, sau mọi flag.
        assert_eq!(
            &args[args.len() - 2..],
            ["https://a.example", "https://b.example"]
        );
        assert!(args[..args.len() - 2].iter().all(|a| a.starts_with("--")));

        // Custom nhưng danh sách rỗng → không có positional, cũng không restore.
        p.startup_urls = serde_json::json!([]);
        let args = build_args(&p, None, 1);
        assert!(!args.iter().any(|a| a == "--restore-last-session"));
        assert!(args.iter().all(|a| a.starts_with("--")));
    }

    #[test]
    fn fp_noise_flag_only_when_disabled() {
        let mut p = base_profile();
        // Mặc định (bật) → không emit flag.
        assert_eq!(count_key(&build_args(&p, None, 1), "--fingerprint-noise"), 0);
        p.fp_noise = false;
        let args = build_args(&p, None, 1);
        assert_eq!(value_of(&args, "--fingerprint-noise"), Some("false"));
        assert_eq!(count_key(&args, "--fingerprint-noise"), 1);
    }

    #[test]
    fn webrtc_masked_sets_ip_real_omits() {
        let mut p = base_profile();
        // real (mặc định) → không có flag.
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--fingerprint-webrtc-ip"),
            0
        );
        // masked không IP → "auto".
        p.webrtc_mode = "masked".into();
        assert_eq!(
            value_of(&build_args(&p, None, 1), "--fingerprint-webrtc-ip"),
            Some("auto")
        );
        // masked + IP cụ thể (có khoảng trắng thừa) → dùng IP đó.
        p.webrtc_ip = Some(" 203.0.113.7 ".into());
        assert_eq!(
            value_of(&build_args(&p, None, 1), "--fingerprint-webrtc-ip"),
            Some("203.0.113.7")
        );
        // real + IP set sẵn → vẫn không emit.
        p.webrtc_mode = "real".into();
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--fingerprint-webrtc-ip"),
            0
        );
    }

    #[test]
    fn geolocation_manual_sets_location_when_complete() {
        let mut p = base_profile();
        // auto → không có flag.
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--fingerprint-location"),
            0
        );
        // manual nhưng thiếu toạ độ → bỏ qua.
        p.geolocation_mode = "manual".into();
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--fingerprint-location"),
            0
        );
        p.geo_latitude = Some("52.5".into());
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--fingerprint-location"),
            0
        );
        // Đủ lat+lon → lat,lon (trim).
        p.geo_longitude = Some(" 13.4 ".into());
        assert_eq!(
            value_of(&build_args(&p, None, 1), "--fingerprint-location"),
            Some("52.5,13.4")
        );
    }

    #[test]
    fn empty_platform_falls_back_to_host_default() {
        let mut p = base_profile();
        p.platform = "".into();
        let args = build_args(&p, None, 1);
        let v = value_of(&args, "--fingerprint-platform").unwrap();
        assert!(v == "macos" || v == "windows");
    }

    // (P3-5a) Fingerprint controls sâu: mặc định (profile cũ) KHÔNG emit flag nào.
    #[test]
    fn deep_fp_defaults_emit_nothing() {
        let args = build_args(&base_profile(), None, 1);
        for key in [
            "--fingerprint-brand",
            "--fingerprint-brand-version",
            "--fingerprint-platform-version",
            "--fingerprint-device-memory",
            "--fingerprint-fonts-dir",
            "--fingerprint-windows-font-metrics",
            "--fingerprint-storage-quota",
        ] {
            assert_eq!(count_key(&args, key), 0, "unexpected {key}");
        }
    }

    // (P3-5a) Có giá trị → emit đúng flag + giá trị cho ≥5 field mới.
    #[test]
    fn deep_fp_string_flags_emit_when_set() {
        let mut p = base_profile();
        p.nav_brand = Some("Edge".into());
        p.nav_brand_version = Some("120.0.0.0".into());
        p.platform_version = Some("15.0.0".into());
        p.fonts_dir = Some("/home/u/.fonts/win".into());
        let args = build_args(&p, None, 1);
        assert_eq!(value_of(&args, "--fingerprint-brand"), Some("Edge"));
        assert_eq!(
            value_of(&args, "--fingerprint-brand-version"),
            Some("120.0.0.0")
        );
        assert_eq!(
            value_of(&args, "--fingerprint-platform-version"),
            Some("15.0.0")
        );
        assert_eq!(
            value_of(&args, "--fingerprint-fonts-dir"),
            Some("/home/u/.fonts/win")
        );
    }

    // (P3-5a) String rỗng KHÔNG emit (giống các field chuyên biệt khác).
    #[test]
    fn deep_fp_empty_strings_omitted() {
        let mut p = base_profile();
        p.nav_brand = Some("".into());
        p.fonts_dir = Some("".into());
        let args = build_args(&p, None, 1);
        assert_eq!(count_key(&args, "--fingerprint-brand"), 0);
        assert_eq!(count_key(&args, "--fingerprint-fonts-dir"), 0);
    }

    // (P3-5a) device_memory/storage_quota: chỉ emit khi Some & >0.
    #[test]
    fn deep_fp_numeric_flags_emit_only_when_positive() {
        let mut p = base_profile();
        p.device_memory = Some(0);
        p.storage_quota = Some(0);
        let args = build_args(&p, None, 1);
        assert_eq!(count_key(&args, "--fingerprint-device-memory"), 0);
        assert_eq!(count_key(&args, "--fingerprint-storage-quota"), 0);

        p.device_memory = Some(8);
        p.storage_quota = Some(5000);
        let args = build_args(&p, None, 1);
        assert_eq!(value_of(&args, "--fingerprint-device-memory"), Some("8"));
        assert_eq!(value_of(&args, "--fingerprint-storage-quota"), Some("5000"));
    }

    // (P3-5a) windows_font_metrics: cờ boolean, chỉ emit khi true.
    #[test]
    fn deep_fp_windows_font_metrics_bool_flag() {
        let mut p = base_profile();
        assert_eq!(
            count_key(&build_args(&p, None, 1), "--fingerprint-windows-font-metrics"),
            0
        );
        p.windows_font_metrics = true;
        let args = build_args(&p, None, 1);
        assert!(args
            .iter()
            .any(|a| a == "--fingerprint-windows-font-metrics"));
    }
}
