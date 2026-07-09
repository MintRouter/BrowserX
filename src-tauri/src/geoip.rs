//! GeoIP auto-match (W35): suy timezone/locale/geolocation từ exit IP của proxy.
//!
//! Port semantics từ refs/CloakBrowser/cloakbrowser/geoip.py#L54-L109 nhưng
//! KHÔNG tải GeoLite2 mmdb (~70 MB): tái dùng đường HTTP của `proxy_check`
//! (IP-echo qua proxy → ipapi.co JSON) — không thêm dependency mới.
//!
//! Resolve là best-effort: mọi lỗi mạng/parse trả `None`, launch vẫn tiếp tục.
//! Semantics áp dụng nằm trong `launcher::build_args` (thủ công thắng GeoIP).

use std::time::Duration;

use crate::models::Profile;
use crate::proxy_check::{fetch_text, parse_ip_response, proxied_client};

/// Budget TỔNG cho toàn bộ resolve GeoIP (tối đa 2 IP-echo + 1 geo lookup,
/// tuần tự). Cũng là timeout per-request của client, nhưng tổng thời gian bị
/// chặn bởi `tokio::time::timeout` trong `resolve_geo_with` — worst-case
/// launch chỉ trễ ~10s dù mọi request treo, không phải 3×10s.
const GEOIP_TIMEOUT: Duration = Duration::from_secs(10);

/// Kết quả GeoIP đã map sẵn cho launcher. Field nào không suy được thì None.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeoInfo {
    /// IANA timezone, ví dụ "Europe/Berlin".
    pub timezone: Option<String>,
    /// BCP-47 locale suy từ country ISO code (map port từ geoip.py).
    pub locale: Option<String>,
    /// Vĩ độ (chuỗi, ví dụ "52.52"). Chỉ Some khi có ĐỦ cả lat lẫn lon.
    pub latitude: Option<String>,
    /// Kinh độ (chuỗi, ví dụ "13.405").
    pub longitude: Option<String>,
}

/// Country ISO code → BCP-47 locale. Port COUNTRY_LOCALE_MAP (geoip.py#L36-L51).
pub fn country_locale(iso: &str) -> Option<&'static str> {
    Some(match iso.trim().to_ascii_uppercase().as_str() {
        "US" => "en-US",
        "GB" => "en-GB",
        "AU" => "en-AU",
        "CA" => "en-CA",
        "NZ" => "en-NZ",
        "IE" => "en-IE",
        "ZA" => "en-ZA",
        "SG" => "en-SG",
        "DE" => "de-DE",
        "AT" => "de-AT",
        "CH" => "de-CH",
        "FR" => "fr-FR",
        "BE" => "fr-BE",
        "ES" => "es-ES",
        "MX" => "es-MX",
        "AR" => "es-AR",
        "CO" => "es-CO",
        "CL" => "es-CL",
        "BR" => "pt-BR",
        "PT" => "pt-PT",
        "IT" => "it-IT",
        "NL" => "nl-NL",
        "JP" => "ja-JP",
        "KR" => "ko-KR",
        "CN" => "zh-CN",
        "TW" => "zh-TW",
        "HK" => "zh-HK",
        "RU" => "ru-RU",
        "UA" => "uk-UA",
        "PL" => "pl-PL",
        "CZ" => "cs-CZ",
        "RO" => "ro-RO",
        "IL" => "he-IL",
        "TR" => "tr-TR",
        "SA" => "ar-SA",
        "AE" => "ar-AE",
        "EG" => "ar-EG",
        "IN" => "hi-IN",
        "ID" => "id-ID",
        "PH" => "en-PH",
        "TH" => "th-TH",
        "VN" => "vi-VN",
        "MY" => "ms-MY",
        "SE" => "sv-SE",
        "NO" => "nb-NO",
        "DK" => "da-DK",
        "FI" => "fi-FI",
        "GR" => "el-GR",
        "HU" => "hu-HU",
        "BG" => "bg-BG",
        _ => return None,
    })
}

