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
use crate::models::{Extension, Folder, Profile, ProfileTemplate};

// ---------------------------------------------------------------------------
// Type nội bộ của DB layer (W3a map sang models::Proxy sau khi giải mã)
// ---------------------------------------------------------------------------

/// Input tạo profile mới. Field `None` → dùng default (giống semantics
/// `create_profile` Python: seed random 10000–99999, platform "windows",
/// screen 1920×1080, human_preset "default", user_data_dir `<data_dir>/profiles/<id>`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileInput {
    /// `#[serde(default)]` để deserialize được từ template config không có name
    /// (create_profile vẫn validate name không rỗng).
    #[serde(default)]
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
    /// "restore" | "custom" (None → "restore").
    pub startup_behavior: Option<String>,
    /// Mảng JSON URL mở khi khởi động (dùng khi startup_behavior = "custom").
    pub startup_urls: Option<serde_json::Value>,
    /// (W24b) Mảng JSON đường dẫn unpacked extension local.
    pub extensions: Option<serde_json::Value>,
    /// Gán proxy ngay khi tạo (FK → proxies.id).
    pub proxy_id: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Đánh dấu quick profile (dùng-xong-xoá, W18b). None = false = profile thường.
    pub is_quick: Option<bool>,
    /// (W19c) Noise injection master switch (None → default true = bật).
    pub fp_noise: Option<bool>,
    /// (W19c) WebRTC mode "real" | "masked" (None → "real").
    pub webrtc_mode: Option<String>,
    /// (W19c) IP spoof WebRTC khi masked.
    pub webrtc_ip: Option<String>,
    /// (W19c) Geolocation mode "auto" | "manual" (None → "auto").
    pub geolocation_mode: Option<String>,
    pub geo_latitude: Option<String>,
    pub geo_longitude: Option<String>,
    /// (W20b) Lưu lịch sử duyệt web (None → default true).
    pub store_history: Option<bool>,
    /// (W20b) Lưu mật khẩu (None → default true).
    pub store_passwords: Option<bool>,
    /// (W20b) Giữ service-worker cache (None → default true).
    pub store_sw_cache: Option<bool>,
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
    /// "restore" | "custom".
    pub startup_behavior: Option<String>,
    /// Mảng JSON URL mở khi khởi động (dùng khi startup_behavior = "custom").
    pub startup_urls: Option<serde_json::Value>,
    /// (W24b) Mảng JSON đường dẫn unpacked extension local.
    pub extensions: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
    /// (W19c) Noise injection master switch.
    pub fp_noise: Option<bool>,
    /// (W19c) WebRTC mode "real" | "masked".
    pub webrtc_mode: Option<String>,
    /// (W19c) IP spoof WebRTC khi masked.
    pub webrtc_ip: Option<String>,
    /// (W19c) Geolocation mode "auto" | "manual".
    pub geolocation_mode: Option<String>,
    pub geo_latitude: Option<String>,
    pub geo_longitude: Option<String>,
    /// (W20b) Lưu lịch sử duyệt web.
    pub store_history: Option<bool>,
    /// (W20b) Lưu mật khẩu.
    pub store_passwords: Option<bool>,
    /// (W20b) Giữ service-worker cache.
    pub store_sw_cache: Option<bool>,
}

/// (P3-2a) Bộ lọc nâng cao cho [`Db::search_profiles`] — chỉ gồm tiêu chí có
/// cột thật trong DB. Field `None` = bỏ qua tiêu chí; filter rỗng
/// (`ProfileFilter::default()`) = hành vi cũ (chỉ lọc theo tên).
/// Trạng thái running/stopped là runtime (ProcessManager) và dung lượng
/// storage không có cột trong DB → các tiêu chí đó lọc ở FE, KHÔNG vào SQL.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileFilter {
    /// Target OS fingerprint (cột `profiles.platform`): "windows" | "macos" | "linux".
    pub os: Option<String>,
    /// `true` = chỉ profile có gán proxy (bảng `profile_proxy`), `false` = chỉ
    /// profile chưa gán.
    pub has_proxy: Option<bool>,
    /// Chỉ profile có tag này (bảng `profile_tags`, so khớp chính xác).
    pub tag: Option<String>,
    /// Chỉ profile thuộc folder này (cột `profiles.folder_id`).
    pub folder_id: Option<String>,
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

/// (P3-3a) Bản ghi proxy template trong DB (bảng `proxy_templates`): cấu hình
/// proxy dùng lại được. Credential là BLOB **đã mã hoá** bởi `crypto` như
/// `proxies`. `sticky_session`/`traffic_saver` là metadata theo ngữ nghĩa
/// NHÀ CUNG CẤP proxy (điều khiển qua username/host convention riêng từng
/// nhà cung cấp) — KHÔNG map ra flag launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTemplateRecord {
    pub id: String,
    pub name: String,
    /// "http" | "https" | "socks5".
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username_enc: Option<Vec<u8>>,
    pub password_enc: Option<Vec<u8>>,
    pub sticky_session: bool,
    pub traffic_saver: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// (P3-3a) Input tạo proxy template mới (credential đã mã hoá sẵn).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyTemplateInput {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username_enc: Option<Vec<u8>>,
    pub password_enc: Option<Vec<u8>>,
    #[serde(default)]
    pub sticky_session: bool,
    #[serde(default)]
    pub traffic_saver: bool,
}

/// (P3-3a) Update proxy template từng phần; `clear_credentials=true` xoá cả 2
/// blob về NULL (giống [`ProxyUpdate`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyTemplateUpdate {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username_enc: Option<Vec<u8>>,
    pub password_enc: Option<Vec<u8>>,
    pub sticky_session: Option<bool>,
    pub traffic_saver: Option<bool>,
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
const SCHEMA_VERSION: i64 = 10;

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

/// Migration v1→v2 (docs/08 UI Multilogin): folders + favorite + trash (soft-delete).
/// ALTER TABLE không có IF NOT EXISTS — idempotency đảm bảo bởi guard `user_version < 2`.
const SCHEMA_V2: &str = "
CREATE TABLE IF NOT EXISTS folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

ALTER TABLE profiles ADD COLUMN folder_id TEXT REFERENCES folders(id);
ALTER TABLE profiles ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
ALTER TABLE profiles ADD COLUMN deleted_at TEXT;

CREATE INDEX IF NOT EXISTS idx_profiles_folder_id ON profiles(folder_id);
CREATE INDEX IF NOT EXISTS idx_profiles_deleted_at ON profiles(deleted_at);
";

/// Migration v2→v3: `last_start_at` — lần launch thành công gần nhất (RFC3339 UTC, nullable).
/// ALTER TABLE không có IF NOT EXISTS — idempotency đảm bảo bởi guard `user_version < 3`.
const SCHEMA_V3: &str = "
ALTER TABLE profiles ADD COLUMN last_start_at TEXT;
";

/// Migration v3→v4 (W18b): `is_quick` — quick profile dùng-xong-xoá; khi Stop
/// UI hỏi Save as regular (bỏ cờ) / Close & delete (purge data).
/// ALTER TABLE không có IF NOT EXISTS — idempotency đảm bảo bởi guard `user_version < 4`.
const SCHEMA_V4: &str = "
ALTER TABLE profiles ADD COLUMN is_quick INTEGER NOT NULL DEFAULT 0;
";

/// Migration v4→v5 (W18c): hành vi khởi động — `startup_behavior` ("restore" |
/// "custom") + `startup_urls` (JSON array chuỗi, dùng khi "custom").
/// ALTER TABLE không có IF NOT EXISTS — idempotency đảm bảo bởi guard `user_version < 5`.
const SCHEMA_V5: &str = "
ALTER TABLE profiles ADD COLUMN startup_behavior TEXT NOT NULL DEFAULT 'restore';
ALTER TABLE profiles ADD COLUMN startup_urls TEXT NOT NULL DEFAULT '[]';
";

/// Migration v5→v6 (W19c): fingerprint controls map sang flag CloakBrowser thật —
/// `fp_noise` (--fingerprint-noise=false khi tắt), `webrtc_mode`/`webrtc_ip`
/// (--fingerprint-webrtc-ip), `geolocation_mode` + `geo_latitude`/`geo_longitude`
/// (--fingerprint-location=lat,lon). ALTER TABLE không có IF NOT EXISTS —
/// idempotency đảm bảo bởi guard `user_version < 6`.
const SCHEMA_V6: &str = "
ALTER TABLE profiles ADD COLUMN fp_noise INTEGER NOT NULL DEFAULT 1;
ALTER TABLE profiles ADD COLUMN webrtc_mode TEXT NOT NULL DEFAULT 'real';
ALTER TABLE profiles ADD COLUMN webrtc_ip TEXT;
ALTER TABLE profiles ADD COLUMN geolocation_mode TEXT NOT NULL DEFAULT 'auto';
ALTER TABLE profiles ADD COLUMN geo_latitude TEXT;
ALTER TABLE profiles ADD COLUMN geo_longitude TEXT;
";

/// Migration v6→v7 (W20b): profile templates + storage options per-profile.
/// - `profile_templates`: config JSON shape `ProfileInput` (giống pattern
///   launch_args/startup_urls lưu JSON TEXT).
/// - `store_history`/`store_passwords`/`store_sw_cache`: default 1 (giữ dữ liệu).
///   `0` → dọn file tương ứng khi phiên dừng (binary không có flag disable —
///   cơ chế là cleanup, xem `storage::clear_storage_options`).
///
/// ALTER TABLE không có IF NOT EXISTS — idempotency đảm bảo bởi guard `user_version < 7`.
const SCHEMA_V7: &str = "
CREATE TABLE IF NOT EXISTS profile_templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

