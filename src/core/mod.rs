//! Application Core：聚合搜索、索引、存储、剪贴板、热键、系统动作，向 UI 提供统一 API。

pub mod apps;
pub mod cli;
pub mod hotkey;
pub mod indexer;
pub mod launcher;
pub mod search;
pub mod settings;
pub mod storage;
pub mod win_thread;

use crate::models::{BackendNotification, HotkeyBindingModel, ItemType, SearchItemModel, SearchRequest, SearchResponse};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use search::{SearchEngine, SearchProvider};
use settings::AppSettings;
use std::sync::Arc;
use storage::{HotkeyRecord, Storage};
use win_thread::{SysEvent, SystemBus, BINDING_ID_BASE};

pub type Notifier = Arc<dyn Fn(BackendNotification) + Send + Sync>;

pub struct AppCore {
    pub settings: RwLock<AppSettings>,
    pub storage: Arc<Storage>,
    pub engine: Arc<SearchEngine>,
    pub indexer: Arc<indexer::Indexer>,
    pub bus: Arc<SystemBus>,
    notifier: Notifier,
    hotkeys: RwLock<Vec<HotkeyBindingModel>>,
    apps_ready: std::sync::atomic::AtomicBool,
    /// 索引库整理进行中。用于禁用设置页按钮、防止重复触发 VACUUM。
    compacting: Arc<std::sync::atomic::AtomicBool>,
}

