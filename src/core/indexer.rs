//! 文件索引：初始遍历 + ReadDirectoryChangesW（notify）增量监控 + 文本正文索引。
//!
//! 说明：设计文档规划的 NTFS USN Journal 增量方案需要以管理员权限打开卷句柄，
//! 当前版本采用 `notify`（ReadDirectoryChangesW）作为非特权增量感知实现，
//! 接口保持不变，后续可在 `watch_roots` 中替换为 USN 读取器。

use crate::core::settings::AppSettings;
use crate::core::storage::{FileRecord, Storage};
use crate::models::BackendNotification;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use walkdir::WalkDir;

pub type Notifier = Arc<dyn Fn(BackendNotification) + Send + Sync>;

#[derive(Default)]
pub struct IndexStats {
    pub files: AtomicU64,
    pub content_files: AtomicU64,
    pub done: AtomicBool,
    pub scanning: AtomicBool,
}

pub struct Indexer {
    storage: Arc<Storage>,
    settings: parking_lot::RwLock<AppSettings>,
    pub stats: Arc<IndexStats>,
    notifier: Notifier,
    watcher: parking_lot::Mutex<Option<RecommendedWatcher>>,
}

const CONTENT_EXTS: &[&str] = &[
    "md", "txt", "rs", "js", "ts", "tsx", "jsx", "py", "json", "toml", "yml", "yaml", "slint", "html",
    "css", "csv", "log", "ini", "cfg", "xml", "sql", "sh", "ps1", "bat", "cmd", "go", "java", "kt",
    "c", "cpp", "h", "cs", "vue", "env", "conf",
];

fn lower_thread_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