ALTER TABLE profiles ADD COLUMN store_history INTEGER NOT NULL DEFAULT 1;
ALTER TABLE profiles ADD COLUMN store_passwords INTEGER NOT NULL DEFAULT 1;
ALTER TABLE profiles ADD COLUMN store_sw_cache INTEGER NOT NULL DEFAULT 1;

CREATE INDEX IF NOT EXISTS idx_profile_templates_name ON profile_templates(name);
";

/// Migration v7→v8 (W24b): `extensions` — mảng JSON đường dẫn unpacked extension
/// local per-profile (giống pattern launch_args/startup_urls lưu JSON TEXT).
/// ALTER TABLE không có IF NOT EXISTS — idempotency đảm bảo bởi guard `user_version < 8`.
const SCHEMA_V8: &str = "
ALTER TABLE profiles ADD COLUMN extensions TEXT NOT NULL DEFAULT '[]';
";

/// Migration v8→v9 (P3-1a): kho extension trung tâm — bảng `extensions`
/// (folder unpacked local hoặc tải từ Chrome Web Store) + `profile_extensions`
/// N-N (giống pattern profile_tags, CASCADE cả 2 chiều).
const SCHEMA_V9: &str = "
CREATE TABLE IF NOT EXISTS extensions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'folder',
    source_ref TEXT NOT NULL,
    unpacked_path TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS profile_extensions (
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
    PRIMARY KEY (profile_id, extension_id)
);

CREATE INDEX IF NOT EXISTS idx_profile_extensions_ext ON profile_extensions(extension_id);
";

/// Migration v9→v10 (P3-3a): proxy templates — cấu hình proxy dùng lại được
/// (protocol/host/port + credential BLOB mã hoá bởi `crypto` như `proxies`).
/// `sticky_session`/`traffic_saver` là ngữ nghĩa NHÀ CUNG CẤP proxy (điều
/// khiển qua username/host suffix theo convention riêng từng nhà cung cấp,
/// KHÔNG có flag Chromium/CloakBrowser tương ứng) → chỉ lưu metadata,
/// không áp vào launch.
const SCHEMA_V10: &str = "
CREATE TABLE IF NOT EXISTS proxy_templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'http',
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username_enc BLOB,
    password_enc BLOB,
    sticky_session INTEGER NOT NULL DEFAULT 0,
    traffic_saver INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_proxy_templates_name ON proxy_templates(name);
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
        let dir = home.join(".browserx");
        // (W25b) TRƯỚC khi mở DB (open_at_dir tạo dir trống nếu thiếu → nhìn
        // như mất sạch dữ liệu): hoàn tất/rollback swap restore bị kill dở.
        if let Some(action) = crate::backup::recover_interrupted_restore(&dir)? {
            tracing::warn!("recovered interrupted restore swap at startup: {action:?}");
        }
        Self::open_at_dir(dir)
    }

    /// Mở DB tại `<dir>/browserx.db` — dùng cho test hoặc data-dir tuỳ biến.
    /// Nếu DB cũ hơn [`SCHEMA_VERSION`] → backup file trước khi migrate
    /// (browserx.db.bak-v{N}, N = version cũ) để user còn đường lùi nếu hỏng.
    pub fn open_at_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("browserx.db");
        let conn = Connection::open(&db_path)?;
        Self::init_conn(&conn)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 0 && version < SCHEMA_VERSION {
            // Checkpoint WAL về file chính để bản copy là snapshot đầy đủ.
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            backup_db_file(&db_path, version)?;
        }
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

    /// (W23a) Checkpoint WAL về file DB chính rồi cắt file `-wal` về 0
    /// (`PRAGMA wal_checkpoint(TRUNCATE)`) — gọi trước khi thoát app sạch.
    pub fn wal_checkpoint_truncate(&self) -> Result<()> {
        let conn = self.lock();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }
}

/// Số bản backup `browserx.db.bak-v{N}` giữ lại tối đa (bản version cũ nhất bị xoá trước).
const MAX_DB_BACKUPS: usize = 3;

/// Copy file DB → `<file>.bak-v{N}` (N = user_version cũ) trước khi migrate;
/// ghi đè bản backup cùng version nếu đã có, rồi tỉa bớt chỉ giữ
/// [`MAX_DB_BACKUPS`] bản mới nhất.
fn backup_db_file(db_path: &Path, version: i64) -> Result<()> {
    let dir = db_path.parent().unwrap_or(Path::new("."));
    let file_name = db_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("browserx.db");
    std::fs::copy(db_path, dir.join(format!("{file_name}.bak-v{version}")))?;
    // Tỉa backup cũ theo suffix ".bak-vN" — giữ các N lớn nhất.
    let prefix = format!("{file_name}.bak-v");
    let mut versions: Vec<i64> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        if let Some(v) = entry?
            .file_name()
            .to_str()
            .and_then(|n| n.strip_prefix(&prefix))
            .and_then(|rest| rest.parse::<i64>().ok())
        {
            versions.push(v);
        }
    }
    versions.sort_unstable();
    while versions.len() > MAX_DB_BACKUPS {
        let v = versions.remove(0);
        let _ = std::fs::remove_file(dir.join(format!("{prefix}{v}")));
    }
    Ok(())
}

/// Migration idempotent theo `PRAGMA user_version`; chạy lại không lỗi
/// (mọi CREATE đều `IF NOT EXISTS`). Migration tương lai: thêm nhánh `< 2`, `< 3`…
fn migrate(conn: &Connection) -> Result<()> {
    migrate_inner(conn, |_| Ok(()))
}

