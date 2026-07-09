//! W55a — POC tdlib-rs: auth + private channel + upload/download (nhánh spike/w55a-tdlib).
//!
//! Chạy: `TDLIB_API_ID=... TDLIB_API_HASH=... cargo run --bin tdlib_poc`
//! (đăng ký app tại https://my.telegram.org để lấy api_id/api_hash).
//!
//! BẢO MẬT (POC-only, KHÔNG copy nguyên vào app thật):
//! - Session TDLib lưu ở /tmp/browserx-tdlib-poc/td_db. Auth key MTProto trong đó = TOÀN QUYỀN
//!   tài khoản Telegram. Bản production phải lưu trong app-data dir quyền hạn chế, đặt
//!   database_encryption_key lấy từ OS keychain, và có flow logout/revoke session.
//! - api_id/api_hash chỉ đọc từ env, không hardcode, không log.
//! - Không log OTP/password/nội dung session; stdin đọc plaintext (echo) — chấp nhận cho POC.

use sha2::{Digest, Sha256};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tdlib_rs::enums::{
    AuthorizationState, InputFile, InputMessageContent, MessageContent, Update,
};
use tdlib_rs::{functions, types};
use tokio::sync::mpsc::{self, Receiver, Sender};

const POC_DIR: &str = "/tmp/browserx-tdlib-poc";
const CHANNEL_TITLE: &str = "BrowserX_CloudSync_POC";
const TEST_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB, KHÔNG split
const MB: u64 = 1024 * 1024;

/// Event nội bộ forward từ update-loop về main flow.
enum PocEvent {
    Auth(AuthorizationState),
    SendSucceeded { old_message_id: i64, message: Box<types::Message> },
    SendFailed { old_message_id: i64, error: types::Error },
}

fn ask_user(prompt: &str) -> String {
    println!("{prompt}");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

/// Sinh file test bằng LCG (deterministic, không cần crate rand), trả về SHA256 hex.
fn generate_test_file(path: &Path, size: u64) -> std::io::Result<String> {
    let f = std::fs::File::create(path)?;
    let mut w = BufWriter::new(f);
    let mut hasher = Sha256::new();
    let mut state: u64 = 0x5DEECE66D;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut written: u64 = 0;
    while written < size {
        let n = std::cmp::min(buf.len() as u64, size - written) as usize;
        for b in buf[..n].iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *b = (state >> 33) as u8;
        }
        w.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        written += n as u64;
    }
    w.flush()?;
    Ok(hex::encode(hasher.finalize()))
}

/// SHA256 hex của một file (streaming, không load hết vào RAM).
fn sha256_of_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Nếu error là FLOOD_WAIT (code 429 / "Too Many Requests: retry after N") → Some(N giây).
fn flood_wait_secs(error: &types::Error) -> Option<u64> {
    let msg = &error.message;
    if error.code == 429 || msg.contains("Too Many Requests") || msg.contains("FLOOD_WAIT") {
        let digits: String = msg
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return digits.parse::<u64>().ok().or(Some(0));
    }
    None
}

/// Dừng lịch sự khi dính flood-wait: log số giây rồi exit — KHÔNG retry loop vô hạn.
fn bail_if_flood_wait(error: &types::Error, context: &str) {
    if let Some(secs) = flood_wait_secs(error) {
        eprintln!("[flood-wait] {context}: Telegram yêu cầu chờ {secs}s. Dừng POC tại đây (không retry).");
        std::process::exit(3);
    }
}