impl AppCore {
    /// 初始化核心：打开数据库、加载配置、启动系统线程与索引线程。
    pub fn bootstrap(notifier: Notifier) -> Result<Arc<AppCore>> {
        let settings = settings::load_settings();
        let storage = Arc::new(Storage::open(&settings::db_path())?);
        let engine = Arc::new(SearchEngine::new(Arc::clone(&storage)));
        let indexer = indexer::Indexer::new(Arc::clone(&storage), settings.clone(), Arc::clone(&notifier));

        let core_slot: Arc<RwLock<Option<std::sync::Weak<AppCore>>>> = Arc::new(RwLock::new(None));
        let slot = Arc::clone(&core_slot);
        let bus = SystemBus::start(Arc::new(move |ev: SysEvent| {
            if let Some(core) = slot.read().as_ref().and_then(|w| w.upgrade()) {
                core.on_sys_event(ev);
            }
        }));

        let core = Arc::new(AppCore {
            settings: RwLock::new(settings),
            storage,
            engine,
            indexer,
            bus,
            notifier,
            hotkeys: RwLock::new(Vec::new()),
            apps_ready: std::sync::atomic::AtomicBool::new(false),
            compacting: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        *core_slot.write() = Some(Arc::downgrade(&core));

        core.reload_hotkeys();
        core.apply_wake_hotkey();
        core.bus.set_clipboard_listening(core.settings.read().clipboard_enabled);
        core.indexer.start();
        core.spawn_app_scan();
        Ok(core)
    }

    fn notify(&self, n: BackendNotification) {
        (self.notifier)(n);
    }

    fn toast(&self, text: impl Into<String>, icon: &str) {
        self.notify(BackendNotification::ToastMessage { text: text.into(), icon: icon.into() });
    }

    // ------------------------------------------------------------------
    // 应用扫描
    // ------------------------------------------------------------------
    pub fn spawn_app_scan(self: &Arc<Self>) {
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name("anycast-appscan".into())
            .spawn(move || {
                let apps = apps::scan_apps();
                log::info!("应用扫描完成：{} 个", apps.len());
                let _ = this.storage.replace_apps(&apps);
                this.engine.set_apps(this.storage.load_apps());
                this.apps_ready.store(true, std::sync::atomic::Ordering::Relaxed);
                // 扫描完成后才有应用可预置（设计稿 65.4「置顶空状态」）
                this.seed_default_pins_if_needed();
                this.notify(BackendNotification::RecentItemsReady { items: this.recent_items() });
            })
            .expect("spawn appscan");
    }

    /// 首次启动把常用应用预置到置顶展架。
    ///
    /// 设计稿 65.4 指出：置顶为空时不应显示一行生硬的大灰字占位，
    /// 而应「预置 Terminal、VSCode、Anycast 等常用卡片」。
    ///
    /// 实现要点：
    /// - 按关键词从**已扫描到的真实应用**中挑选，因此不依赖用户具体装了什么；
    /// - 通过 `pins_seeded` 标记保证只执行一次，用户日后手动清空置顶不会被重新灌入；
    /// - 若本次扫描没匹配到任何应用（如扫描异常），不写标记，留待下次启动重试。
    fn seed_default_pins_if_needed(&self) {
        if self.settings.read().pins_seeded {
            return;
        }
        // 已有置顶（例如从旧版本升级而来）→ 只补标记，绝不覆盖用户数据
        if !self.storage.list_pins().is_empty() {
            self.mark_pins_seeded();
            return;
        }

        // 关键词按优先级排列，逐个匹配去重，最多取 4 个。
        // 每个关键词取「名称最短」的匹配项，以优先命中规范入口——
        // 例如「Command Prompt」优先于「Developer Command Prompt for VS 2022」。
        const KEYWORDS: &[&str] = &[
            "windows terminal",
            "terminal",
            "powershell",
            "command prompt",
            "git bash",
            "visual studio code",
            "vscode",
            "cursor",
            "anycast",
        ];
        let apps = self.storage.load_apps();
        let mut chosen: Vec<&storage::AppRecord> = Vec::new();
        for kw in KEYWORDS {
            if chosen.len() >= 4 {
                break;
            }
            let best = apps
                .iter()
                .filter(|a| a.name.to_lowercase().contains(kw))
                .min_by_key(|a| a.name.chars().count());
            if let Some(a) = best {
                if !chosen.iter().any(|c| c.launch_path == a.launch_path) {
                    chosen.push(a);
                }
            }
        }
        if chosen.is_empty() {
            log::info!("无可预置的常用应用，跳过置顶预置");
            return;
        }
        for a in &chosen {
            let entry = search::item_to_entry(&search::app_to_item(a));
            if let Err(e) = self.storage.add_pin(&entry) {
                log::warn!("预置置顶失败（{}）: {e}", a.name);
            }
        }
        log::info!("已预置 {} 个常用应用到置顶展架", chosen.len());
        self.mark_pins_seeded();
    }

    fn mark_pins_seeded(&self) {
        let mut s = self.settings.write();
        s.pins_seeded = true;
        if let Err(e) = settings::save_settings(&s) {
            log::warn!("保存置顶预置标记失败: {e}");
        }
    }

    // ------------------------------------------------------------------
    // 搜索
    // ------------------------------------------------------------------
    pub fn search(&self, req: &SearchRequest) -> SearchResponse {
        self.engine.search(req)
    }

    pub fn recent_items(&self) -> Vec<SearchItemModel> {
        self.engine.search(&SearchRequest { category: "all".into(), ..Default::default() }).items
    }

    pub fn pinned_items(&self) -> Vec<SearchItemModel> {
        self.storage
            .list_pins()
            .iter()
            .map(|e| {
                let mut i = search::entry_to_item(e, "置顶");
                i.is_pinned = true;
                i
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // 条目动作
    // ------------------------------------------------------------------
    /// 执行主操作：应用启动 / 文件打开 / 剪贴板复制或打开链接。返回 Toast 文案。
    pub fn activate(&self, item: &SearchItemModel) -> Result<String> {
        let msg = match item.item_type {
            ItemType::Clipboard => {
                if item.sub_type == "url" {
                    launcher::shell_open(item.full_path.trim())?;
                    format!("已在浏览器中打开: {}", item.title)
                } else {
                    launcher::set_clipboard_text(&item.full_path)?;
                    format!("已复制到剪贴板: {}", item.title)
                }
            }
            ItemType::App | ItemType::Command => {
                let launch_path = item.id.strip_prefix("app:").map(|s| s.to_string()).unwrap_or_default();
                let path = self
                    .engine_launch_path(&launch_path)
                    .unwrap_or_else(|| item.full_path.clone());
                launcher::shell_open(&path)?;
                format!("已启动: {}", item.title)
            }
            ItemType::Folder => {
                launcher::shell_open(&item.full_path)?;
                format!("已在资源管理器中打开: {}", item.title)
            }
            ItemType::File => {
                launcher::shell_open(&item.full_path)?;
                format!("已使用系统关联程序打开: {}", item.title)
            }
        };
        let _ = self.storage.touch_recent(&search::item_to_entry(item));
        Ok(msg)
    }

    fn engine_launch_path(&self, launch_lower: &str) -> Option<String> {
        self.storage
            .load_apps()
            .into_iter()
            .find(|a| a.launch_path.to_lowercase() == launch_lower)
            .map(|a| a.launch_path)
    }

    pub fn reveal(&self, item: &SearchItemModel) -> Result<String> {
        let path = match item.item_type {
            ItemType::Clipboard => return Err(anyhow!("剪贴板记录没有磁盘路径")),
            _ => item.full_path.clone(),
        };
        launcher::reveal_in_explorer(&path)?;
        Ok(format!("已在资源管理器中定位: {}", item.title))
    }

    pub fn copy_path(&self, item: &SearchItemModel) -> Result<String> {
        launcher::set_clipboard_text(&item.full_path)?;
        Ok(format!("已复制: {}", item.full_path.chars().take(60).collect::<String>()))
    }

    pub fn toggle_pin(&self, item: &SearchItemModel) -> Result<(bool, String)> {
        if self.storage.is_pinned(&item.id) {
            self.storage.remove_pin(&item.id)?;
            Ok((false, format!("已从置顶移除: {}", item.title)))
        } else {
            self.storage.add_pin(&search::item_to_entry(item))?;
            Ok((true, format!("已固定至置顶: {}", item.title)))
        }
    }

    pub fn remove_recent(&self, item: &SearchItemModel) -> Result<String> {
        self.storage.remove_recent(&item.id)?;
        if item.item_type == ItemType::Clipboard {
            if let Some(id) = item.id.strip_prefix("clip:").and_then(|s| s.parse::<i64>().ok()) {
                self.storage.delete_clip(id)?;
            }
        }
        Ok(format!("已移除: {}", item.title))
    }

    // ------------------------------------------------------------------
    // 系统事件（热键 / 剪贴板）
    // ------------------------------------------------------------------
    fn on_sys_event(&self, ev: SysEvent) {
        match ev {
            SysEvent::WakeHotkey => self.notify(BackendNotification::WakeRequested),
            SysEvent::BindingHotkey(id) => {
                let idx = (id - BINDING_ID_BASE) as usize;
                let binding = self.hotkeys.read().get(idx).cloned();
                if let Some(b) = binding {
                    if !b.enabled || !self.settings.read().hotkeys_master_enabled {
                        return;
                    }
                    match launcher::activate_or_launch(&b.target_path, &b.item_type) {
                        Ok(_) => self.notify(BackendNotification::HotkeyTriggered { binding_id: b.id.clone(), app_name: b.name.clone() }),
                        Err(e) => self.toast(format!("快捷直达失败: {e}"), "x"),
                    }
                }
            }
            SysEvent::ClipboardText(text) => self.on_clipboard(text),
        }
    }

    fn on_clipboard(&self, text: String) {
        let settings = self.settings.read().clone();
        if !settings.clipboard_enabled {
            return;
        }
        if text.len() > 200_000 {
            return;
        }
        if settings.clipboard_ignore_password_managers {
            let fg = launcher::foreground_exe_name().to_lowercase();
            if ["1password", "bitwarden", "keepass", "lastpass", "dashlane", "enpass"].iter().any(|p| fg.contains(p)) {
                return;
            }
        }
        let kind = search::clip_kind(&text);
        if let Ok(Some(rec)) = self.storage.add_clip(&text, kind, settings.clipboard_dedupe, settings.clipboard_limit) {
            let item = search::clip_to_item(&rec);
            let _ = self.storage.touch_recent(&search::item_to_entry(&item));
            self.notify(BackendNotification::ClipboardItemAdded { item });
        }
    }

    // ------------------------------------------------------------------
    // 热键管理
    // ------------------------------------------------------------------
    pub fn hotkeys(&self) -> Vec<HotkeyBindingModel> {
        self.hotkeys.read().clone()
    }

    fn reload_hotkeys(&self) {
        let list: Vec<HotkeyBindingModel> = self
            .storage
            .list_hotkeys()
            .into_iter()
            .map(|h| {
                let spec = hotkey::parse_hotkey(&h.hotkey);
                HotkeyBindingModel {
                    id: h.id,
                    name: h.name,
                    target_path: h.target_path,
                    hotkey: hotkey::normalize(&h.hotkey),
                    modifiers: spec.map(|s| s.modifiers).unwrap_or(0),
                    vk_code: spec.map(|s| s.vk).unwrap_or(0),
                    item_type: h.item_type,
                    enabled: h.enabled,
                }
            })
            .collect();
        *self.hotkeys.write() = list;
        self.sync_bindings();
    }

    fn sync_bindings(&self) {
        let master = self.settings.read().hotkeys_master_enabled;
        let specs: Vec<(i32, hotkey::HotkeySpec)> = if master {
            self.hotkeys
                .read()
                .iter()
                .enumerate()
                .filter(|(_, b)| b.enabled)
                .filter_map(|(i, b)| hotkey::parse_hotkey(&b.hotkey).map(|s| (BINDING_ID_BASE + i as i32, s)))
                .collect()
        } else {
            Vec::new()
        };
        self.bus.set_bindings(specs);
    }

    fn apply_wake_hotkey(&self) {
        let spec = hotkey::parse_hotkey(&self.settings.read().wake_hotkey);
        self.bus.set_wake_hotkey(spec);
    }

    /// 检测冲突：内部绑定重复 / 系统保留 / 已被其他程序注册
    pub fn detect_conflict(&self, hotkey_text: &str, exclude_id: Option<&str>) -> Option<String> {
        let norm = hotkey::normalize(hotkey_text);
        if norm.eq_ignore_ascii_case(&hotkey::normalize(&self.settings.read().wake_hotkey)) && exclude_id != Some("__wake__") {
            return Some("与唤醒快捷键相同".into());
        }
        if let Some(b) = self.hotkeys.read().iter().find(|b| b.hotkey.eq_ignore_ascii_case(&norm) && Some(b.id.as_str()) != exclude_id) {
            return Some(format!("已被「{}」绑定", b.name));
        }
        if let Some(why) = hotkey::system_conflict_hint(&norm) {
            return Some(format!("与系统快捷键冲突（{why}）"));
        }
        None
    }

    pub fn probe_hotkey(&self, hotkey_text: &str) -> Result<(), String> {
        let spec = hotkey::parse_hotkey(hotkey_text).ok_or_else(|| "无法解析组合键".to_string())?;
        // 与自身已注册的组合相同则视为可用（后续 sync 会重新注册）
        let norm = hotkey::normalize(hotkey_text);
        if self.hotkeys.read().iter().any(|b| b.enabled && b.hotkey.eq_ignore_ascii_case(&norm))
            || hotkey::normalize(&self.settings.read().wake_hotkey).eq_ignore_ascii_case(&norm)
        {
            return Ok(());
        }
        self.bus.probe(spec)
    }

    pub fn save_hotkey(&self, binding: HotkeyBindingModel) -> Result<()> {
        if hotkey::parse_hotkey(&binding.hotkey).is_none() {
            return Err(anyhow!("无效的快捷键: {}", binding.hotkey));
        }
        if let Some(why) = self.detect_conflict(&binding.hotkey, Some(&binding.id)) {
            self.notify(BackendNotification::HotkeyConflictDetected { hotkey: binding.hotkey.clone(), reason: why.clone() });
            return Err(anyhow!("{why}"));
        }
        if let Err(why) = self.probe_hotkey(&binding.hotkey) {
            self.notify(BackendNotification::HotkeyConflictDetected { hotkey: binding.hotkey.clone(), reason: why.clone() });
            return Err(anyhow!("{why}"));
        }
        self.storage.upsert_hotkey(&HotkeyRecord {
            id: binding.id.clone(),
            name: binding.name.clone(),
            target_path: binding.target_path.clone(),
            hotkey: hotkey::normalize(&binding.hotkey),
            item_type: binding.item_type.clone(),
            enabled: binding.enabled,
        })?;
        self.reload_hotkeys();
        Ok(())
    }

    pub fn set_hotkey_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let existing = self.hotkeys.read().iter().find(|b| b.id == id).cloned();
        if let Some(mut b) = existing {
            b.enabled = enabled;
            self.storage.upsert_hotkey(&HotkeyRecord {
                id: b.id.clone(),
                name: b.name.clone(),
                target_path: b.target_path.clone(),
                hotkey: b.hotkey.clone(),
                item_type: b.item_type.clone(),
                enabled,
            })?;
            self.reload_hotkeys();
        }
        Ok(())
    }

    pub fn delete_hotkey(&self, id: &str) -> Result<()> {
        self.storage.delete_hotkey(id)?;
        self.reload_hotkeys();
        Ok(())
    }

    pub fn test_hotkey(&self, id: &str) -> Result<String> {
        let b = self.hotkeys.read().iter().find(|b| b.id == id).cloned().ok_or_else(|| anyhow!("绑定不存在"))?;
        let how = launcher::activate_or_launch(&b.target_path, &b.item_type)?;
        Ok(if how == "activated" { format!("已激活前置: {}", b.name) } else { format!("已启动: {}", b.name) })
    }

    pub fn set_hotkeys_master(&self, enabled: bool) {
        self.settings.write().hotkeys_master_enabled = enabled;
        self.save_settings();
        self.sync_bindings();
    }

    pub fn set_wake_hotkey(&self, text: &str) -> Result<()> {
        if hotkey::parse_hotkey(text).is_none() {
            return Err(anyhow!("无效的快捷键"));
        }
        if let Some(why) = self.detect_conflict(text, Some("__wake__")) {
            return Err(anyhow!("{why}"));
        }
        self.settings.write().wake_hotkey = hotkey::normalize(text);
        self.save_settings();
        self.apply_wake_hotkey();
        Ok(())
    }

    // ------------------------------------------------------------------
    // 配置
    // ------------------------------------------------------------------
    pub fn save_settings(&self) {
        let s = self.settings.read().clone();
        if let Err(e) = settings::save_settings(&s) {
            log::warn!("保存配置失败: {e}");
        }
        self.indexer.update_settings(s);
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut AppSettings)) {
        let (clip_before, startup_before) = {
            let s = self.settings.read();
            (s.clipboard_enabled, s.launch_on_startup)
        };
        f(&mut self.settings.write());
        self.save_settings();
        let s = self.settings.read().clone();
        if s.clipboard_enabled != clip_before {
            self.bus.set_clipboard_listening(s.clipboard_enabled);
        }
        if s.launch_on_startup != startup_before {
            if let Err(e) = launcher::set_launch_on_startup(s.launch_on_startup) {
                log::warn!("设置开机自启失败: {e}");
            }
        }
    }

    pub fn rebuild_index(&self) {
        self.indexer.rebuild();
    }

    /// 只做增量对账（不清空），用于设置页「立即检查更新」
    pub fn reconcile_index(&self) {
        self.indexer.reconcile_now();
    }

    /// 全量校验：强制检查每个目录的子项，但不清空。
    ///
    /// 用于补回「应用未运行期间文件被改写、而目录 mtime 不变」的情形 ——
    /// 增量对账看不到这类变更（见 `indexer.rs` 的 `FULL_RESCAN_INTERVAL` 注释）。
    pub fn full_rescan_index(&self) {
        self.indexer.full_rescan();
    }

    /// 索引一致性快照 + 上一轮扫描状态。
    ///
    /// 设置页用它把「正文索引 543 条却只有 1 个文件」这类不一致直接摆到用户面前。
    pub fn index_health(&self) -> (storage::IntegrityReport, indexer::IndexStatus) {
        (self.storage.integrity(), self.indexer.status())
    }

    /// 强制整理索引库：WAL checkpoint + VACUUM。
    /// 实测本机 99.6MB 的库整理后为 46.3MB。
    pub fn compact_index(&self) -> Result<String> {
        let r = self.storage.maintenance(true)?;
        let mb = |b: i64| format!("{:.1} MB", b as f64 / 1_048_576.0);
        Ok(if r.vacuumed {
            format!(
                "索引库已整理：{} → {}（空闲页 {} → {}）",
                mb(r.before_bytes),
                mb(r.after_bytes),
                mb(r.before_free),
                mb(r.after_free)
            )
        } else {
            format!("索引库无需整理（当前 {}，空闲页 {}）", mb(r.after_bytes), mb(r.after_free))
        })
    }

    /// 清理正文索引中已找不到对应文件的条目，返回清理条数
    pub fn purge_orphan_content(&self) -> Result<usize> {
        self.storage.purge_orphan_content()
    }

    /// 后台整理索引库（WAL checkpoint + VACUUM），完成后用 Toast 回报结果。
    ///
    /// VACUUM 需要重建整库、期间独占写锁，放在 UI 线程上会把搜索热路径卡住
    /// （实测 46 MB 的库约 330 ms，更大的库按比例更久）。
    /// 因此这里丢到独立线程跑 —— 通知走 `upgrade_in_event_loop`，回到 UI 线程是安全的。
    ///
    /// 重复调用会被 `compacting` 挡掉：两个 VACUUM 并发只会互相抢锁。
    pub fn compact_index_async(&self) {
        use std::sync::atomic::Ordering;
        if self.compacting.swap(true, Ordering::SeqCst) {
            return;
        }
        let storage = Arc::clone(&self.storage);
        let notifier = Arc::clone(&self.notifier);
        let flag = Arc::clone(&self.compacting);
        let spawned = std::thread::Builder::new()
            .name("anycast-compact".into())
            .spawn(move || {
                let mb = |b: i64| format!("{:.1} MB", b as f64 / 1_048_576.0);
                let (text, icon) = match storage.maintenance(true) {
                    Ok(r) if r.vacuumed => (
                        format!(
                            "索引库已整理：{} → {}，空闲页 {} → {}",
                            mb(r.before_bytes),
                            mb(r.after_bytes),
                            mb(r.before_free),
                            mb(r.after_free)
                        ),
                        "database",
                    ),
                    Ok(r) => (
                        format!(
                            "索引库无需整理（当前 {}，空闲页 {}）",
                            mb(r.after_bytes),
                            mb(r.after_free)
                        ),
                        "check",
                    ),
                    Err(e) => (format!("索引库整理失败：{e}"), "alert"),
                };
                flag.store(false, Ordering::SeqCst);
                notifier(BackendNotification::ToastMessage { text, icon: icon.to_string() });
            });
        if let Err(e) = spawned {
            self.compacting.store(false, Ordering::SeqCst);
            log::warn!("索引库整理线程启动失败：{e}");
        }
    }

    /// 索引库是否正在整理中（供设置页禁用按钮）
    pub fn is_compacting(&self) -> bool {
        self.compacting.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn clear_clipboard_history(&self) -> Result<usize> {
        self.storage.clear_unpinned_clips()
    }

    pub fn index_summary(&self) -> (i64, i64, i64, usize, i64) {
        (
            self.storage.file_count(),
            self.storage.content_count(),
            self.storage.db_size_bytes(),
            self.engine.app_count(),
            self.storage.clip_count(),
        )
    }

    pub fn shutdown(&self) {
        self.bus.quit();
    }
}

impl Drop for AppCore {
    fn drop(&mut self) {
        self.bus.quit();
    }
}
