//! Cookie import/export (W24a) — parser JSON ↔ Netscape, KHÔNG đụng SQLite/os_crypt.
//!
//! - `CookieItem` là shape trung lập (camelCase JSON, tương thích Cookie-Editor/
//!   EditThisCookie: nhận cả `expirationDate`, sameSite kiểu "no_restriction"…).
//! - `parse(input)` auto-detect: bắt đầu bằng `[`/`{` → JSON, ngược lại Netscape.
//! - Netscape: 7 cột tab-separated `domain⇥subdomains⇥path⇥secure⇥expires⇥name⇥value`,
//!   hỗ trợ prefix `#HttpOnly_` (convention của curl/wget); expires 0 = session cookie.
//! - Chuyển đổi sang/từ CDP (Storage.getCookies/setCookies) nằm ở `cdp.rs`.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

/// Header chuẩn của file cookie Netscape.
const NETSCAPE_HEADER: &str = "# Netscape HTTP Cookie File";
/// Prefix đánh dấu cookie HttpOnly trong file Netscape (convention curl).
const HTTPONLY_PREFIX: &str = "#HttpOnly_";

/// Format cookie hỗ trợ export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Netscape,
}

impl Format {
    /// Tên format dạng chuỗi (cho audit log/FE).
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Netscape => "netscape",
        }
    }

    /// Parse chuỗi format từ FE ("json" | "netscape").
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Format::Json),
            "netscape" => Ok(Format::Netscape),
            other => Err(AppError::InvalidInput(format!(
                "unsupported cookie format {other:?} (expected \"json\" or \"netscape\")"
            ))),
        }
    }
}

/// Một cookie ở dạng trung lập giữa CDP, JSON export và Netscape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieItem {
    pub name: String,
    pub value: String,
    pub domain: String,
    #[serde(default = "default_path")]
    pub path: String,
    /// Giây kể từ UNIX epoch; None = session cookie. Nhận cả `expirationDate`
    /// (Cookie-Editor/EditThisCookie) khi import JSON.
    #[serde(default, alias = "expirationDate", skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default)]
    pub secure: bool,
    /// "Strict" | "Lax" | "None" (đã chuẩn hoá). None = không chỉ định.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
}

fn default_path() -> String {
    "/".into()
}

/// (W33b) Một entry localStorage trong storage_state (shape Playwright:
/// `{ "name": …, "value": … }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalStorageEntry {
    pub name: String,
    pub value: String,
}

/// (W33b) localStorage của một origin trong storage_state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginState {
    pub origin: String,
    #[serde(default)]
    pub local_storage: Vec<LocalStorageEntry>,
}

/// (W33b) Full storage_state = cookies + localStorage theo origin — cùng shape
/// với Playwright `storageState()` để quen thuộc/tương thích. `origins` rỗng
/// tương đương export cookie-only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageState {
    #[serde(default)]
    pub cookies: Vec<CookieItem>,
    #[serde(default)]
    pub origins: Vec<OriginState>,
}

/// Chuẩn hoá sameSite từ các biến thể export phổ biến về "Strict"/"Lax"/"None".
pub fn normalize_same_site(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "strict" => Some("Strict".into()),
        "lax" => Some("Lax".into()),
        "none" | "no_restriction" => Some("None".into()),
        _ => None,
    }
}

/// Loại cookie thiếu domain/name + chuẩn hoá sameSite (SameSite=None ép
/// secure=true — Chromium từ chối nếu không). Dùng chung cho `parse` và
/// `parse_storage_state`.
fn sanitize(items: &mut Vec<CookieItem>) {
    items.retain(|c| !c.domain.trim().is_empty() && !c.name.trim().is_empty());
    for c in items {
        c.same_site = c.same_site.as_deref().and_then(normalize_same_site);
        if c.same_site.as_deref() == Some("None") {
            c.secure = true;
        }
    }
}