/// Parse body JSON từ ipapi.co `/{ip}/json/`: lấy `timezone`, `country_code`
/// (→ locale), `latitude`/`longitude` (số hoặc chuỗi). Trả None nếu body báo
/// lỗi hoặc không trích được field nào hữu ích.
pub fn parse_geo_response(body: &str) -> Option<GeoInfo> {
    let v: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    if v.get("error").and_then(|e| e.as_bool()) == Some(true) {
        return None;
    }
    let str_field = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    // ipapi trả số; chấp nhận cả chuỗi cho chắc.
    let coord = |k: &str| match v.get(k) {
        Some(x) if x.is_number() => x.as_f64().map(|f| f.to_string()),
        Some(x) => x
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.parse::<f64>().is_ok())
            .map(String::from),
        None => None,
    };
    let timezone = str_field("timezone");
    let locale = str_field("country_code")
        .as_deref()
        .and_then(country_locale)
        .map(String::from);
    let (latitude, longitude) = match (coord("latitude"), coord("longitude")) {
        (Some(lat), Some(lon)) => (Some(lat), Some(lon)),
        _ => (None, None),
    };
    if timezone.is_none() && locale.is_none() && latitude.is_none() {
        return None;
    }
    Some(GeoInfo {
        timezone,
        locale,
        latitude,
        longitude,
    })
}

/// Profile có cần resolve GeoIP không: `geoip=true` VÀ còn ít nhất một field
/// chưa set thủ công (timezone/locale trống, hoặc geolocation chưa manual đủ
/// toạ độ). Đủ hết → khỏi gọi mạng.
pub fn profile_needs_geoip(profile: &Profile) -> bool {
    if !profile.geoip {
        return false;
    }
    let missing = |o: &Option<String>| {
        o.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
    };
    let manual_geo = profile.geolocation_mode == "manual"
        && !missing(&profile.geo_latitude)
        && !missing(&profile.geo_longitude);
    missing(&profile.timezone) || missing(&profile.locale) || !manual_geo
}

/// Resolve GeoIP qua proxy với endpoint production. Best-effort — None khi lỗi.
pub async fn resolve_geo(proxy_url: &str) -> Option<GeoInfo> {
    resolve_geo_with(
        proxy_url,
        &[
            "https://api.ipify.org?format=json",
            "https://ifconfig.me/ip",
        ],
        "https://ipapi.co/{ip}/json/",
        GEOIP_TIMEOUT,
    )
    .await
}

/// Lõi resolve với endpoint + budget tham số hoá (test/offline harness trỏ vào
/// server local — cùng pattern `proxy_check::check_proxy_url_with`):
/// 1) IP-echo QUA proxy để lấy exit IP (thử lần lượt `ip_echo_urls`);
/// 2) GET `geo_url_template` (placeholder `{ip}`) qua proxy → parse GeoInfo.
///
/// `budget` là bound TỔNG cho cả chuỗi request (không phải per-request):
/// hết budget → None, launch vẫn tiếp tục (best-effort giữ nguyên).
pub async fn resolve_geo_with(
    proxy_url: &str,
    ip_echo_urls: &[&str],
    geo_url_template: &str,
    budget: Duration,
) -> Option<GeoInfo> {
    tokio::time::timeout(budget, async {
        let client = proxied_client(proxy_url, budget).ok()?;
        let mut ip = None;
        for url in ip_echo_urls {
            if let Ok(body) = fetch_text(&client, url).await {
                if let Some(found) = parse_ip_response(&body) {
                    ip = Some(found);
                    break;
                }
            }
        }
        let body = fetch_text(&client, &geo_url_template.replace("{ip}", &ip?))
            .await
            .ok()?;
        parse_geo_response(&body)
    })
    .await
    .ok()
    .flatten()
}

