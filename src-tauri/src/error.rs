//! Kiểu lỗi dùng chung cho toàn bộ app.
//!
//! Mọi Tauri command trả `crate::error::Result<T>`; `AppError` serialize thành
//! chuỗi message để frontend nhận qua `invoke().catch(...)`.

use thiserror::Error;

/// Lỗi cấp ứng dụng, gom mọi nguồn lỗi của các module.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("binary error: {0}")]
    Binary(String),

    #[error("launch error: {0}")]
    Launch(String),

    #[error("cdp error: {0}")]
    Cdp(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Result dùng chung cho toàn bộ app.
pub type Result<T> = std::result::Result<T, AppError>;
