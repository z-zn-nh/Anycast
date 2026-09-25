//! Slint UI 适配层：状态容器、回调绑定、模型转换、窗口管理（显示/隐藏/居中/失焦）。

pub mod dialogs;
pub mod theme;

use crate::core::{hotkey, launcher, AppCore};
use crate::models::{
    BackendNotification, HotkeyBindingModel, ItemType, SearchItemModel, SearchMode, SearchRequest, SearchScopeFilter,
};
use crate::{AppWindow, CalDay, GridSection, HotkeyBinding, IntentChip, LocationEntry, ResultItem, ResultRow, Settings, Theme, TrayIcon};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 42.0;
const CELL_H: f32 = 92.0;

/// 发起闸门 2 / 3 之前等多久，用来判断用户是不是还在打字。
///
/// 比正常打字间隔（约 150~250 ms）长一点，比人的「说完一句」停顿短得多。
/// 云端本身 p50 ≈ 900 ms，这点等待用户感知不到。
const UPGRADE_SETTLE_MS: u64 = 400;

struct UiState {
    results: Vec<SearchItemModel>,
    pinned: Vec<SearchItemModel>,
    selected: usize,
    pinned_selected: Option<usize>,
    scope: SearchScopeFilter,
    location_name: String,
    location_value: String,
    cal_year: i32,
    cal_month: u32,
    cal_start: Option<NaiveDate>,
    cal_end: Option<NaiveDate>,
    cal_hover: Option<NaiveDate>,
    ctx_target: Option<SearchItemModel>,
    recording: Option<String>,
    shown_at: Option<Instant>,
    suppress_blur: bool,
    last_size: (u32, u32),
    size_changed_at: Option<Instant>,
    hwnd: Option<HWND>,
    effects_applied: bool,
    taskbar_fixed: bool,
    last_elapsed_ms: u32,
}

pub struct Gui {
    ui: AppWindow,
    tray: TrayIcon,
    core: Arc<AppCore>,
    state: RefCell<UiState>,
    /// 查询代数。每次 `refresh_search` 自增，用来丢弃过期的异步结果。
    ///
    /// 是 `Arc<AtomicU64>` 而不是普通字段：升级线程需要**在发请求前**
    /// 读一眼当前代数，好判断「用户是不是还在打字」——
    /// 否则每敲一个字都会发一次云端调用（要花钱的）。
    query_gen: Arc<AtomicU64>,
    toast_timer: slint::Timer,
    poll_timer: slint::Timer,
}

thread_local! {
    static GUI: RefCell<Option<Rc<Gui>>> = const { RefCell::new(None) };
}

fn with_gui(f: impl FnOnce(&Rc<Gui>)) {
    GUI.with(|g| {
        if let Some(g) = g.borrow().as_ref() {
            f(g);
        }
    });
}

fn ss(s: &str) -> SharedString {
    SharedString::from(s)
}

/// 增量补充的纯计算部分 —— 从「已展示的 id」与「升级后重新检索的完整列表」里，
/// 算出该插入哪些新条目、以及选中项的新下标。
///
/// 抽成纯函数是为了能单测：这里的 off-by-one（位移量到底是「新条目数」
/// 还是「新条目数 + 1」）是这条路径上最容易错的地方，而它错了以后
/// 表现得像「键盘上下键不太灵」，极难在真机上定位。
///
/// ⚠️ 位移量是**新条目数**，不是「新条目数 + 1」：
/// `selected` 是 `st.results` 的下标，而分组标题只存在于行模型里、不进 `results`。
/// 多算一行就会选到原本选中项的下一条。
///
/// 返回 `None` 表示没有新条目（此时调用方应当**什么都不做**）。
fn plan_merge(
    shown_ids: &HashSet<String>,
    selected: usize,
    extra: Vec<SearchItemModel>,
) -> Option<(Vec<SearchItemModel>, usize)> {
    let fresh: Vec<SearchItemModel> = extra.into_iter().filter(|i| !shown_ids.contains(&i.id)).collect();
    if fresh.is_empty() {
        return None;
    }
    // 用户已经用方向键选过了就保住他那一条；
    // 还停在第一条（没动过）就留在新的第一条上 —— 让他直接看到模型找到的东西。
    let selected = if selected > 0 { selected + fresh.len() } else { 0 };
    Some((fresh, selected))
}

fn to_ui_item(item: &SearchItemModel) -> ResultItem {
    ResultItem {
        id: ss(&item.id),
        kind: ss(item.item_type.as_str()),
        title: ss(&item.title),
        subtitle: ss(&item.subtitle),
        badge: ss(&item.badge),
        icon: ss(&item.icon_name),
        section: ss(&item.section),
        reason: ss(&item.reason),
        action: ss(&item.action_hint),
        is_pinned: item.is_pinned,
        preview: ss(&item.preview),
        sub_type: ss(&item.sub_type),
    }
}