// ---------------------------------------------------------------------------
// (W56b) No-proxy geo fallback từ OS locale — port ý tưởng từ
// VFlowX_V2/internal/gologin/no_proxy_defaults.go + os_locale*.go.
//
// Khác VFlowX ở 2 điểm chủ đích:
// - Timezone dùng timezone THẬT của máy (iana-time-zone), KHÔNG suy từ city:
//   không proxy → site thấy IP thật → tz thật khớp IP, chính xác hơn tz city.
// - Deterministic per-profile: PRNG seed từ profile id (FNV-1a → splitmix64)
//   để cùng profile launch lại không nhảy city/toạ độ.
//
// Hoàn toàn offline — không request mạng nào.
// ---------------------------------------------------------------------------

/// Toạ độ city lớn (port majorCityGeolocations từ VFlowX fingerprint_geo.go).
/// KHÔNG kèm timezone — xem ghi chú W56b ở trên.
const CITY_GEOS: &[(&str, f64, f64)] = &[
    // US
    ("New York", 40.7128, -74.0060),
    ("Los Angeles", 34.0522, -118.2437),
    ("Chicago", 41.8781, -87.6298),
    ("Houston", 29.7604, -95.3698),
    ("San Francisco", 37.7749, -122.4194),
    ("Miami", 25.7617, -80.1918),
    ("Dallas", 32.7767, -96.7970),
    ("Seattle", 47.6062, -122.3321),
    ("Denver", 39.7392, -104.9903),
    ("Phoenix", 33.4484, -112.0740),
    ("Atlanta", 33.7490, -84.3880),
    ("Boston", 42.3601, -71.0589),
    // Europe
    ("London", 51.5074, -0.1278),
    ("Berlin", 52.5200, 13.4050),
    ("Paris", 48.8566, 2.3522),
    ("Amsterdam", 52.3676, 4.9041),
    ("Madrid", 40.4168, -3.7038),
    ("Rome", 41.9028, 12.4964),
    ("Warsaw", 52.2297, 21.0122),
    ("Stockholm", 59.3293, 18.0686),
    ("Vienna", 48.2082, 16.3738),
    ("Prague", 50.0755, 14.4378),
    ("Zurich", 47.3769, 8.5417),
    // Asia
    ("Tokyo", 35.6762, 139.6503),
    ("Singapore", 1.3521, 103.8198),
    ("Seoul", 37.5665, 126.9780),
    ("Mumbai", 19.0760, 72.8777),
    ("Bangkok", 13.7563, 100.5018),
    ("Jakarta", -6.2088, 106.8456),
    ("Taipei", 25.0330, 121.5654),
    ("Hong Kong", 22.3193, 114.1694),
    ("Ho Chi Minh City", 10.7626, 106.6602),
    // Oceania / Americas (non-US) / Middle East
    ("Sydney", -33.8688, 151.2093),
    ("Toronto", 43.6532, -79.3832),
    ("São Paulo", -23.5505, -46.6333),
    ("Mexico City", 19.4326, -99.1332),
    ("Dubai", 25.2048, 55.2708),
];

/// Subset US city dùng làm fallback an toàn khi OS locale không map được
/// (port usCityNames): giữ bộ ba geo↔locale↔en-US nhất quán nội bộ.
const US_CITY_NAMES: &[&str] = &[
    "New York",
    "Los Angeles",
    "Chicago",
    "Houston",
    "San Francisco",
    "Miami",
    "Dallas",
    "Seattle",
    "Denver",
    "Phoenix",
    "Atlanta",
    "Boston",
];

fn city_coords(name: &str) -> Option<(f64, f64)> {
    CITY_GEOS
        .iter()
        .find(|(c, _, _)| *c == name)
        .map(|(_, lat, lon)| (*lat, *lon))
}

