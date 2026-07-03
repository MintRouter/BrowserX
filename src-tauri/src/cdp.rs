//! CDP client (W4): attach/verify endpoint + automation goto/eval qua chromiumoxide.
//!
//! - `attach(port)`  — chờ `GET /json/version` sẵn sàng (retry), verify có
//!   `webSocketDebuggerUrl`, trả `http://127.0.0.1:<port>`.
//! - `goto(port, url)` — connect ws, navigate tab đầu tiên (tạo tab nếu chưa có).
//! - `eval(port, js)`  — connect ws, `Runtime.evaluate` trên tab đầu tiên, trả JSON.
//! - (W24a) `get_all_cookies`/`set_cookies` — Storage.getCookies/setCookies ở
//!   browser-level; `close_browser` — Browser.close (shutdown mềm, flush profile).
//!
//! Connect qua `Browser::connect(ws_url)`: browser KHÔNG do chromiumoxide spawn
//! (`child = None`) nên drop `Browser` chỉ ngắt kết nối, không kill tiến trình.
//! Handler-loop được spawn làm task riêng và abort sau khi xong.

use std::time::Duration;

use chromiumoxide::cdp::browser_protocol::browser::CloseParams;
use chromiumoxide::cdp::browser_protocol::network::{CookieParam, CookieSameSite, TimeSinceEpoch};
use chromiumoxide::cdp::browser_protocol::storage::{GetCookiesParams, SetCookiesParams};
use chromiumoxide::{Browser, Page};
use futures::StreamExt;

use crate::cookies::CookieItem;
use crate::error::{AppError, Result};

const ATTACH_RETRIES: u32 = 40;
const ATTACH_DELAY: Duration = Duration::from_millis(250);
/// Timeout tổng cho một thao tác automation (goto/eval), tránh treo vô hạn.
const OP_TIMEOUT: Duration = Duration::from_secs(20);

/// Lấy `webSocketDebuggerUrl` từ body `/json/version` (chỉ nhận scheme ws/wss).
fn parse_ws_url(body: &serde_json::Value) -> Option<&str> {
    body.get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("ws://") || s.starts_with("wss://"))
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?)
}

/// Chờ CDP endpoint tại `127.0.0.1:<port>` sẵn sàng (retry tối đa ~10s),
/// verify `/json/version` có `webSocketDebuggerUrl`, trả `cdp_url`
/// dạng `http://127.0.0.1:<port>`.
pub async fn attach(port: u16) -> Result<String> {
    let base = format!("http://127.0.0.1:{port}");
    let url = format!("{base}/json/version");
    let client = http_client()?;

    let mut last_err = String::from("chưa nhận được phản hồi");
    for _ in 0..ATTACH_RETRIES {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                if parse_ws_url(&body).is_some() {
                    return Ok(base);
                }
                last_err = "thiếu webSocketDebuggerUrl trong /json/version".into();
            }
            Ok(resp) => last_err = format!("HTTP {}", resp.status()),
            Err(e) => last_err = e.to_string(),
        }
        tokio::time::sleep(ATTACH_DELAY).await;
    }
    Err(AppError::Cdp(format!(
        "không attach được CDP tại {url} sau {ATTACH_RETRIES} lần thử: {last_err}"
    )))
}

/// (W24c) Lấy `webSocketDebuggerUrl` (`ws://127.0.0.1:<port>/devtools/browser/…`)
/// từ `/json/version` của phiên trên `port` — dùng cho "Copy CDP URL" và connect ws.
pub async fn ws_url(port: u16) -> Result<String> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let body: serde_json::Value = http_client()?.get(&url).send().await?.json().await?;
    parse_ws_url(&body)
        .map(str::to_string)
        .ok_or_else(|| AppError::Cdp(format!("thiếu webSocketDebuggerUrl tại {url}")))
}

/// Phiên CDP đã connect: giữ `Browser` + task poll handler-loop.
struct CdpSession {
    browser: Browser,
    handler_task: tokio::task::JoinHandle<()>,
}

