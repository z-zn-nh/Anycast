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

    pub fn time_upper_bound(&self) -> Option<i64> {
        if self.time_preset == "range" {
            self.custom_end_time.map(|e| e + 86_399)
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
    ClipboardItemAdded { item: SearchItemModel },
    IndexProgress { files: u64, content_files: u64, done: bool },
    ToastMessage { text: String, icon: String },
    WakeRequested,
}