/// Chuẩn hoá OS locale thô ("vi_VN.UTF-8", "en-US@calendar=gregorian", "vi")
/// về BCP-47 ("vi-VN", "en-US", "vi"). Trả None khi rỗng/không parse được
/// (port normalizeOSLocale từ os_locale.go — giữ base + subtag đầu tiên).
pub fn normalize_os_locale(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if s.is_empty() || s == "C" || s == "POSIX" {
        return None;
    }
    if let Some(i) = s.find('.') {
        s = &s[..i];
    }
    if let Some(i) = s.find('@') {
        s = &s[..i];
    }
    let s = s.replace('_', "-");
    let mut parts = s.split('-');
    let base = parts.next().unwrap_or("").to_ascii_lowercase();
    let base_ok = (2..=3).contains(&base.len()) && base.bytes().all(|b| b.is_ascii_lowercase());
    if !base_ok {
        return None;
    }
    match parts.next().filter(|p| !p.is_empty()) {
        None => Some(base),
        // Region 2 chữ → UPPER ("vn"→"VN"); script 4 chữ → Titlecase ("hant"→"Hant").
        Some(sub) => {
            let canon = match sub.len() {
                2 => sub.to_ascii_uppercase(),
                4 if sub.is_ascii() => {
                    let mut c = sub.to_ascii_lowercase();
                    c[..1].make_ascii_uppercase();
                    c
                }
                _ => sub.to_string(),
            };
            Some(format!("{base}-{canon}"))
        }
    }
}

/// OS locale (đã chuẩn hoá) → tên city trong CITY_GEOS. Thử exact tag trước,
/// rồi base language ("fr-CA" → "fr" → Paris). Port localeToCityName.
fn locale_city(normalized: &str) -> Option<&'static str> {
    fn by_tag(tag: &str) -> Option<&'static str> {
        Some(match tag {
            "vi" | "vi-vn" => "Ho Chi Minh City",
            "en" | "en-us" => "New York",
            "en-gb" => "London",
            "en-au" => "Sydney",
            "en-ca" => "Toronto",
            "ja" | "ja-jp" => "Tokyo",
            "zh" | "zh-cn" | "zh-sg" | "zh-hans" => "Singapore",
            "zh-tw" | "zh-hant" => "Taipei",
            "zh-hk" => "Hong Kong",
            "ko" | "ko-kr" => "Seoul",
            "th" | "th-th" => "Bangkok",
            "id" | "id-id" => "Jakarta",
            "de" | "de-de" => "Berlin",
            "de-at" => "Vienna",
            "de-ch" => "Zurich",
            "fr" | "fr-fr" => "Paris",
            "es" | "es-es" => "Madrid",
            "es-mx" => "Mexico City",
            "it" | "it-it" => "Rome",
            "nl" | "nl-nl" => "Amsterdam",
            "pl" | "pl-pl" => "Warsaw",
            "sv" | "sv-se" => "Stockholm",
            "pt" | "pt-br" | "pt-pt" => "São Paulo",
            "ar" | "ar-ae" => "Dubai",
            "hi" | "hi-in" => "Mumbai",
            "cs" | "cs-cz" => "Prague",
            _ => return None,
        })
    }
    let s = normalized.to_ascii_lowercase();
    by_tag(&s).or_else(|| s.split('-').next().and_then(by_tag))
}

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// PRNG splitmix64 — deterministic, không phụ thuộc crate `rand` (rand không
/// cam kết stream ổn định giữa version; ở đây cần cùng seed → cùng toạ độ mãi).
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform trong (0, 1] — không bao giờ 0 để ln() an toàn.
    fn next_unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) + 1) as f64 / (1u64 << 53) as f64
    }

    /// Jitter Gaussian (Box-Muller) stddev=0.05°, clamp ±0.15° — cùng tham số
    /// gaussianOffset của VFlowX: cụm bell-curve tự nhiên quanh tâm city.
    fn gaussian_offset(&mut self, stddev: f64, max_offset: f64) -> f64 {
        let u1 = self.next_unit();
        let u2 = self.next_unit();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        (z * stddev).clamp(-max_offset, max_offset)
    }
}