fn hotkey_icon(b: &HotkeyBindingModel) -> (&'static str, &'static str) {
    match b.item_type.as_str() {
        "app" => (crate::core::search::app_icon(&b.name, &b.target_path), "应用"),
        "folder" => ("folder", "文件夹"),
        "script" => ("terminal", "脚本"),
        _ => {
            let ext = std::path::Path::new(&b.target_path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            (crate::core::search::file_icon(&ext, false), crate::core::search::file_badge(&ext, false))
        }
    }
}

pub fn run(silent: bool) -> anyhow::Result<()> {
    let ui = AppWindow::new()?;
    let tray = TrayIcon::new()?;
    let weak: Weak<AppWindow> = ui.as_weak();
    let core = AppCore::bootstrap(Arc::new(move |n: BackendNotification| {
        let _ = weak.upgrade_in_event_loop(move |_| with_gui(|g| g.handle_notification(n)));
    }))?;

    let settings = core.settings.read().clone();
    let (y, m) = {
        let now = Local::now();
        (now.year(), now.month())
    };
    let gui = Rc::new(Gui {
        ui,
        tray,
        core,
        state: RefCell::new(UiState {
            results: Vec::new(),
            pinned: Vec::new(),
            selected: 0,
            pinned_selected: None,
            scope: SearchScopeFilter { time_preset: "all".into(), type_category: "all".into(), location_scope: "all".into(), ..Default::default() },
            location_name: String::new(),
            location_value: "all".into(),
            cal_year: y,
            cal_month: m,
            cal_start: None,
            cal_end: None,
            cal_hover: None,
            ctx_target: None,
            recording: None,
            shown_at: None,
            suppress_blur: false,
            last_size: (settings.window_width, settings.window_height),
            size_changed_at: None,
            hwnd: None,
            effects_applied: false,
            taskbar_fixed: false,
            last_elapsed_ms: 0,
        }),
        query_gen: Arc::new(AtomicU64::new(0)),
        toast_timer: slint::Timer::default(),
        poll_timer: slint::Timer::default(),
    });
    GUI.with(|g| *g.borrow_mut() = Some(Rc::clone(&gui)));

    gui.apply_theme_from_settings();
    gui.ui.set_mode(ss(&settings.default_mode));
    gui.ui.set_view_mode(ss(&settings.view_mode));
    gui.ui.set_pinned_collapsed(settings.pinned_collapsed);
    gui.ui.set_filter_open(settings.filter_shelf_open);
    gui.bind_callbacks();
    gui.sync_settings_to_ui();
    gui.refresh_pinned();
    gui.refresh_locations();
    gui.refresh_calendar();
    gui.refresh_filter_labels();
    gui.refresh_search();
    gui.start_poll_timer();

    if !silent {
        gui.show_window();
    }
    slint::run_event_loop_until_quit()?;
    gui.core.shutdown();
    Ok(())
}

impl Gui {
    // ------------------------------------------------------------------
    // 窗口
    // ------------------------------------------------------------------
    fn hwnd(&self) -> Option<HWND> {
        if let Some(h) = self.state.borrow().hwnd {
            return Some(h);
        }
        let handle = self.ui.window().window_handle();
        let raw = handle.window_handle().ok()?.as_raw();
        if let RawWindowHandle::Win32(w) = raw {
            let h = HWND(w.hwnd.get() as *mut _);
            self.state.borrow_mut().hwnd = Some(h);
            Some(h)
        } else {
            None
        }
    }

    pub fn show_window(self: &Rc<Self>) {
        let (w, h) = {
            let s = self.core.settings.read();
            (s.window_width.max(720) as f32, s.window_height.max(480) as f32)
        };
        let work = theme::cursor_monitor_work_area();
        let center = |scale: f32| {
            let pw = (w * scale) as i32;
            let ph = (h * scale) as i32;
            let x = work.left + ((work.right - work.left) - pw) / 2;
            let y = work.top + ((work.bottom - work.top) - ph) * 2 / 5;
            slint::PhysicalPosition::new(x, y)
        };
        let scale = theme::cursor_monitor_scale();
        self.ui.window().set_size(slint::LogicalSize::new(w, h));
        self.ui.window().set_position(center(scale));
        self.ui.window().set_minimized(false);
        let _ = self.ui.show();
        self.ui.window().set_size(slint::LogicalSize::new(w, h));
        self.ui.window().set_position(center(scale));
        {
            let mut st = self.state.borrow_mut();
            st.shown_at = Some(Instant::now());
            st.pinned_selected = None;
        }
        self.finish_native_show(0);
        self.ui.invoke_focus_search();
        self.refresh_search();
        self.refresh_pinned();
    }

    /// 应用 DWM 效果并前置窗口。首次显示时 winit 窗口在事件循环启动前尚未创建（hwnd 为 None），
    /// 因此拿不到句柄时用单次定时器短暂重试。
    fn finish_native_show(self: &Rc<Self>, attempt: u32) {
        match self.hwnd() {
            Some(hwnd) => {
                self.fix_taskbar(hwnd);
                if !self.state.borrow().effects_applied && std::env::var("ANYCAST_NO_EFFECTS").is_err() {
                    self.apply_effects();
                }
                if std::env::var("ANYCAST_NO_FG").is_err() {
                    launcher::force_foreground(hwnd);
                }
                self.ui.invoke_focus_search();
            }
            None if attempt < 40 => {
                let weak = Rc::downgrade(self);
                slint::Timer::single_shot(Duration::from_millis(25), move || {
                    if let Some(g) = weak.upgrade() {
                        g.finish_native_show(attempt + 1);
                    }
                });
            }
            None => log::warn!("无法获取窗口句柄，DWM 效果与前置聚焦未应用"),
        }
    }

    pub fn hide_window(&self) {
        self.ui.set_ctx_visible(false);
        self.ui.set_open_menu(ss(""));
        self.ui.set_settings_visible(false);
        self.stop_recording();
        self.persist_size(true);
        let _ = self.ui.hide();
    }

    fn toggle_window(self: &Rc<Self>) {
        let visible = self.ui.window().is_visible();
        let is_fg = self.hwnd().map(|h| unsafe { GetForegroundWindow() } == h).unwrap_or(false);
        if visible && is_fg {
            self.hide_window();
        } else {
            self.show_window();
        }
    }

    /// 首次拿到 HWND 时把窗口标记为工具窗口，从任务栏与 Alt+Tab 移除（只做一次）。
    /// 诊断开关：`ANYCAST_KEEP_TASKBAR=1` 时保持原样（用于 A/B 对照）。
    fn fix_taskbar(&self, hwnd: HWND) {
        if self.state.borrow().taskbar_fixed {
            return;
        }
        if std::env::var("ANYCAST_KEEP_TASKBAR").is_err() {
            theme::hide_from_taskbar(hwnd);
        }
        self.state.borrow_mut().taskbar_fixed = true;
    }

    fn apply_effects(&self) {
        let (dark, blur) = {
            let s = self.core.settings.read();
            (s.theme != "light", s.gpu_blur)
        };
        if let Some(hwnd) = self.hwnd() {
            theme::apply_window_effects(hwnd, dark, blur);
            self.state.borrow_mut().effects_applied = true;
        }
    }

    fn apply_theme_from_settings(&self) {
        let s = self.core.settings.read().clone();
        let theme = self.ui.global::<Theme>();
        theme.set_dark(s.theme != "light");
        theme.set_opacity_preset(ss(&s.acrylic_opacity));
        theme.set_rim_light(s.rim_light);
        theme.set_opaque_bg(std::env::var("ANYCAST_OPAQUE_BG").ok().as_deref() == Some("1"));
    }

    fn start_poll_timer(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.poll_timer.start(slint::TimerMode::Repeated, Duration::from_millis(200), move || {
            let Some(g) = weak.upgrade() else { return };
            g.poll_tick();
        });
    }

    fn poll_tick(&self) {
        if !self.ui.window().is_visible() {
            return;
        }
        // 失焦自动隐藏
        let hide_on_blur = self.core.settings.read().hide_on_blur;
        let (suppress, grace) = {
            let st = self.state.borrow();
            (st.suppress_blur, st.shown_at.map(|t| t.elapsed() < Duration::from_millis(600)).unwrap_or(false))
        };
        if hide_on_blur && !suppress && !grace {
            if let Some(hwnd) = self.hwnd() {
                let fg = unsafe { GetForegroundWindow() };
                if fg != hwnd && !fg.0.is_null() {
                    self.hide_window();
                    return;
                }
            }
        }
        // 尺寸持久化（用户拖拽边缘缩放后）
        let size = self.ui.window().size();
        let scale = self.ui.window().scale_factor().max(0.5);
        let lw = (size.width as f32 / scale).round() as u32;
        let lh = (size.height as f32 / scale).round() as u32;
        let mut st = self.state.borrow_mut();
        if (lw, lh) != st.last_size && lw > 0 && lh > 0 {
            st.last_size = (lw, lh);
            st.size_changed_at = Some(Instant::now());
        } else if let Some(t) = st.size_changed_at {
            if t.elapsed() > Duration::from_millis(800) {
                st.size_changed_at = None;
                drop(st);
                self.persist_size(false);
            }
        }
    }

    fn persist_size(&self, _force: bool) {
        let (lw, lh) = self.state.borrow().last_size;
        let current = {
            let s = self.core.settings.read();
            (s.window_width, s.window_height)
        };
        if lw >= 720 && lh >= 480 && (lw, lh) != current {
            self.core.update_settings(|s| {
                s.window_width = lw;
                s.window_height = lh;
            });
        }
    }

    // ------------------------------------------------------------------
    // Toast
    // ------------------------------------------------------------------
    fn toast(self: &Rc<Self>, text: &str, icon: &str) {
        self.ui.set_toast_text(ss(text));
        self.ui.set_toast_icon(ss(icon));
        self.ui.set_toast_shown(true);
        let weak = Rc::downgrade(self);
        self.toast_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(2600), move || {
            if let Some(g) = weak.upgrade() {
                g.ui.set_toast_shown(false);
            }
        });
    }

    // ------------------------------------------------------------------
    // 搜索与结果
    // ------------------------------------------------------------------
    fn build_request(&self) -> SearchRequest {
        let st = self.state.borrow();
        SearchRequest {
            query: self.ui.get_query().to_string(),
            mode: SearchMode::parse(&self.ui.get_mode()),
            scope: st.scope.clone(),
            category: self.ui.get_category().to_string(),
            clip_sub: self.ui.get_clip_sub().to_string(),
            limit: 80,
        }
    }

    fn refresh_search(self: &Rc<Self>) {
        let req = self.build_request();
        let gen = self.query_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let core = Arc::clone(&self.core);
        let gen_counter = Arc::clone(&self.query_gen);
        let weak = self.ui.as_weak();
        let weak2 = self.ui.as_weak();
        std::thread::spawn(move || {
            // ── 首屏：闸门 1（同步、< 1ms）+ 检索，**绝不等待模型** ──
            let (resp, worth_upgrade) = core.search_gated(&req);
            let elapsed = resp.elapsed_ms;
            let chips = resp.intent_chips;
            let _ = weak.upgrade_in_event_loop(move |_| {
                with_gui(|g| {
                    if g.query_gen.load(Ordering::SeqCst) == gen {
                        g.apply_results(resp.items, elapsed, chips);
                    }
                });
            });

            // ── 闸门 2 / 3 ──
            // 纯关键词输入到此为止（§5.7 硬性规则 3：永不触发）。
            if !worth_upgrade {
                return;
            }
            // **防抖**：等用户把话说完再问模型。
            //
            // 不做这一步的话，智能模式下**每敲一个字都会发一次云端调用** ——
            // 实测确认过：打字途中会有多个请求在途。Jev 单次 p50 ≈ 900 ms
            // 且按次计费，敲 20 个字符就是 20 次调用。
            // 首屏可以逐字符刷新（本地、免费），升级不行。
            std::thread::sleep(Duration::from_millis(UPGRADE_SETTLE_MS));
            // 睡醒后已经有更新的查询 → 这次不问了，让最后那个线程去问
            if gen_counter.load(Ordering::SeqCst) != gen {
                return;
            }
            // 无可用后端 / 超时 / 失败一律返回 None —— 静默降级，
            // 首屏那份结果照常留着，用户不会看到任何错误提示。
            let Some(intent) = core.upgrade_intent(&req.query) else {
                return;
            };
            let resp2 = core.search_with_intent(&req, &intent);
            let elapsed2 = resp2.elapsed_ms;
            let _ = weak2.upgrade_in_event_loop(move |_| {
                with_gui(|g| {
                    // 查询已经变了就丢掉这次升级结果（同一把代数锁）
                    if g.query_gen.load(Ordering::SeqCst) == gen {
                        g.merge_upgraded(resp2.items, elapsed2, resp2.intent_chips);
                    }
                });
            });
        });
    }

    fn apply_results(self: &Rc<Self>, items: Vec<SearchItemModel>, elapsed: u32, chips: Vec<IntentChipModel>) {
        self.render_rows(items, elapsed, chips, 0, true);
    }

    /// **增量替换**：把升级后新出现的条目插到列表最前面，**不清空已有列表**。
    ///
    /// §5.7 硬性规则 1。清空再填会让用户看到一次闪烁；更糟的是升级失败或超时时，
    /// 他刚看到的结果会凭空消失 —— 那比不升级还差。
    ///
    /// 新条目单独归到「深度匹配」分组：用户得能看出这几条是模型解析之后才找到的，
    /// 而不是以为自己第一次就搜出了这些。
    fn merge_upgraded(self: &Rc<Self>, extra: Vec<SearchItemModel>, elapsed: u32, chips: Vec<IntentChipModel>) {
        let (items, selected) = {
            let mut st = self.state.borrow_mut();
            let shown: HashSet<String> = st.results.iter().map(|i| i.id.clone()).collect();
            let Some((mut fresh, selected)) = plan_merge(&shown, st.selected, extra) else {
                // 模型没带来任何新东西 —— 连芯片都不动，
                // 别让界面「抖一下」却什么都没变。
                return;
            };
            for it in &mut fresh {
                it.section = "深度匹配".into();
            }
            let mut items = fresh;
            items.append(&mut st.results);
            (items, selected)
        };
        self.render_rows(items, elapsed, chips, selected, false);
    }

    /// 重建行模型并刷新视图。
    ///
    /// `fresh = true`：全新一次搜索 —— 选中归零、置顶面板取消、列表滚回顶部。
    /// `fresh = false`：增量补充 —— 保住用户当前的位置，**不滚动**
    /// （否则他正在看的那一行会被顶走）。
    fn render_rows(
        self: &Rc<Self>,
        items: Vec<SearchItemModel>,
        elapsed: u32,
        chips: Vec<IntentChipModel>,
        selected: usize,
        fresh: bool,
    ) {
        let mut rows: Vec<ResultRow> = Vec::with_capacity(items.len() + 4);
        let mut sections: Vec<GridSection> = Vec::new();
        let mut last_section = String::new();
        for (i, it) in items.iter().enumerate() {
            if it.section != last_section {
                rows.push(ResultRow { is_header: true, header: ss(&it.section), index: -1, item: ResultItem::default() });
                sections.push(GridSection { title: ss(&it.section), start: i as i32, count: 0 });
                last_section = it.section.clone();
            }
            if let Some(sec) = sections.last_mut() {
                sec.count += 1;
            }
            rows.push(ResultRow { is_header: false, header: ss(""), index: i as i32, item: to_ui_item(it) });
        }
        let ui_items: Vec<ResultItem> = items.iter().map(to_ui_item).collect();
        let selected = selected.min(items.len().saturating_sub(1));
        {
            let mut st = self.state.borrow_mut();
            st.results = items;
            st.selected = selected;
            st.last_elapsed_ms = elapsed;
            if fresh {
                st.pinned_selected = None;
            }
        }
        self.ui.set_rows(ModelRc::new(VecModel::from(rows)));
        self.ui.set_items(ModelRc::new(VecModel::from(ui_items)));
        self.ui.set_sections(ModelRc::new(VecModel::from(sections)));
        self.ui.set_chips(ModelRc::new(VecModel::from(
            chips.into_iter().map(|c| IntentChip { key: ss(&c.key), val: ss(&c.val) }).collect::<Vec<_>>(),
        )));
        self.ui.set_selected(selected as i32);
        if fresh {
            self.ui.set_pinned_selected(-1);
        }
        self.update_selection_geometry();
        if fresh {
            self.ui.invoke_scroll_top();
        }
        self.update_status();
    }

    fn update_status(&self) {
        let st = self.state.borrow();
        let (files, _content, _db, _apps, _clips) = self.core.index_summary();
        let indexing = self.core.indexer.stats.scanning.load(Ordering::Relaxed);
        // 设计稿 65.4 / 阶段十八明确要求底栏右侧保持纯净：
        // 移除常驻状态胶囊与「N 应用 · M 文件已索引」读数，避免右下角视觉噪音。
        // 仅在建立索引期间显示进度，让用户知道后台正在干活；空闲时完全不占位。
        let text = if indexing {
            format!("正在索引 {} 个文件… · {} 项 · {}ms", files, st.results.len(), st.last_elapsed_ms)
        } else {
            String::new()
        };
        self.ui.set_status_text(ss(&text));
    }

    fn update_selection_geometry(&self) {
        let st = self.state.borrow();
        let sel = st.selected;
        let grid = self.ui.get_view_mode() == "grid";
        let mut y = 0.0f32;
        let mut h = ROW_H;
        let mut last_section = String::new();
        if grid {
            let cols = self.ui.get_grid_cols().max(1) as usize;
            let mut sec_start = 0usize;
            let mut found = false;
            for (i, it) in st.results.iter().enumerate() {
                if it.section != last_section {
                    if i > 0 {
                        let count = i - sec_start;
                        y += ((count + cols - 1) / cols) as f32 * CELL_H;
                    }
                    y += HEADER_H;
                    sec_start = i;
                    last_section = it.section.clone();
                }
                if i == sel {
                    let r = (i - sec_start) / cols;
                    y += r as f32 * CELL_H;
                    h = CELL_H;
                    found = true;
                    break;
                }
            }
            if !found {
                y = 0.0;
            }
        } else {
            for (i, it) in st.results.iter().enumerate() {
                if it.section != last_section {
                    y += HEADER_H;
                    last_section = it.section.clone();
                }
                if i == sel {
                    break;
                }
                y += ROW_H;
            }
        }
        drop(st);
        self.ui.set_selected_y(y);
        self.ui.set_selected_h(h);
        self.ui.invoke_ensure_visible();
    }

    fn set_selected(&self, idx: usize) {
        let len = self.state.borrow().results.len();
        if len == 0 {
            return;
        }
        let idx = idx.min(len - 1);
        {
            let mut st = self.state.borrow_mut();
            st.selected = idx;
            st.pinned_selected = None;
        }
        self.ui.set_selected(idx as i32);
        self.ui.set_pinned_selected(-1);
        self.update_selection_geometry();
    }

    fn selected_item(&self) -> Option<SearchItemModel> {
        let st = self.state.borrow();
        if let Some(p) = st.pinned_selected {
            return st.pinned.get(p).cloned();
        }
        st.results.get(st.selected).cloned()
    }

    fn move_selection(&self, dx: i32, dy: i32) {
        let (len, sel, pinned_sel, pinned_len) = {
            let st = self.state.borrow();
            (st.results.len() as i32, st.selected as i32, st.pinned_selected, st.pinned.len() as i32)
        };
        let shelf_open = !self.ui.get_pinned_collapsed() && pinned_len > 0;
        // 置顶展架内导航
        if let Some(p) = pinned_sel {
            let p = p as i32;
            if dy > 0 || (dx != 0 && pinned_len == 0) {
                self.set_selected(0);
                return;
            }
            if dy < 0 {
                return;
            }
            let np = (p + dx).rem_euclid(pinned_len.max(1));
            self.state.borrow_mut().pinned_selected = Some(np as usize);
            self.ui.set_pinned_selected(np);
            return;
        }
        if len == 0 {
            if shelf_open && dy < 0 {
                self.state.borrow_mut().pinned_selected = Some(0);
                self.ui.set_pinned_selected(0);
            }
            return;
        }
        let grid = self.ui.get_view_mode() == "grid";
        let step = if grid && dy != 0 { dy * self.ui.get_grid_cols().max(1) } else { dx + dy };
        let next = sel + step;
        if next < 0 {
            if shelf_open && dy < 0 {
                self.state.borrow_mut().pinned_selected = Some(0);
                self.ui.set_pinned_selected(0);
                self.ui.set_selected(-1);
                return;
            }
            self.set_selected((len - 1) as usize);
            return;
        }
        if next >= len {
            if grid && dy > 0 {
                self.set_selected((len - 1) as usize);
            } else {
                self.set_selected(0);
            }
            return;
        }
        self.set_selected(next as usize);
    }

    fn refresh_pinned(&self) {
        let pins = self.core.pinned_items();
        let ui_items: Vec<ResultItem> = pins.iter().map(to_ui_item).collect();
        self.state.borrow_mut().pinned = pins;
        self.ui.set_pinned(ModelRc::new(VecModel::from(ui_items)));
    }

    // ------------------------------------------------------------------
    // 条目动作
    // ------------------------------------------------------------------
    fn activate_item(self: &Rc<Self>, item: SearchItemModel) {
        match self.core.activate(&item) {
            Ok(msg) => {
                if item.item_type == ItemType::Clipboard && item.sub_type != "url" {
                    self.toast(&msg, "copy");
                    self.hide_window();
                } else {
                    self.hide_window();
                    log::info!("{msg}");
                }
            }
            Err(e) => self.toast(&format!("操作失败: {e}"), "x"),
        }
    }

    fn open_context_for(&self, item: SearchItemModel, x: f32, y: f32) {
        let is_folder = item.item_type == ItemType::Folder;
        let has_path = item.item_type != ItemType::Clipboard;
        let open_label = match item.item_type {
            ItemType::Clipboard => if item.sub_type == "url" { "在浏览器中打开" } else { "复制到剪贴板" },
            ItemType::App | ItemType::Command => "立即启动",
            ItemType::Folder => "打开文件夹",
            ItemType::File => "立即打开",
        };
        let pinned = self.core.storage.is_pinned(&item.id);
        self.ui.set_ctx_open_label(ss(open_label));
        self.ui.set_ctx_pin_label(ss(if pinned { "取消置顶" } else { "固定到置顶" }));
        self.ui.set_ctx_enter_label(ss(&format!("进入「{}」查找", item.title)));
        self.ui.set_ctx_is_folder(is_folder);
        self.ui.set_ctx_has_path(has_path);
        self.ui.set_ctx_x(x);
        self.ui.set_ctx_y(y);
        self.state.borrow_mut().ctx_target = Some(item);
        self.ui.set_ctx_visible(true);
    }

    fn run_action(self: &Rc<Self>, action: &str, item: SearchItemModel) {
        match action {
            "open" => self.activate_item(item),
            "reveal" => match self.core.reveal(&item) {
                Ok(msg) => {
                    self.toast(&msg, "folderOpen");
                    self.hide_window();
                }
                Err(e) => self.toast(&format!("{e}"), "x"),
            },
            "copy" => match self.core.copy_path(&item) {
                Ok(msg) => self.toast(&msg, "copy"),
                Err(e) => self.toast(&format!("{e}"), "x"),
            },
            "pin" => match self.core.toggle_pin(&item) {
                Ok((_, msg)) => {
                    self.toast(&msg, "pin");
                    self.refresh_pinned();
                    self.refresh_search();
                }
                Err(e) => self.toast(&format!("{e}"), "x"),
            },
            "remove" => match self.core.remove_recent(&item) {
                Ok(msg) => {
                    self.toast(&msg, "trash");
                    self.refresh_search();
                }
                Err(e) => self.toast(&format!("{e}"), "x"),
            },
            "enter" => self.enter_folder(item),
            _ => {}
        }
    }

    fn enter_folder(self: &Rc<Self>, item: SearchItemModel) {
        let path = if item.item_type == ItemType::Folder {
            item.full_path.clone()
        } else {
            std::path::Path::new(&item.full_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or(item.full_path.clone())
        };
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
        {
            let mut st = self.state.borrow_mut();
            st.scope.location_scope = "custom".into();
            st.scope.custom_directory = Some(path.clone());
            st.location_name = name.clone();
            st.location_value = format!("custom:{}", path.to_lowercase());
        }
        self.ui.set_filter_open(true);
        self.core.update_settings(|s| s.filter_shelf_open = true);
        self.ui.set_open_menu(ss(""));
        self.ui.set_query(ss(""));
        self.refresh_filter_labels();
        self.refresh_search();
        self.ui.invoke_focus_search();
        self.toast(&format!("已进入文件夹「{name}」限定检索"), "folder");
    }

    // ------------------------------------------------------------------
    // 筛选
    // ------------------------------------------------------------------
    fn refresh_filter_labels(&self) {
        let st = self.state.borrow();
        let sc = &st.scope;
        let time_label = match sc.time_preset.as_str() {
            "today" => "今天".to_string(),
            "3days" => "3天内".into(),
            "7days" | "week" => "7天内".into(),
            "30days" | "month" => "30天内".into(),
            "year" => "1年内".into(),
            "range" => match (st.cal_start, st.cal_end) {
                (Some(s), Some(e)) if s == e => format!("{}/{}", s.month(), s.day()),
                (Some(s), Some(e)) => format!("{}/{}-{}/{}", s.month(), s.day(), e.month(), e.day()),
                _ => "自定义".into(),
            },
            _ => "时间".into(),
        };
        let type_label = match sc.type_category.as_str() {
            "app" => "应用",
            "document" => "文档",
            "code" => "代码",
            "media" => "媒体",
            "archive" => "压缩包",
            "folder" => "文件夹",
            "clipboard" => "剪贴板",
            _ => "类型",
        };
        let loc_active = sc.location_scope != "all" && !sc.location_scope.is_empty();
        let loc_label = if loc_active { st.location_name.clone() } else { "位置".into() };
        self.ui.set_time_label(ss(&time_label));
        self.ui.set_type_label(ss(type_label));
        self.ui.set_location_label(ss(&loc_label));
        self.ui.set_time_active(sc.time_preset != "all" && !sc.time_preset.is_empty());
        self.ui.set_type_active(sc.type_category != "all" && !sc.type_category.is_empty());
        self.ui.set_location_active(loc_active);
        self.ui.set_time_preset(ss(&sc.time_preset));
        self.ui.set_type_value(ss(&sc.type_category));
        self.ui.set_location_value(ss(&st.location_value));
    }

    fn refresh_calendar(&self) {
        let st = self.state.borrow();
        let (y, m) = (st.cal_year, st.cal_month);
        let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
        let days_in_month = {
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            NaiveDate::from_ymd_opt(ny, nm, 1).unwrap().signed_duration_since(first).num_days() as u32
        };
        let lead = first.weekday().num_days_from_monday() as i64;
        let today = Local::now().date_naive();
        let (mut eff_start, mut eff_end) = (st.cal_start, st.cal_end);
        if let (Some(s), None, Some(h)) = (st.cal_start, st.cal_end, st.cal_hover) {
            if h < s {
                eff_start = Some(h);
                eff_end = Some(s);
            } else {
                eff_end = Some(h);
            }
        }
        let mut days: Vec<CalDay> = Vec::with_capacity(42);
        let start_date = first - chrono::Duration::days(lead);
        let total = ((lead + days_in_month as i64 + 6) / 7) * 7;
        for i in 0..total {
            let d = start_date + chrono::Duration::days(i);
            let other = d.month() != m;
            days.push(CalDay {
                day: d.day() as i32,
                date: ss(&d.format("%Y-%m-%d").to_string()),
                other_month: other,
                today: d == today,
                range_start: !other && eff_start == Some(d),
                range_end: !other && eff_end == Some(d),
                in_range: !other && matches!((eff_start, eff_end), (Some(s), Some(e)) if d > s && d < e),
            });
        }
        let summary = match (st.cal_start, st.cal_end) {
            (Some(s), Some(e)) => format!("{}月{}日 ~ {}月{}日 (共{}天)", s.month(), s.day(), e.month(), e.day(), (e - s).num_days() + 1),
            (Some(s), None) => format!("起点: {}月{}日，请选结束", s.month(), s.day()),
            _ => "点击选择起止日期范围".into(),
        };
        drop(st);
        self.ui.set_cal_title(ss(&format!("{y}年 {m}月")));
        self.ui.set_cal_days(ModelRc::new(VecModel::from(days)));
        self.ui.set_cal_summary(ss(&summary));
        self.ui.set_cal_can_apply(self.state.borrow().cal_start.is_some());
    }

    fn refresh_locations(&self) {
        let mut entries: Vec<LocationEntry> = Vec::new();
        for letter in b'C'..=b'Z' {
            let root = format!("{}:\\", letter as char);
            if std::fs::metadata(&root).is_ok() {
                entries.push(LocationEntry {
                    id: ss(&format!("drive-{}", (letter as char).to_ascii_lowercase())),
                    name: ss(&format!("{}:\\ 磁盘", letter as char)),
                    path: ss(&root),
                    sub: ss(if letter == b'C' { "系统与程序" } else { "" }),
                    icon: ss("drive"),
                    group: 0,
                });
            }
        }
        if let Some(ud) = directories::UserDirs::new() {
            for (id, name, icon, p) in [
                ("desktop", "桌面 (Desktop)", "desktop", ud.desktop_dir()),
                ("downloads", "下载 (Downloads)", "download", ud.download_dir()),
                ("documents", "个人文档", "fileText", ud.document_dir()),
            ] {
                if let Some(p) = p {
                    entries.push(LocationEntry { id: ss(id), name: ss(name), path: ss(&p.to_string_lossy()), sub: ss(""), icon: ss(icon), group: 0 });
                }
            }
        }
        // 工作区：索引目录 + 最近使用/置顶的文件夹
        let mut seen = std::collections::HashSet::new();
        let roots = self.core.settings.read().index_roots.clone();
        let mut ws: Vec<(String, String)> = roots.iter().map(|r| {
            let name = std::path::Path::new(r).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(r.clone());
            (name, r.clone())
        }).collect();
        for e in self.core.storage.list_pins().into_iter().chain(self.core.storage.list_recent(40)) {
            if e.kind == "folder" && !e.path.is_empty() {
                ws.push((e.title.clone(), e.path.clone()));
            }
        }
        for (name, path) in ws {
            let key = path.to_lowercase();
            if seen.insert(key.clone()) && entries.iter().all(|e| e.path.to_string().to_lowercase() != key) {
                entries.push(LocationEntry { id: ss(&format!("custom:{key}")), name: ss(&name), path: ss(&path), sub: ss(&path), icon: ss("folder"), group: 1 });
            }
            if entries.len() > 24 {
                break;
            }
        }
        self.ui.set_locations(ModelRc::new(VecModel::from(entries)));
    }

    fn apply_time_range(&self) {
        let mut st = self.state.borrow_mut();
        if let (Some(s), Some(e)) = (st.cal_start, st.cal_end.or(st.cal_start)) {
            st.cal_end = Some(e);
            let to_ts = |d: NaiveDate| Local.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).single().map(|t| t.timestamp()).unwrap_or(0);
            st.scope.time_preset = "range".into();
            st.scope.custom_start_time = Some(to_ts(s));
            // 用户选的是「日期」，语义上应当**含当天** —— 上限取当天 23:59:59。
            // 只传零点会让最后一天整天被排除（`time_upper_bound` 不再补一天）。
            st.scope.custom_end_time = Some(to_ts(e) + 86_399);
        }
    }

    // ------------------------------------------------------------------
    // 设置
    // ------------------------------------------------------------------
    fn sync_settings_to_ui(&self) {
        let s = self.core.settings.read().clone();
        let g = self.ui.global::<Settings>();
        g.set_wake_hotkey(ss(&s.wake_hotkey));
        g.set_launch_on_startup(s.launch_on_startup);
        g.set_hide_on_blur(s.hide_on_blur);
        g.set_default_mode(ss(&s.default_mode));
        g.set_double_click(s.double_click_launch);
        g.set_theme(ss(&s.theme));
        g.set_opacity_preset(ss(&s.acrylic_opacity));
        g.set_gpu_blur(s.gpu_blur);
        g.set_rim_light(s.rim_light);
        g.set_hotkeys_master(s.hotkeys_master_enabled);
        g.set_incremental(s.incremental_index);
        g.set_content_index(s.content_index_enabled);
        g.set_include_hidden(s.include_hidden);
        g.set_exclude_caches(s.exclude_build_caches);
        g.set_index_roots(ss(&s.index_roots.join("; ")));
        g.set_clip_enabled(s.clipboard_enabled);
        g.set_clip_limit(ss(&s.clipboard_limit.to_string()));
        g.set_clip_dedupe(s.clipboard_dedupe);
        g.set_clip_ignore_pm(s.clipboard_ignore_password_managers);
        g.set_clip_mask(s.clipboard_mask_sensitive);
        g.set_ai_enabled(s.ai_enabled);
        g.set_ai_lazy(s.ai_lazy_load);
        g.set_ai_intent(s.ai_intent_parsing);
        g.set_ai_model(ss(&format!("EmbeddingBackend: {} · bge-small-zh 待接入", self.core.engine.embedding_name())));
        self.sync_stats_to_ui();
        self.sync_hotkeys_to_ui();
        let recent: Vec<ResultItem> = self
            .core
            .recent_items()
            .iter()
            .filter(|i| i.item_type != ItemType::Clipboard)
            .take(12)
            .map(to_ui_item)
            .collect();
        g.set_recent_picker(ModelRc::new(VecModel::from(recent)));
    }

    fn sync_stats_to_ui(&self) {
        let (files, content, db, apps, clips) = self.core.index_summary();
        let g = self.ui.global::<Settings>();
        let indexing = self.core.indexer.stats.scanning.load(Ordering::Relaxed);
        g.set_indexing(indexing);
        g.set_index_stats(ss(&format!(
            "已索引 {} 个文件 · {} 篇正文 · {} 个应用 · 索引库 {}",
            files,
            content,
            apps,
            crate::core::search::fmt_size(db)
        )));

        // 健康度：把「正文索引 543 条、文件索引却只有 1 条」这类不一致直接摆到用户面前，
        // 而不是等到搜索里发现打不开文件才察觉。
        let (rep, st) = self.core.index_health();
        g.set_index_integrity(ss(&if rep.healthy() {
            format!("文件 {} · 正文 {} · 孤儿正文 0 · 悬空引用 0 —— 一致性正常", rep.files, rep.content)
        } else {
            format!(
                "文件 {} · 正文 {} · 孤儿正文 {} · 悬空置顶 {} · 悬空最近 {} —— 存在不一致，建议重建索引",
                rep.files, rep.content, rep.orphan_content, rep.dangling_pins, rep.dangling_recent
            )
        }));

        let mode_cn = match st.mode.as_str() {
            "rebuild" => "全量重建",
            "full" => "全量校验",
            "reconcile" => "增量对账",
            _ => "尚未扫描",
        };
        let free_pct = if rep.db_bytes > 0 {
            rep.freelist_bytes as f64 * 100.0 / rep.db_bytes as f64
        } else {
            0.0
        };
        g.set_index_space(ss(&format!(
            "索引库 {} · 空闲页 {} ({:.1}%) · 上轮{} {:.2} s · 移除 {} 条{}",
            crate::core::search::fmt_size(rep.db_bytes),
            crate::core::search::fmt_size(rep.freelist_bytes),
            free_pct,
            mode_cn,
            st.last_scan_ms as f64 / 1000.0,
            st.removed,
            if st.dropped > 0 { format!(" · 丢弃 {} 条（异常）", st.dropped) } else { String::new() }
        )));
        g.set_index_maintaining(self.core.is_compacting());

        g.set_clip_stats(ss(&format!("剪贴板历史 {} 条 · 位于 {}", clips, crate::core::settings::db_path().display())));
        g.set_cache_info(ss(&format!("索引库与配置位于 {} · 当前 {}", crate::core::settings::data_dir().display(), crate::core::search::fmt_size(db))));
    }

    fn sync_hotkeys_to_ui(&self) {
        let list: Vec<HotkeyBinding> = self
            .core
            .hotkeys()
            .iter()
            .map(|b| {
                let (icon, badge) = hotkey_icon(b);
                HotkeyBinding {
                    id: ss(&b.id),
                    name: ss(&b.name),
                    path: ss(&b.target_path),
                    hotkey: ss(&b.hotkey),
                    kind: ss(&b.item_type),
                    enabled: b.enabled,
                    icon: ss(icon),
                    badge: ss(badge),
                }
            })
            .collect();
        self.ui.global::<Settings>().set_hotkeys(ModelRc::new(VecModel::from(list)));
    }

    fn open_settings(&self) {
        self.sync_settings_to_ui();
        self.ui.set_ctx_visible(false);
        self.ui.set_open_menu(ss(""));
        self.ui.set_settings_visible(true);
    }

    fn close_settings(&self) {
        self.stop_recording();
        self.ui.global::<Settings>().set_add_open(false);
        self.ui.set_settings_visible(false);
        self.ui.invoke_focus_search();
    }

    fn start_recording(&self, id: &str) {
        self.state.borrow_mut().recording = Some(id.to_string());
        self.ui.global::<Settings>().set_recording_id(ss(id));
        self.ui.invoke_focus_root();
    }

    fn stop_recording(&self) {
        self.state.borrow_mut().recording = None;
        self.ui.global::<Settings>().set_recording_id(ss(""));
    }

    fn finish_recording(self: &Rc<Self>, combo: &str) {
        let Some(id) = self.state.borrow().recording.clone() else { return };
        self.stop_recording();
        let g = self.ui.global::<Settings>();
        match id.as_str() {
            "__new__" => {
                g.set_new_combo(ss(combo));
                if let Some(why) = self.core.detect_conflict(combo, None) {
                    self.toast(&format!("⚠ {combo} {why}"), "alert");
                } else {
                    self.toast(&format!("已录制快捷键: {combo}"), "check");
                }
            }
            "__wake__" => match self.core.set_wake_hotkey(combo) {
                Ok(()) => {
                    g.set_wake_hotkey(ss(&hotkey::normalize(combo)));
                    self.toast(&format!("唤醒快捷键已更新: {}", hotkey::normalize(combo)), "check");
                }
                Err(e) => self.toast(&format!("⚠ {e}"), "alert"),
            },
            _ => {
                if let Some(mut b) = self.core.hotkeys().into_iter().find(|b| b.id == id) {
                    b.hotkey = combo.to_string();
                    match self.core.save_hotkey(b.clone()) {
                        Ok(()) => self.toast(&format!("已更新快捷键: {} → {}", b.name, hotkey::normalize(combo)), "check"),
                        Err(e) => self.toast(&format!("⚠ {e}"), "alert"),
                    }
                    self.sync_hotkeys_to_ui();
                }
            }
        }
    }

    fn save_new_hotkey(self: &Rc<Self>) {
        let g = self.ui.global::<Settings>();
        let name = g.get_new_name().trim().to_string();
        let path = g.get_new_path().trim().trim_matches('"').to_string();
        let kind = g.get_new_kind().to_string();
        let combo = g.get_new_combo().to_string();
        if name.is_empty() {
            self.toast("请输入绑定目标名称", "x");
            return;
        }
        if path.is_empty() {
            self.toast("请输入程序或文件路径", "x");
            return;
        }
        let binding = HotkeyBindingModel {
            id: format!("hk-{}", chrono::Utc::now().timestamp_millis()),
            name: name.clone(),
            target_path: path,
            hotkey: combo.clone(),
            item_type: if kind.is_empty() { "app".into() } else { kind },
            enabled: true,
            ..Default::default()
        };
        match self.core.save_hotkey(binding) {
            Ok(()) => {
                g.set_add_open(false);
                g.set_new_name(ss(""));
                g.set_new_path(ss(""));
                g.set_new_combo(ss("Alt+K"));
                self.sync_hotkeys_to_ui();
                self.toast(&format!("已绑定快捷键: {} → {}", hotkey::normalize(&combo), name), "check");
            }
            Err(e) => self.toast(&format!("⚠ {e}"), "alert"),
        }
    }

    fn set_setting_bool(self: &Rc<Self>, key: &str, v: bool) {
        match key {
            "hotkeys_master_enabled" => {
                self.core.set_hotkeys_master(v);
                self.toast(if v { "全局快捷直达服务已开启" } else { "全局快捷直达服务已暂停" }, "keyboard");
                return;
            }
            _ => {}
        }
        let mut reindex = false;
        self.core.update_settings(|s| match key {
            "launch_on_startup" => s.launch_on_startup = v,
            "hide_on_blur" => s.hide_on_blur = v,
            "double_click_launch" => s.double_click_launch = v,
            "gpu_blur" => s.gpu_blur = v,
            "rim_light" => s.rim_light = v,
            "incremental_index" => s.incremental_index = v,
            "content_index_enabled" => {
                s.content_index_enabled = v;
                reindex = v;
            }
            "include_hidden" => {
                s.include_hidden = v;
                reindex = true;
            }
            "exclude_build_caches" => {
                s.exclude_build_caches = v;
                reindex = true;
            }
            "clipboard_enabled" => s.clipboard_enabled = v,
            "clipboard_dedupe" => s.clipboard_dedupe = v,
            "clipboard_ignore_password_managers" => s.clipboard_ignore_password_managers = v,
            "clipboard_mask_sensitive" => s.clipboard_mask_sensitive = v,
            "ai_enabled" => s.ai_enabled = v,
            "ai_lazy_load" => s.ai_lazy_load = v,
            "ai_intent_parsing" => s.ai_intent_parsing = v,
            _ => {}
        });
        match key {
            "gpu_blur" => self.apply_effects(),
            "rim_light" => self.apply_theme_from_settings(),
            // 以下三个开关都有真实后端副作用（起停文件监听 / 起停剪贴板监听 / 开关 FTS），
            // 但界面上看不出任何变化，用户会以为没生效 —— 补上明确提示。
            "incremental_index" => self.toast(
                if v { "实时增量索引已开启" } else { "实时增量索引已暂停" }, "database"),
            "content_index_enabled" if !v => self.toast("已关闭正文全文检索（索引数据保留）", "database"),
            "clipboard_enabled" => self.toast(
                if v { "剪贴板监控已开启" } else { "剪贴板监控已暂停" }, "clipboard"),
            _ => {}
        }
        if reindex {
            self.toast("索引设置已更新，将在后台重建索引", "database");
            self.core.rebuild_index();
        }
        self.sync_settings_to_ui();
    }

    fn set_setting_string(self: &Rc<Self>, key: &str, v: &str) {
        let v = v.to_string();
        match key {
            "index_roots" => {
                let roots: Vec<String> = v.split(';').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect();
                // 空输入必须挡掉：filter 之后 roots 会是空 vec，而空 vec 能通过下面的
                // 「路径都存在」校验，结果是把 index_roots 静默清空、再拿零个根目录
                // 重建索引（实测索引文件数直接归零），却弹出「已保存」的成功提示。
                if roots.is_empty() {
                    self.toast("索引目录不能为空，已忽略本次保存", "alert");
                    return;
                }
                let missing: Vec<&String> = roots.iter().filter(|r| !std::path::Path::new(r).exists()).collect();
                if !missing.is_empty() {
                    self.toast(&format!("目录不存在: {}", missing[0]), "x");
                    return;
                }
                self.core.update_settings(|s| s.index_roots = roots);
                self.core.rebuild_index();
                self.refresh_locations();
                self.toast("索引目录已保存，正在后台重建索引", "database");
            }
            "clipboard_limit" => {
                // UI 上是固定选项下拉（100/300/500/1000），正常走不到 Err 分支；
                // 保留解析校验作为防御，并且失败时给出提示而不是静默吞掉。
                match v.parse::<u32>() {
                    Ok(n) => self.core.update_settings(|s| s.clipboard_limit = n),
                    Err(_) => self.toast(&format!("保留容量必须是数字，已忽略: {v}"), "alert"),
                }
            }
            "default_mode" => self.core.update_settings(|s| s.default_mode = v.clone()),
            "theme" => {
                self.core.update_settings(|s| s.theme = v.clone());
                self.apply_theme_from_settings();
                self.apply_effects();
            }
            "acrylic_opacity" => {
                self.core.update_settings(|s| s.acrylic_opacity = v.clone());
                self.apply_theme_from_settings();
            }
            _ => {}
        }
        self.sync_settings_to_ui();
    }

    // ------------------------------------------------------------------
    // 键盘
    // ------------------------------------------------------------------
    fn on_key(self: &Rc<Self>, text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> bool {
        const ESC: &str = "\u{1b}";
        const TAB: &str = "\t";
        const ENTER: &str = "\n";
        const UP: &str = "\u{F700}";
        const DOWN: &str = "\u{F701}";
        const LEFT: &str = "\u{F702}";
        const RIGHT: &str = "\u{F703}";
        const DEL: &str = "\u{7f}";

        // 热键录制模式
        if self.state.borrow().recording.is_some() {
            if text == ESC {
                self.stop_recording();
                self.toast("已取消快捷键录制", "x");
                return true;
            }
            let Some(name) = hotkey::key_name_from_slint(text) else { return true };
            if !(ctrl || alt || meta) && !(name.starts_with('F') && name.len() > 1) {
                self.toast("请使用带修饰键的组合（如 Alt+C / Ctrl+Shift+T）", "alert");
                return true;
            }
            let combo = hotkey::combo(ctrl, alt, shift, meta, &name);
            self.finish_recording(&combo);
            return true;
        }

        if text == ESC {
            if !self.ui.get_open_menu().is_empty() {
                self.ui.set_open_menu(ss(""));
            } else if self.ui.get_ctx_visible() {
                self.ui.set_ctx_visible(false);
            } else if self.ui.get_settings_visible() {
                self.close_settings();
            } else if self.ui.get_filter_open() {
                self.ui.set_filter_open(false);
                self.core.update_settings(|s| s.filter_shelf_open = false);
            } else if !self.ui.get_query().is_empty() {
                self.ui.set_query(ss(""));
                self.refresh_search();
            } else {
                self.hide_window();
            }
            return true;
        }
        if self.ui.get_settings_visible() {
            return false;
        }
        if ctrl && text == "," {
            self.open_settings();
            return true;
        }
        if text == TAB {
            if let Some(item) = self.selected_item() {
                if item.item_type == ItemType::Folder {
                    self.enter_folder(item);
                    return true;
                }
            }
            self.toggle_mode();
            return true;
        }
        if text == ENTER {
            self.ui.set_ctx_visible(false);
            if let Some(item) = self.selected_item() {
                self.activate_item(item);
            }
            return true;
        }
        if text == UP {
            self.move_selection(0, -1);
            return true;
        }
        if text == DOWN {
            self.move_selection(0, 1);
            return true;
        }
        if text == LEFT || text == RIGHT {
            let in_shelf = self.state.borrow().pinned_selected.is_some();
            if in_shelf || self.ui.get_view_mode() == "grid" {
                self.move_selection(if text == LEFT { -1 } else { 1 }, 0);
                return true;
            }
            return false;
        }
        if alt && !ctrl {
            let cat = match text {
                "1" => Some("all"),
                "2" => Some("file"),
                "3" => Some("app"),
                "4" => Some("clipboard"),
                _ => None,
            };
            if let Some(c) = cat {
                self.set_category(c);
                return true;
            }
        }
        if ctrl && !alt {
            let lower = text.to_lowercase();
            match lower.as_str() {
                "k" => {
                    if self.ui.get_ctx_visible() {
                        self.ui.set_ctx_visible(false);
                    } else if let Some(item) = self.selected_item() {
                        let y = 150.0 + self.ui.get_selected_y().min(200.0);
                        self.open_context_for(item, 80.0, y);
                    }
                    return true;
                }
                "p" => {
                    if let Some(item) = self.selected_item() {
                        self.run_action("pin", item);
                    }
                    return true;
                }
                "o" => {
                    if let Some(item) = self.selected_item() {
                        self.run_action("reveal", item);
                    }
                    return true;
                }
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                    let idx = lower.parse::<usize>().unwrap_or(1) - 1;
                    let item = self.state.borrow().pinned.get(idx).cloned();
                    if let Some(item) = item {
                        self.activate_item(item);
                    }
                    return true;
                }
                _ => {}
            }
        }
        if shift && text == DEL {
            if let Some(item) = self.selected_item() {
                self.run_action("remove", item);
            }
            return true;
        }
        false
    }

    fn toggle_mode(self: &Rc<Self>) {
        let next = if self.ui.get_mode() == "smart" { "fast" } else { "smart" };
        self.ui.set_mode(ss(next));
        self.toast(if next == "smart" { "已切换至 ✦ 智能搜索模式 (Tab 可切回极速)" } else { "已切换至 ⚡ 极速搜索模式 (Tab 可切至智能)" }, if next == "smart" { "sparkles" } else { "lightning" });
        self.refresh_search();
    }

    fn set_category(self: &Rc<Self>, cat: &str) {
        self.ui.set_category(ss(cat));
        self.refresh_search();
    }

    // ------------------------------------------------------------------
    // 后端通知
    // ------------------------------------------------------------------
    fn handle_notification(self: &Rc<Self>, n: BackendNotification) {
        match n {
            BackendNotification::WakeRequested => self.toggle_window(),
            BackendNotification::HotkeyTriggered { app_name, .. } => {
                if self.ui.window().is_visible() {
                    self.toast(&format!("快捷直达: {app_name}"), "lightning");
                }
            }
            BackendNotification::HotkeyConflictDetected { hotkey, reason } => {
                self.toast(&format!("⚠ 快捷键 {hotkey} 冲突: {reason}"), "alert");
            }
            BackendNotification::ClipboardItemAdded { .. } => {
                if self.ui.window().is_visible() && self.ui.get_query().is_empty() {
                    self.refresh_search();
                }
            }
            BackendNotification::IndexProgress { .. } => {
                self.update_status();
                if self.ui.get_settings_visible() {
                    self.sync_stats_to_ui();
                }
            }
            BackendNotification::RecentItemsReady { .. } => {
                if self.ui.window().is_visible() {
                    self.refresh_search();
                }
                // 应用扫描完成后可能触发首次置顶预置（设计稿 65.4），
                // 因此这里一并刷新置顶展架，避免预置卡片要等下次交互才出现。
                self.refresh_pinned();
                self.update_status();
            }
            BackendNotification::ToastMessage { text, icon } => {
                self.toast(&text, &icon);
                // 索引库整理是在后台线程跑的，完成通知回到这里时按钮要恢复可点、
                // 空闲页读数也要跟着刷新。
                if self.ui.get_settings_visible() {
                    self.sync_stats_to_ui();
                }
            }
            BackendNotification::SearchResultsReady { .. } => {}
        }
    }

    // ------------------------------------------------------------------
    // 回调绑定
    // ------------------------------------------------------------------
    fn bind_callbacks(self: &Rc<Self>) {
        let ui = &self.ui;
        macro_rules! bind {
            ($setter:ident, |$g:ident $(, $arg:ident)*| $body:block) => {{
                let weak = Rc::downgrade(self);
                ui.$setter(move |$($arg),*| {
                    if let Some($g) = weak.upgrade() { $body }
                });
            }};
        }
        macro_rules! bind_ret {
            ($setter:ident, |$g:ident $(, $arg:ident)*| $body:block, $default:expr) => {{
                let weak = Rc::downgrade(self);
                ui.$setter(move |$($arg),*| {
                    if let Some($g) = weak.upgrade() { $body } else { $default }
                });
            }};
        }

        bind!(on_query_changed, |g, _q| { g.refresh_search(); });
        bind!(on_submit, |g| {
            if let Some(item) = g.selected_item() { g.activate_item(item); }
        });
        bind_ret!(on_key_pressed, |g, text, ctrl, alt, shift, meta| { g.on_key(text.as_str(), ctrl, alt, shift, meta) }, false);
        bind!(on_select, |g, i| { g.set_selected(i.max(0) as usize); });
        bind!(on_activate, |g, i| {
            g.set_selected(i.max(0) as usize);
            if let Some(item) = g.selected_item() { g.activate_item(item); }
        });
        bind!(on_context, |g, i, x, y| {
            g.set_selected(i.max(0) as usize);
            if let Some(item) = g.selected_item() { g.open_context_for(item, x, y); }
        });
        bind!(on_pinned_select, |g, i| {
            g.state.borrow_mut().pinned_selected = Some(i.max(0) as usize);
            g.ui.set_pinned_selected(i);
            g.ui.set_selected(-1);
        });
        bind!(on_pinned_activate, |g, i| {
            let item = g.state.borrow().pinned.get(i.max(0) as usize).cloned();
            if let Some(item) = item { g.activate_item(item); }
        });
        bind!(on_pinned_context, |g, i, x, y| {
            let item = g.state.borrow().pinned.get(i.max(0) as usize).cloned();
            if let Some(item) = item { g.open_context_for(item, x, y); }
        });
        bind!(on_toggle_pinned, |g| {
            let v = !g.ui.get_pinned_collapsed();
            g.ui.set_pinned_collapsed(v);
            g.core.update_settings(|s| s.pinned_collapsed = v);
        });
        bind!(on_set_category, |g, c| { g.set_category(c.as_str()); });
        bind!(on_set_clip_sub, |g, s| { g.ui.set_clip_sub(s); g.refresh_search(); });
        bind!(on_toggle_view, |g| {
            let next = if g.ui.get_view_mode() == "grid" { "list" } else { "grid" };
            g.ui.set_view_mode(ss(next));
            g.core.update_settings(|s| s.view_mode = next.into());
            g.update_selection_geometry();
        });
        bind!(on_toggle_mode, |g| { g.toggle_mode(); });
        bind!(on_toggle_filter, |g| {
            let v = !g.ui.get_filter_open();
            g.ui.set_filter_open(v);
            if !v { g.ui.set_open_menu(ss("")); }
            g.core.update_settings(|s| s.filter_shelf_open = v);
        });
        bind!(on_ctx_action, |g, a| {
            let target = g.state.borrow().ctx_target.clone();
            if let Some(item) = target { g.run_action(a.as_str(), item); }
        });
        bind!(on_open_group, |g, grp| {
            let cur = g.ui.get_open_menu();
            if cur.as_str() == grp.as_str() {
                g.ui.set_open_menu(ss(""));
            } else {
                g.ui.set_open_menu(grp.clone());
                if grp.as_str() == "time" { g.refresh_calendar(); }
                if grp.as_str() == "location" { g.refresh_locations(); g.ui.invoke_focus_location_box(); }
            }
        });
        bind!(on_clear_group, |g, grp| {
            {
                let mut st = g.state.borrow_mut();
                match grp.as_str() {
                    "time" => { st.scope.time_preset = "all".into(); st.scope.custom_start_time = None; st.scope.custom_end_time = None; st.cal_start = None; st.cal_end = None; }
                    "type" => st.scope.type_category = "all".into(),
                    _ => { st.scope.location_scope = "all".into(); st.scope.custom_directory = None; st.location_name.clear(); st.location_value = "all".into(); }
                }
            }
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_calendar();
            g.refresh_search();
        });
        bind!(on_reset_filters, |g| {
            {
                let mut st = g.state.borrow_mut();
                st.scope = SearchScopeFilter { time_preset: "all".into(), type_category: "all".into(), location_scope: "all".into(), ..Default::default() };
                st.cal_start = None; st.cal_end = None; st.cal_hover = None;
                st.location_name.clear(); st.location_value = "all".into();
            }
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_calendar();
            g.refresh_search();
            g.toast("已重置所有检索筛选条件", "filter");
        });
        bind!(on_time_preset_picked, |g, p| {
            {
                let mut st = g.state.borrow_mut();
                st.scope.time_preset = p.to_string();
                st.scope.custom_start_time = None; st.scope.custom_end_time = None;
                st.cal_start = None; st.cal_end = None; st.cal_hover = None;
            }
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_calendar();
            g.refresh_search();
        });
        bind!(on_cal_prev, |g| {
            { let mut st = g.state.borrow_mut(); if st.cal_month == 1 { st.cal_month = 12; st.cal_year -= 1; } else { st.cal_month -= 1; } }
            g.refresh_calendar();
        });
        bind!(on_cal_next, |g| {
            { let mut st = g.state.borrow_mut(); if st.cal_month == 12 { st.cal_month = 1; st.cal_year += 1; } else { st.cal_month += 1; } }
            g.refresh_calendar();
        });
        bind!(on_cal_day, |g, d| {
            if let Ok(date) = NaiveDate::parse_from_str(d.as_str(), "%Y-%m-%d") {
                let mut st = g.state.borrow_mut();
                match (st.cal_start, st.cal_end) {
                    (Some(s), None) => {
                        if date < s { st.cal_end = Some(s); st.cal_start = Some(date); } else { st.cal_end = Some(date); }
                        st.cal_hover = None;
                    }
                    _ => { st.cal_start = Some(date); st.cal_end = None; st.cal_hover = None; }
                }
            }
            g.refresh_calendar();
        });
        bind!(on_cal_hover, |g, d| {
            let needs = { let st = g.state.borrow(); st.cal_start.is_some() && st.cal_end.is_none() };
            if needs {
                if let Ok(date) = NaiveDate::parse_from_str(d.as_str(), "%Y-%m-%d") {
                    g.state.borrow_mut().cal_hover = Some(date);
                    g.refresh_calendar();
                }
            }
        });
        bind!(on_cal_clear, |g| {
            { let mut st = g.state.borrow_mut(); st.cal_start = None; st.cal_end = None; st.cal_hover = None;
              if st.scope.time_preset == "range" { st.scope.time_preset = "all".into(); st.scope.custom_start_time = None; st.scope.custom_end_time = None; } }
            g.refresh_filter_labels();
            g.refresh_calendar();
            g.refresh_search();
        });
        bind!(on_cal_apply, |g| {
            g.apply_time_range();
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_calendar();
            g.refresh_search();
        });
        bind!(on_pick_type, |g, t| {
            g.state.borrow_mut().scope.type_category = t.to_string();
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_search();
        });
        bind!(on_pick_location, |g, id, name, path| {
            {
                let mut st = g.state.borrow_mut();
                let id_s = id.to_string();
                if id_s == "all" {
                    st.scope.location_scope = "all".into();
                    st.scope.custom_directory = None;
                    st.location_name.clear();
                } else if id_s.starts_with("custom:") || id_s.starts_with("drive-") && id_s.len() > 7 {
                    st.scope.location_scope = "custom".into();
                    st.scope.custom_directory = Some(path.to_string());
                    st.location_name = name.to_string();
                } else {
                    st.scope.location_scope = id_s.clone();
                    st.scope.custom_directory = Some(path.to_string());
                    st.location_name = name.to_string();
                }
                st.location_value = id_s;
            }
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_search();
        });
        bind!(on_custom_location, |g, p| {
            let path = p.trim().trim_matches('"').to_string();
            if !std::path::Path::new(&path).is_dir() {
                g.toast(&format!("文件夹不存在: {path}"), "x");
                return;
            }
            let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
            {
                let mut st = g.state.borrow_mut();
                st.scope.location_scope = "custom".into();
                st.scope.custom_directory = Some(path.clone());
                st.location_name = name;
                st.location_value = format!("custom:{}", path.to_lowercase());
            }
            g.ui.set_open_menu(ss(""));
            g.refresh_filter_labels();
            g.refresh_search();
        });
        bind!(on_browse_folder, |g| {
            g.state.borrow_mut().suppress_blur = true;
            let picked = dialogs::pick_path(true, "选择要检索的文件夹");
            g.state.borrow_mut().suppress_blur = false;
            if let Some(hwnd) = g.hwnd() { launcher::force_foreground(hwnd); }
            if let Some(path) = picked {
                let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
                {
                    let mut st = g.state.borrow_mut();
                    st.scope.location_scope = "custom".into();
                    st.scope.custom_directory = Some(path.clone());
                    st.location_name = name.clone();
                    st.location_value = format!("custom:{}", path.to_lowercase());
                }
                g.ui.set_open_menu(ss(""));
                g.refresh_filter_labels();
                g.refresh_search();
                g.toast(&format!("已进入文件夹: {path}"), "folder");
            }
            g.ui.invoke_focus_search();
        });
        bind!(on_open_settings, |g| { g.open_settings(); });
        bind!(on_close_settings, |g| { g.close_settings(); });
        bind!(on_hide_window, |g| { g.hide_window(); });

        // 托盘
        {
            let weak = Rc::downgrade(self);
            self.tray.on_open(move || { if let Some(g) = weak.upgrade() { g.show_window(); } });
            let weak = Rc::downgrade(self);
            self.tray.on_open_settings(move || { if let Some(g) = weak.upgrade() { g.show_window(); g.open_settings(); } });
            let weak = Rc::downgrade(self);
            self.tray.on_quit(move || {
                if let Some(g) = weak.upgrade() { g.persist_size(true); g.core.shutdown(); }
                let _ = slint::quit_event_loop();
            });
        }

        // 设置面板回调
        let settings = ui.global::<Settings>();
        macro_rules! sbind {
            ($setter:ident, |$g:ident $(, $arg:ident)*| $body:block) => {{
                let weak = Rc::downgrade(self);
                settings.$setter(move |$($arg),*| {
                    if let Some($g) = weak.upgrade() { $body }
                });
            }};
        }
        sbind!(on_set_bool, |g, key, v| { g.set_setting_bool(key.as_str(), v); });
        sbind!(on_set_string, |g, key, v| { g.set_setting_string(key.as_str(), v.as_str()); });
        sbind!(on_action, |g, a| {
            match a.as_str() {
                "clear-cache" => {
                    let _ = g.core.storage.kv_set("last_cache_clear", &chrono::Utc::now().to_rfc3339());
                    g.toast("已清理运行时缓存", "check");
                }
                "rebuild-index" => {
                    g.core.rebuild_index();
                    g.toast("已触发全量索引重建（后台低优先级运行）", "database");
                    g.sync_stats_to_ui();
                }
                "reconcile-index" => {
                    g.core.reconcile_index();
                    g.toast("已触发增量对账（只检查变化的目录）", "search");
                    g.sync_stats_to_ui();
                }
                "compact-index" => {
                    // 真正的 VACUUM 在后台线程跑，完成后由 ToastMessage 通知回来
                    g.core.compact_index_async();
                    g.sync_stats_to_ui();
                }
                "clear-clipboard" => match g.core.clear_clipboard_history() {
                    Ok(n) => { g.toast(&format!("已清除 {n} 条未固定的剪贴板记录"), "trash"); g.sync_stats_to_ui(); g.refresh_search(); }
                    Err(e) => g.toast(&format!("{e}"), "x"),
                },
                "save-hotkey" => g.save_new_hotkey(),
                "cancel-hotkey" => g.stop_recording(),
                "browse-target" => {
                    let kind = g.ui.global::<Settings>().get_new_kind().to_string();
                    g.state.borrow_mut().suppress_blur = true;
                    let picked = dialogs::pick_path(kind == "folder", "选择要绑定的目标");
                    g.state.borrow_mut().suppress_blur = false;
                    if let Some(hwnd) = g.hwnd() { launcher::force_foreground(hwnd); }
                    if let Some(p) = picked {
                        let s = g.ui.global::<Settings>();
                        s.set_new_path(ss(&p));
                        if s.get_new_name().is_empty() {
                            let stem = std::path::Path::new(&p).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            s.set_new_name(ss(&stem));
                        }
                    }
                }
                _ => {}
            }
        });
        sbind!(on_hotkey_toggle, |g, id, v| {
            if let Err(e) = g.core.set_hotkey_enabled(id.as_str(), v) { g.toast(&format!("{e}"), "x"); }
            g.sync_hotkeys_to_ui();
        });
        sbind!(on_hotkey_test, |g, id| {
            g.state.borrow_mut().suppress_blur = true;
            match g.core.test_hotkey(id.as_str()) {
                Ok(msg) => g.toast(&msg, "play"),
                Err(e) => g.toast(&format!("测试失败: {e}"), "x"),
            }
            let weak = Rc::downgrade(&g);
            slint::Timer::single_shot(Duration::from_millis(1500), move || {
                if let Some(g) = weak.upgrade() { g.state.borrow_mut().suppress_blur = false; }
            });
        });
        sbind!(on_hotkey_delete, |g, id| {
            if let Err(e) = g.core.delete_hotkey(id.as_str()) { g.toast(&format!("{e}"), "x"); } else { g.toast("已移除快捷直达绑定", "trash"); }
            g.sync_hotkeys_to_ui();
        });
        sbind!(on_hotkey_record, |g, id| {
            if g.state.borrow().recording.as_deref() == Some(id.as_str()) {
                g.stop_recording();
            } else {
                g.start_recording(id.as_str());
                g.toast("正在录制快捷键：请按下组合键（Esc 取消）", "keyboard");
            }
        });
        sbind!(on_pick_recent, |g, i| {
            let s = g.ui.global::<Settings>();
            let items = g.core.recent_items();
            let list: Vec<&SearchItemModel> = items.iter().filter(|i| i.item_type != ItemType::Clipboard).take(12).collect();
            if let Some(it) = list.get(i.max(0) as usize) {
                s.set_new_name(ss(&it.title));
                s.set_new_path(ss(&it.full_path));
                s.set_new_kind(ss(match it.item_type { ItemType::App | ItemType::Command => "app", ItemType::Folder => "folder", _ => "file" }));
            }
        });
    }
}