/// Toàn bộ chuỗi migration chạy trong MỘT transaction (BEGIN IMMEDIATE … COMMIT):
/// `PRAGMA user_version` có transactional trong SQLite nên được set bên trong —
/// kill/fail giữa chừng → rollback nguyên vẹn về version cũ, không nửa vời.
/// `checkpoint(v)` được gọi sau khi áp xong bậc schema v — production là no-op,
/// test dùng để inject lỗi mô phỏng crash giữa migration.
fn migrate_inner(conn: &Connection, checkpoint: impl Fn(i64) -> Result<()>) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            checkpoint(1)?;
        }
        if version < 2 {
            conn.execute_batch(SCHEMA_V2)?;
            // Seed 1 folder mặc định nếu bảng rỗng (UI Multilogin luôn có "Default folder").
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0))?;
            if count == 0 {
                conn.execute(
                    "INSERT INTO folders (id, name, created_at) VALUES (?1, ?2, ?3)",
                    params![uuid::Uuid::new_v4().to_string(), "Default folder", now()],
                )?;
            }
            checkpoint(2)?;
        }
        if version < 3 {
            conn.execute_batch(SCHEMA_V3)?;
            checkpoint(3)?;
        }
        if version < 4 {
            conn.execute_batch(SCHEMA_V4)?;
            checkpoint(4)?;
        }
        if version < 5 {
            conn.execute_batch(SCHEMA_V5)?;
            checkpoint(5)?;
        }
        if version < 6 {
            conn.execute_batch(SCHEMA_V6)?;
            checkpoint(6)?;
        }
        if version < 7 {
            conn.execute_batch(SCHEMA_V7)?;
            checkpoint(7)?;
        }
        if version < 8 {
            conn.execute_batch(SCHEMA_V8)?;
            checkpoint(8)?;
        }
        if version < 9 {
            conn.execute_batch(SCHEMA_V9)?;
            checkpoint(9)?;
        }
        if version < 10 {
            conn.execute_batch(SCHEMA_V10)?;
            checkpoint(10)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Thời điểm hiện tại, RFC3339 UTC (tương đương `_now()` Python).
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// `startup_behavior` chỉ nhận "restore" | "custom".
fn validate_startup_behavior(v: &str) -> Result<()> {
    match v {
        "restore" | "custom" => Ok(()),
        other => Err(AppError::InvalidInput(format!(
            "startup_behavior must be \"restore\" or \"custom\", got {other:?}"
        ))),
    }
}

/// (W19c) `webrtc_mode` chỉ nhận "real" | "masked".
fn validate_webrtc_mode(v: &str) -> Result<()> {
    match v {
        "real" | "masked" => Ok(()),
        other => Err(AppError::InvalidInput(format!(
            "webrtc_mode must be \"real\" or \"masked\", got {other:?}"
        ))),
    }
}

/// (W19c) `geolocation_mode` chỉ nhận "auto" | "manual".
fn validate_geolocation_mode(v: &str) -> Result<()> {
    match v {
        "auto" | "manual" => Ok(()),
        other => Err(AppError::InvalidInput(format!(
            "geolocation_mode must be \"auto\" or \"manual\", got {other:?}"
        ))),
    }
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

/// Chuỗi placeholder "?start, ?start+1, …" cho mệnh đề `IN (…)` với `n` phần tử.
fn placeholders(start: usize, n: usize) -> String {
    (start..start + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Map một row (SELECT p.*, pp.proxy_id) → `models::Profile` (tags rỗng, caller tự điền).
fn row_to_profile(row: &Row) -> rusqlite::Result<Profile> {
    let launch_args_raw: String = row.get("launch_args")?;
    let launch_args: serde_json::Value =
        serde_json::from_str(&launch_args_raw).unwrap_or_else(|_| serde_json::Value::Array(vec![]));
    let startup_urls_raw: String = row.get("startup_urls")?;
    let startup_urls: serde_json::Value = serde_json::from_str(&startup_urls_raw)
        .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
    let extensions_raw: String = row.get("extensions")?;
    let extensions: serde_json::Value = serde_json::from_str(&extensions_raw)
        .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
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
        folder_id: row.get("folder_id")?,
        favorite: row.get("favorite")?,
        is_quick: row.get("is_quick")?,
        proxy_id: row.get("proxy_id")?,
        tags: Vec::new(),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_start_at: row.get("last_start_at")?,
        startup_behavior: row.get("startup_behavior")?,
        startup_urls,
        fp_noise: row.get("fp_noise")?,
        webrtc_mode: row.get("webrtc_mode")?,
        webrtc_ip: row.get("webrtc_ip")?,
        geolocation_mode: row.get("geolocation_mode")?,
        geo_latitude: row.get("geo_latitude")?,
        geo_longitude: row.get("geo_longitude")?,
        store_history: row.get("store_history")?,
        store_passwords: row.get("store_passwords")?,
        store_sw_cache: row.get("store_sw_cache")?,
        extensions,
    })
}

/// Map một row của [`FOLDER_SELECT`] → `models::Folder`.
fn row_to_folder(row: &Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get("id")?,
        name: row.get("name")?,
        profile_count: row.get("profile_count")?,
        created_at: row.get("created_at")?,
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
        if let Some(b) = input.startup_behavior.as_deref() {
            validate_startup_behavior(b)?;
        }
        if let Some(m) = input.webrtc_mode.as_deref() {
            validate_webrtc_mode(m)?;
        }
        if let Some(m) = input.geolocation_mode.as_deref() {
            validate_geolocation_mode(m)?;
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
        let startup_urls = input
            .startup_urls
            .unwrap_or_else(|| serde_json::Value::Array(vec![]));
        let extensions = input
            .extensions
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
                    color_scheme, launch_args, user_data_dir, notes, created_at, updated_at,
                    is_quick, startup_behavior, startup_urls,
                    fp_noise, webrtc_mode, webrtc_ip, geolocation_mode, geo_latitude, geo_longitude,
                    store_history, store_passwords, store_sw_cache, extensions
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34)",
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
                    input.is_quick.unwrap_or(false),
                    input.startup_behavior.as_deref().unwrap_or("restore"),
                    serde_json::to_string(&startup_urls)?,
                    input.fp_noise.unwrap_or(true),
                    input.webrtc_mode.as_deref().unwrap_or("real"),
                    input.webrtc_ip,
                    input.geolocation_mode.as_deref().unwrap_or("auto"),
                    input.geo_latitude,
                    input.geo_longitude,
                    input.store_history.unwrap_or(true),
                    input.store_passwords.unwrap_or(true),
                    input.store_sw_cache.unwrap_or(true),
                    serde_json::to_string(&extensions)?,
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

    /// Danh sách profile còn sống (không tính trash), mới cập nhật trước
    /// (batch-load tags, không N+1).
    pub fn list_profiles(&self) -> Result<Vec<Profile>> {
        let conn = self.lock();
        let sql = format!("{PROFILE_SELECT} WHERE p.deleted_at IS NULL ORDER BY p.updated_at DESC");
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

    /// Search theo tên (LIKE, không phân biệt hoa thường ASCII) + bộ lọc đa
    /// tiêu chí (P3-2a). Filter rỗng = hành vi cũ (chỉ lọc theo tên).
    /// SQL build động nhưng THAM SỐ HOÁ hoàn toàn — giá trị người dùng không
    /// bao giờ nối thẳng vào chuỗi SQL (chỉ số placeholder do code sinh).
    /// Chỉ trả profile còn sống (deleted_at IS NULL).
    pub fn search_profiles(&self, query: &str, filter: &ProfileFilter) -> Result<Vec<Profile>> {
        let conn = self.lock();
        let pattern = format!("%{}%", escape_like(query));
        let mut sql =
            format!("{PROFILE_SELECT} WHERE p.deleted_at IS NULL AND p.name LIKE ?1 ESCAPE '\\'");
        let mut values: Vec<SqlValue> = vec![sql_text(pattern)];
        if let Some(tag) = &filter.tag {
            values.push(sql_text(tag.clone()));
            sql.push_str(&format!(
                " AND p.id IN (SELECT profile_id FROM profile_tags WHERE tag = ?{})",
                values.len()
            ));
        }
        if let Some(os) = &filter.os {
            values.push(sql_text(os.clone()));
            sql.push_str(&format!(" AND p.platform = ?{}", values.len()));
        }
        if let Some(folder_id) = &filter.folder_id {
            values.push(sql_text(folder_id.clone()));
            sql.push_str(&format!(" AND p.folder_id = ?{}", values.len()));
        }
        if let Some(has_proxy) = filter.has_proxy {
            sql.push_str(if has_proxy {
                " AND pp.proxy_id IS NOT NULL"
            } else {
                " AND pp.proxy_id IS NULL"
            });
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
        if let Some(v) = input.startup_behavior {
            validate_startup_behavior(&v)?;
            cols.push("startup_behavior");
            values.push(sql_text(v));
        }
        if let Some(v) = input.startup_urls {
            cols.push("startup_urls");
            values.push(sql_text(serde_json::to_string(&v)?));
        }
        if let Some(v) = input.extensions {
            cols.push("extensions");
            values.push(sql_text(serde_json::to_string(&v)?));
        }
        if let Some(v) = input.fp_noise {
            cols.push("fp_noise");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.webrtc_mode {
            validate_webrtc_mode(&v)?;
            cols.push("webrtc_mode");
            values.push(sql_text(v));
        }
        if let Some(v) = input.webrtc_ip {
            cols.push("webrtc_ip");
            values.push(sql_text(v));
        }
        if let Some(v) = input.geolocation_mode {
            validate_geolocation_mode(&v)?;
            cols.push("geolocation_mode");
            values.push(sql_text(v));
        }
        if let Some(v) = input.geo_latitude {
            cols.push("geo_latitude");
            values.push(sql_text(v));
        }
        if let Some(v) = input.geo_longitude {
            cols.push("geo_longitude");
            values.push(sql_text(v));
        }
        if let Some(v) = input.store_history {
            cols.push("store_history");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.store_passwords {
            cols.push("store_passwords");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.store_sw_cache {
            cols.push("store_sw_cache");
            values.push(sql_bool(v));
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
// Trash (soft-delete) + favorite
// ---------------------------------------------------------------------------

impl Db {
    /// Danh sách profile trong thùng rác (deleted_at IS NOT NULL), mới cập nhật trước.
    pub fn list_trash(&self) -> Result<Vec<Profile>> {
        let conn = self.lock();
        let sql =
            format!("{PROFILE_SELECT} WHERE p.deleted_at IS NOT NULL ORDER BY p.updated_at DESC");
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

    /// Chuyển các profile vào thùng rác (set deleted_at = now). Trả số hàng thay đổi;
    /// id không tồn tại hoặc đã trong trash bị bỏ qua.
    pub fn trash_profiles(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.lock();
        let sql = format!(
            "UPDATE profiles SET deleted_at = ?1, updated_at = ?1
             WHERE deleted_at IS NULL AND id IN ({})",
            placeholders(2, ids.len())
        );
        let mut values: Vec<SqlValue> = vec![sql_text(now())];
        values.extend(ids.iter().cloned().map(sql_text));
        Ok(conn.execute(&sql, rusqlite::params_from_iter(values))?)
    }

    /// Khôi phục các profile từ thùng rác (deleted_at về NULL). Trả số hàng thay đổi.
    pub fn restore_profiles(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.lock();
        let sql = format!(
            "UPDATE profiles SET deleted_at = NULL, updated_at = ?1
             WHERE deleted_at IS NOT NULL AND id IN ({})",
            placeholders(2, ids.len())
        );
        let mut values: Vec<SqlValue> = vec![sql_text(now())];
        values.extend(ids.iter().cloned().map(sql_text));
        Ok(conn.execute(&sql, rusqlite::params_from_iter(values))?)
    }

    /// Xoá hẳn các profile khỏi DB (cascade tags/proxy assignment). Trả số hàng xoá.
    /// KHÔNG đụng `user_data_dir` trên filesystem — caller quyết định.
    pub fn purge_profiles(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.lock();
        let sql = format!(
            "DELETE FROM profiles WHERE id IN ({})",
            placeholders(1, ids.len())
        );
        let values: Vec<SqlValue> = ids.iter().cloned().map(sql_text).collect();
        Ok(conn.execute(&sql, rusqlite::params_from_iter(values))?)
    }

    /// Bật/tắt yêu thích cho 1 profile. `NotFound` nếu không tồn tại.
    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE profiles SET favorite = ?1, updated_at = ?2 WHERE id = ?3",
            params![favorite, now(), id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("profile {id}")));
        }
        Ok(())
    }

    /// Bật/tắt cờ quick (dùng-xong-xoá, W18b) cho 1 profile. `NotFound` nếu không
    /// tồn tại. "Save as regular" = `set_quick(id, false)` — giữ nguyên user_data_dir.
    pub fn set_quick(&self, id: &str, is_quick: bool) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE profiles SET is_quick = ?1, updated_at = ?2 WHERE id = ?3",
            params![is_quick, now(), id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("profile {id}")));
        }
        Ok(())
    }

    /// Ghi nhận launch thành công: `last_start_at = now` (KHÔNG đụng `updated_at`).
    /// `NotFound` nếu không tồn tại.
    pub fn touch_last_start(&self, id: &str) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE profiles SET last_start_at = ?1 WHERE id = ?2",
            params![now(), id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("profile {id}")));
        }
        Ok(())
    }

    /// Chuyển các profile vào folder (`Some(folder_id)`) hoặc ra ngoài (`None`).
    /// `NotFound` nếu folder không tồn tại. Trả số hàng thay đổi.
    pub fn move_profiles_to_folder(
        &self,
        ids: &[String],
        folder_id: Option<&str>,
    ) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.lock();
        if let Some(fid) = folder_id {
            let exists = conn
                .query_row("SELECT 1 FROM folders WHERE id = ?1", params![fid], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();
            if !exists {
                return Err(AppError::NotFound(format!("folder {fid}")));
            }
        }
        let sql = format!(
            "UPDATE profiles SET folder_id = ?1, updated_at = ?2 WHERE id IN ({})",
            placeholders(3, ids.len())
        );
        let mut values: Vec<SqlValue> = vec![
            folder_id.map_or(SqlValue::Null, |s| sql_text(s.to_string())),
            sql_text(now()),
        ];
        values.extend(ids.iter().cloned().map(sql_text));
        Ok(conn.execute(&sql, rusqlite::params_from_iter(values))?)
    }
}

// ---------------------------------------------------------------------------
// Folders CRUD
// ---------------------------------------------------------------------------

/// SELECT folder kèm `profile_count` (chỉ đếm profile còn sống — deleted_at IS NULL).
const FOLDER_SELECT: &str = "SELECT f.id, f.name, f.created_at, COUNT(p.id) AS profile_count
     FROM folders f
     LEFT JOIN profiles p ON p.folder_id = f.id AND p.deleted_at IS NULL";

impl Db {
    /// Danh sách folder (kèm số profile còn sống), thứ tự tạo trước → sau.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let conn = self.lock();
        let sql = format!("{FOLDER_SELECT} GROUP BY f.id ORDER BY f.created_at, f.name");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_folder)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Đọc 1 folder. `NotFound` nếu không tồn tại.
    pub fn get_folder(&self, id: &str) -> Result<Folder> {
        let conn = self.lock();
        let sql = format!("{FOLDER_SELECT} WHERE f.id = ?1 GROUP BY f.id");
        conn.query_row(&sql, params![id], row_to_folder)
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("folder {id}")))
    }

    /// Tạo folder mới. Tên trim, không rỗng, không trùng (UNIQUE).
    pub fn create_folder(&self, name: &str) -> Result<Folder> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "folder name must not be empty".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        {
            let conn = self.lock();
            let dup = conn
                .query_row("SELECT 1 FROM folders WHERE name = ?1", params![name], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();
            if dup {
                return Err(AppError::InvalidInput(format!(
                    "folder name already exists: {name}"
                )));
            }
            conn.execute(
                "INSERT INTO folders (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![id, name, now()],
            )?;
        }
        self.get_folder(&id)
    }

    /// Đổi tên folder. `NotFound` nếu không tồn tại, `InvalidInput` nếu tên rỗng/trùng.
    pub fn rename_folder(&self, id: &str, name: &str) -> Result<Folder> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "folder name must not be empty".into(),
            ));
        }
        {
            let conn = self.lock();
            let dup = conn
                .query_row(
                    "SELECT 1 FROM folders WHERE name = ?1 AND id != ?2",
                    params![name, id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if dup {
                return Err(AppError::InvalidInput(format!(
                    "folder name already exists: {name}"
                )));
            }
            let n = conn.execute(
                "UPDATE folders SET name = ?1 WHERE id = ?2",
                params![name, id],
            )?;
            if n == 0 {
                return Err(AppError::NotFound(format!("folder {id}")));
            }
        }
        self.get_folder(id)
    }

    /// Xoá folder; profiles thuộc folder về `folder_id = NULL` (không xoá profile).
    /// Trả `true` nếu có xoá.
    pub fn delete_folder(&self, id: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE profiles SET folder_id = NULL WHERE folder_id = ?1",
            params![id],
        )?;
        let n = tx.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        tx.commit()?;
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
// Proxy templates (P3-3a)
// ---------------------------------------------------------------------------

/// Map một row của bảng `proxy_templates` → [`ProxyTemplateRecord`].
fn row_to_proxy_template(row: &Row) -> rusqlite::Result<ProxyTemplateRecord> {
    Ok(ProxyTemplateRecord {
        id: row.get("id")?,
        name: row.get("name")?,
        protocol: row.get("protocol")?,
        host: row.get("host")?,
        port: row.get("port")?,
        username_enc: row.get("username_enc")?,
        password_enc: row.get("password_enc")?,
        sticky_session: row.get("sticky_session")?,
        traffic_saver: row.get("traffic_saver")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl Db {
    /// Tạo proxy template mới. Credential đã mã hoá sẵn bởi `crypto` (như
    /// [`Db::create_proxy`]). Tên trim, không rỗng; validate protocol/host
    /// giống proxies.
    pub fn create_proxy_template(&self, input: ProxyTemplateInput) -> Result<ProxyTemplateRecord> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "proxy template name must not be empty".into(),
            ));
        }
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
        {
            let conn = self.lock();
            conn.execute(
                "INSERT INTO proxy_templates (
                    id, name, protocol, host, port, username_enc, password_enc,
                    sticky_session, traffic_saver, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    id,
                    name,
                    input.protocol,
                    input.host,
                    input.port,
                    input.username_enc,
                    input.password_enc,
                    input.sticky_session,
                    input.traffic_saver,
                    ts,
                    ts,
                ],
            )?;
        }
        self.get_proxy_template(&id)
    }

    /// Đọc 1 proxy template. `NotFound` nếu không tồn tại.
    pub fn get_proxy_template(&self, id: &str) -> Result<ProxyTemplateRecord> {
        let conn = self.lock();
        conn.query_row(
            "SELECT * FROM proxy_templates WHERE id = ?1",
            params![id],
            row_to_proxy_template,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("proxy template {id}")))
    }

    /// Danh sách toàn bộ proxy template, mới cập nhật trước.
    pub fn list_proxy_templates(&self) -> Result<Vec<ProxyTemplateRecord>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT * FROM proxy_templates ORDER BY updated_at DESC, name")?;
        let rows = stmt
            .query_map([], row_to_proxy_template)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Update proxy template từng phần; `clear_credentials=true` xoá cả 2 blob
    /// về NULL (giống [`Db::update_proxy`]).
    pub fn update_proxy_template(
        &self,
        id: &str,
        input: ProxyTemplateUpdate,
    ) -> Result<ProxyTemplateRecord> {
        let mut cols: Vec<&str> = Vec::new();
        let mut values: Vec<SqlValue> = Vec::new();
        if let Some(v) = input.name {
            let v = v.trim().to_string();
            if v.is_empty() {
                return Err(AppError::InvalidInput(
                    "proxy template name must not be empty".into(),
                ));
            }
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
        if let Some(v) = input.sticky_session {
            cols.push("sticky_session");
            values.push(sql_bool(v));
        }
        if let Some(v) = input.traffic_saver {
            cols.push("traffic_saver");
            values.push(sql_bool(v));
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
                .query_row(
                    "SELECT 1 FROM proxy_templates WHERE id = ?1",
                    params![id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Err(AppError::NotFound(format!("proxy template {id}")));
            }
            if !cols.is_empty() {
                let assignments: Vec<String> = cols
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{c} = ?{}", i + 1))
                    .collect();
                let sql = format!(
                    "UPDATE proxy_templates SET {}, updated_at = ?{} WHERE id = ?{}",
                    assignments.join(", "),
                    cols.len() + 1,
                    cols.len() + 2,
                );
                values.push(sql_text(now()));
                values.push(sql_text(id.to_string()));
                conn.execute(&sql, rusqlite::params_from_iter(values))?;
            }
        }
        self.get_proxy_template(id)
    }

    /// Xoá proxy template. Trả `true` nếu có xoá (không đụng proxy đã tạo từ template).
    pub fn delete_proxy_template(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM proxy_templates WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Tạo proxy mới từ template: copy protocol/host/port + credential blob
    /// nguyên vẹn (cùng master key, blob `[nonce][ct+tag]` tự chứa nên tái
    /// dùng được — không cần giải mã/mã hoá lại). `name` None/rỗng → dùng tên
    /// template. `sticky_session`/`traffic_saver` là metadata của template,
    /// bảng `proxies` không có cột tương ứng nên không copy.
    pub fn create_proxy_from_template(
        &self,
        template_id: &str,
        name: Option<&str>,
    ) -> Result<ProxyRecord> {
        let tpl = self.get_proxy_template(template_id)?;
        let name = match name.map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => tpl.name,
        };
        self.create_proxy(ProxyInput {
            name,
            protocol: tpl.protocol,
            host: tpl.host,
            port: tpl.port,
            username_enc: tpl.username_enc,
            password_enc: tpl.password_enc,
        })
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
// Profile templates (W20b)
// ---------------------------------------------------------------------------

/// Map một row của bảng `profile_templates` → `models::ProfileTemplate`.
fn row_to_template(row: &Row) -> rusqlite::Result<ProfileTemplate> {
    let config_raw: String = row.get("config")?;
    let config: serde_json::Value = serde_json::from_str(&config_raw)
        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
    Ok(ProfileTemplate {
        id: row.get("id")?,
        name: row.get("name")?,
        config,
        created_at: row.get("created_at")?,
    })
}

impl Db {
    /// Tạo template mới. `config` là JSON shape `ProfileInput` (field lạ bị bỏ
    /// qua lúc tạo profile). Tên trim, không rỗng.
    pub fn create_template(
        &self,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<ProfileTemplate> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "template name must not be empty".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        {
            let conn = self.lock();
            conn.execute(
                "INSERT INTO profile_templates (id, name, config, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, serde_json::to_string(config)?, now()],
            )?;
        }
        self.get_template(&id)
    }

    /// Đọc 1 template. `NotFound` nếu không tồn tại.
    pub fn get_template(&self, id: &str) -> Result<ProfileTemplate> {
        let conn = self.lock();
        conn.query_row(
            "SELECT * FROM profile_templates WHERE id = ?1",
            params![id],
            row_to_template,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("template {id}")))
    }

    /// Danh sách template, mới tạo trước.
    pub fn list_templates(&self) -> Result<Vec<ProfileTemplate>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT * FROM profile_templates ORDER BY created_at DESC, name")?;
        let rows = stmt
            .query_map([], row_to_template)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Cập nhật template (F2b): đổi tên + tuỳ chọn thay config. `config` None →
    /// giữ config cũ (rename thuần). Tên trim, không rỗng; `NotFound` nếu id lạ.
    pub fn update_template(
        &self,
        id: &str,
        name: &str,
        config: Option<&serde_json::Value>,
    ) -> Result<ProfileTemplate> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "template name must not be empty".into(),
            ));
        }
        {
            let conn = self.lock();
            let n = match config {
                Some(cfg) => conn.execute(
                    "UPDATE profile_templates SET name = ?2, config = ?3 WHERE id = ?1",
                    params![id, name, serde_json::to_string(cfg)?],
                )?,
                None => conn.execute(
                    "UPDATE profile_templates SET name = ?2 WHERE id = ?1",
                    params![id, name],
                )?,
            };
            if n == 0 {
                return Err(AppError::NotFound(format!("template {id}")));
            }
        }
        self.get_template(id)
    }

    /// Xoá template. Trả `true` nếu có xoá (không đụng profile đã tạo từ template).
    pub fn delete_template(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM profile_templates WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Tạo profile mới điền sẵn field từ template. Seed + user_data_dir LUÔN
    /// cấp mới (mỗi profile một fingerprint/thư mục riêng); is_quick bị bỏ qua.
    /// `name` None/rỗng → dùng tên template.
    pub fn create_profile_from_template(
        &self,
        template_id: &str,
        name: Option<&str>,
    ) -> Result<Profile> {
        let tpl = self.get_template(template_id)?;
        let mut input: ProfileInput = serde_json::from_value(tpl.config.clone())
            .map_err(|e| AppError::InvalidInput(format!("invalid template config: {e}")))?;
        input.name = name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or(tpl.name);
        input.fingerprint_seed = None;
        input.user_data_dir = None;
        input.is_quick = None;
        self.create_profile(input)
    }
}

