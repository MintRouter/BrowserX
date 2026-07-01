//! DB layer (rusqlite/SQLite): schema, migrations, CRUD cho profiles/proxies/tags/settings/audit.
//!
//! Wave 2a. Port schema từ `refs/CloakBrowser-Manager/backend/database.py#L34-L66`,
//! mở rộng theo docs/03 §2: proxies tách riêng (credential BLOB đã mã hoá bởi
//! `crypto` — DB layer KHÔNG tự mã hoá), profile_proxy, tags, settings, audit.
//!
//! File DB: `~/.browserx/browserx.db` (WAL, foreign_keys ON). Timestamp RFC3339 UTC.
//! Type dùng chung lấy từ `crate::models`; các type chỉ-DB (input/record) định nghĩa
//! nội bộ tại đây để không đụng file dùng chung (models.rs) trong wave parallel.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::RngExt;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::models::Profile;

// ---------------------------------------------------------------------------
// Type nội bộ của DB layer (W3a map sang models::Proxy sau khi giải mã)
// ---------------------------------------------------------------------------

/// Input tạo profile mới. Field `None` → dùng default (giống semantics
/// `create_profile` Python: seed random 10000–99999, platform "windows",
/// screen 1920×1080, human_preset "default", user_data_dir `<data_dir>/profiles/<id>`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileInput {
    pub name: String,
    pub fingerprint_seed: Option<String>,
    pub platform: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
    pub gpu_vendor: Option<String>,
    pub gpu_renderer: Option<String>,
    pub hardware_concurrency: Option<u32>,
    pub humanize: Option<bool>,
    pub human_preset: Option<String>,
    pub headless: Option<bool>,
    pub geoip: Option<bool>,
    pub color_scheme: Option<String>,
    /// Mảng JSON flag bổ sung, ví dụ `["--lang=vi"]`.
    pub launch_args: Option<serde_json::Value>,
    pub user_data_dir: Option<String>,
    pub notes: Option<String>,
    /// Gán proxy ngay khi tạo (FK → proxies.id).
    pub proxy_id: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Update từng phần: chỉ field `Some(_)` được ghi đè (giống `update_profile` Python).
/// Gán/bỏ proxy dùng [`Db::assign_proxy`]; đổi tags qua field `tags` hoặc
/// [`Db::set_profile_tags`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileUpdate {
    pub name: Option<String>,
    pub fingerprint_seed: Option<String>,
    pub platform: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
    pub gpu_vendor: Option<String>,
    pub gpu_renderer: Option<String>,
    pub hardware_concurrency: Option<u32>,
    pub humanize: Option<bool>,
    pub human_preset: Option<String>,
    pub headless: Option<bool>,
    pub geoip: Option<bool>,
    pub color_scheme: Option<String>,
    pub launch_args: Option<serde_json::Value>,
    pub user_data_dir: Option<String>,
    pub notes: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Bản ghi proxy như lưu trong DB: credential là BLOB **đã mã hoá** bởi layer
/// `crypto` (XChaCha20-Poly1305). DB layer không encode/decode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRecord {
    pub id: String,
    pub name: String,
    /// "http" | "https" | "socks5".
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username_enc: Option<Vec<u8>>,
    pub password_enc: Option<Vec<u8>>,
    /// "ok" | "fail" | None (chưa check).
    pub health_status: Option<String>,
    pub last_checked_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Input tạo proxy mới (credential đã mã hoá sẵn).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyInput {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username_enc: Option<Vec<u8>>,
    pub password_enc: Option<Vec<u8>>,
}

/// Update proxy từng phần; `clear_credentials=true` xoá username/password_enc về NULL.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyUpdate {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username_enc: Option<Vec<u8>>,
    pub password_enc: Option<Vec<u8>>,
    #[serde(default)]
    pub clear_credentials: bool,
}

/// Một tag + màu (bảng `tags`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagInfo {
    pub tag: String,
    pub color: Option<String>,
}

/// Một dòng audit log (bảng `audit`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub action: String,
    pub target_id: Option<String>,
    pub meta: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Db: mở/khởi tạo + migration
// ---------------------------------------------------------------------------