type IntentChipModel = crate::models::IntentChip;

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> SearchItemModel {
        SearchItemModel { id: id.into(), title: id.into(), ..Default::default() }
    }

    fn shown(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| (*s).to_string()).collect()
    }

    fn ids(items: &[SearchItemModel]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    /// 首屏已有的条目不能被重复插入 —— 否则列表里会出现两行一模一样的结果。
    #[test]
    fn already_shown_items_are_dropped() {
        let extra = vec![item("a"), item("b"), item("c")];
        let (fresh, _) = plan_merge(&shown(&["a", "c"]), 0, extra).expect("b 是新条目");
        assert_eq!(ids(&fresh), vec!["b"]);
    }

    /// 没有任何新条目时必须返回 `None`，调用方据此**什么都不做**。
    /// 若返回空 Vec 而不是 None，调用方就会白刷一次界面（可见的抖动）。
    #[test]
    fn no_new_items_means_do_nothing() {
        let extra = vec![item("a"), item("b")];
        assert!(plan_merge(&shown(&["a", "b"]), 0, extra).is_none());
    }

    /// 用户没动过选中（还停在第一条）时，选中留在**新的**第一条上 ——
    /// 他刚敲完字，应该直接看到模型补出来的东西。
    #[test]
    fn untouched_selection_follows_the_new_first_row() {
        let extra = vec![item("x"), item("y"), item("a")];
        let (fresh, selected) = plan_merge(&shown(&["a"]), 0, extra).unwrap();
        assert_eq!(ids(&fresh), vec!["x", "y"]);
        assert_eq!(selected, 0, "没动过选中就不该往下移");
    }

    /// 用户已经用方向键选过了，就必须保住他选的那一条。
    ///
    /// 这是 off-by-one 的哨兵：位移量只能是**新条目数**（2），
    /// 写成「新条目数 + 1」（3）会让他选中的变成原来那条的下一条。
    #[test]
    fn navigated_selection_stays_on_the_same_item() {
        // 首屏 5 条：a b c d e，用户选了下标 3（d）
        let first_screen = ["a", "b", "c", "d", "e"];
        let extra = vec![item("x"), item("y"), item("a"), item("b"), item("c"), item("d"), item("e")];
        let (fresh, selected) = plan_merge(&shown(&first_screen), 3, extra).unwrap();
        assert_eq!(ids(&fresh), vec!["x", "y"]);

        // 合并后列表是 [x, y, a, b, c, d, e]，原下标 3 的 d 现在在 5
        let merged: Vec<String> = fresh
            .iter()
            .map(|i| i.id.clone())
            .chain(first_screen.iter().map(|s| (*s).to_string()))
            .collect();
        assert_eq!(selected, 5);
        assert_eq!(merged[selected], "d", "选中的必须还是 d");
    }

    /// 新条目插在最前面，原列表一条不少 —— §5.7 硬性规则 1 不得清空。
    #[test]
    fn existing_items_are_never_dropped() {
        let first_screen = ["a", "b"];
        let extra = vec![item("x"), item("a"), item("b")];
        let (fresh, _) = plan_merge(&shown(&first_screen), 1, extra).unwrap();
        assert_eq!(ids(&fresh), vec!["x"]);
        let merged: Vec<String> = fresh
            .iter()
            .map(|i| i.id.clone())
            .chain(first_screen.iter().map(|s| (*s).to_string()))
            .collect();
        assert_eq!(merged, vec!["x", "a", "b"], "首屏两条必须原样保留");
    }

    /// 空列表（首屏什么都没搜到）时，升级结果全部算新条目。
    #[test]
    fn empty_first_screen_takes_everything() {
        let extra = vec![item("x"), item("y")];
        let (fresh, selected) = plan_merge(&shown(&[]), 0, extra).unwrap();
        assert_eq!(ids(&fresh), vec!["x", "y"]);
        assert_eq!(selected, 0);
    }

    /// 升级结果为空（模型把范围收得太窄，一条都没匹配上）：
    /// 不得改动已渲染的列表。
    #[test]
    fn empty_upgrade_leaves_the_list_alone() {
        assert!(plan_merge(&shown(&["a", "b"]), 0, Vec::new()).is_none());
    }
}
