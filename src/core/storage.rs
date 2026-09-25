//! SQLite 数据层：文件索引（FTS5 trigram）、正文索引、应用、剪贴板、置顶、最近、热键、KV。

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use crate::models::SearchScopeFilter;

#[derive(Clone, Debug, Default)]
pub struct FileRecord {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub is_dir: bool,
    pub size: i64,
    pub mtime: i64,
}

#[derive(Clone, Debug, Default)]
pub struct AppRecord {
    pub id: i64,
    pub name: String,
    /// 启动路径（.lnk 或 .exe）
    pub launch_path: String,
    /// 解析后的目标 exe（可能为空）
    pub target: String,
    pub args: String,
    pub pinyin: String,
    pub initials: String,
}

#[derive(Clone, Debug, Default)]
pub struct ClipRecord {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub created: i64,
    pub pinned: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EntryRecord {
    pub item_id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub path: String,
    pub badge: String,
    pub icon: String,
    pub last_used: i64,
    pub use_count: i64,
    pub sort: i64,
}

#[derive(Clone, Debug, Default)]
pub struct HotkeyRecord {
    pub id: String,
    pub name: String,
    pub target_path: String,
    pub hotkey: String,
    pub item_type: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ContentHit {
    pub path: String,
    pub snippet: String,
}

pub struct Storage {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    ext TEXT NOT NULL DEFAULT '',
    is_dir INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_files_name ON files(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_files_mtime ON files(mtime);
CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(name, content='files', content_rowid='id', tokenize='trigram');
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
  INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
  INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
END;
CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE OF name ON files BEGIN
  INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
  INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
END;
CREATE VIRTUAL TABLE IF NOT EXISTS content_fts USING fts5(path UNINDEXED, body, tokenize='trigram');
CREATE TABLE IF NOT EXISTS apps (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    launch_path TEXT NOT NULL UNIQUE,
    target TEXT NOT NULL DEFAULT '',
    args TEXT NOT NULL DEFAULT '',
    pinyin TEXT NOT NULL DEFAULT '',
    initials TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS clipboard (
    id INTEGER PRIMARY KEY,
    hash TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    created INTEGER NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS pins (
    item_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    subtitle TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL DEFAULT '',
    badge TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL DEFAULT '',
    sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS recent (
    item_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    subtitle TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL DEFAULT '',
    badge TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL DEFAULT '',
    last_used INTEGER NOT NULL,
    use_count INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS hotkeys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    target_path TEXT NOT NULL,
    hotkey TEXT NOT NULL,
    item_type TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn today_start_local() -> i64 {
    use chrono::{Local, TimeZone};
    let now = Local::now();
    let date = now.date_naive();
    Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .map(|d| d.timestamp())
        .unwrap_or(now.timestamp() - 86_400)
}

/// 将用户输入转换为 FTS5 trigram 短语查询（"..."，内部引号加倍）
fn fts_phrase(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

fn like_pattern(query: &str) -> String {
    let mut s = String::with_capacity(query.len() + 2);
    s.push('%');
    for c in query.chars() {
        if c == '%' || c == '_' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

pub fn type_extensions(category: &str) -> Option<&'static [&'static str]> {
    match category {
        "document" => Some(&[
            "md", "txt", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "rtf", "odt", "csv",
            "epub", "one", "wps",
        ]),
        "code" => Some(&[
            "rs", "js", "ts", "tsx", "jsx", "py", "go", "java", "kt", "c", "cpp", "h", "hpp", "cs",
            "swift", "rb", "php", "lua", "sh", "ps1", "bat", "cmd", "yml", "yaml", "json", "toml",
            "xml", "html", "css", "scss", "sql", "slint", "vue", "svelte", "ini", "cfg", "env",
            "lock", "gradle", "cmake", "mk",
        ]),
        "media" => Some(&[
            "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "heic", "mp4", "mkv", "mov",
            "avi", "webm", "mp3", "wav", "flac", "aac", "ogg", "m4a",
        ]),
        "archive" => Some(&["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "iso", "cab"]),
        _ => None,
    }
}

/// 构造范围过滤 SQL 片段（针对 files 表别名 f）
fn scope_sql(scope: &SearchScopeFilter, args: &mut Vec<rusqlite::types::Value>) -> String {
    let mut sql = String::new();
    let now = now();
    if let Some(lower) = scope.time_lower_bound(now, today_start_local()) {
        sql.push_str(" AND f.mtime >= ?");
        args.push(lower.into());
    }
    if let Some(upper) = scope.time_upper_bound() {
        sql.push_str(" AND f.mtime <= ?");
        args.push(upper.into());
    }
    match scope.type_category.as_str() {
        "folder" => sql.push_str(" AND f.is_dir = 1"),
        "app" => {
            sql.push_str(" AND f.is_dir = 0 AND f.ext IN ('exe','lnk','msi','appref-ms')");
        }
        cat => {
            if let Some(exts) = type_extensions(cat) {
                sql.push_str(" AND f.is_dir = 0 AND f.ext IN (");
                for (i, e) in exts.iter().enumerate() {
                    if i > 0 {
                        sql.push(',');
                    }
                    sql.push('?');
                    args.push(e.to_string().into());
                }
                sql.push(')');
            }
        }
    }
    let loc = scope.location_scope.as_str();
    let prefix: Option<String> = match loc {
        "all" | "" => None,
        "drive-c" => Some("C:\\".into()),
        "drive-d" => Some("D:\\".into()),
        "desktop" => directories::UserDirs::new()
            .and_then(|d| d.desktop_dir().map(|p| p.to_string_lossy().to_string())),
        "downloads" => directories::UserDirs::new()
            .and_then(|d| d.download_dir().map(|p| p.to_string_lossy().to_string())),
        "documents" => directories::UserDirs::new()
            .and_then(|d| d.document_dir().map(|p| p.to_string_lossy().to_string())),
        _ => scope.custom_directory.clone().filter(|s| !s.is_empty()),
    };
    if let Some(mut p) = prefix {
        if !p.ends_with('\\') {
            p.push('\\');
        }
        sql.push_str(" AND f.path LIKE ? ESCAPE '\\'");
        let mut pat = String::new();
        for c in p.chars() {
            if c == '%' || c == '_' || c == '\\' {
                pat.push('\\');
            }
            pat.push(c);
        }
        pat.push('%');
        args.push(pat.into());
    }
    sql
}

impl Storage {
    pub fn open(path: &Path) -> Result<Storage> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).with_context(|| format!("打开数据库 {path:?}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-16000;",
        )?;
        conn.execute_batch(SCHEMA).context("初始化数据库结构")?;
        Ok(Storage { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Storage> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Storage { conn: Mutex::new(conn) })
    }

    // ---------------- KV ----------------
    pub fn kv_get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO kv(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------------- 文件索引 ----------------
    pub fn upsert_files(&self, batch: &[FileRecord]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO files(path, name, ext, is_dir, size, mtime) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET name = excluded.name, ext = excluded.ext, is_dir = excluded.is_dir, size = excluded.size, mtime = excluded.mtime",
            )?;
            for f in batch {
                stmt.execute(params![f.path, f.name, f.ext, f.is_dir as i64, f.size, f.mtime])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_file(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        conn.execute("DELETE FROM content_fts WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// 删除某目录及其下所有条目
    pub fn remove_path_tree(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock();
        let mut prefix = String::new();
        for c in path.chars() {
            if c == '%' || c == '_' || c == '\\' {
                prefix.push('\\');
            }
            prefix.push(c);
        }
        let pat = format!("{prefix}\\%");
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        conn.execute("DELETE FROM files WHERE path LIKE ?1 ESCAPE '\\'", params![pat])?;
        conn.execute("DELETE FROM content_fts WHERE path = ?1", params![path])?;
        conn.execute("DELETE FROM content_fts WHERE path LIKE ?1 ESCAPE '\\'", params![pat])?;
        Ok(())
    }

    pub fn clear_files(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch("DELETE FROM files; DELETE FROM content_fts; INSERT INTO files_fts(files_fts) VALUES('rebuild');")?;
        Ok(())
    }

    pub fn file_count(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn content_count(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM content_fts", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn db_size_bytes(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    pub fn file_by_path(&self, path: &str) -> Option<FileRecord> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, path, name, ext, is_dir, size, mtime FROM files WHERE path = ?1",
            params![path],
            map_file,
        )
        .optional()
        .ok()
        .flatten()
    }

    /// 文件名检索：>=3 字符走 FTS5 trigram，否则走 LIKE。
    pub fn search_files(&self, query: &str, scope: &SearchScopeFilter, limit: usize) -> Result<Vec<FileRecord>> {
        let q = query.trim();
        if q.is_empty() {
            return self.list_files_by_scope(scope, limit);
        }
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        let use_fts = q.chars().count() >= 3;
        let mut sql = if use_fts {
            args.push(fts_phrase(q).into());
            String::from(
                "SELECT f.id, f.path, f.name, f.ext, f.is_dir, f.size, f.mtime FROM files_fts JOIN files f ON f.id = files_fts.rowid WHERE files_fts MATCH ?",
            )
        } else {
            args.push(like_pattern(q).into());
            String::from(
                "SELECT f.id, f.path, f.name, f.ext, f.is_dir, f.size, f.mtime FROM files f WHERE f.name LIKE ? ESCAPE '\\'",
            )
        };
        sql.push_str(&scope_sql(scope, &mut args));
        sql.push_str(" ORDER BY length(f.name) ASC, f.mtime DESC LIMIT ?");
        args.push((limit as i64).into());
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), map_file)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn list_files_by_scope(&self, scope: &SearchScopeFilter, limit: usize) -> Result<Vec<FileRecord>> {
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        let mut sql = String::from(
            "SELECT f.id, f.path, f.name, f.ext, f.is_dir, f.size, f.mtime FROM files f WHERE 1 = 1",
        );
        sql.push_str(&scope_sql(scope, &mut args));
        sql.push_str(" ORDER BY f.mtime DESC LIMIT ?");
        args.push((limit as i64).into());
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), map_file)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------------- 正文索引 ----------------
    pub fn upsert_content(&self, path: &str, body: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM content_fts WHERE path = ?1", params![path])?;
        conn.execute("INSERT INTO content_fts(path, body) VALUES (?1, ?2)", params![path, body])?;
        Ok(())
    }

    pub fn has_content(&self, path: &str) -> bool {
        let conn = self.conn.lock();
        conn.query_row("SELECT 1 FROM content_fts WHERE path = ?1 LIMIT 1", params![path], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn search_content(&self, query: &str, limit: usize) -> Result<Vec<ContentHit>> {
        let q = query.trim();
        if q.chars().count() < 3 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT path, snippet(content_fts, 1, '', '', '…', 12) FROM content_fts WHERE content_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_phrase(q), limit as i64], |r| {
            Ok(ContentHit { path: r.get(0)?, snippet: r.get(1)? })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------------- 应用 ----------------
    pub fn replace_apps(&self, apps: &[AppRecord]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM apps", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO apps(name, launch_path, target, args, pinyin, initials) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for a in apps {
                stmt.execute(params![a.name, a.launch_path, a.target, a.args, a.pinyin, a.initials])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_apps(&self) -> Vec<AppRecord> {
        let conn = self.conn.lock();
        let Ok(mut stmt) = conn.prepare("SELECT id, name, launch_path, target, args, pinyin, initials FROM apps ORDER BY name") else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok(AppRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                launch_path: r.get(2)?,
                target: r.get(3)?,
                args: r.get(4)?,
                pinyin: r.get(5)?,
                initials: r.get(6)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    // ---------------- 剪贴板 ----------------
    pub fn add_clip(&self, content: &str, kind: &str, dedupe: bool, limit: u32) -> Result<Option<ClipRecord>> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut h);
        let hash = format!("{:016x}", h.finish());
        let created = now();
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row("SELECT id FROM clipboard WHERE hash = ?1", params![hash], |r| r.get(0))
            .optional()?;
        let id = match existing {
            Some(id) if dedupe => {
                tx.execute("UPDATE clipboard SET created = ?1 WHERE id = ?2", params![created, id])?;
                id
            }
            Some(id) => id,
            None => {
                tx.execute(
                    "INSERT INTO clipboard(hash, kind, content, created) VALUES (?1, ?2, ?3, ?4)",
                    params![hash, kind, content, created],
                )?;
                tx.last_insert_rowid()
            }
        };
        // 超出容量时清理最旧的未固定记录
        tx.execute(
            "DELETE FROM clipboard WHERE pinned = 0 AND id NOT IN (SELECT id FROM clipboard WHERE pinned = 0 ORDER BY created DESC LIMIT ?1)",
            params![limit as i64],
        )?;
        tx.commit()?;
        Ok(Some(ClipRecord { id, kind: kind.into(), content: content.into(), created, pinned: false }))
    }

    pub fn clip_by_id(&self, id: i64) -> Option<ClipRecord> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, kind, content, created, pinned FROM clipboard WHERE id = ?1",
            params![id],
            map_clip,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn search_clips(&self, query: &str, sub: &str, limit: usize) -> Result<Vec<ClipRecord>> {
        let conn = self.conn.lock();
        let mut sql = String::from("SELECT id, kind, content, created, pinned FROM clipboard WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if !query.trim().is_empty() {
            sql.push_str(" AND content LIKE ? ESCAPE '\\'");
            args.push(like_pattern(query.trim()).into());
        }
        if !sub.is_empty() && sub != "all" {
            sql.push_str(" AND kind = ?");
            args.push(sub.to_string().into());
        }
        sql.push_str(" ORDER BY created DESC LIMIT ?");
        args.push((limit as i64).into());
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), map_clip)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_clip(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM clipboard WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_clip_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("UPDATE clipboard SET pinned = ?1 WHERE id = ?2", params![pinned as i64, id])?;
        Ok(())
    }

    pub fn clear_unpinned_clips(&self) -> Result<usize> {
        let conn = self.conn.lock();
        Ok(conn.execute("DELETE FROM clipboard WHERE pinned = 0", [])?)
    }

    pub fn clip_count(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM clipboard", [], |r| r.get(0)).unwrap_or(0)
    }

    // ---------------- 置顶 ----------------
    pub fn list_pins(&self) -> Vec<EntryRecord> {
        let conn = self.conn.lock();
        let Ok(mut stmt) = conn.prepare(
            "SELECT item_id, kind, title, subtitle, path, badge, icon, sort FROM pins ORDER BY sort ASC, rowid ASC",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok(EntryRecord {
                item_id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                subtitle: r.get(3)?,
                path: r.get(4)?,
                badge: r.get(5)?,
                icon: r.get(6)?,
                sort: r.get(7)?,
                ..Default::default()
            })
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    pub fn is_pinned(&self, item_id: &str) -> bool {
        let conn = self.conn.lock();
        conn.query_row("SELECT 1 FROM pins WHERE item_id = ?1", params![item_id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn add_pin(&self, e: &EntryRecord) -> Result<()> {
        let conn = self.conn.lock();
        let next: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM pins", [], |r| r.get(0))
            .unwrap_or(1);
        conn.execute(
            "INSERT OR REPLACE INTO pins(item_id, kind, title, subtitle, path, badge, icon, sort) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![e.item_id, e.kind, e.title, e.subtitle, e.path, e.badge, e.icon, next],
        )?;
        if e.kind == "clipboard" {
            if let Some(id) = e.item_id.strip_prefix("clip:").and_then(|s| s.parse::<i64>().ok()) {
                conn.execute("UPDATE clipboard SET pinned = 1 WHERE id = ?1", params![id])?;
            }
        }
        Ok(())
    }

    pub fn remove_pin(&self, item_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM pins WHERE item_id = ?1", params![item_id])?;
        if let Some(id) = item_id.strip_prefix("clip:").and_then(|s| s.parse::<i64>().ok()) {
            conn.execute("UPDATE clipboard SET pinned = 0 WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    // ---------------- 最近使用 ----------------
    pub fn touch_recent(&self, e: &EntryRecord) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO recent(item_id, kind, title, subtitle, path, badge, icon, last_used, use_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
             ON CONFLICT(item_id) DO UPDATE SET last_used = excluded.last_used, use_count = recent.use_count + 1, title = excluded.title, subtitle = excluded.subtitle, path = excluded.path, badge = excluded.badge, icon = excluded.icon",
            params![e.item_id, e.kind, e.title, e.subtitle, e.path, e.badge, e.icon, now()],
        )?;
        conn.execute(
            "DELETE FROM recent WHERE item_id NOT IN (SELECT item_id FROM recent ORDER BY last_used DESC LIMIT 200)",
            [],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> Vec<EntryRecord> {
        let conn = self.conn.lock();
        let Ok(mut stmt) = conn.prepare(
            "SELECT item_id, kind, title, subtitle, path, badge, icon, last_used, use_count FROM recent ORDER BY last_used DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(EntryRecord {
                item_id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                subtitle: r.get(3)?,
                path: r.get(4)?,
                badge: r.get(5)?,
                icon: r.get(6)?,
                last_used: r.get(7)?,
                use_count: r.get(8)?,
                sort: 0,
            })
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    /// 返回 (last_used, use_count)
    pub fn recent_stats(&self, item_id: &str) -> Option<(i64, i64)> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT last_used, use_count FROM recent WHERE item_id = ?1",
            params![item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn remove_recent(&self, item_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM recent WHERE item_id = ?1", params![item_id])?;
        Ok(())
    }

    // ---------------- 热键 ----------------
    pub fn list_hotkeys(&self) -> Vec<HotkeyRecord> {
        let conn = self.conn.lock();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, name, target_path, hotkey, item_type, enabled FROM hotkeys ORDER BY sort ASC, rowid ASC",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok(HotkeyRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                target_path: r.get(2)?,
                hotkey: r.get(3)?,
                item_type: r.get(4)?,
                enabled: r.get::<_, i64>(5)? != 0,
            })
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    pub fn upsert_hotkey(&self, h: &HotkeyRecord) -> Result<()> {
        let conn = self.conn.lock();
        let next: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM hotkeys", [], |r| r.get(0))
            .unwrap_or(1);
        conn.execute(
            "INSERT INTO hotkeys(id, name, target_path, hotkey, item_type, enabled, sort) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, target_path = excluded.target_path, hotkey = excluded.hotkey, item_type = excluded.item_type, enabled = excluded.enabled",
            params![h.id, h.name, h.target_path, h.hotkey, h.item_type, h.enabled as i64, next],
        )?;
        Ok(())
    }

    pub fn delete_hotkey(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM hotkeys WHERE id = ?1", params![id])?;
        Ok(())
    }
}

fn map_file(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: r.get(0)?,
        path: r.get(1)?,
        name: r.get(2)?,
        ext: r.get(3)?,
        is_dir: r.get::<_, i64>(4)? != 0,
        size: r.get(5)?,
        mtime: r.get(6)?,
    })
}

fn map_clip(r: &rusqlite::Row<'_>) -> rusqlite::Result<ClipRecord> {
    Ok(ClipRecord {
        id: r.get(0)?,
        kind: r.get(1)?,
        content: r.get(2)?,
        created: r.get(3)?,
        pinned: r.get::<_, i64>(4)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_and_like_search() {
        let s = Storage::open_in_memory().unwrap();
        s.upsert_files(&[
            FileRecord { path: "D:\\Infra\\docker-compose.yml".into(), name: "docker-compose.yml".into(), ext: "yml".into(), mtime: 100, ..Default::default() },
            FileRecord { path: "D:\\Projects".into(), name: "Projects".into(), is_dir: true, mtime: 50, ..Default::default() },
        ])
        .unwrap();
        let scope = SearchScopeFilter::default();
        assert_eq!(s.search_files("docker", &scope, 10).unwrap().len(), 1);
        assert_eq!(s.search_files("pr", &scope, 10).unwrap().len(), 1);
        assert_eq!(s.search_files("zzz", &scope, 10).unwrap().len(), 0);
        s.remove_file("D:\\Infra\\docker-compose.yml").unwrap();
        assert_eq!(s.search_files("docker", &scope, 10).unwrap().len(), 0);
    }

    #[test]
    fn clipboard_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        s.add_clip("docker compose up -d", "code", true, 10).unwrap();
        s.add_clip("hello", "text", true, 10).unwrap();
        assert_eq!(s.search_clips("", "all", 10).unwrap().len(), 2);
        assert_eq!(s.search_clips("compose", "code", 10).unwrap().len(), 1);
        assert_eq!(s.search_clips("", "url", 10).unwrap().len(), 0);
    }
}