async fn handle_update(update: Update, tx: &Sender<PocEvent>, upload_progress: &AtomicI64) {
    match update {
        Update::AuthorizationState(u) => {
            let _ = tx.send(PocEvent::Auth(u.authorization_state)).await;
        }
        Update::MessageSendSucceeded(u) => {
            let _ = tx
                .send(PocEvent::SendSucceeded {
                    old_message_id: u.old_message_id,
                    message: Box::new(u.message),
                })
                .await;
        }
        Update::MessageSendFailed(u) => {
            let _ = tx
                .send(PocEvent::SendFailed { old_message_id: u.old_message_id, error: u.error })
                .await;
        }
        Update::File(u) => {
            // Log tiến độ upload mỗi ~10MB (không log path/nội dung).
            let uploaded = u.file.remote.uploaded_size;
            let last = upload_progress.load(Ordering::Relaxed);
            if uploaded > 0 && uploaded - last >= (10 * MB) as i64 {
                upload_progress.store(uploaded, Ordering::Relaxed);
                println!("[upload] {} MB đã lên...", uploaded / MB as i64);
            }
        }
        _ => {}
    }
}

/// Xử lý auth flow: phone → OTP → 2FA password. Trả lại receiver để main dùng tiếp.
async fn handle_authorization_state(
    client_id: i32,
    api_id: i32,
    api_hash: String,
    mut rx: Receiver<PocEvent>,
    run_flag: Arc<AtomicBool>,
) -> Receiver<PocEvent> {
    while let Some(ev) = rx.recv().await {
        let state = match ev {
            PocEvent::Auth(s) => s,
            _ => continue,
        };
        match state {
            AuthorizationState::WaitTdlibParameters => {
                let db_dir = format!("{POC_DIR}/td_db");
                let response = functions::set_tdlib_parameters(
                    false,
                    db_dir,
                    String::new(),
                    String::new(), // POC: DB không mã hoá; bản thật phải dùng key từ keychain
                    true,
                    true,
                    true,
                    false,
                    api_id,
                    api_hash.clone(),
                    "en".into(),
                    "BrowserX-POC".into(),
                    String::new(),
                    "0.1.0".into(),
                    client_id,
                )
                .await;
                if let Err(e) = response {
                    eprintln!("[auth] set_tdlib_parameters lỗi: {}", e.message);
                }
            }
            AuthorizationState::WaitPhoneNumber => loop {
                let input = ask_user("Nhập số điện thoại (kèm mã quốc gia, vd +84...):");
                match functions::set_authentication_phone_number(input, None, client_id).await {
                    Ok(_) => break,
                    Err(e) => {
                        bail_if_flood_wait(&e, "set_authentication_phone_number");
                        eprintln!("[auth] {}", e.message);
                    }
                }
            },
            AuthorizationState::WaitCode(_) => loop {
                let input = ask_user("Nhập mã OTP Telegram gửi về:");
                match functions::check_authentication_code(input, client_id).await {
                    Ok(_) => break,
                    Err(e) => {
                        bail_if_flood_wait(&e, "check_authentication_code");
                        eprintln!("[auth] {}", e.message);
                    }
                }
            },
            AuthorizationState::WaitPassword(_) => loop {
                let input = ask_user("Nhập mật khẩu 2FA (cảnh báo: input hiện echo — POC only):");
                match functions::check_authentication_password(input, client_id).await {
                    Ok(_) => break,
                    Err(e) => {
                        bail_if_flood_wait(&e, "check_authentication_password");
                        eprintln!("[auth] {}", e.message);
                    }
                }
            },
            AuthorizationState::Ready => {
                println!("[auth] Đăng nhập OK. Session lưu tại {POC_DIR}/td_db");
                break;
            }
            AuthorizationState::Closed => {
                run_flag.store(false, Ordering::Release);
                break;
            }
            other => {
                println!("[auth] state chưa xử lý trong POC: {other:?}");
            }
        }
    }
    rx
}

/// Tìm channel theo title trong chat list đã load; None nếu chưa có.
async fn find_channel(client_id: i32) -> Option<i64> {
    // Load chat list (lặp đến khi TDLib trả 404 = hết chats).
    for _ in 0..20 {
        match functions::load_chats(None, 100, client_id).await {
            Ok(_) => continue,
            Err(_) => break, // 404: đã load hết
        }
    }
    let chats = functions::search_chats(CHANNEL_TITLE.into(), 20, client_id).await.ok()?;
    let tdlib_rs::enums::Chats::Chats(chats) = chats;
    for chat_id in chats.chat_ids {
        if let Ok(tdlib_rs::enums::Chat::Chat(chat)) = functions::get_chat(chat_id, client_id).await {
            if chat.title == CHANNEL_TITLE {
                return Some(chat.id);
            }
        }
    }
    None
}

