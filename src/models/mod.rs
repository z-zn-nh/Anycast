//! 前后端共享数据契约（对应《前后端交互契约与后端Agent开发交接文档》第 2 节）。

use serde::{Deserialize, Serialize};

/// 条目类型：App | File | Folder | Clipboard | Command
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    App,
    #[default]
    File,
    Folder,
    Clipboard,
    Command,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemType::App => "app",
            ItemType::File => "file",
            ItemType::Folder => "folder",
            ItemType::Clipboard => "clipboard",
            ItemType::Command => "command",
        }
    }

    pub fn parse(s: &str) -> ItemType {
        match s {
            "app" => ItemType::App,
            "folder" => ItemType::Folder,
            "clipboard" | "clip" => ItemType::Clipboard,
            "command" => ItemType::Command,
            _ => ItemType::File,
        }
    }

    /// 分类栏归属：应用 / 文件（含文件夹）/ 剪贴板
    pub fn category(&self) -> &'static str {
        match self {
            ItemType::App | ItemType::Command => "应用",
            ItemType::File | ItemType::Folder => "文件",
            ItemType::Clipboard => "剪贴板",
        }
    }
}

/// 搜索条目模型 `SearchItemModel`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct SearchItemModel {
    pub id: String,
    pub item_type: ItemType,
    pub title: String,
    pub subtitle: String,
    pub full_path: String,
    pub icon_name: String,
    pub badge: String,
    pub is_pinned: bool,
    pub score: f32,
    pub size_bytes: Option<u64>,
    pub modified_time: Option<i64>,
    pub action_hint: String,
    /// 分组标题（最近使用 / 应用 / 文件 / 剪贴板 / 智能匹配）
    pub section: String,
    /// 智能搜索匹配原因
    pub reason: String,
    /// 剪贴板二级类型：text | code | url
    pub sub_type: String,
    /// 预览内容（剪贴板正文 / 文件片段）
    pub preview: String,
    /// 最近使用时间戳（秒）
    pub last_used: Option<i64>,
}

/// 快捷直达全局绑定模型 `HotkeyBindingModel`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct HotkeyBindingModel {
    pub id: String,
    pub name: String,
    pub target_path: String,
    pub hotkey: String,
    pub modifiers: u32,
    pub vk_code: u32,
    pub item_type: String,
    pub enabled: bool,
}

/// 多维筛选范围模型 `SearchScopeFilter`
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchScopeFilter {
    /// "all" | "today" | "3days" | "7days" | "week" | "30days" | "month" | "year" | "range"
    pub time_preset: String,
    pub custom_start_time: Option<i64>,
    pub custom_end_time: Option<i64>,
    /// "all" | "app" | "document" | "code" | "media" | "archive" | "clipboard" | "folder"
    pub type_category: String,
    /// "all" | "drive-c" | "drive-d" | "desktop" | "downloads" | "documents" | "custom"
    pub location_scope: String,
    pub custom_directory: Option<String>,
}

impl SearchScopeFilter {
    pub fn is_active(&self) -> bool {
        (!self.time_preset.is_empty() && self.time_preset != "all")
            || (!self.type_category.is_empty() && self.type_category != "all")
            || (!self.location_scope.is_empty() && self.location_scope != "all")
    }

    /// 解析时间下限（Unix 秒）。`today_start` 为本地时区今日零点。
    pub fn time_lower_bound(&self, now: i64, today_start: i64) -> Option<i64> {
        const DAY: i64 = 86_400;
        match self.time_preset.as_str() {
            "today" => Some(today_start),
            "3days" => Some(now - 3 * DAY),
            "7days" | "week" => Some(now - 7 * DAY),
            "30days" | "month" => Some(now - 30 * DAY),
            "year" => Some(now - 365 * DAY),
            "range" => self.custom_start_time,
            _ => None,
        }
    }