/// Parse chuỗi cookie, auto-detect JSON hay Netscape. Cookie thiếu domain bị
/// loại (trả về cùng số lượng bị bỏ qua).
pub fn parse(input: &str) -> Result<Vec<CookieItem>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("cookie data is empty".into()));
    }
    let mut items = if trimmed.starts_with('[') || trimmed.starts_with('{') {
        parse_json(trimmed)?
    } else {
        parse_netscape(trimmed)?
    };
    sanitize(&mut items);
    if items.is_empty() {
        return Err(AppError::InvalidInput(
            "no valid cookies found in input".into(),
        ));
    }
    Ok(items)
}

/// (W33b) Parse storage_state — BACKWARD-COMPATIBLE: nhận cả full storage_state
/// (`{cookies, origins}`), lẫn dữ liệu cookie-only cũ (JSON array, wrapped
/// `{"cookies": […]}` hay Netscape → `origins` rỗng). Cookie được sanitize như
/// `parse`; lỗi nếu không có cookie hợp lệ NÀO và cũng không có localStorage.
pub fn parse_storage_state(input: &str) -> Result<StorageState> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("storage state data is empty".into()));
    }
    if trimmed.starts_with('{') {
        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| AppError::InvalidInput(format!("invalid storage state JSON: {e}")))?;
        if value.get("cookies").is_some() || value.get("origins").is_some() {
            let mut state: StorageState = serde_json::from_value(value)
                .map_err(|e| AppError::InvalidInput(format!("invalid storage state JSON: {e}")))?;
            sanitize(&mut state.cookies);
            let has_local_storage = state.origins.iter().any(|o| !o.local_storage.is_empty());
            if state.cookies.is_empty() && !has_local_storage {
                return Err(AppError::InvalidInput(
                    "no valid cookies or localStorage entries found in input".into(),
                ));
            }
            return Ok(state);
        }
    }
    // Cookie-only (JSON array hoặc Netscape) — đường import_cookies cũ.
    Ok(StorageState {
        cookies: parse(trimmed)?,
        origins: Vec::new(),
    })
}

/// (W33b) Serialize full storage_state ra JSON (pretty, camelCase).
pub fn serialize_storage_state(state: &StorageState) -> Result<String> {
    Ok(serde_json::to_string_pretty(state)?)
}

/// JSON: mảng cookie object, hoặc object bọc `{"cookies": [...]}`.
fn parse_json(input: &str) -> Result<Vec<CookieItem>> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| AppError::InvalidInput(format!("invalid cookie JSON: {e}")))?;
    let arr = match &value {
        serde_json::Value::Array(_) => value,
        serde_json::Value::Object(map) => map
            .get("cookies")
            .cloned()
            .filter(|v| v.is_array())
            .ok_or_else(|| {
                AppError::InvalidInput(
                    "cookie JSON must be an array or an object with a \"cookies\" array".into(),
                )
            })?,
        _ => {
            return Err(AppError::InvalidInput(
                "cookie JSON must be an array of cookie objects".into(),
            ))
        }
    };
    serde_json::from_value(arr)
        .map_err(|e| AppError::InvalidInput(format!("invalid cookie JSON: {e}")))
}

/// Netscape cookies.txt: bỏ qua comment/dòng trống; chấp nhận `#HttpOnly_`.
fn parse_netscape(input: &str) -> Result<Vec<CookieItem>> {
    let mut items = Vec::new();
    for (idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        let (line, http_only) = match line.strip_prefix(HTTPONLY_PREFIX) {
            Some(rest) => (rest, true),
            None => (line, false),
        };
        if line.trim().is_empty() || (!http_only && line.trim_start().starts_with('#')) {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let fields = if fields.len() >= 7 {
            fields
        } else {
            // Fallback: file dùng space thay tab (7 cột, value không chứa space).
            line.split_whitespace().collect()
        };
        if fields.len() < 7 {
            return Err(AppError::InvalidInput(format!(
                "invalid Netscape cookie line {} (expected 7 tab-separated fields)",
                idx + 1
            )));
        }
        let expires_raw: f64 = fields[4].trim().parse().map_err(|_| {
            AppError::InvalidInput(format!("invalid expires value on line {}", idx + 1))
        })?;
        items.push(CookieItem {
            name: fields[5].trim().to_string(),
            value: fields[6..].join("\t").trim().to_string(),
            domain: fields[0].trim().to_string(),
            path: fields[2].trim().to_string(),
            expires: (expires_raw > 0.0).then_some(expires_raw),
            http_only,
            secure: fields[3].trim().eq_ignore_ascii_case("TRUE"),
            same_site: None,
        });
    }
    Ok(items)
}

/// Serialize danh sách cookie theo `format`.
pub fn serialize(items: &[CookieItem], format: Format) -> Result<String> {
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(items)?),
        Format::Netscape => Ok(to_netscape(items)),
    }
}