#[tokio::main]
async fn main() {
    // B0: credentials từ env — bắt buộc trước khi khởi tạo TDLib.
    let (api_id, api_hash) = match (
        std::env::var("TDLIB_API_ID").ok().and_then(|v| v.parse::<i32>().ok()),
        std::env::var("TDLIB_API_HASH").ok().filter(|v| !v.is_empty()),
    ) {
        (Some(id), Some(hash)) => (id, hash),
        _ => {
            eprintln!(
                "MISSING CREDENTIALS: cần env TDLIB_API_ID (số) và TDLIB_API_HASH.\n\
                 Đăng ký application tại https://my.telegram.org → API development tools.\n\
                 Ví dụ: TDLIB_API_ID=12345 TDLIB_API_HASH=abcdef... cargo run --bin tdlib_poc"
            );
            std::process::exit(2);
        }
    };
    std::fs::create_dir_all(POC_DIR).expect("không tạo được POC dir");

    let client_id = tdlib_rs::create_client();
    let (tx, rx) = mpsc::channel::<PocEvent>(64);
    let run_flag = Arc::new(AtomicBool::new(true));
    let run_flag_clone = run_flag.clone();
    let upload_progress = Arc::new(AtomicI64::new(0));
    let upload_progress_clone = upload_progress.clone();

    let handle = tokio::spawn(async move {
        while run_flag_clone.load(Ordering::Acquire) {
            let result = tokio::task::spawn_blocking(tdlib_rs::receive).await.unwrap();
            if let Some((update, _client_id)) = result {
                handle_update(update, &tx, &upload_progress_clone).await;
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    });

    functions::set_log_verbosity_level(1, client_id).await.unwrap();

    // B1: Auth phone → OTP → 2FA.
    let mut rx =
        handle_authorization_state(client_id, api_id, api_hash, rx, run_flag.clone()).await;

    // B2: Tạo hoặc tái dùng private channel.
    let chat_id = match find_channel(client_id).await {
        Some(id) => {
            println!("[channel] Tái dùng channel '{CHANNEL_TITLE}' (chat_id={id})");
            id
        }
        None => match functions::create_new_supergroup_chat(
            CHANNEL_TITLE.into(),
            false,
            true, // is_channel = private channel (không public username)
            "BrowserX Cloud Sync POC storage".into(),
            None,
            0,
            false,
            client_id,
        )
        .await
        {
            Ok(tdlib_rs::enums::Chat::Chat(chat)) => {
                println!("[channel] Đã tạo channel '{CHANNEL_TITLE}' (chat_id={})", chat.id);
                chat.id
            }
            Err(e) => {
                bail_if_flood_wait(&e, "create_new_supergroup_chat");
                panic!("[channel] tạo channel thất bại: {}", e.message);
            }
        },
    };

    // B3: Generate file test 100MB + SHA256, upload KHÔNG split, caption chứa SHA256.
    let test_path = PathBuf::from(POC_DIR).join("testfile_100mb.bin");
    println!("[gen] Đang sinh file test {} MB...", TEST_FILE_SIZE / MB);
    let t0 = Instant::now();
    let sha_orig = generate_test_file(&test_path, TEST_FILE_SIZE).expect("generate fail");
    println!("[gen] Xong sau {:.1}s, sha256={sha_orig}", t0.elapsed().as_secs_f64());

    let caption = types::FormattedText { text: format!("sha256:{sha_orig}"), entities: vec![] };
    let content = InputMessageContent::InputMessageDocument(types::InputMessageDocument {
        document: InputFile::Local(types::InputFileLocal {
            path: test_path.to_string_lossy().into_owned(),
        }),
        thumbnail: None,
        disable_content_type_detection: true,
        caption: Some(caption),
    });

    println!("[upload] Gửi file lên channel...");
    let t_up = Instant::now();
    let temp_msg_id = match functions::send_message(chat_id, None, None, None, content, client_id).await {
        Ok(tdlib_rs::enums::Message::Message(m)) => m.id,
        Err(e) => {
            bail_if_flood_wait(&e, "send_message");
            panic!("[upload] send_message thất bại: {}", e.message);
        }
    };

    // Chờ updateMessageSendSucceeded/Failed cho message vừa gửi.
    let sent_message = loop {
        match rx.recv().await {
            Some(PocEvent::SendSucceeded { old_message_id, message }) if old_message_id == temp_msg_id => {
                break message;
            }
            Some(PocEvent::SendFailed { old_message_id, error }) if old_message_id == temp_msg_id => {
                bail_if_flood_wait(&error, "send (async)");
                panic!("[upload] gửi thất bại: {} (code {})", error.message, error.code);
            }
            Some(_) => continue,
            None => panic!("[upload] update loop đóng bất ngờ"),
        }
    };
    let up_secs = t_up.elapsed().as_secs_f64();
    println!(
        "[upload] OK sau {:.1}s (~{:.1} MB/s), message_id={}",
        up_secs,
        (TEST_FILE_SIZE / MB) as f64 / up_secs,
        sent_message.id
    );

    // B4: Xoá file gốc để ép TDLib download thật (không dedupe local), rồi verify SHA256.
    let file_id = match &sent_message.content {
        MessageContent::MessageDocument(doc) => doc.document.document.id,
        other => panic!("[download] message content không phải document: {other:?}"),
    };
    std::fs::remove_file(&test_path).expect("không xoá được file gốc");
    println!("[download] Đã xoá file gốc, tải lại từ Telegram (file_id={file_id})...");
    let t_dl = Instant::now();
    let downloaded = match functions::download_file(file_id, 32, 0, 0, true, client_id).await {
        Ok(tdlib_rs::enums::File::File(f)) => f,
        Err(e) => {
            bail_if_flood_wait(&e, "download_file");
            panic!("[download] thất bại: {}", e.message);
        }
    };
    let dl_secs = t_dl.elapsed().as_secs_f64();
    let dl_path = PathBuf::from(&downloaded.local.path);
    let sha_downloaded = sha256_of_file(&dl_path).expect("hash file tải về fail");
    println!(
        "[download] OK sau {:.1}s (~{:.1} MB/s)",
        dl_secs,
        (TEST_FILE_SIZE / MB) as f64 / dl_secs
    );

    if sha_downloaded == sha_orig {
        println!("✅ VERIFY OK: SHA256 khớp ({sha_orig})");
    } else {
        eprintln!("❌ VERIFY FAIL: gốc={sha_orig} tải_về={sha_downloaded}");
    }

    functions::close(client_id).await.unwrap();
    handle_authorization_state(client_id, api_id, String::new(), rx, run_flag.clone()).await;
    handle.await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_verify_sha256_roundtrip() {
        let dir = std::env::temp_dir().join("browserx-tdlib-poc-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.bin");
        let size = 3 * 1024 * 1024 + 123; // không tròn MB để test nhánh cuối
        let h1 = generate_test_file(&path, size).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), size);
        let h2 = sha256_of_file(&path).unwrap();
        assert_eq!(h1, h2);
        // Deterministic: sinh lại phải ra đúng hash cũ.
        let h3 = generate_test_file(&path, size).unwrap();
        assert_eq!(h1, h3);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn flood_wait_parsing() {
        let e = types::Error { code: 429, message: "Too Many Requests: retry after 17".into() };
        assert_eq!(flood_wait_secs(&e), Some(17));
        let e2 = types::Error { code: 420, message: "FLOOD_WAIT_33".into() };
        assert_eq!(flood_wait_secs(&e2), Some(33));
        let e3 = types::Error { code: 400, message: "CHAT_NOT_FOUND".into() };
        assert_eq!(flood_wait_secs(&e3), None);
    }
}