    /// 解析时间上限（Unix 秒）。
    ///
    /// ⚠️ **`custom_end_time` 的约定是「当天 23:59:59」，即上限本身已经含当天。**
    ///
    /// 这里曾经写过 `e + 86_399`，是给日历选择器的 `to_ts(e)`（当天 **零点**）
    /// 打的补丁 —— 但 `search::parse_intent` 传进来的 `day_range()` 已经是
    /// 当天 23:59:59，再补一次就**多算一整天**：实测「昨天」会把今天改过的文件
    /// 也算进来。两处调用方约定不一致，补丁只能打在调用方。
    ///
    /// 回归测试：`tests::time_upper_bound_includes_end_day`
    pub fn time_upper_bound(&self) -> Option<i64> {
        if self.time_preset == "range" {
            self.custom_end_time
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    #[default]
    Fast,
    Smart,
}

impl SearchMode {
    pub fn parse(s: &str) -> SearchMode {
        if s == "smart" {
            SearchMode::Smart
        } else {
            SearchMode::Fast
        }
    }
}

/// 智能搜索意图芯片
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct IntentChip {
    pub key: String,
    pub val: String,
}

/// 一次搜索的完整请求
#[derive(Clone, Debug, Default)]
pub struct SearchRequest {
    pub query: String,
    pub mode: SearchMode,
    pub scope: SearchScopeFilter,
    /// "all" | "file" | "app" | "clipboard"
    pub category: String,
    /// "all" | "text" | "code" | "url"
    pub clip_sub: String,
    pub limit: usize,
}

/// 搜索响应
#[derive(Clone, Debug, Default)]
pub struct SearchResponse {
    pub items: Vec<SearchItemModel>,
    pub elapsed_ms: u32,
    pub intent_chips: Vec<IntentChip>,
    /// 各分类命中数（全部 / 文件 / 应用 / 剪贴板）
    pub counts: [usize; 4],
}

/// 前端发往后端的事件命令
#[derive(Clone, Debug)]
pub enum FrontendEvent {
    QueryChanged { request: SearchRequest },
    ItemActivated { item_id: String },
    TogglePin { item_id: String },
    RegisterHotkey { binding: HotkeyBindingModel },
    UnregisterHotkey { binding_id: String },
    TestHotkeyAction { binding_id: String },
    SaveWindowSize { width: u32, height: u32 },
    ClearCache,
}

/// 后端推向前台的状态通知
#[derive(Clone, Debug)]
pub enum BackendNotification {
    SearchResultsReady { response: SearchResponse },
    RecentItemsReady { items: Vec<SearchItemModel> },
    HotkeyTriggered { binding_id: String, app_name: String },
    HotkeyConflictDetected { hotkey: String, reason: String },
    /// **运行时**注册失败 —— 与 `HotkeyConflictDetected` 不是一回事：
    /// 后者是「你刚保存的没通过校验」（瞬时、绑在保存动作上），
    /// 这个是「这个热键现在真的用不了」（持续状态，可能是开机时被别人先占了）。
    /// `label` 已是人话，如「唤醒快捷键 Alt+Space」。
    HotkeyRegisterFailed { label: String, reason: String },
    ClipboardItemAdded { item: SearchItemModel },
    IndexProgress { files: u64, content_files: u64, done: bool },
    ToastMessage { text: String, icon: String },
    WakeRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    /// 回归：`custom_end_time` 约定为「含当天的 23:59:59」，不能再补一天。
    ///
    /// 这个补丁曾经让「昨天」把今天改过的文件也算进来（见 `time_upper_bound` 注释）。
    #[test]
    fn time_upper_bound_includes_end_day() {
        let end_of_day = 1_700_000_000 / DAY * DAY + DAY - 1;
        let scope = SearchScopeFilter {
            time_preset: "range".into(),
            custom_start_time: Some(end_of_day - 3 * DAY),
            custom_end_time: Some(end_of_day),
            ..Default::default()
        };
        assert_eq!(scope.time_upper_bound(), Some(end_of_day));
    }

    #[test]
    fn time_bounds_are_none_outside_range_preset() {
        let scope = SearchScopeFilter { time_preset: "today".into(), ..Default::default() };
        assert_eq!(scope.time_upper_bound(), None);
        // "today" 的下限由 today_start 决定，与 custom_* 无关
        assert_eq!(scope.time_lower_bound(1_700_000_000, 1_699_900_000), Some(1_699_900_000));
    }

    /// 区间两端都取得到：恰好落在上限的文件不该被滤掉。
    #[test]
    fn range_is_closed_at_both_ends() {
        let start = 1_700_000_000 / DAY * DAY;
        let end = start + DAY - 1;
        let scope = SearchScopeFilter {
            time_preset: "range".into(),
            custom_start_time: Some(start),
            custom_end_time: Some(end),
            ..Default::default()
        };
        let lower = scope.time_lower_bound(end, start).unwrap();
        let upper = scope.time_upper_bound().unwrap();
        assert!(lower <= start, "起点必须落在区间内");
        assert!(upper >= end, "终点必须落在区间内");
    }

    #[test]
    fn is_active_ignores_default_values() {
        let mut scope = SearchScopeFilter { time_preset: "all".into(), type_category: "all".into(), location_scope: "all".into(), ..Default::default() };
        assert!(!scope.is_active());
        scope.type_category = "code".into();
        assert!(scope.is_active());
    }
}
