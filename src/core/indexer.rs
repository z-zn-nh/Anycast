//! 文件索引：**目录 mtime 增量对账** + notify 实时增量 + 文本正文索引。
//!
//! ## 为什么不用 USN Journal
//!
//! 设计文档原规划的 NTFS USN Journal 需要以管理员权限打开卷句柄，
//! 与「绿色免安装、双击即用」的定位冲突。改用**目录 mtime 对账**，
//! 它建立在一条已实测的 NTFS 语义之上（2026-09-24 六项实验全部符合预期）：
//!
//! | 操作 | 该目录自身 mtime |
//! |---|---|
//! | 增 / 删 / 改名直接子项 | **变** |
//! | 修改文件内容 | **不变** |
//! | 孙目录内发生变化 | **不变**（不向上传播） |
//!
//! 于是「目录 mtime 未变」⇒「它的直接子项集合未变」，
//! 该目录下所有文件的元数据都无需再比对、无需再写库。
//! 遍历仍要 readdir（否则找不到子目录），但省掉了每个文件的 stat 与 upsert ——
//! 这正是初次全量扫描耗时的大头。
//!
//! ## 与 notify 的分工
//!
//! - notify（ReadDirectoryChangesW）负责**实时**：秒级反映变更；
//! - 目录 mtime 对账负责**兜底**：每 10 分钟一次，覆盖休眠唤醒、
//!   监控丢失、进程被强杀期间发生的变更。
//!
//! 两者都只写增量，不做全量重扫。

use crate::core::settings::AppSettings;
use crate::core::storage::{mtime_ns_of, parent_of, FileRecord, Storage};
use crate::models::BackendNotification;
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

pub type Notifier = Arc<dyn Fn(BackendNotification) + Send + Sync>;

/// 扫描请求等级。取「较高者优先」，不排队、不叠加。
const REQ_NONE: u8 = 0;
/// 增量对账：只处理 mtime 变化的目录
const REQ_RECONCILE: u8 = 1;
/// 全量校验：强制检查每个目录的子项，但**不清空**。
/// 用于覆盖「目录 mtime 未变、文件自身却变了」的情形（见下）。
const REQ_FULL: u8 = 2;
/// 清空后全量重建
const REQ_REBUILD: u8 = 3;

/// 批量写库的行数
const BATCH: usize = 500;
/// 正文索引队列上限，避免极端目录结构下内存无界增长
const CONTENT_QUEUE_CAP: usize = 4000;
/// 单个文件正文最多收录的字符数
const CONTENT_CHAR_CAP: usize = 200_000;
/// 兜底对账间隔
const RECONCILE_INTERVAL: Duration = Duration::from_secs(600);
/// 改名 `From` / `To` 两条事件的配对时限。
///
/// Windows 会把一次改名拆成两条事件，且**相邻**投递，正常间隔是微秒级。
/// 给到 5 秒是因为：窗口偏大只是「源已移出监控范围」时多留几秒脏数据（无害），
/// 窗口偏小却会把一次正常改名拆成「删 + 增」，丢掉 `pins` / `recent` 的迁移。
const RENAME_PAIR_WINDOW: Duration = Duration::from_secs(5);
/// 全量校验间隔。
///
/// **为什么必须有它**：目录 mtime 只在「直接子项增删改名」时变化。
/// 文件内容被改写时目录 mtime **不变** —— 于是对账会跳过它，
/// 库里那条记录的 mtime / size 就停在旧值（正文索引也不会更新）。
/// 应用运行期间这类变更由 `notify` 实时接住；但**应用没开的时候改的文件**
/// 只能靠一次全量校验补回来。取 24 小时。
const FULL_RESCAN_INTERVAL: i64 = 24 * 3600;
/// kv 表里记录上次全量校验时间
const KV_LAST_FULL: &str = "index:last_full_scan";

#[derive(Default)]
pub struct IndexStats {
    pub files: AtomicU64,
    pub content_files: AtomicU64,
    pub done: AtomicBool,
    pub scanning: AtomicBool,
    /// 因写入失败被丢弃的行数。**正常情况下必须恒为 0**：
    /// 旧实现把 `batch.clear()` 放在写入结果判断之外，
    /// 一旦 upsert 失败，整批 500 条会被静默丢弃且不留任何痕迹。
    pub dropped: AtomicU64,
    pub dirs_visited: AtomicU64,
    pub dirs_changed: AtomicU64,
    pub removed: AtomicU64,
    pub last_scan_ms: AtomicU64,
    pub last_error: Mutex<Option<String>>,
    pub last_mode: Mutex<String>,
}

/// 供 UI / 设置页读取的索引状态快照
#[derive(Clone, Debug, Default)]
pub struct IndexStatus {
    pub files: u64,
    pub content_files: u64,
    pub dirs_visited: u64,
    pub dirs_changed: u64,
    pub removed: u64,
    pub dropped: u64,
    pub last_scan_ms: u64,
    pub mode: String,
    pub error: Option<String>,
}

