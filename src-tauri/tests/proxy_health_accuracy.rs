//! W33a — Đo độ chính xác phân loại proxy health-check trên bộ mẫu chuẩn OFFLINE.
//!
//! Harness tất định, KHÔNG cần internet: mock IP-echo + mock HTTP forward proxy
//! chỉ bằng `std::net` (pattern local server như tests/stealth.rs). Mock proxy
//! hỗ trợ cả absolute-form GET (reqwest dùng dạng này cho target `http://` qua
//! HTTP proxy) lẫn CONNECT tunnel — nên các case healthy đi qua ĐÚNG tầng proxy
//! của `reqwest::Proxy::all`, không phải shortcut trỏ thẳng vào ip-echo.
//!
//! 24 kịch bản có nhãn healthy/unhealthy. Mỗi kịch bản chạy
//! `check_proxy_url_with` với endpoint local + timeout ngắn, so `ok` với nhãn,
//! in bảng scenario→expected→got→OK/X rồi assert tỉ lệ đúng ≥ 0.95.
//!
//! Kịch bản #24: body JSON không có key "ip" và không chứa whitespace
//! (vd `{"error":"forbidden"}`) — `parse_ip_response` đã validate token bằng
//! `IpAddr` nên từ chối đúng → phân loại unhealthy chính xác, 24/24 = 100%.
//!
//! Chạy: cd src-tauri && cargo test --test proxy_health_accuracy -- --nocapture

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use browserx_lib::proxy_check::check_proxy_url_with;

/// Dựng raw HTTP/1.1 response với Content-Length đúng và Connection: close.
fn http_response(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Server trả đúng 1 canned response cho MỌI connection (thread daemon,
/// cổng ephemeral 127.0.0.1:0 — như start_local_http trong tests/stealth.rs).
fn start_canned_server(response: String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind canned server");
    let port = listener.local_addr().expect("local_addr").port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let resp = response.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            });
        }
    });
    port
}

/// Server accept nhưng KHÔNG trả byte nào rồi mới đóng (case timeout).
/// Giữ connection 3s — dài hơn hẳn timeout 300ms mà test inject.
fn start_silent_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind silent server");
    let port = listener.local_addr().expect("local_addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                drop(stream);
            });
        }
    });
    port
}

/// Server trả header dở dang rồi treo (timeout giữa chừng response).
fn start_stall_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stall server");
    let port = listener.local_addr().expect("local_addr").port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n");
                let _ = stream.flush();
                std::thread::sleep(Duration::from_secs(3));
            });
        }
    });
    port
}

/// Bind rồi drop listener → lấy 1 port vừa được OS cấp nhưng không ai nghe
/// (connection refused). Race tái-cấp port là có nhưng xác suất rất nhỏ.
fn dead_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind dead port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Đọc tới hết header block `\r\n\r\n` (request GET không có body).
fn read_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => return None,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    return Some(buf);
                }
                if buf.len() > 64 * 1024 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
}

/// Mock HTTP forward proxy TỐI THIỂU: absolute-form GET (rewrite về
/// origin-form rồi forward tới upstream) + CONNECT tunnel 2 chiều.
fn start_forward_proxy() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind forward proxy");
    let port = listener.local_addr().expect("local_addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || handle_proxy_conn(stream));
        }
    });
    port
}

