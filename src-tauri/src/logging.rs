//! Logging file (W21b): tracing → `~/.browserx/logs/browserx.log` với rotation
//! theo ngày (tự viết — không dùng tracing-appender), giữ tối đa
//! [`MAX_ARCHIVED_LOGS`] file cũ. Kèm panic hook ghi panic + backtrace vào log.
//! Local-only: KHÔNG telemetry/phone-home.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Số file log đã rotate giữ lại (không tính `browserx.log` hiện tại).
const MAX_ARCHIVED_LOGS: usize = 5;

/// Tên file log hiện tại trong thư mục logs.
const CURRENT_LOG: &str = "browserx.log";

/// Thư mục chứa log: `~/.browserx/logs`.
pub fn logs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".browserx")
        .join("logs")
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Xoá bớt file `browserx-YYYY-MM-DD.log` cũ, giữ lại `keep` file mới nhất.
fn prune_archived(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut archived: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("browserx-") && n.ends_with(".log"))
        })
        .collect();
    // Tên chứa ngày ISO nên sort theo tên = sort theo thời gian.
    archived.sort();
    let excess = archived.len().saturating_sub(keep);
    for p in archived.into_iter().take(excess) {
        let _ = fs::remove_file(p);
    }
}

struct Inner {
    /// Ngày (YYYY-MM-DD) của file đang mở; rỗng = chưa mở.
    day: String,
    file: Option<File>,
}

/// Writer cho tracing-subscriber: ghi `browserx.log`, tự rotate khi sang ngày
/// mới (rename thành `browserx-<ngày cũ>.log` rồi mở file mới).
#[derive(Clone)]
pub struct DailyLogWriter {
    dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl DailyLogWriter {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        // File cũ từ lần chạy trước: nếu mtime khác hôm nay thì archive luôn.
        let current = dir.join(CURRENT_LOG);
        if let Ok(meta) = fs::metadata(&current) {
            if let Ok(modified) = meta.modified() {
                let day = chrono::DateTime::<chrono::Local>::from(modified)
                    .format("%Y-%m-%d")
                    .to_string();
                if day != today() {
                    let _ = fs::rename(&current, dir.join(format!("browserx-{day}.log")));
                    prune_archived(&dir, MAX_ARCHIVED_LOGS);
                }
            }
        }
        Self {
            dir,
            inner: Arc::new(Mutex::new(Inner {
                day: String::new(),
                file: None,
            })),
        }
    }

    /// Mở/rotate file nếu chưa mở hoặc đã sang ngày mới.
    fn ensure_file(&self, inner: &mut Inner) -> io::Result<()> {
        let now = today();
        if inner.file.is_some() && inner.day == now {
            return Ok(());
        }
        let current = self.dir.join(CURRENT_LOG);
        if inner.file.take().is_some() && !inner.day.is_empty() {
            let _ = fs::rename(&current, self.dir.join(format!("browserx-{}.log", inner.day)));
            prune_archived(&self.dir, MAX_ARCHIVED_LOGS);
        }
        fs::create_dir_all(&self.dir)?;
        inner.file = Some(OpenOptions::new().create(true).append(true).open(&current)?);
        inner.day = now;
        Ok(())
    }
}

impl Write for DailyLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_file(&mut inner)?;
        inner.file.as_mut().expect("file opened").write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.file.as_mut() {
            Some(f) => f.flush(),
            None => Ok(()),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for DailyLogWriter {
    type Writer = DailyLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Init tracing (file `~/.browserx/logs/browserx.log`, rotate ngày) + panic hook
/// ghi panic kèm backtrace vào log. Gọi SỚM trong `run()`, trước tauri::Builder.
pub fn init() {
    let writer = DailyLogWriter::new(logs_dir());
    let max_level = if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    // try_init: không panic nếu subscriber đã được set (vd. trong test).
    let _ = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(max_level)
        .try_init();

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!("panic: {info}\nbacktrace:\n{backtrace}");
        prev_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("browserx-log-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn logs_dir_under_browserx() {
        let p = logs_dir();
        assert!(p.ends_with(Path::new(".browserx").join("logs")));
    }

    #[test]
    fn writer_creates_current_log_and_appends() {
        let dir = temp_dir();
        let mut w = DailyLogWriter::new(dir.clone());
        w.write_all(b"hello\n").unwrap();
        w.flush().unwrap();
        let content = fs::read_to_string(dir.join(CURRENT_LOG)).unwrap();
        assert_eq!(content, "hello\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prune_keeps_newest_archives() {
        let dir = temp_dir();
        for day in ["2026-01-01", "2026-01-02", "2026-01-03"] {
            fs::write(dir.join(format!("browserx-{day}.log")), b"x").unwrap();
        }
        fs::write(dir.join(CURRENT_LOG), b"x").unwrap();
        prune_archived(&dir, 2);
        assert!(!dir.join("browserx-2026-01-01.log").exists());
        assert!(dir.join("browserx-2026-01-02.log").exists());
        assert!(dir.join("browserx-2026-01-03.log").exists());
        assert!(dir.join(CURRENT_LOG).exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