fn to_netscape(items: &[CookieItem]) -> String {
    let mut out = String::from(NETSCAPE_HEADER);
    out.push('\n');
    for c in items {
        let prefix = if c.http_only { HTTPONLY_PREFIX } else { "" };
        let subdomains = if c.domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let secure = if c.secure { "TRUE" } else { "FALSE" };
        let expires = c.expires.map(|e| e as i64).unwrap_or(0);
        out.push_str(&format!(
            "{prefix}{}\t{subdomains}\t{}\t{secure}\t{expires}\t{}\t{}\n",
            c.domain, c.path, c.name, c.value
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<CookieItem> {
        vec![
            CookieItem {
                name: "sid".into(),
                value: "abc123".into(),
                domain: ".example.com".into(),
                path: "/".into(),
                expires: Some(1_900_000_000.0),
                http_only: true,
                secure: true,
                same_site: Some("Lax".into()),
            },
            CookieItem {
                name: "theme".into(),
                value: "dark".into(),
                domain: "app.example.com".into(),
                path: "/settings".into(),
                expires: None,
                http_only: false,
                secure: false,
                same_site: None,
            },
        ]
    }

    #[test]
    fn json_roundtrip_preserves_all_fields() {
        let items = sample();
        let json = serialize(&items, Format::Json).unwrap();
        let parsed = parse(&json).unwrap();
        assert_eq!(parsed, items);
    }

    #[test]
    fn netscape_roundtrip_preserves_core_fields() {
        let items = sample();
        let text = serialize(&items, Format::Netscape).unwrap();
        assert!(text.starts_with(NETSCAPE_HEADER));
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        // sameSite không tồn tại trong Netscape — các field còn lại giữ nguyên.
        assert_eq!(parsed[0].name, "sid");
        assert_eq!(parsed[0].value, "abc123");
        assert_eq!(parsed[0].domain, ".example.com");
        assert_eq!(parsed[0].path, "/");
        assert_eq!(parsed[0].expires, Some(1_900_000_000.0));
        assert!(parsed[0].http_only, "#HttpOnly_ prefix must roundtrip");
        assert!(parsed[0].secure);
        // Session cookie: expires 0 → None.
        assert_eq!(parsed[1].expires, None);
        assert!(!parsed[1].http_only);
        assert!(!parsed[1].secure);
    }

    #[test]
    fn json_to_netscape_to_json_roundtrip() {
        let items = sample();
        let netscape = serialize(&items, Format::Netscape).unwrap();
        let back = parse(&netscape).unwrap();
        let json = serialize(&back, Format::Json).unwrap();
        let again = parse(&json).unwrap();
        assert_eq!(back, again);
    }

    #[test]
    fn parse_accepts_editthiscookie_style_json() {
        let input = r#"[{
            "domain": ".example.com",
            "expirationDate": 1900000000.5,
            "hostOnly": false,
            "httpOnly": true,
            "name": "sid",
            "path": "/",
            "sameSite": "no_restriction",
            "secure": false,
            "session": false,
            "value": "xyz"
        }]"#;
        let items = parse(input).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].expires, Some(1_900_000_000.5));
        assert!(items[0].http_only);
        assert_eq!(items[0].same_site.as_deref(), Some("None"));
        // SameSite=None ép secure=true (Chromium từ chối nếu không).
        assert!(items[0].secure);
    }

    #[test]
    fn parse_accepts_wrapped_cookies_object() {
        let input = r#"{"cookies":[{"name":"a","value":"1","domain":"x.com"}]}"#;
        let items = parse(input).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "/", "path defaults to /");
    }

    #[test]
    fn parse_skips_comments_and_rejects_garbage() {
        let text = "# Netscape HTTP Cookie File\n# comment\n\n.x.com\tTRUE\t/\tFALSE\t0\tn\tv\n";
        assert_eq!(parse(text).unwrap().len(), 1);

        let err = parse("not a cookie file").unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));

        let err = parse("   ").unwrap_err();
        assert!(err.to_string().contains("empty"));

        let err = parse("[]").unwrap_err();
        assert!(err.to_string().contains("no valid cookies"));
    }

    #[test]
    fn format_parse_accepts_known_values_only() {
        assert_eq!(Format::parse("json").unwrap(), Format::Json);
        assert_eq!(Format::parse(" Netscape ").unwrap(), Format::Netscape);
        assert!(Format::parse("csv").is_err());
    }

    /// (W33b) Full storage_state serialize → parse giữ nguyên cookies + origins.
    #[test]
    fn storage_state_roundtrip_preserves_cookies_and_origins() {
        let state = StorageState {
            cookies: sample(),
            origins: vec![OriginState {
                origin: "http://127.0.0.1:8080".into(),
                local_storage: vec![
                    LocalStorageEntry {
                        name: "alpha".into(),
                        value: "1".into(),
                    },
                    LocalStorageEntry {
                        name: "beta".into(),
                        value: "x y=z;".into(),
                    },
                ],
            }],
        };
        let json = serialize_storage_state(&state).unwrap();
        // Shape Playwright: key "localStorage" (camelCase).
        assert!(json.contains("\"localStorage\""));
        let parsed = parse_storage_state(&json).unwrap();
        assert_eq!(parsed, state);
    }

    /// (W33b) Backward-compat: dữ liệu cookie-only cũ (JSON array, wrapped
    /// object, Netscape) parse được thành storage_state với origins rỗng.
    #[test]
    fn parse_storage_state_accepts_cookie_only_inputs() {
        let items = sample();
        let json_array = serialize(&items, Format::Json).unwrap();
        let st = parse_storage_state(&json_array).unwrap();
        assert_eq!(st.cookies, items);
        assert!(st.origins.is_empty());

        let wrapped = r#"{"cookies":[{"name":"a","value":"1","domain":"x.com"}]}"#;
        let st = parse_storage_state(wrapped).unwrap();
        assert_eq!(st.cookies.len(), 1);
        assert!(st.origins.is_empty());

        let netscape = serialize(&items, Format::Netscape).unwrap();
        let st = parse_storage_state(&netscape).unwrap();
        assert_eq!(st.cookies.len(), 2);
        assert!(st.origins.is_empty());
    }

    /// (W33b) storage_state chỉ có localStorage (không cookie) vẫn hợp lệ;
    /// rỗng cả hai → lỗi.
    #[test]
    fn parse_storage_state_localstorage_only_and_empty() {
        let ls_only = r#"{"cookies":[],"origins":[{"origin":"http://a","localStorage":[{"name":"k","value":"v"}]}]}"#;
        let st = parse_storage_state(ls_only).unwrap();
        assert!(st.cookies.is_empty());
        assert_eq!(st.origins[0].local_storage[0].name, "k");

        let err = parse_storage_state(r#"{"cookies":[],"origins":[]}"#).unwrap_err();
        assert!(err.to_string().contains("no valid cookies or localStorage"));

        let err = parse_storage_state("   ").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    /// (W33b) Cookie trong storage_state được sanitize như `parse` (sameSite
    /// chuẩn hoá, None → secure).
    #[test]
    fn parse_storage_state_sanitizes_cookies() {
        let input = r#"{"cookies":[{"name":"t","value":"1","domain":".x.com","sameSite":"no_restriction","secure":false}],"origins":[]}"#;
        let st = parse_storage_state(input).unwrap();
        assert_eq!(st.cookies[0].same_site.as_deref(), Some("None"));
        assert!(st.cookies[0].secure);
    }
}