fn handle_proxy_conn(mut client: TcpStream) {
    let Some(head) = read_head(&mut client) else {
        return;
    };
    let head_str = String::from_utf8_lossy(&head).to_string();
    let mut lines = head_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("HTTP/1.1");

    if method.eq_ignore_ascii_case("CONNECT") {
        let Ok(mut upstream) = TcpStream::connect(target) else {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
            return;
        };
        if client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .is_err()
        {
            return;
        }
        let (Ok(mut c2), Ok(mut u2)) = (client.try_clone(), upstream.try_clone()) else {
            return;
        };
        let t = std::thread::spawn(move || {
            let _ = std::io::copy(&mut c2, &mut u2);
        });
        let _ = std::io::copy(&mut upstream, &mut client);
        let _ = t.join();
        return;
    }

    // Absolute-form: `GET http://host:port/path HTTP/1.1` → rewrite origin-form.
    let Some(rest) = target.strip_prefix("http://") else {
        let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        return;
    };
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:80")
    };
    let Ok(mut upstream) = TcpStream::connect(&addr) else {
        let _ = client.write_all(
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    };
    let mut req = format!("{method} {path} {version}\r\n");
    for line in lines {
        if line.is_empty() {
            break;
        }
        req.push_str(line);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    if upstream.write_all(req.as_bytes()).is_err() {
        return;
    }
    let _ = std::io::copy(&mut upstream, &mut client);
}

/// 1 kịch bản có nhãn: URL proxy + danh sách endpoint IP-echo + timeout riêng.
struct Scenario {
    name: &'static str,
    proxy_url: String,
    echo_urls: Vec<String>,
    timeout_ms: u64,
    expect_healthy: bool,
}

impl Scenario {
    fn new(
        name: &'static str,
        proxy_url: String,
        echo_urls: Vec<String>,
        timeout_ms: u64,
        expect_healthy: bool,
    ) -> Self {
        Self {
            name,
            proxy_url,
            echo_urls,
            timeout_ms,
            expect_healthy,
        }
    }
}

fn echo_url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_health_classification_accuracy() {
    // ---- Hạ tầng mock (mỗi server 1 thread daemon, cổng ephemeral) ----
    let proxy_a = start_forward_proxy();
    let proxy_b = start_forward_proxy();
    let silent_proxy = start_silent_server();
    let stall_proxy = start_stall_server();
    // "Proxy" hỏng kiểu captive-portal: trả HTML thay vì forward.
    let portal_proxy = start_canned_server(http_response(
        "200 OK",
        "text/html",
        "<html><body>please login to wifi</body></html>",
    ));
    // "Proxy" đòi auth: trả 407 cho mọi request.
    let auth_proxy = start_canned_server(http_response(
        "407 Proxy Authentication Required",
        "text/plain",
        "proxy auth required",
    ));

    let echo_json = start_canned_server(http_response(
        "200 OK",
        "application/json",
        r#"{"ip":"1.2.3.4"}"#,
    ));
    let echo_json2 = start_canned_server(http_response(
        "200 OK",
        "application/json",
        r#"{"ip":"9.9.9.9"}"#,
    ));
    let echo_json_v6 = start_canned_server(http_response(
        "200 OK",
        "application/json",
        r#"{"ip":"2001:db8::1"}"#,
    ));
    let echo_json_padded = start_canned_server(http_response(
        "200 OK",
        "application/json",
        r#"{"ip":"  8.8.8.8  "}"#,
    ));
    let echo_text = start_canned_server(http_response("200 OK", "text/plain", "5.6.7.8\n"));
    let echo_text_v6 = start_canned_server(http_response(
        "200 OK",
        "text/plain",
        "2606:2800:220:1:248:1893:25c8:1946",
    ));
    let echo_garbage = start_canned_server(http_response(
        "200 OK",
        "text/html",
        "<html><body>service temporarily unavailable</body></html>",
    ));
    let echo_empty = start_canned_server(http_response("200 OK", "text/plain", ""));
    let echo_json_no_ip = start_canned_server(http_response(
        "200 OK",
        "application/json",
        r#"{"msg":"no ip"}"#,
    ));
    let echo_500 = start_canned_server(http_response(
        "500 Internal Server Error",
        "text/plain",
        "internal error",
    ));
    let echo_long_token = start_canned_server(http_response(
        "200 OK",
        "text/plain",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ));
    // JSON không có "ip", KHÔNG whitespace → parse_ip_response validate
    // bằng IpAddr nên từ chối đúng (không còn nhận nhầm là IP).
    let echo_json_error = start_canned_server(http_response(
        "200 OK",
        "application/json",
        r#"{"error":"forbidden"}"#,
    ));

    let proxy_url = |p: u16| format!("http://127.0.0.1:{p}");
    let dead_echo = dead_port();

    // ---- Bộ mẫu chuẩn: 24 kịch bản có nhãn (9 healthy + 15 unhealthy) ----
    // Timeout 3000ms cho case bình thường; 300ms cho case treo (không chờ 10s).
    let scenarios = vec![
        // -------- HEALTHY: đi qua mock forward proxy THẬT tới ip-echo --------
        Scenario::new(
            "H01 proxy→echo JSON ipify-style (1.2.3.4)",
            proxy_url(proxy_a),
            vec![echo_url(echo_json, "/?format=json")],
            3000,
            true,
        ),
        Scenario::new(
            "H02 proxy→echo plain-text (5.6.7.8)",
            proxy_url(proxy_a),
            vec![echo_url(echo_text, "/ip")],
            3000,
            true,
        ),
        Scenario::new(
            "H03 proxy→echo JSON IPv6 (2001:db8::1)",
            proxy_url(proxy_a),
            vec![echo_url(echo_json_v6, "/?format=json")],
            3000,
            true,
        ),
        Scenario::new(
            "H04 fallback: endpoint1 rác HTML, endpoint2 JSON ok",
            proxy_url(proxy_a),
            vec![
                echo_url(echo_garbage, "/"),
                echo_url(echo_json, "/?format=json"),
            ],
            3000,
            true,
        ),
        Scenario::new(
            "H05 fallback: endpoint1 cổng chết, endpoint2 JSON ok",
            proxy_url(proxy_a),
            vec![
                echo_url(dead_echo, "/"),
                echo_url(echo_json, "/?format=json"),
            ],
            3000,
            true,
        ),
        Scenario::new(
            "H06 proxy instance khác→echo JSON (9.9.9.9)",
            proxy_url(proxy_b),
            vec![echo_url(echo_json2, "/?format=json")],
            3000,
            true,
        ),
        Scenario::new(
            "H07 proxy→echo plain-text IPv6",
            proxy_url(proxy_b),
            vec![echo_url(echo_text_v6, "/ip")],
            3000,
            true,
        ),
        Scenario::new(
            "H08 proxy→echo JSON ip có padding whitespace",
            proxy_url(proxy_b),
            vec![echo_url(echo_json_padded, "/?format=json")],
            3000,
            true,
        ),
        Scenario::new(
            "H09 fallback: endpoint1 body rỗng, endpoint2 text ok",
            proxy_url(proxy_b),
            vec![echo_url(echo_empty, "/"), echo_url(echo_text, "/ip")],
            3000,
            true,
        ),
        // -------- UNHEALTHY --------
        Scenario::new(
            "U10 proxy cổng đóng (connection refused)",
            proxy_url(dead_port()),
            vec![echo_url(echo_json, "/?format=json")],
            3000,
            false,
        ),
        Scenario::new(
            "U11 proxy cổng đóng (biến thể port khác)",
            proxy_url(dead_port()),
            vec![echo_url(echo_text, "/ip")],
            3000,
            false,
        ),
        Scenario::new(
            "U12 proxy accept nhưng im lặng → timeout 300ms",
            proxy_url(silent_proxy),
            vec![echo_url(echo_json, "/?format=json")],
            300,
            false,
        ),
        Scenario::new(
            "U13 proxy trả header dở dang rồi treo → timeout 300ms",
            proxy_url(stall_proxy),
            vec![echo_url(echo_json, "/?format=json")],
            300,
            false,
        ),
        Scenario::new(
            "U14 cả 2 endpoint đều trả rác HTML",
            proxy_url(proxy_a),
            vec![echo_url(echo_garbage, "/"), echo_url(echo_garbage, "/ip")],
            3000,
            false,
        ),
        Scenario::new(
            "U15 proxy URL sai định dạng (\"not a url\")",
            "not a url".to_string(),
            vec![echo_url(echo_json, "/?format=json")],
            3000,
            false,
        ),
        Scenario::new(
            "U16 proxy URL thiếu scheme (\"http//host:1\")",
            "http//host:1".to_string(),
            vec![echo_url(echo_json, "/?format=json")],
            3000,
            false,
        ),
        Scenario::new(
            "U17 proxy sống nhưng endpoint IP-echo cổng chết",
            proxy_url(proxy_a),
            vec![echo_url(dead_echo, "/")],
            3000,
            false,
        ),
        Scenario::new(
            "U18 echo trả body rỗng (cả 2 endpoint)",
            proxy_url(proxy_a),
            vec![echo_url(echo_empty, "/"), echo_url(echo_empty, "/ip")],
            3000,
            false,
        ),
        Scenario::new(
            "U19 echo trả JSON không có key ip (có whitespace)",
            proxy_url(proxy_a),
            vec![echo_url(echo_json_no_ip, "/?format=json")],
            3000,
            false,
        ),
        Scenario::new(
            "U20 echo trả HTTP 500",
            proxy_url(proxy_a),
            vec![echo_url(echo_500, "/")],
            3000,
            false,
        ),
        Scenario::new(
            "U21 \"proxy\" captive-portal trả HTML thay vì forward",
            proxy_url(portal_proxy),
            vec![echo_url(echo_json, "/?format=json")],
            3000,
            false,
        ),
        Scenario::new(
            "U22 proxy trả 407 Proxy Authentication Required",
            proxy_url(auth_proxy),
            vec![echo_url(echo_json, "/?format=json")],
            3000,
            false,
        ),
        Scenario::new(
            "U23 echo trả token dài >45 ký tự không whitespace",
            proxy_url(proxy_b),
            vec![echo_url(echo_long_token, "/")],
            3000,
            false,
        ),
        Scenario::new(
            "U24 echo trả JSON lỗi không whitespace (không phải IP)",
            proxy_url(proxy_a),
            vec![echo_url(echo_json_error, "/?format=json")],
            3000,
            false,
        ),
    ];

    // ---- Chạy tuần tự, so nhãn, in bảng ----
    let total = scenarios.len();
    let mut correct = 0usize;
    println!();
    println!("{:<58} {:>9} {:>9}  result", "scenario", "expected", "got");
    println!("{}", "-".repeat(88));
    for s in &scenarios {
        let echo_refs: Vec<&str> = s.echo_urls.iter().map(String::as_str).collect();
        let res = check_proxy_url_with(
            &s.proxy_url,
            &echo_refs,
            None,
            Duration::from_millis(s.timeout_ms),
        )
        .await;
        let got_healthy = res.ok;
        let ok = got_healthy == s.expect_healthy;
        if ok {
            correct += 1;
        }
        let label = |h: bool| if h { "healthy" } else { "unhealthy" };
        println!(
            "{:<58} {:>9} {:>9}  {}",
            s.name,
            label(s.expect_healthy),
            label(got_healthy),
            if ok { "OK" } else { "X" }
        );
        if !ok {
            println!(
                "    └─ chi tiết: ip={:?} err={:?}",
                res.external_ip, res.error
            );
        }
    }
    println!("{}", "-".repeat(88));
    let rate = correct as f64 / total as f64;
    println!(
        "Độ chính xác phân loại: {correct}/{total} = {:.2}% (ngưỡng ≥95%)",
        rate * 100.0
    );
    assert!(
        rate >= 0.95,
        "độ chính xác phân loại {correct}/{total} = {:.2}% < 95%",
        rate * 100.0
    );
}