/// Lõi thuần (testable, không đọc OS): dựng GeoInfo từ OS locale + timezone
/// máy + profile id. Locale map được → city tương ứng; không map được →
/// US city chọn deterministic theo profile id. Toạ độ = tâm city + jitter
/// Gaussian (seed cố định theo profile id → launch lại không đổi).
pub fn geo_from_locale_parts(
    os_locale: Option<&str>,
    machine_tz: Option<String>,
    profile_id: &str,
) -> GeoInfo {
    let normalized = os_locale.and_then(normalize_os_locale);
    let mut rng = SplitMix64(fnv1a64(profile_id));
    // Luôn rút index fallback trước để stream jitter cố định theo profile id,
    // không phụ thuộc nhánh locale map được hay không.
    let fallback_idx = (rng.next_u64() % US_CITY_NAMES.len() as u64) as usize;
    let city = normalized
        .as_deref()
        .and_then(locale_city)
        .unwrap_or(US_CITY_NAMES[fallback_idx]);
    // Defensive: mọi city trong 2 bảng đều có toạ độ (test đảm bảo).
    let (lat, lon) = city_coords(city).unwrap_or((40.7128, -74.0060));
    let lat = lat + rng.gaussian_offset(0.05, 0.15);
    let lon = lon + rng.gaussian_offset(0.05, 0.15);
    GeoInfo {
        timezone: machine_tz
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        locale: Some(normalized.unwrap_or_else(|| "en-US".into())),
        latitude: Some(format!("{lat:.4}")),
        longitude: Some(format!("{lon:.4}")),
    }
}