/// Schema version hiện tại (PRAGMA user_version).
const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    fingerprint_seed TEXT NOT NULL,
    platform TEXT NOT NULL DEFAULT 'windows',
    timezone TEXT,
    locale TEXT,
    screen_width INTEGER NOT NULL DEFAULT 1920,
    screen_height INTEGER NOT NULL DEFAULT 1080,
    gpu_vendor TEXT,
    gpu_renderer TEXT,
    hardware_concurrency INTEGER NOT NULL DEFAULT 8,
    humanize INTEGER NOT NULL DEFAULT 0,
    human_preset TEXT DEFAULT 'default',
    headless INTEGER NOT NULL DEFAULT 0,
    geoip INTEGER NOT NULL DEFAULT 0,
    color_scheme TEXT,
    launch_args TEXT NOT NULL DEFAULT '[]',
    user_data_dir TEXT NOT NULL,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS proxies (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'http',
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username_enc BLOB,
    password_enc BLOB,
    health_status TEXT,
    last_checked_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS profile_proxy (
    profile_id TEXT PRIMARY KEY REFERENCES profiles(id) ON DELETE CASCADE,
    proxy_id TEXT NOT NULL REFERENCES proxies(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tags (
    tag TEXT PRIMARY KEY,
    color TEXT
);

CREATE TABLE IF NOT EXISTS profile_tags (
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    tag TEXT NOT NULL REFERENCES tags(tag) ON DELETE CASCADE,
    PRIMARY KEY (profile_id, tag)
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    action TEXT NOT NULL,
    target_id TEXT,
    meta TEXT
);

CREATE INDEX IF NOT EXISTS idx_profiles_name ON profiles(name);
CREATE INDEX IF NOT EXISTS idx_profiles_updated_at ON profiles(updated_at);
CREATE INDEX IF NOT EXISTS idx_profile_tags_tag ON profile_tags(tag);
CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit(ts);
";

/// Kết nối SQLite của app. `Mutex<Connection>` để dùng được trong `tauri::State`
/// (single-process, mọi thao tác tuần tự qua lock — đủ cho local app).
pub struct Db {
    conn: Mutex<Connection>,
    /// Thư mục dữ liệu app (mặc định `~/.browserx`) — dùng suy ra `user_data_dir` mặc định.
    data_dir: PathBuf,
}

impl Db {
    /// Mở DB mặc định tại `~/.browserx/browserx.db` (tạo thư mục nếu chưa có).
    pub fn open_default() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::InvalidInput("cannot resolve home directory".into()))?;
        Self::open_at_dir(home.join(".browserx"))
    }

    /// Mở DB tại `<dir>/browserx.db` — dùng cho test hoặc data-dir tuỳ biến.
    pub fn open_at_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("browserx.db"))?;
        Self::init_conn(&conn)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            data_dir: dir,
        })
    }

    /// DB in-memory (test nhanh, không đụng đĩa).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_conn(&conn)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            data_dir: std::env::temp_dir(),
        })
    }

    fn init_conn(conn: &Connection) -> Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(())
    }

    /// Thư mục dữ liệu app đang dùng.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }
}

/// Migration idempotent theo `PRAGMA user_version`; chạy lại không lỗi
/// (mọi CREATE đều `IF NOT EXISTS`). Migration tương lai: thêm nhánh `< 2`, `< 3`…
fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Thời điểm hiện tại, RFC3339 UTC (tương đương `_now()` Python).
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Escape `%`/`_`/`\` cho pattern LIKE (dùng với `ESCAPE '\'`).
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn sql_text(s: String) -> SqlValue {
    SqlValue::Text(s)
}

fn sql_int(n: i64) -> SqlValue {
    SqlValue::Integer(n)
}

fn sql_bool(b: bool) -> SqlValue {
    SqlValue::Integer(b as i64)
}

