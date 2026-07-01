//! CDP client — Wave 3a giữ TỐI GIẢN: verify/attach endpoint CDP của phiên
//! vừa spawn qua `GET /json/version` (reqwest, retry).
//!
//! Ghi chú W4: automation sâu hơn (goto/eval) qua chromiumoxide
//! `Browser::connect(ws_url)` cần spawn handler-loop poll stream riêng —
//! để lại Wave 4, không đáng phức tạp hoá W3a.

use std::time::Duration;

use crate::error::{AppError, Result};

const ATTACH_RETRIES: u32 = 40;
const ATTACH_DELAY: Duration = Duration::from_millis(250);

/// Chờ CDP endpoint tại `127.0.0.1:<port>` sẵn sàng (retry tối đa ~10s),
/// verify `/json/version` có `webSocketDebuggerUrl`, trả `cdp_url`
/// dạng `http://127.0.0.1:<port>`.
pub async fn attach(port: u16) -> Result<String> {
    let base = format!("http://127.0.0.1:{port}");
    let url = format!("{base}/json/version");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?;

    let mut last_err = String::from("chưa nhận được phản hồi");
    for _ in 0..ATTACH_RETRIES {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                if body
                    .get("webSocketDebuggerUrl")
                    .and_then(|v| v.as_str())
                    .is_some()
                {
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