fn file_record(path: &Path, meta: &std::fs::Metadata) -> FileRecord {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let ext = if meta.is_dir() {
        String::new()
    } else {
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default()
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    FileRecord {
        id: 0,
        path: path.to_string_lossy().to_string(),
        name,
        ext,
        is_dir: meta.is_dir(),
        size: meta.len() as i64,
        mtime,
    }
}

impl Indexer {
    pub fn new(storage: Arc<Storage>, settings: AppSettings, notifier: Notifier) -> Arc<Indexer> {
        Arc::new(Indexer {
            storage,
            settings: parking_lot::RwLock::new(settings),
            stats: Arc::new(IndexStats::default()),
            notifier,
            watcher: parking_lot::Mutex::new(None),
        })
    }

    pub fn update_settings(&self, settings: AppSettings) {
        *self.settings.write() = settings;
    }

    fn is_excluded(&self, path: &Path, settings: &AppSettings) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if !settings.include_hidden && name.starts_with('.') {
            return true;
        }
        if settings.exclude_build_caches && settings.excluded_dirs.iter().any(|e| e.eq_ignore_ascii_case(name)) {
            return true;
        }
        false
    }

    fn content_indexable(&self, rec: &FileRecord, settings: &AppSettings) -> bool {
        settings.content_index_enabled
            && !rec.is_dir
            && rec.size > 0
            && rec.size <= (settings.content_max_kb as i64) * 1024
            && CONTENT_EXTS.contains(&rec.ext.as_str())
    }

    fn index_content(&self, rec: &FileRecord) {
        let Ok(bytes) = std::fs::read(&rec.path) else { return };
        if bytes.iter().take(4096).any(|&b| b == 0) {
            return; // 二进制
        }
        let text = String::from_utf8_lossy(&bytes);
        let body: String = text.chars().take(200_000).collect();
        if self.storage.upsert_content(&rec.path, &body).is_ok() {
            self.stats.content_files.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 启动后台线程：全量扫描 + 增量监控。
    pub fn start(self: &Arc<Self>) {
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name("anycast-indexer".into())
            .spawn(move || {
                lower_thread_priority();
                this.full_scan();
                this.watch_roots();
            })
            .expect("spawn indexer thread");
    }

    /// 重建：清空后全量扫描
    pub fn rebuild(self: &Arc<Self>) {
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name("anycast-reindex".into())
            .spawn(move || {
                lower_thread_priority();
                let _ = this.storage.clear_files();
                this.stats.files.store(0, Ordering::Relaxed);
                this.stats.content_files.store(0, Ordering::Relaxed);
                this.full_scan();
                this.watch_roots();
            })
            .expect("spawn reindex thread");
    }

    fn full_scan(&self) {
        if self.stats.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stats.done.store(false, Ordering::Relaxed);
        let settings = self.settings.read().clone();
        let started = std::time::Instant::now();
        let mut batch: Vec<FileRecord> = Vec::with_capacity(512);
        let mut content_queue: Vec<FileRecord> = Vec::new();
        let mut total: u64;

        for root in &settings.index_roots {
            let root_path = PathBuf::from(root);
            if !root_path.exists() {
                continue;
            }
            let walker = WalkDir::new(&root_path).follow_links(false).into_iter();
            let mut it = walker.filter_entry(|e| !(e.depth() > 0 && e.file_type().is_dir() && self.is_excluded(e.path(), &settings)));
            while let Some(entry) = it.next() {
                let Ok(entry) = entry else { continue };
                if entry.depth() == 0 {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                if !settings.include_hidden {
                    if let Some(n) = entry.file_name().to_str() {
                        if n.starts_with('.') {
                            continue;
                        }
                    }
                }
                let rec = file_record(entry.path(), &meta);
                if self.content_indexable(&rec, &settings) && !self.storage.has_content(&rec.path) {
                    content_queue.push(rec.clone());
                }
                batch.push(rec);
                if batch.len() >= 500 {
                    if self.storage.upsert_files(&batch).is_ok() {
                        total = self.storage.file_count().max(0) as u64;
                        self.stats.files.store(total, Ordering::Relaxed);
                        (self.notifier)(BackendNotification::IndexProgress {
                            files: total,
                            content_files: self.stats.content_files.load(Ordering::Relaxed),
                            done: false,
                        });
                    }
                    batch.clear();
                }
            }
        }
        if !batch.is_empty() {
            let _ = self.storage.upsert_files(&batch);
        }
        let count = self.storage.file_count().max(0) as u64;
        self.stats.files.store(count, Ordering::Relaxed);
        log::info!("文件索引扫描完成：{} 条，耗时 {:?}", count, started.elapsed());

        // 正文索引（低优先级，逐个处理）
        for rec in content_queue {
            self.index_content(&rec);
            std::thread::sleep(Duration::from_millis(1));
        }
        self.stats.content_files.store(self.storage.content_count().max(0) as u64, Ordering::Relaxed);
        self.stats.done.store(true, Ordering::Relaxed);
        self.stats.scanning.store(false, Ordering::SeqCst);
        (self.notifier)(BackendNotification::IndexProgress {
            files: count,
            content_files: self.stats.content_files.load(Ordering::Relaxed),
            done: true,
        });
    }

    fn handle_fs_event(&self, event: Event) {
        let settings = self.settings.read().clone();
        match event.kind {
            EventKind::Access(_) | EventKind::Other => return,
            _ => {}
        }
        for path in event.paths {
            // 路径中包含排除目录则忽略
            if path.components().any(|c| {
                let n = c.as_os_str().to_string_lossy();
                settings.excluded_dirs.iter().any(|e| e.eq_ignore_ascii_case(&n))
            }) {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    let rec = file_record(&path, &meta);
                    if self.storage.upsert_files(std::slice::from_ref(&rec)).is_ok() {
                        if self.content_indexable(&rec, &settings) {
                            self.index_content(&rec);
                        }
                    }
                }
                Err(_) => {
                    let _ = self.storage.remove_path_tree(&path.to_string_lossy());
                }
            }
        }
        self.stats.files.store(self.storage.file_count().max(0) as u64, Ordering::Relaxed);
    }

    fn watch_roots(self: &Arc<Self>) {
        let settings = self.settings.read().clone();
        if !settings.incremental_index {
            return;
        }
        let weak = Arc::downgrade(self);
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let (Ok(ev), Some(this)) = (res, weak.upgrade()) {
                this.handle_fs_event(ev);
            }
        });
        match watcher {
            Ok(mut w) => {
                for root in &settings.index_roots {
                    let p = Path::new(root);
                    if p.exists() {
                        if let Err(e) = w.watch(p, RecursiveMode::Recursive) {
                            log::warn!("监控目录失败 {root}: {e}");
                        }
                    }
                }
                *self.watcher.lock() = Some(w);
            }
            Err(e) => log::warn!("创建文件监控失败: {e}"),
        }
    }
}