/// Map một row (SELECT p.*, pp.proxy_id) → `models::Profile` (tags rỗng, caller tự điền).
fn row_to_profile(row: &Row) -> rusqlite::Result<Profile> {
    let launch_args_raw: String = row.get("launch_args")?;
    let launch_args: serde_json::Value =
        serde_json::from_str(&launch_args_raw).unwrap_or_else(|_| serde_json::Value::Array(vec![]));
    Ok(Profile {
        id: row.get("id")?,
        name: row.get("name")?,
        fingerprint_seed: row.get("fingerprint_seed")?,
        platform: row.get("platform")?,
        timezone: row.get("timezone")?,
        locale: row.get("locale")?,
        screen_width: row.get("screen_width")?,
        screen_height: row.get("screen_height")?,
        gpu_vendor: row.get("gpu_vendor")?,
        gpu_renderer: row.get("gpu_renderer")?,
        hardware_concurrency: row.get("hardware_concurrency")?,
        humanize: row.get("humanize")?,
        human_preset: row.get("human_preset")?,
        headless: row.get("headless")?,
        geoip: row.get("geoip")?,
        color_scheme: row.get("color_scheme")?,
        launch_args,
        user_data_dir: row.get("user_data_dir")?,
        notes: row.get("notes")?,
        proxy_id: row.get("proxy_id")?,
        tags: Vec::new(),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_proxy(row: &Row) -> rusqlite::Result<ProxyRecord> {
    Ok(ProxyRecord {
        id: row.get("id")?,
        name: row.get("name")?,
        protocol: row.get("protocol")?,
        host: row.get("host")?,
        port: row.get("port")?,
        username_enc: row.get("username_enc")?,
        password_enc: row.get("password_enc")?,
        health_status: row.get("health_status")?,
        last_checked_at: row.get("last_checked_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Thay toàn bộ tags của 1 profile (upsert vào bảng `tags` rồi ghi `profile_tags`).
fn set_profile_tags_tx(conn: &Connection, profile_id: &str, tags: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM profile_tags WHERE profile_id = ?1",
        params![profile_id],
    )?;
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        conn.execute("INSERT OR IGNORE INTO tags (tag) VALUES (?1)", params![tag])?;
        conn.execute(
            "INSERT OR IGNORE INTO profile_tags (profile_id, tag) VALUES (?1, ?2)",
            params![profile_id, tag],
        )?;
    }
    Ok(())
}

/// Đọc map profile_id → tags cho một batch profile (tránh N+1 khi list/search).
fn tags_for_profiles(conn: &Connection) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = Default::default();
    let mut stmt = conn.prepare("SELECT profile_id, tag FROM profile_tags ORDER BY tag")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (pid, tag) = row?;
        map.entry(pid).or_default().push(tag);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Profiles CRUD
// ---------------------------------------------------------------------------

const PROFILE_SELECT: &str =
    "SELECT p.*, pp.proxy_id FROM profiles p LEFT JOIN profile_proxy pp ON pp.profile_id = p.id";

impl Db {
    /// Tạo profile mới (port `create_profile` database.py#L87-L138):
    /// seed random 10000–99999 nếu không truyền, user_data_dir mặc định
    /// `<data_dir>/profiles/<id>`, gán tags + proxy trong cùng transaction.
    pub fn create_profile(&self, input: ProfileInput) -> Result<Profile> {
        if input.name.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "profile name must not be empty".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let seed = input
            .fingerprint_seed
            .unwrap_or_else(|| rand::rng().random_range(10000u32..=99999).to_string());
        let user_data_dir = input.user_data_dir.unwrap_or_else(|| {
            self.data_dir
                .join("profiles")
                .join(&id)
                .to_string_lossy()
                .into_owned()
        });
        let launch_args = input
            .launch_args
            .unwrap_or_else(|| serde_json::Value::Array(vec![]));
        let ts = now();

        {
            let mut conn = self.lock();
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO profiles (
                    id, name, fingerprint_seed, platform, timezone, locale,
                    screen_width, screen_height, gpu_vendor, gpu_renderer,
                    hardware_concurrency, humanize, human_preset, headless, geoip,
                    color_scheme, launch_args, user_data_dir, notes, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                params![
                    id,
                    input.name,
                    seed,
                    input.platform.as_deref().unwrap_or("windows"),
                    input.timezone,
                    input.locale,
                    input.screen_width.unwrap_or(1920),
                    input.screen_height.unwrap_or(1080),
                    input.gpu_vendor,
                    input.gpu_renderer,
                    input.hardware_concurrency.unwrap_or(8),
                    input.humanize.unwrap_or(false),
                    input.human_preset.as_deref().unwrap_or("default"),
                    input.headless.unwrap_or(false),
                    input.geoip.unwrap_or(false),
                    input.color_scheme,
                    serde_json::to_string(&launch_args)?,
                    user_data_dir,
                    input.notes,
                    ts,
                    ts,
                ],
            )?;
            if let Some(tags) = &input.tags {
                set_profile_tags_tx(&tx, &id, tags)?;
            }
            if let Some(proxy_id) = &input.proxy_id {
                tx.execute(
                    "INSERT OR REPLACE INTO profile_proxy (profile_id, proxy_id) VALUES (?1, ?2)",
                    params![id, proxy_id],
                )?;
            }
            tx.commit()?;
        }
        self.get_profile(&id)
    }

    /// Đọc 1 profile (kèm tags + proxy_id). `NotFound` nếu không tồn tại.
    pub fn get_profile(&self, id: &str) -> Result<Profile> {
        let conn = self.lock();
        let sql = format!("{PROFILE_SELECT} WHERE p.id = ?1");
        let profile = conn
            .query_row(&sql, params![id], row_to_profile)
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("profile {id}")))?;
        let mut profile = profile;
        let mut stmt =
            conn.prepare("SELECT tag FROM profile_tags WHERE profile_id = ?1 ORDER BY tag")?;
        profile.tags = stmt
            .query_map(params![id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(profile)
    }

    /// Danh sách toàn bộ profile, mới cập nhật trước (batch-load tags, không N+1).
    pub fn list_profiles(&self) -> Result<Vec<Profile>> {
        let conn = self.lock();
        let sql = format!("{PROFILE_SELECT} ORDER BY p.updated_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mut profiles = stmt
            .query_map([], row_to_profile)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tag_map = tags_for_profiles(&conn)?;
        for p in &mut profiles {
            if let Some(tags) = tag_map.remove(&p.id) {
                p.tags = tags;
            }
        }
        Ok(profiles)
    }

    /// Search theo tên (LIKE, không phân biệt hoa thường ASCII) + lọc tag tuỳ chọn.
    pub fn search_profiles(&self, query: &str, tag: Option<&str>) -> Result<Vec<Profile>> {
        let conn = self.lock();
        let pattern = format!("%{}%", escape_like(query));
        let mut sql = format!("{PROFILE_SELECT} WHERE p.name LIKE ?1 ESCAPE '\\'");
        let mut values: Vec<SqlValue> = vec![sql_text(pattern)];
        if let Some(tag) = tag {
            sql.push_str(" AND p.id IN (SELECT profile_id FROM profile_tags WHERE tag = ?2)");
            values.push(sql_text(tag.to_string()));
        }
        sql.push_str(" ORDER BY p.updated_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mut profiles = stmt
            .query_map(rusqlite::params_from_iter(values), row_to_profile)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tag_map = tags_for_profiles(&conn)?;
        for p in &mut profiles {
            if let Some(tags) = tag_map.remove(&p.id) {
                p.tags = tags;
            }
        }
        Ok(profiles)
    }

    /// Update từng phần (port `update_profile` database.py#L172-L217):
    /// chỉ ghi field `Some(_)`; luôn cập nhật `updated_at` khi có thay đổi cột.
    pub fn update_profile(&self, id: &str, input: ProfileUpdate) -> Result<Profile> {
        let mut cols: Vec<&str> = Vec::new();
        let mut values: Vec<SqlValue> = Vec::new();
        if let Some(v) = input.name {
            if v.trim().is_empty() {
                return Err(AppError::InvalidInput(
                    "profile name must not be empty".into(),
                ));
            }
            cols.push("name");
            values.push(sql_text(v));
        }
        if let Some(v) = input.fingerprint_seed {
            cols.push("fingerprint_seed");
            values.push(sql_text(v));
        }
        if let Some(v) = input.platform {
            cols.push("platform");
            values.push(sql_text(v));
        }
        if let Some(v) = input.timezone {
            cols.push("timezone");
            values.push(sql_text(v));
        }
        if let Some(v) = input.locale {
            cols.push("locale");
            values.push(sql_text(v));
        }
        if let Some(v) = input.screen_width {
            cols.push("screen_width");
            values.push(sql_int(v as i64));
        }
        if let Some(v) = input.screen_height {
            cols.push("screen_height");
            values.push(sql_int(v as i64));
        }
        if let Some(v) = input.gpu_vendor {
            cols.push("gpu_vendor");
            values.push(sql_text(v));
        }
        if let Some(v) = input.gpu_renderer {
            cols.push("gpu_renderer");
            values.push(sql_text(v));
        }
        if let Some(v) = input.hardware_concurrency {
            cols.push("hardware_concurrency");
            values.push(sql_int(v as i64));
        }
        if let Some(v) = input.humanize {
            cols.push("humanize");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.human_preset {
            cols.push("human_preset");
            values.push(sql_text(v));
        }
        if let Some(v) = input.headless {
            cols.push("headless");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.geoip {
            cols.push("geoip");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.color_scheme {
            cols.push("color_scheme");
            values.push(sql_text(v));
        }
        if let Some(v) = input.launch_args {
            cols.push("launch_args");
            values.push(sql_text(serde_json::to_string(&v)?));
        }
        if let Some(v) = input.user_data_dir {
            cols.push("user_data_dir");
            values.push(sql_text(v));
        }
        if let Some(v) = input.notes {
            cols.push("notes");
            values.push(sql_text(v));
        }

        {
            let conn = self.lock();
            let exists: bool = conn
                .query_row("SELECT 1 FROM profiles WHERE id = ?1", params![id], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();
            if !exists {
                return Err(AppError::NotFound(format!("profile {id}")));
            }
            if !cols.is_empty() {
                let assignments: Vec<String> = cols
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{c} = ?{}", i + 1))
                    .collect();
                let sql = format!(
                    "UPDATE profiles SET {}, updated_at = ?{} WHERE id = ?{}",
                    assignments.join(", "),
                    cols.len() + 1,
                    cols.len() + 2,
                );
                values.push(sql_text(now()));
                values.push(sql_text(id.to_string()));
                conn.execute(&sql, rusqlite::params_from_iter(values))?;
            }
            if let Some(tags) = &input.tags {
                set_profile_tags_tx(&conn, id, tags)?;
            }
        }
        self.get_profile(id)
    }

    /// Xoá profile (cascade profile_tags + profile_proxy). Trả `true` nếu có xoá.
    /// Không đụng `user_data_dir` trên đĩa — launcher/commands quyết định.
    pub fn delete_profile(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM profiles WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }
}

// ---------------------------------------------------------------------------
// Proxies CRUD + assign
// ---------------------------------------------------------------------------

impl Db {
    /// Tạo proxy mới. `username_enc`/`password_enc` là BLOB đã mã hoá bởi `crypto`.
    pub fn create_proxy(&self, input: ProxyInput) -> Result<ProxyRecord> {
        if input.host.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "proxy host must not be empty".into(),
            ));
        }
        if !matches!(input.protocol.as_str(), "http" | "https" | "socks5") {
            return Err(AppError::InvalidInput(format!(
                "unsupported proxy protocol: {}",
                input.protocol
            )));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO proxies (
                id, name, protocol, host, port, username_enc, password_enc,
                created_at, updated_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                id,
                input.name,
                input.protocol,
                input.host,
                input.port,
                input.username_enc,
                input.password_enc,
                ts,
                ts,
            ],
        )?;
        drop(conn);
        self.get_proxy(&id)
    }

    /// Đọc 1 proxy. `NotFound` nếu không tồn tại.
    pub fn get_proxy(&self, id: &str) -> Result<ProxyRecord> {
        let conn = self.lock();
        conn.query_row(
            "SELECT * FROM proxies WHERE id = ?1",
            params![id],
            row_to_proxy,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("proxy {id}")))
    }

    /// Danh sách toàn bộ proxy, mới cập nhật trước.
    pub fn list_proxies(&self) -> Result<Vec<ProxyRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM proxies ORDER BY updated_at DESC")?;
        let rows = stmt
            .query_map([], row_to_proxy)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Update proxy từng phần; `clear_credentials=true` xoá cả 2 blob về NULL.
    pub fn update_proxy(&self, id: &str, input: ProxyUpdate) -> Result<ProxyRecord> {
        let mut cols: Vec<&str> = Vec::new();
        let mut values: Vec<SqlValue> = Vec::new();
        if let Some(v) = input.name {
            cols.push("name");
            values.push(sql_text(v));
        }
        if let Some(v) = input.protocol {
            if !matches!(v.as_str(), "http" | "https" | "socks5") {
                return Err(AppError::InvalidInput(format!(
                    "unsupported proxy protocol: {v}"
                )));
            }
            cols.push("protocol");
            values.push(sql_text(v));
        }
        if let Some(v) = input.host {
            cols.push("host");
            values.push(sql_text(v));
        }
        if let Some(v) = input.port {
            cols.push("port");
            values.push(sql_int(v as i64));
        }
        if input.clear_credentials {
            cols.push("username_enc");
            values.push(SqlValue::Null);
            cols.push("password_enc");
            values.push(SqlValue::Null);
        } else {
            if let Some(v) = input.username_enc {
                cols.push("username_enc");
                values.push(SqlValue::Blob(v));
            }
            if let Some(v) = input.password_enc {
                cols.push("password_enc");
                values.push(SqlValue::Blob(v));
            }
        }

        {
            let conn = self.lock();
            let exists = conn
                .query_row("SELECT 1 FROM proxies WHERE id = ?1", params![id], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();
            if !exists {
                return Err(AppError::NotFound(format!("proxy {id}")));
            }
            if !cols.is_empty() {
                let assignments: Vec<String> = cols
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{c} = ?{}", i + 1))
                    .collect();
                let sql = format!(
                    "UPDATE proxies SET {}, updated_at = ?{} WHERE id = ?{}",
                    assignments.join(", "),
                    cols.len() + 1,
                    cols.len() + 2,
                );
                values.push(sql_text(now()));
                values.push(sql_text(id.to_string()));
                conn.execute(&sql, rusqlite::params_from_iter(values))?;
            }
        }
        self.get_proxy(id)
    }

    /// Xoá proxy (cascade gỡ mọi gán trong profile_proxy). Trả `true` nếu có xoá.
    pub fn delete_proxy(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM proxies WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Cập nhật kết quả health-check ("ok"/"fail"), set `last_checked_at` = now.
    pub fn set_proxy_health(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE proxies SET health_status = ?1, last_checked_at = ?2 WHERE id = ?3",
            params![status, now(), id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("proxy {id}")));
        }
        Ok(())
    }

    /// Gán proxy cho profile (`Some(proxy_id)`) hoặc bỏ gán (`None`) —
    /// khớp command `assign_proxy(profile_id, proxy_id?)`.
    pub fn assign_proxy(&self, profile_id: &str, proxy_id: Option<&str>) -> Result<()> {
        let conn = self.lock();
        let exists = conn
            .query_row(
                "SELECT 1 FROM profiles WHERE id = ?1",
                params![profile_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::NotFound(format!("profile {profile_id}")));
        }
        match proxy_id {
            Some(pid) => {
                let proxy_exists = conn
                    .query_row("SELECT 1 FROM proxies WHERE id = ?1", params![pid], |_| {
                        Ok(())
                    })
                    .optional()?
                    .is_some();
                if !proxy_exists {
                    return Err(AppError::NotFound(format!("proxy {pid}")));
                }
                conn.execute(
                    "INSERT OR REPLACE INTO profile_proxy (profile_id, proxy_id) VALUES (?1, ?2)",
                    params![profile_id, pid],
                )?;
            }
            None => {
                conn.execute(
                    "DELETE FROM profile_proxy WHERE profile_id = ?1",
                    params![profile_id],
                )?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tags, settings, audit
// ---------------------------------------------------------------------------

impl Db {
    /// Danh sách mọi tag (kèm màu) — khớp command `list_tags()`.
    pub fn list_tags(&self) -> Result<Vec<TagInfo>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT tag, color FROM tags ORDER BY tag")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TagInfo {
                    tag: r.get(0)?,
                    color: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Đặt màu cho 1 tag (tạo tag nếu chưa có).
    pub fn set_tag_color(&self, tag: &str, color: Option<&str>) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO tags (tag, color) VALUES (?1, ?2)
             ON CONFLICT(tag) DO UPDATE SET color = excluded.color",
            params![tag, color],
        )?;
        Ok(())
    }

    /// Thay toàn bộ tags của profile — khớp command `set_profile_tags(id, tags[])`.
    pub fn set_profile_tags(&self, profile_id: &str, tags: &[String]) -> Result<()> {
        let conn = self.lock();
        let exists = conn
            .query_row(
                "SELECT 1 FROM profiles WHERE id = ?1",
                params![profile_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::NotFound(format!("profile {profile_id}")));
        }
        set_profile_tags_tx(&conn, profile_id, tags)
    }

    /// Đọc 1 setting (None nếu chưa đặt).
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v)
    }

    /// Ghi (upsert) 1 setting — khớp command `set_setting(key, value)`.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Toàn bộ settings (key → value) — khớp command `get_settings()`.
    pub fn get_settings(&self) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Ghi 1 dòng audit (action ví dụ: "profile.create", "profile.launch").
    pub fn insert_audit(
        &self,
        action: &str,
        target_id: Option<&str>,
        meta: Option<&serde_json::Value>,
    ) -> Result<i64> {
        let meta_str = meta.map(serde_json::to_string).transpose()?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO audit (ts, action, target_id, meta) VALUES (?1, ?2, ?3, ?4)",
            params![now(), action, target_id, meta_str],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Đọc audit log mới nhất trước, tối đa `limit` dòng.
    pub fn list_audit(&self, limit: u32) -> Result<Vec<AuditEntry>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts, action, target_id, meta FROM audit ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                let meta_raw: Option<String> = r.get(4)?;
                Ok(AuditEntry {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    action: r.get(2)?,
                    target_id: r.get(3)?,
                    meta: meta_raw.and_then(|s| serde_json::from_str(&s).ok()),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_db() -> (Db, TempDir) {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let db = Db::open_at_dir(&dir).expect("open temp db");
        (db, TempDir(dir))
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        {
            let db = Db::open_at_dir(&dir).unwrap();
            db.create_profile(ProfileInput {
                name: "p1".into(),
                ..Default::default()
            })
            .unwrap();
        }
        // Mở lại cùng file DB → migrate chạy lại, không lỗi, dữ liệu còn nguyên.
        let db = Db::open_at_dir(&dir).unwrap();
        assert_eq!(db.list_profiles().unwrap().len(), 1);
        // Gọi migrate thẳng thêm lần nữa cũng không lỗi.
        migrate(&db.lock()).unwrap();
    }

    #[test]
    fn profile_crud_roundtrip() {
        let (db, _guard) = temp_db();

        let created = db
            .create_profile(ProfileInput {
                name: "Work — FB".into(),
                platform: Some("macos".into()),
                timezone: Some("Asia/Ho_Chi_Minh".into()),
                launch_args: Some(serde_json::json!(["--lang=vi"])),
                tags: Some(vec!["work".into(), "fb".into()]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(created.platform, "macos");
        assert_eq!(created.screen_width, 1920);
        assert_eq!(created.tags, vec!["fb".to_string(), "work".to_string()]);
        assert!(created.user_data_dir.contains(&created.id));
        assert!(created.fingerprint_seed.parse::<u32>().is_ok());

        let fetched = db.get_profile(&created.id).unwrap();
        assert_eq!(fetched.launch_args, serde_json::json!(["--lang=vi"]));

        let updated = db
            .update_profile(
                &created.id,
                ProfileUpdate {
                    name: Some("Work — IG".into()),
                    headless: Some(true),
                    tags: Some(vec!["ig".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Work — IG");
        assert!(updated.headless);
        assert_eq!(updated.tags, vec!["ig".to_string()]);
        assert_eq!(updated.timezone.as_deref(), Some("Asia/Ho_Chi_Minh"));
        assert!(updated.updated_at >= created.updated_at);

        // Search theo tên + lọc tag.
        assert_eq!(db.search_profiles("work", None).unwrap().len(), 1);
        assert_eq!(db.search_profiles("work", Some("ig")).unwrap().len(), 1);
        assert_eq!(db.search_profiles("work", Some("fb")).unwrap().len(), 0);
        assert_eq!(db.search_profiles("nope", None).unwrap().len(), 0);

        assert!(db.delete_profile(&created.id).unwrap());
        assert!(!db.delete_profile(&created.id).unwrap());
        assert!(matches!(
            db.get_profile(&created.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn proxy_crud_and_assign() {
        let (db, _guard) = temp_db();

        let proxy = db
            .create_proxy(ProxyInput {
                name: "vn-proxy".into(),
                protocol: "socks5".into(),
                host: "127.0.0.1".into(),
                port: 1080,
                username_enc: Some(vec![1, 2, 3]),
                password_enc: Some(vec![4, 5, 6]),
            })
            .unwrap();
        assert_eq!(proxy.username_enc.as_deref(), Some(&[1u8, 2, 3][..]));
        assert!(proxy.health_status.is_none());

        // Protocol không hỗ trợ → InvalidInput.
        assert!(matches!(
            db.create_proxy(ProxyInput {
                name: "bad".into(),
                protocol: "ftp".into(),
                host: "h".into(),
                port: 1,
                ..Default::default()
            }),
            Err(AppError::InvalidInput(_))
        ));

        let updated = db
            .update_proxy(
                &proxy.id,
                ProxyUpdate {
                    port: Some(9050),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.port, 9050);
        assert_eq!(updated.username_enc.as_deref(), Some(&[1u8, 2, 3][..]));

        db.set_proxy_health(&proxy.id, "ok").unwrap();
        let checked = db.get_proxy(&proxy.id).unwrap();
        assert_eq!(checked.health_status.as_deref(), Some("ok"));
        assert!(checked.last_checked_at.is_some());

        // Gán proxy → get_profile trả proxy_id; bỏ gán → None.
        let p = db
            .create_profile(ProfileInput {
                name: "p".into(),
                ..Default::default()
            })
            .unwrap();
        db.assign_proxy(&p.id, Some(&proxy.id)).unwrap();
        assert_eq!(
            db.get_profile(&p.id).unwrap().proxy_id.as_deref(),
            Some(proxy.id.as_str())
        );
        db.assign_proxy(&p.id, None).unwrap();
        assert!(db.get_profile(&p.id).unwrap().proxy_id.is_none());

        // Xoá proxy đang gán → cascade gỡ gán.
        db.assign_proxy(&p.id, Some(&proxy.id)).unwrap();
        assert!(db.delete_proxy(&proxy.id).unwrap());
        assert!(db.get_profile(&p.id).unwrap().proxy_id.is_none());
        assert_eq!(db.list_proxies().unwrap().len(), 0);
    }

    #[test]
    fn tags_settings_audit() {
        let (db, _guard) = temp_db();

        let p = db
            .create_profile(ProfileInput {
                name: "p".into(),
                tags: Some(vec!["a".into()]),
                ..Default::default()
            })
            .unwrap();
        db.set_profile_tags(&p.id, &["b".into(), "c".into()])
            .unwrap();
        assert_eq!(
            db.get_profile(&p.id).unwrap().tags,
            vec!["b".to_string(), "c".to_string()]
        );
        db.set_tag_color("b", Some("#ff0000")).unwrap();
        let tags = db.list_tags().unwrap();
        assert!(tags
            .iter()
            .any(|t| t.tag == "b" && t.color.as_deref() == Some("#ff0000")));

        db.set_setting("theme", "dark").unwrap();
        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("light"));
        assert!(db.get_setting("missing").unwrap().is_none());
        assert_eq!(
            db.get_settings().unwrap(),
            vec![("theme".to_string(), "light".to_string())]
        );

        db.insert_audit(
            "profile.create",
            Some(&p.id),
            Some(&serde_json::json!({"n": 1})),
        )
        .unwrap();
        db.insert_audit("profile.launch", Some(&p.id), None)
            .unwrap();
        let log = db.list_audit(10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].action, "profile.launch"); // mới nhất trước
        assert_eq!(log[1].meta, Some(serde_json::json!({"n": 1})));
    }

    #[test]
    fn thousand_profiles_list_and_search_are_fast() {
        let (db, _guard) = temp_db();

        let t_insert = std::time::Instant::now();
        for i in 0..1000 {
            db.create_profile(ProfileInput {
                name: format!("profile-{i:04}"),
                tags: if i % 10 == 0 {
                    Some(vec!["vip".into()])
                } else {
                    None
                },
                ..Default::default()
            })
            .unwrap();
        }
        let insert_ms = t_insert.elapsed().as_millis();

        let t_list = std::time::Instant::now();
        let all = db.list_profiles().unwrap();
        let list_ms = t_list.elapsed().as_millis();
        assert_eq!(all.len(), 1000);

        let t_search = std::time::Instant::now();
        let hits = db.search_profiles("profile-09", None).unwrap();
        let search_ms = t_search.elapsed().as_millis();
        assert_eq!(hits.len(), 100); // profile-0900..profile-0999

        let t_tag = std::time::Instant::now();
        let vips = db.search_profiles("profile", Some("vip")).unwrap();
        let tag_ms = t_tag.elapsed().as_millis();
        assert_eq!(vips.len(), 100);

        println!(
            "1000 profiles: insert={insert_ms}ms list={list_ms}ms search={search_ms}ms tag_search={tag_ms}ms"
        );
        // Mục tiêu docs: <200ms p95 (release). Debug build chậm hơn → ngưỡng 1500ms chống flaky.
        assert!(list_ms < 1500, "list_profiles too slow: {list_ms}ms");
        assert!(search_ms < 1500, "search_profiles too slow: {search_ms}ms");
        assert!(tag_ms < 1500, "tag search too slow: {tag_ms}ms");
    }
}
