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
}