/// Fallback geo cho profile KHÔNG proxy (W56b): đọc OS locale (sys-locale —
/// GUI app không có env LANG trên macOS/Windows) + timezone IANA thật của máy
/// (iana-time-zone). Offline hoàn toàn, không chặn launch.
pub fn geo_from_os_locale(profile_id: &str) -> GeoInfo {
    geo_from_locale_parts(
        sys_locale::get_locale().as_deref(),
        iana_time_zone::get_timezone().ok(),
        profile_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_locale_maps_known_and_rejects_unknown() {
        assert_eq!(country_locale("VN"), Some("vi-VN"));
        assert_eq!(country_locale(" de "), Some("de-DE"));
        assert_eq!(country_locale("XX"), None);
        assert_eq!(country_locale(""), None);
    }

    #[test]
    fn parse_geo_full_body() {
        let body = r#"{"ip":"93.184.216.34","timezone":"Europe/Berlin",
            "country_code":"DE","latitude":52.52,"longitude":13.405}"#;
        let g = parse_geo_response(body).unwrap();
        assert_eq!(g.timezone.as_deref(), Some("Europe/Berlin"));
        assert_eq!(g.locale.as_deref(), Some("de-DE"));
        assert_eq!(g.latitude.as_deref(), Some("52.52"));
        assert_eq!(g.longitude.as_deref(), Some("13.405"));
    }

    #[test]
    fn parse_geo_partial_and_invalid() {
        // Thiếu lon → cặp toạ độ bị bỏ, tz/locale vẫn dùng được.
        let g = parse_geo_response(
            r#"{"timezone":"Asia/Ho_Chi_Minh","country_code":"VN","latitude":10.8}"#,
        )
        .unwrap();
        assert_eq!(g.timezone.as_deref(), Some("Asia/Ho_Chi_Minh"));
        assert_eq!(g.locale.as_deref(), Some("vi-VN"));
        assert_eq!(g.latitude, None);
        assert_eq!(g.longitude, None);
        // Country lạ → locale None nhưng tz vẫn có.
        let g = parse_geo_response(r#"{"timezone":"UTC","country_code":"XX"}"#).unwrap();
        assert_eq!(g.locale, None);
        // Body lỗi / không có field hữu ích / rate-limit → None.
        assert_eq!(
            parse_geo_response(r#"{"error":true,"reason":"quota"}"#),
            None
        );
        assert_eq!(parse_geo_response(r#"{"ip":"1.2.3.4"}"#), None);
        assert_eq!(parse_geo_response("<html>oops</html>"), None);
        assert_eq!(parse_geo_response(""), None);
    }

    #[test]
    fn needs_geoip_only_when_enabled_and_fields_missing() {
        let mut p = crate::launcher::test_profile();
        // geoip=false → không bao giờ resolve.
        assert!(!profile_needs_geoip(&p));
        // geoip=true + mọi field trống → cần resolve.
        p.geoip = true;
        assert!(profile_needs_geoip(&p));
        // Đủ tz+locale+geo manual → khỏi gọi mạng.
        p.timezone = Some("Asia/Ho_Chi_Minh".into());
        p.locale = Some("vi-VN".into());
        p.geolocation_mode = "manual".into();
        p.geo_latitude = Some("10.8".into());
        p.geo_longitude = Some("106.6".into());
        assert!(!profile_needs_geoip(&p));
        // Chỉ cần 1 field trống lại (locale) → cần resolve.
        p.locale = Some("  ".into());
        assert!(profile_needs_geoip(&p));
    }

    #[tokio::test]
    async fn resolve_geo_invalid_proxy_returns_none() {
        assert_eq!(resolve_geo("not a url").await, None);
    }

    /// Proxy treo (nhận kết nối nhưng không bao giờ trả lời): tổng thời gian
    /// resolve phải bị chặn ở ~budget. Không có bound tổng, riêng 2 IP-echo
    /// treo đã tốn ≥ 2×budget (mỗi request timeout riêng = budget).
    #[tokio::test]
    async fn resolve_geo_hanging_proxy_bounded_by_total_budget() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                held.push(stream); // giữ kết nối mở, không phản hồi gì
            }
        });
        let budget = Duration::from_millis(300);
        let started = std::time::Instant::now();
        let got = resolve_geo_with(
            &format!("http://{addr}"),
            &["http://10.255.255.1/a", "http://10.255.255.1/b"],
            "http://10.255.255.1/{ip}",
            budget,
        )
        .await;
        let elapsed = started.elapsed();
        assert_eq!(got, None);
        assert!(
            elapsed < budget * 2,
            "resolve mất {elapsed:?} — vượt bound tổng {budget:?}"
        );
    }

    // ------------------------------------------------------------------
    // (W56b) No-proxy geo fallback từ OS locale
    // ------------------------------------------------------------------

    #[test]
    fn normalize_os_locale_handles_common_raw_forms() {
        assert_eq!(normalize_os_locale("vi_VN.UTF-8").as_deref(), Some("vi-VN"));
        assert_eq!(
            normalize_os_locale("en-US@calendar=gregorian").as_deref(),
            Some("en-US")
        );
        assert_eq!(normalize_os_locale(" vi ").as_deref(), Some("vi"));
        assert_eq!(normalize_os_locale("zh_hant_TW").as_deref(), Some("zh-Hant"));
        assert_eq!(normalize_os_locale("EN-us").as_deref(), Some("en-US"));
        assert_eq!(normalize_os_locale(""), None);
        assert_eq!(normalize_os_locale("C"), None);
        assert_eq!(normalize_os_locale("POSIX"), None);
        assert_eq!(normalize_os_locale("1234"), None);
    }

    /// Mọi city trong bảng locale→city lẫn fallback US đều phải có toạ độ —
    /// bảo vệ nhánh defensive unwrap_or trong geo_from_locale_parts.
    #[test]
    fn all_mapped_cities_have_coordinates() {
        for tag in [
            "vi", "en", "en-gb", "en-au", "en-ca", "ja", "zh", "zh-tw", "zh-hk", "ko", "th",
            "id", "de", "de-at", "de-ch", "fr", "es", "es-mx", "it", "nl", "pl", "sv", "pt",
            "ar", "hi", "cs",
        ] {
            let city = locale_city(tag).unwrap();
            assert!(city_coords(city).is_some(), "thiếu toạ độ cho {city}");
        }
        for c in US_CITY_NAMES {
            assert!(city_coords(c).is_some(), "US fallback thiếu toạ độ {c}");
        }
    }

    #[test]
    fn locale_maps_to_city_with_base_language_fallback() {
        let g = geo_from_locale_parts(Some("vi_VN.UTF-8"), None, "p1");
        // HCMC (10.7626, 106.6602) ± 0.15.
        let lat: f64 = g.latitude.as_deref().unwrap().parse().unwrap();
        let lon: f64 = g.longitude.as_deref().unwrap().parse().unwrap();
        assert!((lat - 10.7626).abs() <= 0.15, "lat {lat} lệch quá jitter");
        assert!((lon - 106.6602).abs() <= 0.15, "lon {lon} lệch quá jitter");
        assert_eq!(g.locale.as_deref(), Some("vi-VN"));
        // "fr-CA" không có tag riêng → base "fr" → Paris.
        let g = geo_from_locale_parts(Some("fr-CA"), None, "p1");
        let lat: f64 = g.latitude.as_deref().unwrap().parse().unwrap();
        assert!((lat - 48.8566).abs() <= 0.15);
        assert_eq!(g.locale.as_deref(), Some("fr-CA"));
    }

    #[test]
    fn unmapped_locale_falls_back_to_us_city_and_missing_locale_to_en_us() {
        // "sw-KE" không map → toạ độ phải thuộc 1 US city (±0.15).
        let g = geo_from_locale_parts(Some("sw-KE"), None, "profile-x");
        let lat: f64 = g.latitude.as_deref().unwrap().parse().unwrap();
        let lon: f64 = g.longitude.as_deref().unwrap().parse().unwrap();
        assert!(
            US_CITY_NAMES.iter().any(|c| {
                let (clat, clon) = city_coords(c).unwrap();
                (lat - clat).abs() <= 0.15 && (lon - clon).abs() <= 0.15
            }),
            "({lat},{lon}) không gần US city nào"
        );
        // Locale OS giữ nguyên (không ép en-US khi detect được).
        assert_eq!(g.locale.as_deref(), Some("sw-KE"));
        // Không detect được locale → fallback en-US.
        let g = geo_from_locale_parts(None, None, "profile-x");
        assert_eq!(g.locale.as_deref(), Some("en-US"));
    }

    /// Timezone là timezone THẬT của máy, truyền qua nguyên vẹn — KHÔNG suy
    /// từ city (locale vi-VN nhưng máy ở Berlin → giữ Europe/Berlin).
    #[test]
    fn machine_timezone_passes_through_untouched() {
        let g = geo_from_locale_parts(Some("vi-VN"), Some("Europe/Berlin".into()), "p1");
        assert_eq!(g.timezone.as_deref(), Some("Europe/Berlin"));
        // Không có tz máy → None (browser tự dùng tz thật — vẫn đúng).
        let g = geo_from_locale_parts(Some("vi-VN"), None, "p1");
        assert_eq!(g.timezone, None);
        // Tz rỗng/trắng → None.
        let g = geo_from_locale_parts(Some("vi-VN"), Some("  ".into()), "p1");
        assert_eq!(g.timezone, None);
    }

    #[test]
    fn deterministic_per_profile_id() {
        // Cùng profile id → GeoInfo giống hệt (không nhảy city/toạ độ).
        let a = geo_from_locale_parts(Some("de-DE"), Some("Europe/Berlin".into()), "prof-a");
        let b = geo_from_locale_parts(Some("de-DE"), Some("Europe/Berlin".into()), "prof-a");
        assert_eq!(a, b);
        // Khác profile id → jitter khác (cùng city Berlin).
        let c = geo_from_locale_parts(Some("de-DE"), Some("Europe/Berlin".into()), "prof-b");
        assert_ne!(a.latitude, c.latitude);
    }

    /// geo_from_os_locale (đọc OS thật) không panic và luôn có đủ toạ độ +
    /// locale — smoke test, giá trị cụ thể phụ thuộc máy chạy test.
    #[test]
    fn geo_from_os_locale_always_yields_coordinates_and_locale() {
        let g = geo_from_os_locale("smoke-profile");
        assert!(g.latitude.is_some() && g.longitude.is_some());
        assert!(g.locale.is_some());
        assert_eq!(g, geo_from_os_locale("smoke-profile"));
    }
}
