//! SQLite 数据层：文件索引（FTS5 trigram）、正文索引、应用、剪贴板、置顶、最近、热键、KV。

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::models::SearchScopeFilter;

#[derive(Clone, Debug, Default)]
pub struct FileRecord {
    pub id: i64,
    pub path: String,
    /// 直接父目录。冗余存储并建索引，用于「取某目录的直接子项」——
    /// 这是增量对账的比对基准（LIKE 前缀查询走不了索引，必须靠这一列）。
    pub parent: String,
    pub name: String,
    pub ext: String,
    pub is_dir: bool,
    pub size: i64,
    pub mtime: i64,
    /// 亚秒精度 mtime（纳秒）。目录 mtime 对账必须用它：
    /// 秒精度下同一秒内的二次增删会得到相同的值，变更会被漏掉。
    pub mtime_ns: i64,
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
    parent TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL,
    ext TEXT NOT NULL DEFAULT '',
    is_dir INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0,
    mtime_ns INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_files_name ON files(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_files_mtime ON files(mtime);
-- idx_files_parent 不在这里建：对老库而言上面的 CREATE TABLE 是空操作，
-- parent 列要等 migrate() 里的 ALTER TABLE 之后才存在，否则这里会报
-- "no such column: parent"。统一由 migrate() 负责。
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
CREATE TABLE IF NOT EXISTS content_meta (
    path TEXT PRIMARY KEY,
    mtime_ns INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0,
    indexed_at INTEGER NOT NULL DEFAULT 0
);
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

/// `files` 表的统一列清单（别名固定为 `f`），列顺序须与 `map_file` 一致。
const FILE_COLS: &str = "f.id, f.path, f.parent, f.name, f.ext, f.is_dir, f.size, f.mtime, f.mtime_ns";

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

/// `q` → `q%`，转义 LIKE 元字符。用于 `search_files` 排序键里的「前缀匹配」档。
fn prefix_pattern(query: &str) -> String {
    let mut s = String::with_capacity(query.len() + 1);
    for c in query.chars() {
        if c == '%' || c == '_' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

/// 「使用情况」全量快照：`recent` 的 (last_used, use_count) + `pins` 成员集合。
///
/// 存在的理由是**查询次数**：`recent_stats` / `is_pinned` 各自是一次 SQL，按候选逐个
/// 调用就是 N×2 次查询。`search_files` 的候选上限从 100 提到数千之后，逐个查会直接
/// 拖垮每次击键的搜索；而 `recent` 表本身有 200 行上限、`pins` 更小，一次全读进内存
/// 是常数级成本，比按需查还便宜。
#[derive(Default)]
pub struct UsageStats {
    pub recent: HashMap<String, (i64, i64)>,
    pub pinned: HashSet<String>,
}

/// 取路径的直接父目录。
///
/// - `C:\Users\a.txt` → `C:\Users`
/// - `C:\Users`       → `C:\`
/// - `C:\`            → `""`（驱动器根没有父，必须返回空串，
///   否则递归取子树时会形成自环）
pub fn parent_of(path: &str) -> String {
    if path.ends_with('\\') {
        return String::new();
    }
    match path.rfind('\\') {
        // 形如 `C:\foo`：最后一个分隔符在索引 2，父目录是 `C:\`（含分隔符）
        Some(2) if path.as_bytes().get(1) == Some(&b':') => path[..3].to_string(),
        Some(0) => String::new(),
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// 构造「该目录下所有后代」的 GLOB 模式。
///
/// 用 GLOB 而非 LIKE：实测 `path GLOB 'x\*'` 会被 SQLite 改写为
/// `path > ? AND path < ?` 走 UNIQUE 索引（SEARCH），而
/// `path LIKE 'x\%'` 只能全表扫描（SCAN）。代价是 GLOB 没有转义符，
/// 通配符只能靠字符类 `[*]` 表达。
pub fn glob_descendants(dir: &str) -> String {
    let mut s = String::with_capacity(dir.len() + 2);
    for c in dir.chars() {
        match c {
            '*' | '?' | '[' => {
                s.push('[');
                s.push(c);
                s.push(']');
            }
            _ => s.push(c),
        }
    }
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s.push('*');
    s
}

/// 由 SystemTime 取纳秒时间戳（目录 mtime 对账用）
pub fn mtime_ns_of(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// 老库结构迁移：补齐 `parent` / `mtime_ns` 列并回填，建立 `content_meta`。
///
/// 幂等，且对已迁移的库只做几次 `PRAGMA table_info` 级别的轻量检查。
fn migrate(conn: &Connection) -> Result<()> {
    let mut cols = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare("PRAGMA table_info(files)")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            cols.insert(r.get::<_, String>(1)?);
        }
    }
    if !cols.contains("parent") {
        conn.execute("ALTER TABLE files ADD COLUMN parent TEXT NOT NULL DEFAULT ''", [])?;
        log::info!("索引库迁移：新增 files.parent 列");
    }
    if !cols.contains("mtime_ns") {
        conn.execute("ALTER TABLE files ADD COLUMN mtime_ns INTEGER NOT NULL DEFAULT 0", [])?;
        log::info!("索引库迁移：新增 files.mtime_ns 列");
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_files_parent ON files(parent);")?;

    // 回填 parent（只处理尚未回填且带分隔符的行）
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE parent = '' AND instr(path, char(92)) > 0",
        [],
        |r| r.get(0),
    )?;
    if pending > 0 {
        let pairs: Vec<(i64, String)> = {
            let mut stmt =
                conn.prepare("SELECT id, path FROM files WHERE parent = '' AND instr(path, char(92)) > 0")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE files SET parent = ?1 WHERE id = ?2")?;
            for (id, path) in &pairs {
                stmt.execute(params![parent_of(path), id])?;
            }
        }
        tx.commit()?;
        log::info!("索引库迁移：回填 {} 条 parent", pairs.len());
    }

    // 旧库升级：content_meta 尚空但 content_fts 有数据 → 建索引关系（mtime_ns=0 视为待刷新）
    let has_meta: i64 = conn.query_row("SELECT COUNT(*) FROM content_meta", [], |r| r.get(0))?;
    if has_meta == 0 {
        let has_content: i64 = conn.query_row("SELECT COUNT(*) FROM content_fts", [], |r| r.get(0))?;
        if has_content > 0 {
            conn.execute(
                "INSERT OR IGNORE INTO content_meta(path, mtime_ns, bytes, indexed_at) \
                 SELECT path, 0, length(body), ?1 FROM content_fts",
                params![now()],
            )?;
            log::info!("索引库迁移：为 {has_content} 条既有正文建立 content_meta 记录");
        }
    }
    Ok(())
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
        // 「图片」是 `media` 的精确子集。判断模型的 `image` 槽位（"图片、照片、截图"）
        // 需要它才能真的过滤 —— 否则 `item_in_scope` 走 `_ => None` 分支，
        // 命中 `unwrap_or(true)` 而**静默放行全部文件**（比不筛还糟：看着像筛过了）。
        "image" => Some(&[
            "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "heic", "tif", "tiff", "avif",
        ]),
        // 同上：判断模型的 `executable` 槽位（"可执行程序、exe、安装包"）。
        // 注意这与 scope 里的 `app`（开始菜单扫描出的应用）**不是一回事**。
        "executable" => Some(&["exe", "msi", "com", "msix", "appx"]),
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
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; \
             PRAGMA cache_size=-16000; PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=2000;",
        )?;
        conn.execute_batch(SCHEMA).context("初始化数据库结构")?;
        migrate(&conn).context("迁移数据库结构")?;
        Ok(Storage { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Storage> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
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
                "INSERT INTO files(path, parent, name, ext, is_dir, size, mtime, mtime_ns) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(path) DO UPDATE SET parent = excluded.parent, name = excluded.name, \
                 ext = excluded.ext, is_dir = excluded.is_dir, size = excluded.size, \
                 mtime = excluded.mtime, mtime_ns = excluded.mtime_ns",
            )?;
            for f in batch {
                let parent = if f.parent.is_empty() { parent_of(&f.path) } else { f.parent.clone() };
                stmt.execute(params![
                    f.path,
                    parent,
                    f.name,
                    f.ext,
                    f.is_dir as i64,
                    f.size,
                    f.mtime,
                    f.mtime_ns
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_file(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        conn.execute("DELETE FROM content_fts WHERE path = ?1", params![path])?;
        conn.execute("DELETE FROM content_meta WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// 删除某目录及其下所有条目（含正文索引）。
    ///
    /// 子树用 `path GLOB 'dir\*'` 取：实测该写法被 SQLite 改写为范围查找并
    /// 走 path 的 UNIQUE 索引，而等价的 LIKE 前缀写法只能全表扫描。
    pub fn remove_path_tree(&self, path: &str) -> Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut n = 0usize;
        {
            let glob = glob_descendants(path);
            n += tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
            n += tx.execute("DELETE FROM files WHERE path GLOB ?1", params![glob])?;
            tx.execute("DELETE FROM content_fts WHERE path = ?1", params![path])?;
            tx.execute("DELETE FROM content_fts WHERE path GLOB ?1", params![glob])?;
            tx.execute("DELETE FROM content_meta WHERE path = ?1", params![path])?;
            tx.execute("DELETE FROM content_meta WHERE path GLOB ?1", params![glob])?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// 批量删除（对账时清理已消失的子项）。逐条按子树处理。
    pub fn remove_paths(&self, paths: &[String]) -> Result<usize> {
        let mut total = 0usize;
        for p in paths {
            total += self.remove_path_tree(p)?;
        }
        Ok(total)
    }

    /// 清空文件索引与正文索引。
    ///
    /// 注意：这里**不再**追加 `INSERT INTO files_fts(files_fts) VALUES('rebuild')`。
    /// `files_ad` 触发器已在 `DELETE FROM files` 时逐行维护了 files_fts，
    /// 再 rebuild 一次是重复劳动（旧实现在这里多做一遍，属实测确认的冗余）。
    pub fn clear_files(&self) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM files", [])?;
        tx.execute("DELETE FROM content_fts", [])?;
        tx.execute("DELETE FROM content_meta", [])?;
        tx.commit()?;
        Ok(())
    }

    /// 清空正文索引（保留文件索引）
    pub fn clear_content(&self) -> Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let n = tx.execute("DELETE FROM content_fts", [])?;
        tx.execute("DELETE FROM content_meta", [])?;
        tx.commit()?;
        Ok(n)
    }

    /// 某目录的**直接子项** → (is_dir, mtime_ns, size)。
    ///
    /// 走 `idx_files_parent`，用于对账时比对「磁盘上实际有什么」与「库里记了什么」。
    pub fn children_meta(&self, parent: &str) -> Result<std::collections::HashMap<String, (bool, i64, i64)>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare_cached("SELECT path, is_dir, mtime_ns, size FROM files WHERE parent = ?1")?;
        let rows = stmt.query_map(params![parent], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)? != 0, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?),
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 载入全部目录的 mtime 快照（path → mtime_ns），供对账常驻内存使用。
    ///
    /// 目录数量远少于文件数量，可以整表进内存，避免对每个目录各查一次库。
    pub fn load_dir_mtimes(&self) -> Result<std::collections::HashMap<String, i64>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT path, mtime_ns FROM files WHERE is_dir = 1")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn dir_count(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM files WHERE is_dir = 1", [], |r| r.get(0)).unwrap_or(0)
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

    /// 空闲页字节数。实测旧库 99.6MB 里有 53.3MB 是空闲页（53.5%）。
    pub fn freelist_bytes(&self) -> i64 {
        let conn = self.conn.lock();
        let free: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap_or(0);
        let page: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap_or(0);
        free * page
    }

    pub fn file_by_path(&self, path: &str) -> Option<FileRecord> {
        let conn = self.conn.lock();
        conn.query_row(
            &format!("SELECT {FILE_COLS} FROM files f WHERE f.path = ?1"),
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
            format!(
                "SELECT {FILE_COLS} FROM files_fts JOIN files f ON f.id = files_fts.rowid WHERE files_fts MATCH ?"
            )
        } else {
            args.push(like_pattern(q).into());
            format!("SELECT {FILE_COLS} FROM files f WHERE f.name LIKE ? ESCAPE '\\'")
        };
        sql.push_str(&scope_sql(scope, &mut args));
        // ⚠️ 这个 ORDER BY 是**截断的守卫**，不是最终排名 —— 最终排名在 Rust 侧由
        // `search::name_score` 决定（同档内保持这里的顺序，因为 `sort_by` 是稳定排序）。
        //
        // 为什么必须有档位键：原先只按 `length(f.name) ASC` 排，等于拿「名字短」当
        // 「相关度高」的代理。但 `name_score` 的前三档（精确 100 / 前缀 85 / 词首 70）
        // 是**常数分**，一批长名字的前缀匹配会被大量短名字的「包含」匹配整体挤出 LIMIT。
        //
        // 实测（本机 4.8 万文件，查询 `anim`）：总命中 223 条，其中**以 anim 开头**的
        // 有 40 个 —— 按长度排序时它们全部落在第 100 名开外被切掉，于是这 40 个本该排在
        // 最前的文件**一个都进不了前 50**（实测 0/40）。加上档位键后 40/40 全部进前 50。
        //
        // 加上档位键后，精确/前缀匹配无条件排在最前，截断只可能落在「其余」档内部 ——
        // 而那一档的分数随名字长度单调递减，与 `length ASC` 方向一致，所以截断是安全的。
        // 词首档（70）SQL 里难以干净表达（要枚举分隔符），交给调用方用足够大的候选上限覆盖。
        sql.push_str(
            " ORDER BY CASE \
                 WHEN lower(f.name) = ? THEN 0 \
                 WHEN lower(f.name) LIKE ? ESCAPE '\\' THEN 1 \
                 ELSE 2 END ASC, \
             length(f.name) ASC, f.mtime DESC LIMIT ?",
        );
        args.push(q.to_lowercase().into());
        args.push(prefix_pattern(q).into());
        args.push((limit as i64).into());
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), map_file)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn list_files_by_scope(&self, scope: &SearchScopeFilter, limit: usize) -> Result<Vec<FileRecord>> {
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        let mut sql = format!("SELECT {FILE_COLS} FROM files f WHERE 1 = 1");
        sql.push_str(&scope_sql(scope, &mut args));
        sql.push_str(" ORDER BY f.mtime DESC LIMIT ?");
        args.push((limit as i64).into());
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), map_file)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------------- 正文索引 ----------------
    /// 写入正文索引，同时更新 `content_meta`（记录被索引时的 mtime_ns）。
    pub fn upsert_content(&self, path: &str, body: &str, mtime_ns: i64) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM content_fts WHERE path = ?1", params![path])?;
        tx.execute("INSERT INTO content_fts(path, body) VALUES (?1, ?2)", params![path, body])?;
        tx.execute(
            "INSERT INTO content_meta(path, mtime_ns, bytes, indexed_at) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(path) DO UPDATE SET mtime_ns = excluded.mtime_ns, bytes = excluded.bytes, \
             indexed_at = excluded.indexed_at",
            params![path, mtime_ns, body.len() as i64, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn has_content(&self, path: &str) -> bool {
        let conn = self.conn.lock();
        conn.query_row("SELECT 1 FROM content_meta WHERE path = ?1 LIMIT 1", params![path], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    /// 该文件是否需要（重新）建立正文索引。
    ///
    /// 旧实现用 `SELECT 1 FROM content_fts WHERE path = ?` 判断，而 `path` 是
    /// FTS5 的 UNINDEXED 列 —— 该查询会**全表扫描** FTS5 内容表（实测查询计划为
    /// `SCAN content_fts VIRTUAL TABLE`）。扫描期每个可索引文件调用一次，
    /// 是初次全量扫描慢的主因之一。改查 `content_meta`（path 为主键）后为索引命中。
    pub fn content_needs_update(&self, path: &str, mtime_ns: i64) -> bool {
        let conn = self.conn.lock();
        let existing: Option<i64> = conn
            .query_row("SELECT mtime_ns FROM content_meta WHERE path = ?1", params![path], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        match existing {
            // mtime_ns = 0 表示由老库迁移而来、新鲜度未知，视为待刷新
            Some(mt) => mt != mtime_ns || mt == 0,
            None => true,
        }
    }

    pub fn content_bytes(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row("SELECT COALESCE(SUM(bytes), 0) FROM content_meta", [], |r| r.get(0)).unwrap_or(0)
    }

    /// 正文索引里指向「文件索引中不存在的路径」的条目。
    ///
    /// 这些条目搜得到却打不开 —— 实测旧库里 543 条正文对应只有 1 条文件记录。
    pub fn orphan_content_paths(&self, limit: usize) -> Vec<String> {
        let conn = self.conn.lock();
        let Ok(mut stmt) = conn.prepare(
            "SELECT m.path FROM content_meta m WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.path = m.path) LIMIT ?1",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0));
        rows.map(|it| it.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    pub fn orphan_content_count(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM content_meta m WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.path = m.path)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    /// 删除孤儿正文条目，返回删除条数。
    pub fn purge_orphan_content(&self) -> Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let n = tx.execute(
            "DELETE FROM content_fts WHERE path IN \
             (SELECT m.path FROM content_meta m WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.path = m.path))",
            [],
        )?;
        tx.execute(
            "DELETE FROM content_meta WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.path = content_meta.path)",
            [],
        )?;
        tx.commit()?;
        Ok(n)
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

    /// 一次读全「使用情况」，供排序阶段在内存里查。
    ///
    /// 单条查询失败时返回空集合而不是报错：使用统计只是**加分项**，读不到就当作没有，
    /// 不该让整个搜索失败。
    pub fn usage_stats(&self) -> UsageStats {
        let conn = self.conn.lock();
        let mut stats = UsageStats::default();
        if let Ok(mut stmt) = conn.prepare("SELECT item_id, last_used, use_count FROM recent") {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            }) {
                for (id, last, count) in rows.flatten() {
                    stats.recent.insert(id, (last, count));
                }
            }
        }
        if let Ok(mut stmt) = conn.prepare("SELECT item_id FROM pins") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for id in rows.flatten() {
                    stats.pinned.insert(id);
                }
            }
        }
        stats
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

    // ---------------- 重命名 / 移动：迁移引用方 ----------------
    /// 把 `from`（文件或目录，含其整棵子树）在库中的路径全部改写到 `to`。
    ///
    /// 需要迁移的不只是 `files`：`pins.item_id`、`recent.item_id` 形如
    /// `file:<path>` / `folder:<path>`，`hotkeys.target_path` 存原始路径。
    /// 不迁移的话，文件改名后置顶与最近使用会指向一个不存在的路径 —— 点开即失败。
    pub fn rename_prefix(&self, from: &str, to: &str) -> Result<usize> {
        if from.is_empty() || from == to {
            return Ok(0);
        }
        let glob = glob_descendants(from);
        // SQLite substr 为 1 基：n = from.len() + 1 即「from 之后的剩余部分」
        let n = (from.len() + 1) as i64;
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut affected = 0usize;
        {
            affected += tx.execute(
                "UPDATE files SET path = ?1, parent = ?2 WHERE path = ?3",
                params![to, parent_of(to), from],
            )?;
            affected += tx.execute(
                "UPDATE files SET path = ?1 || substr(path, ?2) WHERE path GLOB ?3",
                params![to, n, glob],
            )?;
            // 子树内部的 parent 也要跟着改（直接子项的 parent 已在上面被重写）
            tx.execute(
                "UPDATE files SET parent = ?1 || substr(parent, ?2) WHERE parent GLOB ?3",
                params![to, n, glob],
            )?;

            tx.execute("UPDATE content_fts SET path = ?1 WHERE path = ?2", params![to, from])?;
            tx.execute(
                "UPDATE content_meta SET path = ?1 || substr(path, ?2) WHERE path GLOB ?3",
                params![to, n, glob],
            )?;
            tx.execute("UPDATE content_meta SET path = ?1 WHERE path = ?2", params![to, from])?;

            for table in ["pins", "recent"] {
                tx.execute(
                    &format!(
                        "UPDATE {table} SET item_id = substr(item_id, 1, instr(item_id, ':')) || ?1 || substr(path, ?2), \
                         path = ?1 || substr(path, ?2) WHERE path = ?3",
                    ),
                    params![to, n, from],
                )?;
                tx.execute(
                    &format!(
                        "UPDATE {table} SET item_id = substr(item_id, 1, instr(item_id, ':')) || ?1 || substr(path, ?2), \
                         path = ?1 || substr(path, ?2) WHERE path GLOB ?3",
                    ),
                    params![to, n, glob],
                )?;
            }
            tx.execute(
                "UPDATE hotkeys SET target_path = ?1 || substr(target_path, ?2) WHERE target_path = ?3",
                params![to, n, from],
            )?;
            tx.execute(
                "UPDATE hotkeys SET target_path = ?1 || substr(target_path, ?2) WHERE target_path GLOB ?3",
                params![to, n, glob],
            )?;
        }
        tx.commit()?;
        Ok(affected)
    }

    // ---------------- 一致性 / 空间维护 ----------------
    pub fn integrity(&self) -> IntegrityReport {
        let conn = self.conn.lock();
        let one = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1) };
        let files = one("SELECT COUNT(*) FROM files");
        let dirs = one("SELECT COUNT(*) FROM files WHERE is_dir = 1");
        let content = one("SELECT COUNT(*) FROM content_fts");
        let meta = one("SELECT COUNT(*) FROM content_meta");
        let orphan = one(
            "SELECT COUNT(*) FROM content_meta m WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.path = m.path)",
        );
        let dangling_pins = one(
            "SELECT COUNT(*) FROM pins p WHERE p.kind IN ('file','folder') AND NOT EXISTS (SELECT 1 FROM files f WHERE f.path = p.path)",
        );
        let dangling_recent = one(
            "SELECT COUNT(*) FROM recent r WHERE r.kind IN ('file','folder') AND NOT EXISTS (SELECT 1 FROM files f WHERE f.path = r.path)",
        );
        let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap_or(0);
        let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap_or(0);
        let freelist: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap_or(0);
        IntegrityReport {
            files,
            dirs,
            content,
            content_meta: meta,
            orphan_content: orphan,
            dangling_pins,
            dangling_recent,
            db_bytes: page_count * page_size,
            freelist_bytes: freelist * page_size,
        }
    }

    /// 空间回收：WAL checkpoint + 按需 VACUUM + 查询计划统计刷新。
    ///
    /// VACUUM 会重写整个库（本机实测 99.6MB → 46.3MB），代价高，
    /// 因此只在空闲页占比超阈值或显式强制时执行。
    pub fn maintenance(&self, force: bool) -> Result<MaintenanceReport> {
        let conn = self.conn.lock();
        let before = {
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap_or(0);
            let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap_or(0);
            let freelist: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap_or(0);
            (page_count * page_size, freelist * page_size)
        };
        // 先把 WAL 落盘并截断，否则 VACUUM 的收益会被 WAL 抵消
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

        let (total, free) = before;
        let should_vacuum = force || (free >= 16 * 1024 * 1024 && total > 0 && free * 100 / total >= 25);
        let mut vacuumed = false;
        if should_vacuum {
            let started = std::time::Instant::now();
            conn.execute_batch("VACUUM;")?;
            vacuumed = true;
            log::info!("索引库 VACUUM 完成，耗时 {:?}", started.elapsed());
        }
        // 让 SQLite 依据真实查询模式更新统计信息（比 ANALYZE 更轻）
        let _ = conn.execute_batch("PRAGMA optimize;");

        let after = {
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap_or(0);
            let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap_or(0);
            let freelist: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap_or(0);
            (page_count * page_size, freelist * page_size)
        };
        Ok(MaintenanceReport {
            vacuumed,
            before_bytes: total,
            after_bytes: after.0,
            before_free: free,
            after_free: after.1,
        })
    }
}

/// 索引一致性快照
#[derive(Clone, Copy, Debug, Default)]
pub struct IntegrityReport {
    pub files: i64,
    pub dirs: i64,
    pub content: i64,
    pub content_meta: i64,
    /// 正文索引里找不到对应文件的条目数（搜得到、打不开）
    pub orphan_content: i64,
    pub dangling_pins: i64,
    pub dangling_recent: i64,
    pub db_bytes: i64,
    pub freelist_bytes: i64,
}

impl IntegrityReport {
    /// 是否处于健康状态（无孤儿正文、无悬空引用）
    pub fn healthy(&self) -> bool {
        self.orphan_content == 0 && self.dangling_pins == 0 && self.dangling_recent == 0
    }
}

/// 空间回收结果
#[derive(Clone, Copy, Debug, Default)]
pub struct MaintenanceReport {
    pub vacuumed: bool,
    pub before_bytes: i64,
    pub after_bytes: i64,
    pub before_free: i64,
    pub after_free: i64,
}

fn map_file(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: r.get(0)?,
        path: r.get(1)?,
        parent: r.get(2)?,
        name: r.get(3)?,
        ext: r.get(4)?,
        is_dir: r.get::<_, i64>(5)? != 0,
        size: r.get(6)?,
        mtime: r.get(7)?,
        mtime_ns: r.get(8)?,
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