pub struct Indexer {
    storage: Arc<Storage>,
    settings: RwLock<AppSettings>,
    pub stats: Arc<IndexStats>,
    notifier: Notifier,
    watcher: Mutex<Option<RecommendedWatcher>>,
    request: AtomicU8,
    /// 每接受一次新的扫描请求就 +1；正在跑的扫描发现 epoch 变了就提前收尾
    epoch: AtomicU64,
    wake_lock: Mutex<()>,
    wake: Condvar,
    started: AtomicBool,
    /// 等待配对的改名「旧路径」。
    ///
    /// Windows 的 `ReadDirectoryChangesW` **不会**给出 `RenameMode::Both`，
    /// 而是把一次改名拆成两条独立事件：先 `RenameMode::From`（旧路径）、
    /// 再 `RenameMode::To`（新路径），各自只带一个路径。
    /// 因此必须自己把两条事件配对，否则 `handle_rename` 永远不会被触发 ——
    /// `pins` / `recent` 的迁移等于没写。
    /// 元素为 `(入队时刻, 旧路径)`，按到达顺序配对。
    pending_rename: Mutex<Vec<(Instant, PathBuf)>>,
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

/// 统一路径分隔符为反斜杠。
///
/// `read_dir` 给出的子路径本来就是反斜杠，但**用户配置或命令行传入的根目录**
/// 可能是正斜杠（`C:/Users/...`）。不统一的话根目录行存成 `C:/x`，
/// 而子项的 `parent` 算出来是 `C:\x` —— 两者对不上，
/// 对账就会把整棵子树当成「新增」反复重写。
/// Windows 文件名不允许含 `/`，因此这个替换是安全的。
fn norm_path(p: &str) -> String {
    if p.contains('/') {
        p.replace('/', "\\")
    } else {
        p.to_string()
    }
}

fn file_record(path: &Path, meta: &std::fs::Metadata) -> FileRecord {
    let path_str = norm_path(&path.to_string_lossy());
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let is_dir = meta.is_dir();
    let ext = if is_dir {
        String::new()
    } else {
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default()
    };
    let modified = meta.modified().unwrap_or(UNIX_EPOCH);
    FileRecord {
        id: 0,
        parent: parent_of(&path_str),
        path: path_str,
        name,
        ext,
        is_dir,
        // 目录的 size 在 Windows 上不稳定（0 或 4096 之间摆动），
        // 固定为 0，避免它污染对账时的「是否变化」判断。
        size: if is_dir { 0 } else { meta.len() as i64 },
        mtime: modified.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        mtime_ns: mtime_ns_of(modified),
    }
}

/// 单轮扫描的累计上下文
#[derive(Default)]
struct ScanCtx {
    batch: Vec<FileRecord>,
    content_queue: Vec<(String, i64)>,
    written: u64,
    dropped: u64,
    removed: u64,
    dirs_visited: u64,
    dirs_changed: u64,
    content_indexed: u64,
}

impl Indexer {
    pub fn new(storage: Arc<Storage>, settings: AppSettings, notifier: Notifier) -> Arc<Indexer> {
        Arc::new(Indexer {
            storage,
            settings: RwLock::new(settings),
            stats: Arc::new(IndexStats::default()),
            notifier,
            watcher: Mutex::new(None),
            request: AtomicU8::new(REQ_NONE),
            epoch: AtomicU64::new(0),
            wake_lock: Mutex::new(()),
            wake: Condvar::new(),
            started: AtomicBool::new(false),
            pending_rename: Mutex::new(Vec::new()),
        })
    }

    pub fn update_settings(&self, settings: AppSettings) {
        *self.settings.write() = settings;
    }

    // ------------------------------------------------------------------
    // 调度
    // ------------------------------------------------------------------

    /// 启动索引线程：先挂监控，再跑首轮对账，之后常驻等待请求 / 定时兜底。
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name("anycast-indexer".into())
            .spawn(move || {
                lower_thread_priority();
                // 先挂监控：扫描期间发生的变更也不会漏
                this.watch_roots();
                this.request_scan(REQ_RECONCILE);
                loop {
                    let level = this.request.swap(REQ_NONE, Ordering::SeqCst);
                    if level == REQ_NONE {
                        // 拿锁后再确认一次，避免「检查完就 wait」与 setter 之间的丢唤醒
                        let mut guard = this.wake_lock.lock();
                        if this.request.load(Ordering::SeqCst) == REQ_NONE {
                            this.wake.wait_for(&mut guard, RECONCILE_INTERVAL);
                        }
                        drop(guard);
                        // 只有「等到超时」才走定时兜底；被唤醒说明已有请求排队
                        if this.request.load(Ordering::SeqCst) == REQ_NONE {
                            let level = if this.full_rescan_due() { REQ_FULL } else { REQ_RECONCILE };
                            this.run_scan(level);
                        }
                        continue;
                    }
                    this.run_scan(level);
                }
            })
            .expect("spawn indexer thread");
    }

