//! Proxy check (W19b): kết nối QUA proxy tới endpoint IP-echo để xác minh proxy
//! sống, đo latency và lấy external IP + country (best-effort).
//!
//! Không log credential plaintext — proxy URL chỉ tồn tại trong RAM của request.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{AppError, Result};

/// Timeout tổng cho 1 lần gọi IP-echo qua proxy.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout riêng cho lookup country (best-effort, lỗi thì bỏ qua).
const COUNTRY_TIMEOUT: Duration = Duration::from_secs(5);

/// Kết quả check trả về FE. `error` chỉ chứa message loại lỗi, KHÔNG chứa credential.
#[derive(Debug, Clone, Serialize)]
pub struct ProxyCheckResult {
    pub ok: bool,
    pub external_ip: Option<String>,
    pub country: Option<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// Percent-encode 1 thành phần userinfo trong URL (giữ unreserved RFC 3986).
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Dựng proxy URL `protocol://[user[:pass]@]host:port` với credential đã
/// percent-encode (khác `proxy_url_from` của launcher: reqwest parse URL nên
/// ký tự đặc biệt trong user/pass phải được escape).
pub fn build_proxy_url(
    protocol: &str,
    host: &str,
    port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<String> {
    let protocol = protocol.trim().to_ascii_lowercase();
    if !matches!(protocol.as_str(), "http" | "https" | "socks5") {
        return Err(AppError::InvalidInput(format!(
            "unsupported proxy protocol: {protocol}"
        )));
    }
    let host = host.trim();
    if host.is_empty() {
        return Err(AppError::InvalidInput("proxy host is empty".into()));
    }
    if port == 0 {
        return Err(AppError::InvalidInput("proxy port is 0".into()));
    }
    let auth = match (username, password) {
        (Some(u), Some(p)) => format!("{}:{}@", encode_component(u), encode_component(p)),
        (Some(u), None) => format!("{}@", encode_component(u)),
        _ => String::new(),
    };
    Ok(format!("{protocol}://{auth}{host}:{port}"))
}

/// Parse body từ endpoint IP-echo: JSON `{"ip":"1.2.3.4"}` (ipify) hoặc
/// plain-text (ifconfig.me). Trả None nếu không nhận ra IP.
pub fn parse_ip_response(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(ip) = v.get("ip").and_then(|x| x.as_str()) {
            return Some(ip.trim().to_string());
        }
    }
    // Plain text: chấp nhận nếu ngắn và không chứa khoảng trắng (tránh nuốt HTML lỗi).
    if trimmed.len() <= 45 && !trimmed.contains(char::is_whitespace) {
        return Some(trimmed.to_string());
    }
    None
}

/// Client reqwest route MỌI request qua `proxy_url`.
fn proxied_client(proxy_url: &str, timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(proxy_url)?)
        .timeout(timeout)
        .build()?)
}

/// GET `url` qua client, trả body text nếu HTTP 2xx.
async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    Ok(client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// Check proxy: IP-echo chính là ipify (JSON), fallback ifconfig.me (text).
/// Latency = thời gian của request thành công. Country best-effort qua ipapi.co.
pub async fn check_proxy_url(proxy_url: &str) -> ProxyCheckResult {
    let fail = |msg: String| ProxyCheckResult {
        ok: false,
        external_ip: None,
        country: None,
        latency_ms: None,
        error: Some(msg),
    };
    let client = match proxied_client(proxy_url, CHECK_TIMEOUT) {
        Ok(c) => c,
        Err(e) => return fail(e.to_string()),
    };

    let mut last_err = String::new();
    let mut found: Option<(String, u64)> = None;
    for url in ["https://api.ipify.org?format=json", "https://ifconfig.me/ip"] {
        let started = Instant::now();
        match fetch_text(&client, url).await {
            Ok(body) => match parse_ip_response(&body) {
                Some(ip) => {
                    found = Some((ip, started.elapsed().as_millis() as u64));
                    break;
                }
                None => last_err = format!("unrecognized response from {url}"),
            },
            Err(e) => last_err = e.to_string(),
        }
    }
    let Some((ip, latency_ms)) = found else {
        return fail(last_err);
    };

    // Country best-effort — đi qua chính proxy để phản ánh ngữ cảnh exit IP.
    let country = match proxied_client(proxy_url, COUNTRY_TIMEOUT) {
        Ok(c) => fetch_text(&c, &format!("https://ipapi.co/{ip}/country_name/"))
            .await
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.contains("error")),
        Err(_) => None,
    };

    ProxyCheckResult {
        ok: true,
        external_ip: Some(ip),
        country,
        latency_ms: Some(latency_ms),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_without_credentials() {
        assert_eq!(
            build_proxy_url("http", "1.2.3.4", 8080, None, None).unwrap(),
            "http://1.2.3.4:8080"
        );
        assert_eq!(
            build_proxy_url("SOCKS5", " proxy.example.com ", 1080, None, None).unwrap(),
            "socks5://proxy.example.com:1080"
        );
    }

    #[test]
    fn build_url_encodes_credentials() {
        assert_eq!(
            build_proxy_url("socks5", "h", 1080, Some("us er"), Some("p@ss:w/d")).unwrap(),
            "socks5://us%20er:p%40ss%3Aw%2Fd@h:1080"
        );
        assert_eq!(
            build_proxy_url("http", "h", 80, Some("user"), None).unwrap(),
            "http://user@h:80"
        );
    }

    #[test]
    fn build_url_rejects_bad_input() {
        assert!(build_proxy_url("ftp", "h", 80, None, None).is_err());
        assert!(build_proxy_url("http", "  ", 80, None, None).is_err());
        assert!(build_proxy_url("http", "h", 0, None, None).is_err());
    }

    #[test]
    fn parse_ip_json_and_plaintext() {
        assert_eq!(
            parse_ip_response(r#"{"ip":"93.184.216.34"}"#).as_deref(),
            Some("93.184.216.34")
        );
        assert_eq!(
            parse_ip_response("  93.184.216.34\n").as_deref(),
            Some("93.184.216.34")
        );
        assert_eq!(
            parse_ip_response("2606:2800:220:1:248:1893:25c8:1946").as_deref(),
            Some("2606:2800:220:1:248:1893:25c8:1946")
        );
        assert_eq!(parse_ip_response(""), None);
        assert_eq!(parse_ip_response("<html>error page</html>"), None);
        assert_eq!(parse_ip_response(r#"{"msg":"no ip"}"#), None);
    }

    /// Test mạng thật — chạy tay: `cargo test --lib proxy_check -- --ignored`.
    #[tokio::test]
    #[ignore = "requires network + a live local proxy"]
    async fn check_proxy_url_against_live_proxy() {
        let res = check_proxy_url("http://127.0.0.1:8080").await;
        println!("{res:?}");
    }

    #[tokio::test]
    async fn check_proxy_url_invalid_url_fails_fast() {
        let res = check_proxy_url("not a url").await;
        assert!(!res.ok);
        assert!(res.error.is_some());
        assert!(res.external_ip.is_none());
    }
}