impl CdpSession {
    async fn connect(port: u16) -> Result<Self> {
        let ws = ws_url(port).await?;

        let (browser, mut handler) = Browser::connect(ws)
            .await
            .map_err(|e| AppError::Cdp(format!("connect ws thất bại: {e}")))?;
        let handler_task = tokio::spawn(async move {
            while let Some(ev) = handler.next().await {
                if ev.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            browser,
            handler_task,
        })
    }

    /// Tab đầu tiên đang mở; tạo `about:blank` nếu chưa có tab nào.
    async fn first_page(&self) -> Result<Page> {
        let pages = self
            .browser
            .pages()
            .await
            .map_err(|e| AppError::Cdp(format!("liệt kê pages thất bại: {e}")))?;
        match pages.into_iter().next() {
            Some(p) => Ok(p),
            None => self
                .browser
                .new_page("about:blank")
                .await
                .map_err(|e| AppError::Cdp(format!("tạo page thất bại: {e}"))),
        }
    }

    /// Ngắt kết nối: drop Browser (không kill tiến trình vì không phải child) + abort handler.
    fn disconnect(self) {
        drop(self.browser);
        self.handler_task.abort();
    }
}

/// Navigate tab đầu tiên của phiên trên `port` tới `url`.
pub async fn goto(port: u16, url: &str) -> Result<()> {
    let session = CdpSession::connect(port).await?;
    let result = tokio::time::timeout(OP_TIMEOUT, async {
        let page = session.first_page().await?;
        page.goto(url)
            .await
            .map_err(|e| AppError::Cdp(format!("goto {url} thất bại: {e}")))?;
        Ok(())
    })
    .await
    .unwrap_or_else(|_| {
        Err(AppError::Cdp(format!(
            "goto {url} timeout sau {OP_TIMEOUT:?}"
        )))
    });
    session.disconnect();
    result
}

/// `Runtime.evaluate` biểu thức `js` trên tab đầu tiên, trả về giá trị JSON (Null nếu không có).
pub async fn eval(port: u16, js: &str) -> Result<serde_json::Value> {
    let session = CdpSession::connect(port).await?;
    let result = tokio::time::timeout(OP_TIMEOUT, async {
        let page = session.first_page().await?;
        let res = page
            .evaluate(js)
            .await
            .map_err(|e| AppError::Cdp(format!("eval thất bại: {e}")))?;
        Ok(res.value().cloned().unwrap_or(serde_json::Value::Null))
    })
    .await
    .unwrap_or_else(|_| Err(AppError::Cdp(format!("eval timeout sau {OP_TIMEOUT:?}"))));
    session.disconnect();
    result
}

/// (W20a) `Page.bringToFront` trên tab đầu tiên — kích hoạt tab và đưa cửa sổ
/// browser lên trước (Chromium headful trên macOS/Windows/Linux đều raise window).
pub async fn bring_to_front(port: u16) -> Result<()> {
    let session = CdpSession::connect(port).await?;
    let result = tokio::time::timeout(OP_TIMEOUT, async {
        let page = session.first_page().await?;
        page.bring_to_front()
            .await
            .map_err(|e| AppError::Cdp(format!("bringToFront thất bại: {e}")))?;
        Ok(())
    })
    .await
    .unwrap_or_else(|_| {
        Err(AppError::Cdp(format!(
            "bringToFront timeout sau {OP_TIMEOUT:?}"
        )))
    });
    session.disconnect();
    result
}

/// Chuyển CDP Cookie → CookieItem trung lập. `expires` -1 nghĩa là session cookie.
fn cookie_to_item(c: &chromiumoxide::cdp::browser_protocol::network::Cookie) -> CookieItem {
    CookieItem {
        name: c.name.clone(),
        value: c.value.clone(),
        domain: c.domain.clone(),
        path: c.path.clone(),
        expires: (c.expires > 0.0).then_some(c.expires),
        http_only: c.http_only,
        secure: c.secure,
        same_site: c.same_site.as_ref().map(|s| s.as_ref().to_string()),
    }
}

/// Chuyển CookieItem → CDP CookieParam (dùng cho Storage.setCookies).
fn item_to_param(c: &CookieItem) -> Result<CookieParam> {
    let mut b = CookieParam::builder()
        .name(&c.name)
        .value(&c.value)
        .domain(&c.domain)
        .path(&c.path)
        .secure(c.secure)
        .http_only(c.http_only);
    if let Some(e) = c.expires {
        b = b.expires(TimeSinceEpoch::new(e));
    }
    match c.same_site.as_deref() {
        Some("Strict") => b = b.same_site(CookieSameSite::Strict),
        Some("Lax") => b = b.same_site(CookieSameSite::Lax),
        Some("None") => b = b.same_site(CookieSameSite::None),
        _ => {}
    }
    b.build()
        .map_err(|e| AppError::InvalidInput(format!("invalid cookie {:?}: {e}", c.name)))
}

/// (W24a) Lấy TOÀN BỘ cookie của phiên trên `port` qua `Storage.getCookies`
/// (browser-level, gồm mọi domain — khác Network.getCookies vốn theo page).
pub async fn get_all_cookies(port: u16) -> Result<Vec<CookieItem>> {
    let session = CdpSession::connect(port).await?;
    let result = tokio::time::timeout(OP_TIMEOUT, async {
        let resp = session
            .browser
            .execute(GetCookiesParams::default())
            .await
            .map_err(|e| AppError::Cdp(format!("Storage.getCookies thất bại: {e}")))?;
        Ok(resp.result.cookies.iter().map(cookie_to_item).collect())
    })
    .await
    .unwrap_or_else(|_| {
        Err(AppError::Cdp(format!(
            "getCookies timeout sau {OP_TIMEOUT:?}"
        )))
    });
    session.disconnect();
    result
}

/// (W24a) Ghi danh sách cookie vào phiên trên `port` qua `Storage.setCookies`.
/// Trả về số cookie đã gửi. Cookie trùng (domain,path,name) bị ghi đè.
pub async fn set_cookies(port: u16, items: &[CookieItem]) -> Result<usize> {
    let cookies: Vec<CookieParam> = items.iter().map(item_to_param).collect::<Result<_>>()?;
    let count = cookies.len();
    let session = CdpSession::connect(port).await?;
    let result = tokio::time::timeout(OP_TIMEOUT, async {
        session
            .browser
            .execute(SetCookiesParams {
                cookies,
                browser_context_id: None,
            })
            .await
            .map_err(|e| AppError::Cdp(format!("Storage.setCookies thất bại: {e}")))?;
        Ok(count)
    })
    .await
    .unwrap_or_else(|_| {
        Err(AppError::Cdp(format!(
            "setCookies timeout sau {OP_TIMEOUT:?}"
        )))
    });
    session.disconnect();
    result
}

/// (W24a) `Browser.close` — shutdown MỀM: Chromium flush cookie/profile xuống đĩa
/// rồi tự thoát (khác kill SIGKILL vốn có thể mất cookie chưa commit).
pub async fn close_browser(port: u16) -> Result<()> {
    let session = CdpSession::connect(port).await?;
    let result = tokio::time::timeout(OP_TIMEOUT, async {
        session
            .browser
            .execute(CloseParams::default())
            .await
            .map_err(|e| AppError::Cdp(format!("Browser.close thất bại: {e}")))?;
        Ok(())
    })
    .await
    .unwrap_or_else(|_| {
        Err(AppError::Cdp(format!(
            "Browser.close timeout sau {OP_TIMEOUT:?}"
        )))
    });
    session.disconnect();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Không có endpoint CDP tại port → bring_to_front phải trả lỗi Cdp (fail
    /// nhanh, không panic). Port 1 luôn đóng với user thường.
    #[tokio::test]
    async fn bring_to_front_fails_without_endpoint() {
        let err = bring_to_front(1).await.unwrap_err();
        assert!(matches!(err, AppError::Cdp(_) | AppError::Http(_)));
    }

    /// (W24c) Không có endpoint CDP tại port → ws_url trả lỗi (không panic).
    #[tokio::test]
    async fn ws_url_fails_without_endpoint() {
        let err = ws_url(1).await.unwrap_err();
        assert!(matches!(err, AppError::Cdp(_) | AppError::Http(_)));
    }

    #[test]
    fn parse_ws_url_ok() {
        let body = json!({
            "Browser": "Chrome/131.0.0.0",
            "webSocketDebuggerUrl": "ws://127.0.0.1:9222/devtools/browser/abc-123"
        });
        assert_eq!(
            parse_ws_url(&body),
            Some("ws://127.0.0.1:9222/devtools/browser/abc-123")
        );
    }

    #[test]
    fn parse_ws_url_missing_or_invalid() {
        // Thiếu field.
        assert_eq!(parse_ws_url(&json!({ "Browser": "Chrome" })), None);
        // Sai kiểu.
        assert_eq!(parse_ws_url(&json!({ "webSocketDebuggerUrl": 42 })), None);
        // Sai scheme (không phải ws/wss).
        assert_eq!(
            parse_ws_url(&json!({ "webSocketDebuggerUrl": "http://127.0.0.1:9222/x" })),
            None
        );
        // wss hợp lệ.
        assert_eq!(
            parse_ws_url(&json!({ "webSocketDebuggerUrl": "wss://h/devtools/browser/1" })),
            Some("wss://h/devtools/browser/1")
        );
    }
}