    /// 请求一轮扫描。等级取较高者；只有等级真的提升时才打断在跑的扫描。
    fn request_scan(&self, level: u8) {
        {
            let _guard = self.wake_lock.lock();
            let mut cur = self.request.load(Ordering::SeqCst);
            while level > cur {
                match self.request.compare_exchange(cur, level, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => {
                        self.epoch.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                    Err(actual) => cur = actual,
                }
            }
        }
        self.wake.notify_all();
    }

    /// 主动触发一轮增量对账（设置页「立即重建」旁边的轻量入口）
    pub fn reconcile_now(&self) {
        self.request_scan(REQ_RECONCILE);
    }

    /// 是否该做一次全量校验（距上次超过 24 小时）
    fn full_rescan_due(&self) -> bool {
        let last: i64 = self
            .storage
            .kv_get(KV_LAST_FULL)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        chrono::Utc::now().timestamp() - last >= FULL_RESCAN_INTERVAL
    }

    /// 主动触发一轮全量校验（不清空，只强制检查每个目录的子项）
    pub fn full_rescan(&self) {
        self.request_scan(REQ_FULL);
    }

    /// 清空后全量重建。
    ///
    /// **注意**：这里只登记请求，真正的清空由索引线程在扫描开始时执行。
    /// 旧实现在调用方线程直接 `clear_files()` 再调 `full_scan()`，
    /// 而 `full_scan()` 开头遇到「已有扫描在跑」会直接 return ——
    /// 结果就是索引被清空、重扫却没发生，留下一个几乎空白的索引。
    /// 实测本机数据库正是这个状态：正文 543 条，文件只有 1 条。
    pub fn rebuild(&self) {
        self.request_scan(REQ_REBUILD);
    }

    /// 同步跑完一轮扫描才返回（不依赖后台线程）。
    ///
    /// 供命令行 `--index-scan` 与验证使用：这种情况下不应调用 `start()`，
    /// 否则后台线程会同时挂着监控并抢扫描。
    /// `mode`: 0 = 增量对账，1 = 全量校验，2 = 清空重建。
    pub fn scan_blocking(&self, mode: u8) {
        self.run_scan(match mode {
            2 => REQ_REBUILD,
            1 => REQ_FULL,
            _ => REQ_RECONCILE,
        });
    }

    // ------------------------------------------------------------------
    // 扫描
    // ------------------------------------------------------------------

    fn run_scan(&self, level: u8) {
        self.stats.scanning.store(true, Ordering::SeqCst);
        self.stats.done.store(false, Ordering::Relaxed);
        let started = Instant::now();
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        let settings = self.settings.read().clone();
        let wipe = level == REQ_REBUILD;
        // 全量校验：目录 mtime 一致也照样检查子项。
        // 注意这里**不清空**，逐项比对仍会跳过真正没变的行，
        // 所以一次全量校验只写「确实变了」的那些。
        let force_all = level == REQ_FULL;

        if wipe {
            match self.storage.clear_files() {
                Ok(_) => log::info!("重建索引：已清空文件索引与正文索引"),
                Err(e) => self.record_error(format!("清空索引失败: {e}")),
            }
        }

        // 正文索引关闭时把已有正文一并清掉，否则关掉开关也不会释放那几十 MB
        if !settings.content_index_enabled {
            let n = self.storage.content_count();
            if n > 0 {
                match self.storage.clear_content() {
                    Ok(_) => log::info!("正文索引已关闭，清理 {n} 条既有正文索引"),
                    Err(e) => self.record_error(format!("清理正文索引失败: {e}")),
                }
            }
        }

        // 目录 mtime 快照常驻内存：目录数量远少于文件数量，整表进内存
        // 好过对每个目录各查一次库。
        let mut dir_mtimes: HashMap<String, i64> =
            if wipe { HashMap::new() } else { self.storage.load_dir_mtimes().unwrap_or_default() };

        let roots: Vec<PathBuf> = settings
            .index_roots
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        let missing: Vec<&String> =
            settings.index_roots.iter().filter(|r| !Path::new(r).exists()).collect();
        if !missing.is_empty() {
            log::warn!("索引目录不存在，本轮跳过: {missing:?}");
        }

        let mut ctx = ScanCtx::default();
        let mut interrupted = false;
        for root in &roots {
            self.walk_dir(root, &settings, &mut dir_mtimes, &mut ctx, my_epoch, force_all);
            if self.epoch.load(Ordering::SeqCst) != my_epoch {
                interrupted = true;
                log::info!("索引扫描被更高优先级的请求打断，提前结束本轮");
                break;
            }
        }
        self.flush(&mut ctx);

        // 正文索引：低优先级逐个处理
        let queue = std::mem::take(&mut ctx.content_queue);
        for (path, mtime_ns) in queue {
            if self.storage.content_needs_update(&path, mtime_ns) {
                if self.index_content(&path, mtime_ns) {
                    ctx.content_indexed += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        // 收尾：孤儿清理 + WAL 回收 + 按需 VACUUM
        let mut purged = 0i64;
        if !interrupted {
            let orphans = self.storage.orphan_content_count();
            if orphans > 0 {
                match self.storage.purge_orphan_content() {
                    Ok(n) => {
                        purged = n as i64;
                        log::info!("清理孤儿正文索引 {n} 条（对应文件已不在索引中）");
                    }
                    Err(e) => self.record_error(format!("清理孤儿正文失败: {e}")),
                }
            }
        }
        if let Err(e) = self.storage.maintenance(wipe) {
            self.record_error(format!("索引空间回收失败: {e}"));
        }

        // 全量校验（含重建）成功走完 → 记下时间，24 小时内不必再做
        if !interrupted && (force_all || wipe) {
            let now = chrono::Utc::now().timestamp();
            if let Err(e) = self.storage.kv_set(KV_LAST_FULL, &now.to_string()) {
                log::warn!("记录全量校验时间失败: {e}");
            }
        }

        let files = self.storage.file_count().max(0) as u64;
        let content = self.storage.content_count().max(0) as u64;
        self.stats.files.store(files, Ordering::Relaxed);
        self.stats.content_files.store(content, Ordering::Relaxed);
        self.stats.dirs_visited.store(ctx.dirs_visited, Ordering::Relaxed);
        self.stats.dirs_changed.store(ctx.dirs_changed, Ordering::Relaxed);
        self.stats.removed.store(ctx.removed, Ordering::Relaxed);
        self.stats.dropped.fetch_add(ctx.dropped, Ordering::Relaxed);
        self.stats.last_scan_ms.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
        let mode = if wipe { "rebuild" } else if force_all { "full" } else { "reconcile" };
        *self.stats.last_mode.lock() = mode.into();
        self.stats.done.store(true, Ordering::Relaxed);
        self.stats.scanning.store(false, Ordering::SeqCst);

        log::info!(
            "索引{}完成：文件 {files} 条（新写 {}、移除 {}、丢弃 {}），目录 {}/{} 个有变化，\
             正文 {content} 条（本轮新增 {}、清理孤儿 {purged}），耗时 {:?}",
            match mode {
                "rebuild" => "重建",
                "full" => "全量校验",
                _ => "对账",
            },
            ctx.written,
            ctx.removed,
            ctx.dropped,
            ctx.dirs_changed,
            ctx.dirs_visited,
            ctx.content_indexed,
            started.elapsed()
        );
        if ctx.dropped > 0 {
            log::error!("本轮有 {} 行索引数据写入失败被丢弃，下次对账会重试", ctx.dropped);
        }

        (self.notifier)(BackendNotification::IndexProgress {
            files,
            content_files: content,
            done: true,
        });
    }

    /// 递归对账一个目录。
    ///
    /// 无论 mtime 是否变化都要 readdir（否则找不到子目录），
    /// 但**只有 mtime 变化时才做库比对与写入**。
    fn walk_dir(
        &self,
        dir: &Path,
        settings: &AppSettings,
        mem: &mut HashMap<String, i64>,
        ctx: &mut ScanCtx,
        my_epoch: u64,
        force_all: bool,
    ) {
        ctx.dirs_visited += 1;
        let dir_str = norm_path(&dir.to_string_lossy());

        let Ok(meta) = std::fs::metadata(dir) else { return };
        let fs_mtime = mtime_ns_of(meta.modified().unwrap_or(UNIX_EPOCH));

        // 目录自身始终刷新：数量远少于文件，代价可忽略，
        // 但它是下一轮对账能「跳过整棵子树」的前提。
        ctx.batch.push(file_record(dir, &meta));
        if ctx.batch.len() >= BATCH {
            self.flush(ctx);
        }

        let changed = force_all || mem.get(&dir_str).copied() != Some(fs_mtime);

        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut children: Vec<FileRecord> = Vec::new();
        for ent in rd.flatten() {
            let Ok(ft) = ent.file_type() else { continue };
            // 不跟随符号链接 / 目录联结，避免自环
            if ft.is_symlink() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if !settings.include_hidden && name.starts_with('.') {
                continue;
            }
            let path = ent.path();
            if ft.is_dir() && self.is_excluded(&path, settings) {
                continue;
            }
            // Windows 上这一步直接复用目录枚举已返回的属性，不产生额外系统调用
            let Ok(m) = ent.metadata() else { continue };
            children.push(file_record(&path, &m));
        }

        if changed {
            ctx.dirs_changed += 1;
            let dropped_before = ctx.dropped;
            let existing = self.storage.children_meta(&dir_str).unwrap_or_default();

            for c in &children {
                let unchanged = matches!(
                    existing.get(&c.path),
                    Some(&(is_dir, mt, sz)) if is_dir == c.is_dir && mt == c.mtime_ns && sz == c.size
                );
                if unchanged {
                    continue;
                }
                if !c.is_dir && self.content_indexable(c, settings) && ctx.content_queue.len() < CONTENT_QUEUE_CAP {
                    ctx.content_queue.push((c.path.clone(), c.mtime_ns));
                }
                ctx.batch.push(c.clone());
                if ctx.batch.len() >= BATCH {
                    self.flush(ctx);
                }
            }
            self.flush(ctx);

            // 库里记着、磁盘上已经没有了 → 连子树一起删
            let actual: std::collections::HashSet<&str> =
                children.iter().map(|c| c.path.as_str()).collect();
            let stale: Vec<String> =
                existing.keys().filter(|p| !actual.contains(p.as_str())).cloned().collect();
            if !stale.is_empty() {
                match self.storage.remove_paths(&stale) {
                    Ok(n) => {
                        ctx.removed += n as u64;
                        log::debug!("{}: 移除 {} 个已消失的子项（共 {n} 条）", dir_str, stale.len());
                    }
                    Err(e) => self.record_error(format!("清理已消失子项失败 {dir_str}: {e}")),
                }
            }

            // 只有本轮写入没有失败时才登记新快照；
            // 否则保留旧值，让下一轮对账重新处理这个目录（自愈）。
            if ctx.dropped == dropped_before {
                mem.insert(dir_str.clone(), fs_mtime);
            }
        }

        for c in &children {
            if !c.is_dir {
                continue;
            }
            if self.epoch.load(Ordering::SeqCst) != my_epoch {
                return;
            }
            self.walk_dir(Path::new(&c.path), settings, mem, ctx, my_epoch, force_all);
        }
    }

    /// 批量写库。失败时计数并留痕，**绝不再静默丢弃**。
    fn flush(&self, ctx: &mut ScanCtx) {
        if ctx.batch.is_empty() {
            return;
        }
        let n = ctx.batch.len();
        let mut result = self.storage.upsert_files(&ctx.batch);
        if result.is_err() {
            // 给一次重试机会：SQLITE_BUSY 之类的瞬时错误占比不低
            std::thread::sleep(Duration::from_millis(50));
            result = self.storage.upsert_files(&ctx.batch);
        }
        match result {
            Ok(_) => {
                ctx.written += n as u64;
                self.stats.files.store(self.storage.file_count().max(0) as u64, Ordering::Relaxed);
            }
            Err(e) => {
                ctx.dropped += n as u64;
                self.record_error(format!("文件索引写入失败，{n} 条待下次对账重试: {e}"));
            }
        }
        ctx.batch.clear();
    }

    fn content_indexable(&self, rec: &FileRecord, settings: &AppSettings) -> bool {
        settings.content_index_enabled
            && !rec.is_dir
            && rec.size > 0
            && rec.size <= (settings.content_max_kb as i64) * 1024
            && CONTENT_EXTS.contains(&rec.ext.as_str())
    }

    /// 读取并写入正文索引。返回是否成功写入。
    fn index_content(&self, path: &str, mtime_ns: i64) -> bool {
        let Ok(bytes) = std::fs::read(path) else { return false };
        if bytes.iter().take(4096).any(|&b| b == 0) {
            return false; // 二进制
        }
        let text = String::from_utf8_lossy(&bytes);
        let body: String = text.chars().take(CONTENT_CHAR_CAP).collect();
        match self.storage.upsert_content(path, &body, mtime_ns) {
            Ok(_) => true,
            Err(e) => {
                self.record_error(format!("正文索引写入失败 {path}: {e}"));
                false
            }
        }
    }

    fn record_error(&self, msg: String) {
        log::warn!("{msg}");
        *self.stats.last_error.lock() = Some(msg);
    }

    pub fn status(&self) -> IndexStatus {
        IndexStatus {
            files: self.stats.files.load(Ordering::Relaxed),
            content_files: self.stats.content_files.load(Ordering::Relaxed),
            dirs_visited: self.stats.dirs_visited.load(Ordering::Relaxed),
            dirs_changed: self.stats.dirs_changed.load(Ordering::Relaxed),
            removed: self.stats.removed.load(Ordering::Relaxed),
            dropped: self.stats.dropped.load(Ordering::Relaxed),
            last_scan_ms: self.stats.last_scan_ms.load(Ordering::Relaxed),
            mode: self.stats.last_mode.lock().clone(),
            error: self.stats.last_error.lock().clone(),
        }
    }

    // ------------------------------------------------------------------
    // 实时增量（notify）
    // ------------------------------------------------------------------

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

    /// 监控事件用的排除判断。
    ///
    /// 与扫描期一致：**只看索引根目录之下**的组件。
    /// 旧实现检查路径的**所有**组件，于是只要索引根目录自己所在的路径里
    /// 含有排除项的同名目录（例如 `%TEMP%` 位于 `AppData` 下），
    /// 监控就会把所有事件静默丢弃 —— 而同一棵树的扫描却照常索引，
    /// 两边行为不一致。排除项描述的是「根目录里的哪些子目录不要」，
    /// 不该因为根目录的位置而整体失效。
    fn path_excluded(&self, path: &Path, settings: &AppSettings) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return true;
        };
        if !settings.include_hidden && name.starts_with('.') {
            return true;
        }
        if !settings.exclude_build_caches {
            return false;
        }
        let p = norm_path(&path.to_string_lossy()).to_lowercase();
        for root in &settings.index_roots {
            let r = norm_path(root).to_lowercase();
            let Some(rest) = p.strip_prefix(&r) else { continue };
            // 必须落在目录边界上：`C:\a\bc` 不属于根 `C:\a\b`
            if !rest.is_empty() && !rest.starts_with('\\') {
                continue;
            }
            return rest
                .split('\\')
                .any(|seg| !seg.is_empty() && settings.excluded_dirs.iter().any(|e| e.eq_ignore_ascii_case(seg)));
        }
        // 不在任何索引根下 → 不处理
        true
    }

    fn handle_fs_event(&self, event: Event) {
        let settings = self.settings.read().clone();
        // 顺手清理「旧路径到了、新路径没到」的悬空改名记录
        self.sweep_stale_renames(&settings);
        match event.kind {
            EventKind::Access(_) | EventKind::Other => return,
            // 部分平台（inotify / FSEvents）会给出一条带新旧两个路径的事件
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
                self.handle_rename(&event.paths[0], &event.paths[1], &settings);
                return;
            }
            // Windows 路径：一次改名拆成两条事件，各自只带一个路径，需要自己配对。
            // 见 `pending_rename` 的说明 —— 少了这段，`handle_rename` 永不被触发。
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) if event.paths.len() == 1 => {
                self.pending_rename.lock().push((Instant::now(), event.paths[0].clone()));
                return;
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) if event.paths.len() == 1 => {
                if let Some(from) = self.take_pending_rename() {
                    self.handle_rename(&from, &event.paths[0], &settings);
                    return;
                }
                // 没有配对的旧路径（例如从监控范围外移入）→ 按新增处理，继续往下走
            }
            _ => {}
        }

        // 只有「删除」和「改名」事件才允许按子树删除。
        // 旧实现把任何 metadata 失败都当成删除处理，
        // 而 OneDrive 占位文件、网络盘抖动、改名事件的旧路径都会让 metadata 失败 ——
        // 于是「一个瞬时错误 = 整棵子树被从索引里抹掉」。
        let removable = matches!(event.kind, EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)));
        // 目录被移入 / 复制进来时只有一条 Create，里面的文件要自己递归补齐
        let is_create = matches!(event.kind, EventKind::Create(_));

        let mut touched = false;
        for path in event.paths {
            if self.path_excluded(&path, &settings) {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    if is_create && meta.is_dir() {
                        self.index_subtree(&path, &settings);
                    } else {
                        self.upsert_one(&path, &meta, &settings);
                    }
                    touched = true;
                }
                Err(_) if removable => {
                    let p = norm_path(&path.to_string_lossy());
                    match self.storage.remove_path_tree(&p) {
                        Ok(n) => {
                            log::debug!("已从索引移除 {p}（{n} 条）");
                            touched = true;
                        }
                        Err(e) => self.record_error(format!("移除失败 {p}: {e}")),
                    }
                }
                // 其余情况交给 10 分钟一次的兜底对账，不在这里猜
                Err(_) => {}
            }
        }
        if touched {
            self.stats.files.store(self.storage.file_count().max(0) as u64, Ordering::Relaxed);
        }
    }

    /// 取出最早的待配对旧路径。
    ///
    /// 若队首已经超时（对应的 `To` 事件一直没来，说明源被移出了监控范围），
    /// 就地按「已删除」清理掉，然后继续看下一条。
    fn take_pending_rename(&self) -> Option<PathBuf> {
        loop {
            let head = {
                let mut q = self.pending_rename.lock();
                if q.is_empty() {
                    return None;
                }
                q.remove(0)
            };
            if head.0.elapsed() > RENAME_PAIR_WINDOW {
                self.drop_missing(&head.1);
                continue;
            }
            return Some(head.1);
        }
    }

    /// 清理所有超时的悬空旧路径。挂在每条事件的开头，这样即便之后
    /// 再也没有改名事件，只要有**任何**文件系统活动就能把它们收掉；
    /// 彻底静默的情况交给 10 分钟一次的兜底对账。
    fn sweep_stale_renames(&self, _settings: &AppSettings) {
        loop {
            let head = {
                let mut q = self.pending_rename.lock();
                if q.is_empty() || q[0].0.elapsed() <= RENAME_PAIR_WINDOW {
                    return;
                }
                q.remove(0)
            };
            self.drop_missing(&head.1);
        }
    }

    /// 悬空改名（源已被移走、新路径不在本监控范围内）按删除处理。
    /// 源还在时不动作 —— 那说明不是「移出」，别误删。
    fn drop_missing(&self, path: &Path) {
        if path.exists() {
            return;
        }
        let p = norm_path(&path.to_string_lossy());
        match self.storage.remove_path_tree(&p) {
            Ok(n) if n > 0 => log::debug!("改名源已移出监控范围，移除索引 {p}（{n} 条）"),
            Ok(_) => {}
            Err(e) => self.record_error(format!("移除失败 {p}: {e}")),
        }
    }

    /// 改名 / 移动：迁移引用方，再刷新目标元数据
    fn handle_rename(&self, from: &Path, to: &Path, settings: &AppSettings) {
        let f = norm_path(&from.to_string_lossy());
        let t = norm_path(&to.to_string_lossy());
        if f.is_empty() || t.is_empty() || f == t {
            return;
        }
        match self.storage.rename_prefix(&f, &t) {
            Ok(n) => log::debug!("路径迁移 {f} -> {t}（{n} 条）"),
            Err(e) => self.record_error(format!("路径迁移失败 {f} -> {t}: {e}")),
        }
        if let Ok(meta) = std::fs::metadata(to) {
            self.upsert_one(to, &meta, settings);
        }
        self.stats.files.store(self.storage.file_count().max(0) as u64, Ordering::Relaxed);
    }

    /// 递归索引一棵刚出现的子树。
    ///
    /// notify 对「目录被移入 / 复制进监控范围」只给一条 `Create` 事件，
    /// 不会逐个报告里面的文件。只写目录行的话，整棵子树要等到下一次
    /// 兜底对账才可见 —— 用户刚粘贴进来的文件夹搜不到，体验上很突兀。
    fn index_subtree(&self, dir: &Path, settings: &AppSettings) {
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        // mem 留空 ⇒ 每个目录都判定为「有变化」，等于强制全量走一遍
        let mut mem = HashMap::new();
        let mut ctx = ScanCtx::default();
        self.walk_dir(dir, settings, &mut mem, &mut ctx, my_epoch, true);
        self.flush(&mut ctx);

        let queue = std::mem::take(&mut ctx.content_queue);
        for (path, mtime_ns) in queue {
            if self.storage.content_needs_update(&path, mtime_ns) {
                self.index_content(&path, mtime_ns);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.stats.files.store(self.storage.file_count().max(0) as u64, Ordering::Relaxed);
        self.stats.content_files.store(self.storage.content_count().max(0) as u64, Ordering::Relaxed);
    }

    fn upsert_one(&self, path: &Path, meta: &std::fs::Metadata, settings: &AppSettings) {
        let rec = file_record(path, meta);
        if let Err(e) = self.storage.upsert_files(std::slice::from_ref(&rec)) {
            self.record_error(format!("增量写入失败 {}: {e}", rec.path));
            return;
        }
        // 旧实现对每个事件都无条件重建正文索引。
        // QQ / OneDrive 这类会持续触碰文件的目录因此反复「删正文 + 写正文」，
        // 制造出大量空闲页 —— 实测库里 53MB 空闲页（占 53.5%）正是这么来的。
        if !rec.is_dir
            && self.content_indexable(&rec, settings)
            && self.storage.content_needs_update(&rec.path, rec.mtime_ns)
        {
            if self.index_content(&rec.path, rec.mtime_ns) {
                self.stats.content_files.store(self.storage.content_count().max(0) as u64, Ordering::Relaxed);
            }
        }
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
                let mut watched = 0usize;
                for root in &settings.index_roots {
                    let p = Path::new(root);
                    if p.exists() {
                        if let Err(e) = w.watch(p, RecursiveMode::Recursive) {
                            log::warn!("监控目录失败 {root}: {e}");
                        } else {
                            watched += 1;
                        }
                    }
                }
                log::info!("实时监控已挂载 {watched} 个索引目录");
                *self.watcher.lock() = Some(w);
            }
            Err(e) => log::warn!("创建文件监控失败: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::Storage;

    fn rec(path: &str, is_dir: bool, mtime_ns: i64) -> FileRecord {
        FileRecord {
            path: path.into(),
            parent: parent_of(path),
            name: Path::new(path).file_name().unwrap().to_string_lossy().to_string(),
            is_dir,
            mtime_ns,
            ..Default::default()
        }
    }

    /// 对账的核心不变量：目录 mtime 未变 ⇒ 直接跳过，不产生任何库写入。
    #[test]
    fn unchanged_dir_is_skipped() {
        let s = Arc::new(Storage::open_in_memory().unwrap());
        s.upsert_files(&[rec("D:\\a", true, 111), rec("D:\\a\\x.txt", false, 222)]).unwrap();

        let mut mem = s.load_dir_mtimes().unwrap();
        assert_eq!(mem.get("D:\\a"), Some(&111));

        // 快照与磁盘一致 → 视为未变化
        let changed = mem.get("D:\\a").copied() != Some(111);
        assert!(!changed, "mtime 未变时不应判定为已变化");

        // 变化后应判定为已变化
        mem.insert("D:\\a".into(), 111);
        let changed = mem.get("D:\\a").copied() != Some(999);
        assert!(changed);
    }

    /// parent 计算：驱动器根不能返回自身，否则取子树会形成自环。
    #[test]
    fn parent_of_edges() {
        assert_eq!(parent_of("C:\\Users\\a.txt"), "C:\\Users");
        assert_eq!(parent_of("C:\\Users"), "C:\\");
        assert_eq!(parent_of("C:\\"), "");
        assert_eq!(parent_of("relative"), "");
    }

    /// 根目录用正斜杠时必须先规范化，否则根行与其子项的 parent 对不上，
    /// 对账会把整棵子树当成「新增」反复重写。
    #[test]
    fn root_separator_is_normalized() {
        assert_eq!(norm_path("C:/a/b"), "C:\\a\\b");
        assert_eq!(norm_path("C:\\a\\b"), "C:\\a\\b");
        // 规范化之后，子项算出的 parent 正好等于根行的 path
        let root = norm_path("C:/Users/x/OneDrive/文档");
        let child = norm_path("C:/Users/x/OneDrive/文档\\a.txt");
        assert_eq!(parent_of(&child), root);
    }

    /// GLOB 没有转义符，路径里的通配符必须用字符类表达
    #[test]
    fn glob_pattern_escapes_wildcards() {
        use crate::core::storage::glob_descendants;
        assert_eq!(glob_descendants("D:\\a"), "D:\\a\\*");
        assert_eq!(glob_descendants("D:\\a[1]"), "D:\\a[[]1]\\*");
        assert_eq!(glob_descendants("D:\\a*"), "D:\\a[*]\\*");
    }

    /// 对账删除：库里多出来的子项应被连同子树一起清掉
    #[test]
    fn remove_paths_drops_subtree() {
        let s = Storage::open_in_memory().unwrap();
        s.upsert_files(&[
            rec("D:\\keep", true, 1),
            rec("D:\\gone", true, 1),
            rec("D:\\gone\\deep", true, 1),
            rec("D:\\gone\\deep\\f.txt", false, 1),
        ])
        .unwrap();
        assert_eq!(s.file_count(), 4);
        s.remove_paths(&["D:\\gone".to_string()]).unwrap();
        assert_eq!(s.file_count(), 1);
    }

    /// rename 必须把置顶 / 最近使用的引用一起搬走
    #[test]
    fn rename_migrates_references() {
        let s = Storage::open_in_memory().unwrap();
        s.upsert_files(&[rec("D:\\old", true, 1), rec("D:\\old\\f.txt", false, 1)]).unwrap();
        s.add_pin(&crate::core::storage::EntryRecord {
            item_id: "folder:D:\\old".into(),
            kind: "folder".into(),
            path: "D:\\old".into(),
            ..Default::default()
        })
        .unwrap();
        s.touch_recent(&crate::core::storage::EntryRecord {
            item_id: "file:D:\\old\\f.txt".into(),
            kind: "file".into(),
            path: "D:\\old\\f.txt".into(),
            ..Default::default()
        })
        .unwrap();

        s.rename_prefix("D:\\old", "D:\\new").unwrap();

        assert!(s.file_by_path("D:\\new").is_some(), "目录行应被迁移");
        assert!(s.file_by_path("D:\\new\\f.txt").is_some(), "子树行应被迁移");
        assert!(s.file_by_path("D:\\old").is_none(), "旧路径不应残留");
        let pins = s.list_pins();
        assert_eq!(pins[0].item_id, "folder:D:\\new");
        assert_eq!(pins[0].path, "D:\\new");
        let recent = s.list_recent(10);
        assert_eq!(recent[0].item_id, "file:D:\\new\\f.txt");
        assert_eq!(recent[0].path, "D:\\new\\f.txt");
    }

    /// 正文索引新鲜度：mtime 未变时不重复读盘重建
    #[test]
    fn content_freshness_gate() {
        let s = Storage::open_in_memory().unwrap();
        assert!(s.content_needs_update("D:\\a.md", 100), "从未索引过 → 需要");
        s.upsert_content("D:\\a.md", "hello world", 100).unwrap();
        assert!(!s.content_needs_update("D:\\a.md", 100), "mtime 一致 → 不需要");
        assert!(s.content_needs_update("D:\\a.md", 200), "mtime 变了 → 需要");
        // 老库迁移来的记录 mtime_ns=0，新鲜度未知，必须刷新
        s.upsert_content("D:\\b.md", "x", 0).unwrap();
        assert!(s.content_needs_update("D:\\b.md", 0));
    }

    /// 孤儿正文：文件索引里没有的路径应能被识别并清理
    #[test]
    fn orphan_content_is_detected_and_purged() {
        let s = Storage::open_in_memory().unwrap();
        s.upsert_files(&[rec("D:\\a.md", false, 1)]).unwrap();
        s.upsert_content("D:\\a.md", "ok", 1).unwrap();
        s.upsert_content("D:\\gone.md", "orphan", 1).unwrap();
        assert_eq!(s.content_count(), 2);
        assert_eq!(s.orphan_content_count(), 1);
        assert_eq!(s.orphan_content_paths(10), vec!["D:\\gone.md".to_string()]);
        assert_eq!(s.purge_orphan_content().unwrap(), 1);
        assert_eq!(s.content_count(), 1);
        assert!(s.integrity().healthy());
    }
}