// ---------------------------------------------------------------------------
// Extensions (P3-1a): kho trung tâm + gán N-N với profile
// ---------------------------------------------------------------------------

fn row_to_extension(row: &Row) -> rusqlite::Result<Extension> {
    Ok(Extension {
        id: row.get("id")?,
        name: row.get("name")?,
        source_type: row.get("source_type")?,
        source_ref: row.get("source_ref")?,
        unpacked_path: row.get("unpacked_path")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        created_at: row.get("created_at")?,
    })
}

impl Db {
    /// Thêm extension vào kho. Từ chối khi `unpacked_path` đã có trong kho
    /// (tránh 2 hàng trỏ cùng thư mục — remove hàng này phá hàng kia).
    pub fn create_extension(
        &self,
        name: &str,
        source_type: &str,
        source_ref: &str,
        unpacked_path: &str,
    ) -> Result<Extension> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "extension name must not be empty".into(),
            ));
        }
        if !matches!(source_type, "folder" | "store") {
            return Err(AppError::InvalidInput(format!(
                "unsupported extension source_type: {source_type}"
            )));
        }
        let id = uuid::Uuid::new_v4().to_string();
        {
            let conn = self.lock();
            let dup = conn
                .query_row(
                    "SELECT 1 FROM extensions WHERE unpacked_path = ?1",
                    params![unpacked_path],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if dup {
                return Err(AppError::InvalidInput(format!(
                    "extension already added: {unpacked_path}"
                )));
            }
            conn.execute(
                "INSERT INTO extensions (id, name, source_type, source_ref, unpacked_path, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![id, name, source_type, source_ref, unpacked_path, now()],
            )?;
        }
        self.get_extension(&id)
    }

    /// Đọc 1 extension. `NotFound` nếu không tồn tại.
    pub fn get_extension(&self, id: &str) -> Result<Extension> {
        let conn = self.lock();
        conn.query_row(
            "SELECT * FROM extensions WHERE id = ?1",
            params![id],
            row_to_extension,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("extension {id}")))
    }

    /// Danh sách extension trong kho, thứ tự thêm trước → sau.
    pub fn list_extensions(&self) -> Result<Vec<Extension>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM extensions ORDER BY created_at, name")?;
        let rows = stmt
            .query_map([], row_to_extension)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bật/tắt 1 extension (tắt = giữ trong kho, không nạp khi launch).
    pub fn set_extension_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE extensions SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("extension {id}")));
        }
        Ok(())
    }

    /// Xoá extension khỏi kho (gán trong `profile_extensions` CASCADE theo).
    /// Trả `true` nếu có xoá. KHÔNG xoá thư mục trên đĩa — caller quyết định.
    pub fn delete_extension(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM extensions WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Thay TOÀN BỘ extension gán cho profile (giống set_profile_tags).
    /// `NotFound` nếu profile hoặc bất kỳ extension id nào không tồn tại.
    pub fn assign_extensions(&self, profile_id: &str, ext_ids: &[String]) -> Result<()> {
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
        for eid in ext_ids {
            let found = conn
                .query_row(
                    "SELECT 1 FROM extensions WHERE id = ?1",
                    params![eid],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !found {
                return Err(AppError::NotFound(format!("extension {eid}")));
            }
        }
        conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<()> {
            conn.execute(
                "DELETE FROM profile_extensions WHERE profile_id = ?1",
                params![profile_id],
            )?;
            for eid in ext_ids {
                conn.execute(
                    "INSERT OR IGNORE INTO profile_extensions (profile_id, extension_id) VALUES (?1, ?2)",
                    params![profile_id, eid],
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Danh sách extension đã gán cho profile (cả disabled — UI hiển thị đủ),
    /// thứ tự thêm vào kho.
    pub fn get_profile_extensions(&self, profile_id: &str) -> Result<Vec<Extension>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT e.* FROM extensions e
             JOIN profile_extensions pe ON pe.extension_id = e.id
             WHERE pe.profile_id = ?1
             ORDER BY e.created_at, e.name",
        )?;
        let rows = stmt
            .query_map(params![profile_id], row_to_extension)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Đường dẫn unpacked của extension ĐANG BẬT gán cho profile — nạp vào
    /// `--load-extension` lúc launch (xem launcher::build_args).
    pub fn profile_extension_paths(&self, profile_id: &str) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT e.unpacked_path FROM extensions e
             JOIN profile_extensions pe ON pe.extension_id = e.id
             WHERE pe.profile_id = ?1 AND e.enabled = 1
             ORDER BY e.created_at, e.name",
        )?;
        let rows = stmt
            .query_map(params![profile_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
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

    /// Filter chỉ có tag (viết gọn cho các test search).
    fn tag_filter(tag: &str) -> ProfileFilter {
        ProfileFilter {
            tag: Some(tag.into()),
            ..Default::default()
        }
    }

    /// (W23a) Checkpoint TRUNCATE phải đưa dữ liệu về file chính và cắt -wal về 0 byte.
    #[test]
    fn wal_checkpoint_truncate_empties_wal_file() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        let db = Db::open_at_dir(&dir).unwrap();
        db.create_profile(ProfileInput {
            name: "p1".into(),
            ..Default::default()
        })
        .unwrap();

        let wal = dir.join("browserx.db-wal");
        assert!(wal.metadata().unwrap().len() > 0, "WAL phải có dữ liệu trước checkpoint");
        db.wal_checkpoint_truncate().unwrap();
        assert_eq!(wal.metadata().unwrap().len(), 0, "WAL phải rỗng sau TRUNCATE");
        // Dữ liệu vẫn đọc được sau checkpoint.
        assert_eq!(db.list_profiles().unwrap().len(), 1);
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
    fn migration_v1_to_v2_upgrades_old_db() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        {
            // DB "cũ" chỉ có schema v1 + 1 profile, user_version = 1.
            let conn = Connection::open(dir.join("browserx.db")).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO profiles (id, name, fingerprint_seed, user_data_dir, created_at, updated_at)
                 VALUES ('old-1', 'old profile', '42', '/tmp/x', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }

        let db = Db::open_at_dir(&dir).unwrap();
        {
            let version: i64 = db
                .lock()
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, SCHEMA_VERSION);
        }
        // Profile cũ đọc được với default mới: folder_id NULL, favorite=false,
        // last_start_at NULL, is_quick=false, startup restore, không trash.
        let profiles = db.list_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].folder_id.is_none());
        assert!(!profiles[0].favorite);
        assert!(profiles[0].last_start_at.is_none());
        assert!(!profiles[0].is_quick);
        assert_eq!(profiles[0].startup_behavior, "restore");
        assert_eq!(profiles[0].startup_urls, serde_json::json!([]));
        assert_eq!(profiles[0].extensions, serde_json::json!([]));
        assert!(db.list_trash().unwrap().is_empty());
        // Folder mặc định được seed.
        let folders = db.list_folders().unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Default folder");
        assert_eq!(folders[0].profile_count, 0);
        // Backup được tạo trước khi migrate từ version cũ.
        assert!(dir.join("browserx.db.bak-v1").exists());
        // Mở lại lần nữa → không lỗi, không seed trùng.
        drop(db);
        let db = Db::open_at_dir(&dir).unwrap();
        assert_eq!(db.list_folders().unwrap().len(), 1);
    }

    #[test]
    fn migration_failure_rolls_back_and_backup_exists() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("browserx.db");
        {
            // DB "cũ" schema v1 + 1 profile, user_version = 1.
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO profiles (id, name, fingerprint_seed, user_data_dir, created_at, updated_at)
                 VALUES ('old-1', 'old profile', '42', '/tmp/x', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }
        {
            // Mô phỏng crash giữa migration: inject lỗi sau khi áp xong bậc v5
            // (v2..v5 đã ALTER TABLE) → toàn bộ phải rollback về v1.
            let conn = Connection::open(&db_path).unwrap();
            Db::init_conn(&conn).unwrap();
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
                .unwrap();
            backup_db_file(&db_path, 1).unwrap();
            let err = migrate_inner(&conn, |v| {
                if v == 5 {
                    Err(AppError::InvalidInput("injected migration failure".into()))
                } else {
                    Ok(())
                }
            });
            assert!(err.is_err());
            let version: i64 = conn
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, 1, "rollback phải giữ nguyên version cũ");
            // Cột thêm bởi v2 (favorite) phải bị rollback — không tồn tại nửa vời.
            assert!(conn
                .query_row("SELECT favorite FROM profiles LIMIT 1", [], |_| Ok(()))
                .is_err());
        }
        // Backup tồn tại sau khi migration fail.
        assert!(db_path.with_file_name("browserx.db.bak-v1").exists());
        // Mở lại → migrate chạy trọn vẹn, KHÔNG lỗi "duplicate column", data còn.
        let db = Db::open_at_dir(&dir).unwrap();
        let version: i64 = db
            .lock()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let p = db.get_profile("old-1").unwrap();
        assert_eq!(p.name, "old profile");
    }

    #[test]
    fn db_backup_overwrites_same_version_and_prunes_old() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("browserx.db");
        std::fs::write(&db_path, b"v-a").unwrap();
        backup_db_file(&db_path, 1).unwrap();
        // Ghi đè bản backup cùng version.
        std::fs::write(&db_path, b"v-b").unwrap();
        backup_db_file(&db_path, 1).unwrap();
        assert_eq!(
            std::fs::read(dir.join("browserx.db.bak-v1")).unwrap(),
            b"v-b"
        );
        // Nhiều version → chỉ giữ MAX_DB_BACKUPS bản mới nhất.
        for v in 2..=5 {
            backup_db_file(&db_path, v).unwrap();
        }
        assert!(!dir.join("browserx.db.bak-v1").exists());
        assert!(!dir.join("browserx.db.bak-v2").exists());
        assert!(dir.join("browserx.db.bak-v3").exists());
        assert!(dir.join("browserx.db.bak-v4").exists());
        assert!(dir.join("browserx.db.bak-v5").exists());
    }

    #[test]
    fn migration_v6_fingerprint_defaults_and_roundtrip() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        {
            // DB "cũ" schema v1 + 1 profile → upgrade qua v6 phải set default mới.
            let conn = Connection::open(dir.join("browserx.db")).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO profiles (id, name, fingerprint_seed, user_data_dir, created_at, updated_at)
                 VALUES ('old-1', 'old profile', '42', '/tmp/x', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }

        let db = Db::open_at_dir(&dir).unwrap();
        // Profile cũ nhận default W19c: noise bật, webrtc real, geolocation auto.
        let old = db.get_profile("old-1").unwrap();
        assert!(old.fp_noise);
        assert_eq!(old.webrtc_mode, "real");
        assert!(old.webrtc_ip.is_none());
        assert_eq!(old.geolocation_mode, "auto");
        assert!(old.geo_latitude.is_none());
        assert!(old.geo_longitude.is_none());

        // Create với giá trị tuỳ chỉnh → đọc lại đúng.
        let p = db
            .create_profile(ProfileInput {
                name: "fp".into(),
                fp_noise: Some(false),
                webrtc_mode: Some("masked".into()),
                webrtc_ip: Some("203.0.113.7".into()),
                geolocation_mode: Some("manual".into()),
                geo_latitude: Some("52.5".into()),
                geo_longitude: Some("13.4".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(!p.fp_noise);
        assert_eq!(p.webrtc_mode, "masked");
        assert_eq!(p.webrtc_ip.as_deref(), Some("203.0.113.7"));
        assert_eq!(p.geolocation_mode, "manual");
        assert_eq!(p.geo_latitude.as_deref(), Some("52.5"));
        assert_eq!(p.geo_longitude.as_deref(), Some("13.4"));

        // Update từng phần → chỉ field gửi lên thay đổi.
        let p2 = db
            .update_profile(
                &p.id,
                ProfileUpdate {
                    fp_noise: Some(true),
                    webrtc_mode: Some("real".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p2.fp_noise);
        assert_eq!(p2.webrtc_mode, "real");
        assert_eq!(p2.webrtc_ip.as_deref(), Some("203.0.113.7"));
        assert_eq!(p2.geolocation_mode, "manual");

        // Mode không hợp lệ → InvalidInput.
        assert!(matches!(
            db.create_profile(ProfileInput {
                name: "bad".into(),
                webrtc_mode: Some("disabled".into()),
                ..Default::default()
            }),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.update_profile(
                &p.id,
                ProfileUpdate {
                    geolocation_mode: Some("block".into()),
                    ..Default::default()
                }
            ),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn migration_v7_storage_defaults_and_template_table() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        {
            // DB "cũ" schema v1 + 1 profile → upgrade thẳng v1→v7.
            let conn = Connection::open(dir.join("browserx.db")).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO profiles (id, name, fingerprint_seed, user_data_dir, created_at, updated_at)
                 VALUES ('old-1', 'old profile', '42', '/tmp/x', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }

        let db = Db::open_at_dir(&dir).unwrap();
        {
            let version: i64 = db
                .lock()
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, SCHEMA_VERSION);
        }
        // Profile cũ nhận default W20b: giữ mọi dữ liệu (store_* = true).
        let old = db.get_profile("old-1").unwrap();
        assert!(old.store_history);
        assert!(old.store_passwords);
        assert!(old.store_sw_cache);

        // Bảng profile_templates dùng được ngay sau migrate.
        assert!(db.list_templates().unwrap().is_empty());
        let tpl = db
            .create_template("Base", &serde_json::json!({ "platform": "macos" }))
            .unwrap();
        assert_eq!(db.list_templates().unwrap().len(), 1);
        assert_eq!(tpl.name, "Base");

        // Create/update storage options roundtrip.
        let p = db
            .create_profile(ProfileInput {
                name: "s".into(),
                store_history: Some(false),
                store_sw_cache: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert!(!p.store_history);
        assert!(p.store_passwords);
        assert!(!p.store_sw_cache);
        let p2 = db
            .update_profile(
                &p.id,
                ProfileUpdate {
                    store_history: Some(true),
                    store_passwords: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p2.store_history);
        assert!(!p2.store_passwords);
        assert!(!p2.store_sw_cache);

        // Mở lại → migrate idempotent, dữ liệu còn nguyên.
        drop(db);
        let db = Db::open_at_dir(&dir).unwrap();
        assert_eq!(db.list_templates().unwrap().len(), 1);
    }

    #[test]
    fn template_crud_and_create_profile_from_template() {
        let (db, _guard) = temp_db();

        // Config shape ProfileInput (field lạ bị bỏ qua khi tạo profile).
        let config = serde_json::json!({
            "platform": "macos",
            "timezone": "Asia/Ho_Chi_Minh",
            "locale": "vi-VN",
            "screen_width": 1440,
            "screen_height": 900,
            "hardware_concurrency": 10,
            "humanize": true,
            "human_preset": "careful",
            "startup_behavior": "custom",
            "startup_urls": ["https://example.com"],
            "launch_args": ["--lang=vi"],
            "webrtc_mode": "masked",
            "webrtc_ip": "203.0.113.7",
            "store_history": false,
            "store_sw_cache": false,
            "tags": ["shop"],
            "unknown_field": "ignored"
        });
        let tpl = db.create_template("VN Shop", &config).unwrap();
        assert_eq!(tpl.name, "VN Shop");
        assert_eq!(tpl.config["platform"], "macos");
        assert!(matches!(
            db.create_template("   ", &config),
            Err(AppError::InvalidInput(_))
        ));

        // Tạo profile từ template → field điền đúng, seed + user_data_dir mới.
        let p = db.create_profile_from_template(&tpl.id, None).unwrap();
        assert_eq!(p.name, "VN Shop");
        assert_eq!(p.platform, "macos");
        assert_eq!(p.timezone.as_deref(), Some("Asia/Ho_Chi_Minh"));
        assert_eq!(p.locale.as_deref(), Some("vi-VN"));
        assert_eq!(p.screen_width, 1440);
        assert_eq!(p.screen_height, 900);
        assert_eq!(p.hardware_concurrency, 10);
        assert!(p.humanize);
        assert_eq!(p.human_preset.as_deref(), Some("careful"));
        assert_eq!(p.startup_behavior, "custom");
        assert_eq!(p.startup_urls, serde_json::json!(["https://example.com"]));
        assert_eq!(p.launch_args, serde_json::json!(["--lang=vi"]));
        assert_eq!(p.webrtc_mode, "masked");
        assert_eq!(p.webrtc_ip.as_deref(), Some("203.0.113.7"));
        assert!(!p.store_history);
        assert!(p.store_passwords);
        assert!(!p.store_sw_cache);
        assert_eq!(p.tags, vec!["shop".to_string()]);
        assert!(p.fingerprint_seed.parse::<u32>().is_ok());
        assert!(p.user_data_dir.contains(&p.id));

        // Tên tuỳ chỉnh + seed mỗi lần một profile độc lập.
        let p2 = db
            .create_profile_from_template(&tpl.id, Some("Shop #2"))
            .unwrap();
        assert_eq!(p2.name, "Shop #2");
        assert_ne!(p2.id, p.id);
        assert_ne!(p2.user_data_dir, p.user_data_dir);

        // Update (F2b): rename thuần giữ config; đổi config khi truyền Some.
        let renamed = db.update_template(&tpl.id, "VN Shop 2", None).unwrap();
        assert_eq!(renamed.name, "VN Shop 2");
        assert_eq!(renamed.config["platform"], "macos");
        let updated = db
            .update_template(
                &tpl.id,
                "VN Shop 3",
                Some(&serde_json::json!({ "platform": "linux" })),
            )
            .unwrap();
        assert_eq!(updated.name, "VN Shop 3");
        assert_eq!(updated.config["platform"], "linux");
        assert!(matches!(
            db.update_template(&tpl.id, "  ", None),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.update_template("missing-id", "X", None),
            Err(AppError::NotFound(_))
        ));

        // Delete + NotFound.
        assert!(db.delete_template(&tpl.id).unwrap());
        assert!(!db.delete_template(&tpl.id).unwrap());
        assert!(matches!(
            db.create_profile_from_template(&tpl.id, None),
            Err(AppError::NotFound(_))
        ));
        // Profile đã tạo từ template không bị ảnh hưởng.
        assert_eq!(db.list_profiles().unwrap().len(), 2);
    }

    /// (P3-3a) Migration v10: DB cũ v1 upgrade thẳng → bảng proxy_templates
    /// dùng được ngay; mở lại idempotent, dữ liệu còn.
    #[test]
    fn migration_v10_proxy_templates_table() {
        let dir = std::env::temp_dir().join(format!("browserx-db-test-{}", uuid::Uuid::new_v4()));
        let _guard = TempDir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        {
            let conn = Connection::open(dir.join("browserx.db")).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }

        let db = Db::open_at_dir(&dir).unwrap();
        {
            let version: i64 = db
                .lock()
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, SCHEMA_VERSION);
        }
        assert!(db.list_proxy_templates().unwrap().is_empty());
        let tpl = db
            .create_proxy_template(ProxyTemplateInput {
                name: "resi".into(),
                protocol: "http".into(),
                host: "gw.example.com".into(),
                port: 8000,
                sticky_session: true,
                ..Default::default()
            })
            .unwrap();
        assert!(tpl.sticky_session);
        assert!(!tpl.traffic_saver);
        // Mở lại → migrate idempotent, dữ liệu còn nguyên.
        drop(db);
        let db = Db::open_at_dir(&dir).unwrap();
        assert_eq!(db.list_proxy_templates().unwrap().len(), 1);
    }

    #[test]
    fn proxy_template_crud_and_create_proxy_from_template() {
        let (db, _guard) = temp_db();

        // Validate: tên rỗng / host rỗng / protocol lạ → InvalidInput.
        let base = ProxyTemplateInput {
            name: "T1".into(),
            protocol: "socks5".into(),
            host: "1.2.3.4".into(),
            port: 1080,
            username_enc: Some(vec![1, 2, 3]),
            password_enc: Some(vec![4, 5, 6]),
            sticky_session: true,
            traffic_saver: false,
        };
        assert!(matches!(
            db.create_proxy_template(ProxyTemplateInput {
                name: "  ".into(),
                ..base.clone()
            }),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.create_proxy_template(ProxyTemplateInput {
                host: " ".into(),
                ..base.clone()
            }),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.create_proxy_template(ProxyTemplateInput {
                protocol: "ftp".into(),
                ..base.clone()
            }),
            Err(AppError::InvalidInput(_))
        ));

        // Create → đọc lại đúng, credential blob giữ nguyên at-rest.
        let tpl = db.create_proxy_template(base).unwrap();
        assert_eq!(tpl.name, "T1");
        assert_eq!(tpl.protocol, "socks5");
        assert_eq!(tpl.port, 1080);
        assert_eq!(tpl.username_enc.as_deref(), Some(&[1u8, 2, 3][..]));
        assert!(tpl.sticky_session);
        assert!(!tpl.traffic_saver);
        assert_eq!(db.list_proxy_templates().unwrap().len(), 1);

        // Update từng phần: chỉ field gửi lên thay đổi.
        let up = db
            .update_proxy_template(
                &tpl.id,
                ProxyTemplateUpdate {
                    name: Some("T1b".into()),
                    traffic_saver: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(up.name, "T1b");
        assert!(up.sticky_session);
        assert!(up.traffic_saver);
        assert_eq!(up.username_enc.as_deref(), Some(&[1u8, 2, 3][..]));
        assert!(matches!(
            db.update_proxy_template(
                &tpl.id,
                ProxyTemplateUpdate {
                    protocol: Some("ftp".into()),
                    ..Default::default()
                }
            ),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.update_proxy_template("missing-id", ProxyTemplateUpdate::default()),
            Err(AppError::NotFound(_))
        ));

        // Tạo proxy từ template → copy config + credential blob nguyên vẹn.
        let px = db
            .create_proxy_from_template(&tpl.id, Some("từ template"))
            .unwrap();
        assert_eq!(px.name, "từ template");
        assert_eq!(px.protocol, "socks5");
        assert_eq!(px.host, "1.2.3.4");
        assert_eq!(px.port, 1080);
        assert_eq!(px.username_enc.as_deref(), Some(&[1u8, 2, 3][..]));
        assert_eq!(px.password_enc.as_deref(), Some(&[4u8, 5, 6][..]));
        // name None → dùng tên template.
        let px2 = db.create_proxy_from_template(&tpl.id, None).unwrap();
        assert_eq!(px2.name, "T1b");

        // clear_credentials xoá cả 2 blob về NULL.
        let cleared = db
            .update_proxy_template(
                &tpl.id,
                ProxyTemplateUpdate {
                    clear_credentials: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(cleared.username_enc.is_none());
        assert!(cleared.password_enc.is_none());

        // Delete + NotFound; proxy đã tạo từ template không bị ảnh hưởng.
        assert!(db.delete_proxy_template(&tpl.id).unwrap());
        assert!(!db.delete_proxy_template(&tpl.id).unwrap());
        assert!(matches!(
            db.create_proxy_from_template(&tpl.id, None),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(db.list_proxies().unwrap().len(), 2);
    }

    #[test]
    fn trash_restore_purge_flow() {
        let (db, _guard) = temp_db();
        let a = db
            .create_profile(ProfileInput {
                name: "trash-a".into(),
                ..Default::default()
            })
            .unwrap();
        let b = db
            .create_profile(ProfileInput {
                name: "trash-b".into(),
                ..Default::default()
            })
            .unwrap();

        // Trash a → list/search ẩn, list_trash hiện.
        assert_eq!(db.trash_profiles(std::slice::from_ref(&a.id)).unwrap(), 1);
        let alive = db.list_profiles().unwrap();
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].id, b.id);
        assert_eq!(
            db.search_profiles("trash-a", &ProfileFilter::default())
                .unwrap()
                .len(),
            0
        );
        let trash = db.list_trash().unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].id, a.id);
        // Trash lại lần nữa → không thay đổi thêm (đã ở trash).
        assert_eq!(db.trash_profiles(std::slice::from_ref(&a.id)).unwrap(), 0);

        // Restore → sống lại.
        assert_eq!(db.restore_profiles(std::slice::from_ref(&a.id)).unwrap(), 1);
        assert_eq!(db.list_profiles().unwrap().len(), 2);
        assert!(db.list_trash().unwrap().is_empty());
        assert_eq!(db.restore_profiles(std::slice::from_ref(&a.id)).unwrap(), 0);

        // Purge → xoá hẳn khỏi DB.
        db.trash_profiles(std::slice::from_ref(&b.id)).unwrap();
        assert_eq!(db.purge_profiles(std::slice::from_ref(&b.id)).unwrap(), 1);
        assert!(db.list_trash().unwrap().is_empty());
        assert!(matches!(
            db.get_profile(&b.id),
            Err(AppError::NotFound(_))
        ));

        // Batch rỗng → no-op.
        assert_eq!(db.trash_profiles(&[]).unwrap(), 0);
        assert_eq!(db.restore_profiles(&[]).unwrap(), 0);
        assert_eq!(db.purge_profiles(&[]).unwrap(), 0);
    }

    #[test]
    fn favorite_toggle() {
        let (db, _guard) = temp_db();
        let p = db
            .create_profile(ProfileInput {
                name: "fav".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(!p.favorite);
        db.set_favorite(&p.id, true).unwrap();
        assert!(db.get_profile(&p.id).unwrap().favorite);
        db.set_favorite(&p.id, false).unwrap();
        assert!(!db.get_profile(&p.id).unwrap().favorite);
        assert!(matches!(
            db.set_favorite("nope", true),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn quick_profile_flag_and_convert_to_regular() {
        let (db, _guard) = temp_db();
        // Mặc định (không truyền is_quick) → profile thường.
        let regular = db
            .create_profile(ProfileInput {
                name: "thường".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(!regular.is_quick);

        let quick = db
            .create_profile(ProfileInput {
                name: "Quick 1".into(),
                is_quick: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert!(quick.is_quick);

        // "Save as regular": bỏ cờ, giữ nguyên user_data_dir.
        db.set_quick(&quick.id, false).unwrap();
        let converted = db.get_profile(&quick.id).unwrap();
        assert!(!converted.is_quick);
        assert_eq!(converted.user_data_dir, quick.user_data_dir);

        assert!(matches!(
            db.set_quick("nope", true),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn folder_crud_and_profile_count() {
        let (db, _guard) = temp_db();
        // DB mới đã seed "Default folder".
        let seeded = db.list_folders().unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].name, "Default folder");

        let f = db.create_folder("Work").unwrap();
        assert_eq!(f.name, "Work");
        assert_eq!(f.profile_count, 0);
        assert!(matches!(
            db.create_folder("Work"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.create_folder("   "),
            Err(AppError::InvalidInput(_))
        ));

        let renamed = db.rename_folder(&f.id, "Work 2").unwrap();
        assert_eq!(renamed.name, "Work 2");
        assert!(matches!(
            db.rename_folder("nope", "x"),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            db.rename_folder(&f.id, "Default folder"),
            Err(AppError::InvalidInput(_))
        ));

        let a = db
            .create_profile(ProfileInput {
                name: "a".into(),
                ..Default::default()
            })
            .unwrap();
        let b = db
            .create_profile(ProfileInput {
                name: "b".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            db.move_profiles_to_folder(&[a.id.clone(), b.id.clone()], Some(&f.id))
                .unwrap(),
            2
        );
        assert_eq!(
            db.get_profile(&a.id).unwrap().folder_id.as_deref(),
            Some(f.id.as_str())
        );
        assert_eq!(db.get_folder(&f.id).unwrap().profile_count, 2);

        // profile_count chỉ đếm profile còn sống.
        db.trash_profiles(std::slice::from_ref(&a.id)).unwrap();
        assert_eq!(db.get_folder(&f.id).unwrap().profile_count, 1);

        // Move ra ngoài folder (None) + folder không tồn tại → NotFound.
        assert_eq!(
            db.move_profiles_to_folder(std::slice::from_ref(&b.id), None)
                .unwrap(),
            1
        );
        assert!(db.get_profile(&b.id).unwrap().folder_id.is_none());
        assert_eq!(db.get_folder(&f.id).unwrap().profile_count, 0);
        assert!(matches!(
            db.move_profiles_to_folder(std::slice::from_ref(&b.id), Some("nope")),
            Err(AppError::NotFound(_))
        ));

        // Xoá folder → profile thuộc folder về NULL, không bị xoá.
        db.move_profiles_to_folder(std::slice::from_ref(&b.id), Some(&f.id))
            .unwrap();
        assert!(db.delete_folder(&f.id).unwrap());
        assert!(!db.delete_folder(&f.id).unwrap());
        assert!(db.get_profile(&b.id).unwrap().folder_id.is_none());
        assert_eq!(db.list_folders().unwrap().len(), 1);
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
        assert!(created.last_start_at.is_none());

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
        let empty = ProfileFilter::default();
        assert_eq!(db.search_profiles("work", &empty).unwrap().len(), 1);
        assert_eq!(db.search_profiles("work", &tag_filter("ig")).unwrap().len(), 1);
        assert_eq!(db.search_profiles("work", &tag_filter("fb")).unwrap().len(), 0);
        assert_eq!(db.search_profiles("nope", &empty).unwrap().len(), 0);

        assert!(db.delete_profile(&created.id).unwrap());
        assert!(!db.delete_profile(&created.id).unwrap());
        assert!(matches!(
            db.get_profile(&created.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn touch_last_start_sets_timestamp_without_touching_updated_at() {
        let (db, _guard) = temp_db();
        let p = db
            .create_profile(ProfileInput {
                name: "ls".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(p.last_start_at.is_none());

        db.touch_last_start(&p.id).unwrap();
        let after = db.get_profile(&p.id).unwrap();
        assert!(after.last_start_at.is_some());
        assert_eq!(after.updated_at, p.updated_at);

        assert!(matches!(
            db.touch_last_start("nope"),
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
    fn five_thousand_profiles_list_and_search_are_fast() {
        let (db, _guard) = temp_db();

        let t_insert = std::time::Instant::now();
        for i in 0..5000 {
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
        assert_eq!(all.len(), 5000);

        let t_search = std::time::Instant::now();
        let hits = db
            .search_profiles("profile-09", &ProfileFilter::default())
            .unwrap();
        let search_ms = t_search.elapsed().as_millis();
        assert_eq!(hits.len(), 100); // profile-0900..profile-0999

        let t_tag = std::time::Instant::now();
        let vips = db.search_profiles("profile", &tag_filter("vip")).unwrap();
        let tag_ms = t_tag.elapsed().as_millis();
        assert_eq!(vips.len(), 500);

        println!(
            "5000 profiles: insert={insert_ms}ms list={list_ms}ms search={search_ms}ms tag_search={tag_ms}ms"
        );
        // Mục tiêu docs: <200ms p95 (release) cho 1000. Debug build chậm hơn và
        // canary 5000 profile (5×) → ngưỡng 5000ms chống flaky.
        assert!(list_ms < 5000, "list_profiles too slow: {list_ms}ms");
        assert!(search_ms < 5000, "search_profiles too slow: {search_ms}ms");
        assert!(tag_ms < 5000, "tag search too slow: {tag_ms}ms");
    }

    #[test]
    fn extensions_crud_assign_and_resolve_paths() {
        let (db, _guard) = temp_db();
        let p = db
            .create_profile(ProfileInput {
                name: "ext-p".into(),
                ..Default::default()
            })
            .unwrap();

        let a = db
            .create_extension("uBlock", "folder", "/src/ublock", "/data/ext/ublock")
            .unwrap();
        let b = db
            .create_extension(
                "Dark Reader",
                "store",
                "eimadpbcbfnmbkopoojfekhnkhdbieeh",
                "/data/ext/dark",
            )
            .unwrap();
        assert!(a.enabled && b.enabled);
        assert_eq!(db.list_extensions().unwrap().len(), 2);

        // Trùng unpacked_path → InvalidInput; source_type lạ → InvalidInput.
        assert!(matches!(
            db.create_extension("dup", "folder", "/x", "/data/ext/ublock"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            db.create_extension("bad", "zip", "/x", "/data/ext/zip"),
            Err(AppError::InvalidInput(_))
        ));

        // Assign cả 2 → get trả đủ, resolve trả đúng path theo thứ tự thêm.
        db.assign_extensions(&p.id, &[a.id.clone(), b.id.clone()])
            .unwrap();
        let assigned = db.get_profile_extensions(&p.id).unwrap();
        assert_eq!(assigned.len(), 2);
        assert_eq!(
            db.profile_extension_paths(&p.id).unwrap(),
            vec!["/data/ext/ublock".to_string(), "/data/ext/dark".to_string()]
        );

        // Disable → vẫn trong danh sách gán, nhưng KHÔNG vào paths lúc launch.
        db.set_extension_enabled(&a.id, false).unwrap();
        assert_eq!(db.get_profile_extensions(&p.id).unwrap().len(), 2);
        assert_eq!(
            db.profile_extension_paths(&p.id).unwrap(),
            vec!["/data/ext/dark".to_string()]
        );

        // Re-assign thay TOÀN BỘ (chỉ còn b).
        db.assign_extensions(&p.id, std::slice::from_ref(&b.id))
            .unwrap();
        assert_eq!(db.get_profile_extensions(&p.id).unwrap().len(), 1);

        // Id lạ → NotFound (profile lẫn extension).
        assert!(matches!(
            db.assign_extensions("missing", &[]),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            db.assign_extensions(&p.id, &["missing".into()]),
            Err(AppError::NotFound(_))
        ));

        // Xoá extension → hàng gán CASCADE theo.
        assert!(db.delete_extension(&b.id).unwrap());
        assert!(!db.delete_extension(&b.id).unwrap());
        assert!(db.get_profile_extensions(&p.id).unwrap().is_empty());
        assert!(db.profile_extension_paths(&p.id).unwrap().is_empty());
    }

    #[test]
    fn extensions_cascade_when_profile_purged() {
        let (db, _guard) = temp_db();
        let p = db
            .create_profile(ProfileInput {
                name: "purge-ext".into(),
                ..Default::default()
            })
            .unwrap();
        let e = db
            .create_extension("E", "folder", "/src/e", "/data/ext/e")
            .unwrap();
        db.assign_extensions(&p.id, std::slice::from_ref(&e.id))
            .unwrap();
        db.trash_profiles(std::slice::from_ref(&p.id)).unwrap();
        db.purge_profiles(std::slice::from_ref(&p.id)).unwrap();
        // Extension còn trong kho; hàng gán bị CASCADE xoá.
        assert_eq!(db.list_extensions().unwrap().len(), 1);
        assert!(db.get_profile_extensions(&p.id).unwrap().is_empty());
    }
}
