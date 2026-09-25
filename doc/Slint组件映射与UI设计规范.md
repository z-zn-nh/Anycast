# Anycast UI 设计系统规范与 Slint 组件映射蓝图

> 对应需求文档：《Windows 本地统一入口 — 项目基础设计文档.md》第 22 节与第 28 节  
> 用途：供后续负责 **Rust + Slint** 实施的 Agent 查阅，作为 Slint UI 组件设计与 Rust Core 绑定的标准蓝图。

---

## 1. 材质与视觉 Token (Design Tokens)

### 1.1 亚克力材质 (Acrylic / Glass Material)
在 Windows 平台，原生推荐通过 `windows-sys` 或 `raw-window-handle` 接入 `DwmSetWindowAttribute` 调用 Windows 11 DWM 的 Acrylic / Mica 效果；在 Slint UI 层叠加细微杂色、微光外边框与柔和投影：

| Token 名称 | 浅色模式 (Light) | 深色模式 (Dark) | 说明 |
| :--- | :--- | :--- | :--- |
| `surface-acrylic` | `rgba(248, 249, 251, 0.82)` | `rgba(24, 26, 32, 0.78)` | 窗口半透明背景 |
| `surface-glass` | `rgba(255, 255, 255, 0.60)` | `rgba(255, 255, 255, 0.04)` | 内嵌卡片/气泡玻璃背景 |
| `border-acrylic` | `rgba(0, 0, 0, 0.08)` | `rgba(255, 255, 255, 0.08)` | 窗口外边框 |
| `border-highlight` | `rgba(255, 255, 255, 0.70)` | `linear-gradient(135deg, #ffffff2e, #ffffff08)` | 顶部/边框微光反射 |
| `blur-gaussian` | `blur(36px) saturate(180%)` | `blur(36px) saturate(180%)` | 高斯模糊与饱和度提升 |
| `corner-radius-window` | `16px` | `16px` | Windows 11 现代圆角 |

### 1.2 颜色与状态体系

- **极速搜索强调色 (Fast Search)**：`#3b82f6` (Fluent Blue)，悬浮 `#60a5fa`，光晕 `rgba(59, 130, 246, 0.35)`
- **AI 语义搜索强调色 (AI Semantic)**：`#a855f7` (Sparkle Violet)，悬浮 `#c084fc`，渐变 `#a855f7 → #6366f1 → #3b82f6`
- **文本层级**：
  - Primary: `#f3f4f6` (深色) / `#18181b` (浅色)
  - Secondary: `#9ca3af` (深色) / `#52525b` (浅色)
  - Tertiary: `#6b7280` (深色) / `#71717a` (浅色)

---

## 2. 第 22 节规划的 22 个 Slint 核心组件结构与契约

| # | 组件名称 | 文件建议 | 职责与属性契约 |
| :-: | :--- | :--- | :--- |
| **1** | `AppWindow` | `app_window.slint` | 全局宿主窗口，无传统边框标题栏，无最大化/最小化，居中浮动，透明背景穿透 |
| **2** | `SearchWindow` | `search_window.slint` | 核心搜索面板容器，默认宽 680px，展开预览面板时平滑扩展至 960px |
| **3** | `SearchBar` | `search_bar.slint` | 输入框、清除按钮、动态图标（极速 🔍 / AI ✦）与模式选择器 |
| **4** | `SearchModeSelector` | `mode_selector.slint` | 极速 (⚡) 与智能 (✦) 滑块胶囊，支持 Tab 热键联动 |
| **5** | `SearchResultList` | `result_list.slint` | 分组滚动容器，承载各类条目与 SectionHeader |
| **6** | `SearchResultItem` | `result_item.slint` | 条目基类（包含图标容器、标题高亮、副标题、徽标、Enter 操作提示） |
| **7** | `AppItem` | `items/app_item.slint` | 应用程序条目（EXE路径、版本信息、立即启动） |
| **8** | `FileItem` | `items/file_item.slint` | 文件条目（文件类型图标、路径、大小、修改时间） |
| **9** | `FolderItem` | `items/folder_item.slint` | 文件夹条目（子项计数、打开目录） |
| **10** | `ClipboardItem` | `items/clip_item.slint` | 剪贴板条目（富文本/代码/图片，Pin 标识，相对时间） |
| **11** | `PinItem` | `items/pin_item.slint` | 置顶固定条目（金星/图钉标记，快捷面板） |
| **12** | `RecentItem` | `items/recent_item.slint` | 最近使用条目（按使用频率自动加权排序） |
| **13** | `SectionHeader` | `section_header.slint` | 分组标题条（大写轻量文字 + 子项数量徽标） |
| **14** | `AcrylicSurface` | `surfaces/acrylic.slint` | 基础亚克力材质矩形封装 |
| **15** | `GlassSurface` | `surfaces/glass.slint` | 玻璃态微质感容器（用于按钮、内嵌卡片） |
| **16** | `ContextMenu` | `menus/context_menu.slint` | 浮动上下文菜单（支持右键呼出与快捷键） |
| **17** | `CommandMenu` | `menus/command_menu.slint` | `Ctrl+K` 快速操作调色板 |
| **18** | `PreviewPanel` | `preview_panel.slint` | 右侧详情预览分栏（元数据清单 + 代码/Markdown预览 + 快捷操作按钮组） |
| **19** | `Toast` | `toast.slint` | 全局浮动轻量提示（复制成功、固定成功等） |
| **20** | `Tooltip` | `tooltip.slint` | 按钮与热键悬浮微型提示 |
| **21** | `Dialog` | `dialog.slint` | 模态确认对话框（删除、授权等） |
| **22** | `SettingsPanel` | `settings_panel.slint` | 设置中心（常规、快捷直达、外观、索引、剪贴板、AI模型参数配置） |
| **23** | `HotkeyBindingCard` | `items/hotkey_card.slint` | 快捷直达卡片槽（3D键帽、独立开关、幽灵测试与删除按钮） |
| **24** | `HotkeyRecordBox` | `controls/hotkey_record.slint` | 物理按键脉冲录制捕获器（支持修饰键组合捕获与去抖） |
| **25** | `SearchFilterShelf` | `search/filter_shelf.slint` | 搜索栏内联微晶筛选抽屉（0.22s 阻尼展开收起） |
| **26** | `CalendarRangePicker` | `controls/calendar_picker.slint` | 带起止范围的高保真 Windows 11 日历选择器 |
| **27** | `LocationFilterPopup` | `search/location_popup.slint` | 路径多级选择与进入文件夹查找浮层 |

---

## 3. Rust Core 与 Slint 数据模型接口契约示例

```rust
// 搜索条目映射契约 (src/models/search.rs)
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SearchItemModel {
    pub id: String,
    pub item_type: String,       // "app" | "file" | "folder" | "clipboard"
    pub title: String,
    pub subtitle: String,
    pub icon_name: String,
    pub badge: String,
    pub is_pinned: bool,
    pub preview_content: Option<String>,
    pub action_hint: String,     // "Enter" | "复制"
}

// 快捷直达全局热键绑定模型 (src/models/hotkey.rs)
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HotkeyBindingModel {
    pub id: String,
    pub name: String,
    pub target_path: String,     // EXE 路径 / 文件夹路径 / 脚本路径
    pub hotkey: String,          // e.g. "Alt+C", "Ctrl+Shift+T"
    pub item_type: String,       // "app" | "folder" | "file" | "script"
    pub enabled: bool,
}

// 全局热键直达管理调度引擎契约 (src/hotkey/mod.rs)
pub trait IHotkeyManager: Send + Sync {
    fn register_all(&mut self, bindings: &[HotkeyBindingModel]) -> Result<(), String>;
    fn register_binding(&mut self, binding: &HotkeyBindingModel) -> Result<(), String>;
    fn unregister_binding(&mut self, id: &str) -> Result<(), String>;
    fn trigger_action(&self, id: &str) -> Result<(), String>; // ShellExecuteExW 或 SetForegroundWindow
    fn detect_conflict(&self, hotkey: &str) -> Option<String>;
}

// 统一搜索接口契约 (src/search/mod.rs)
pub trait ISearchProvider: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Vec<SearchItemModel>;
}

// 极速搜索 (SQLite FTS5 + USN)
pub struct FastSearchProvider { ... }

// 智能语义搜索 (Vector Embedding + RAG)
pub struct SemanticSearchProvider { ... }
```

---

## 4. 键盘交互事件映射表 (Keyboard-first)

| 按键 | 触发行为 |
| :--- | :--- |
| `↑` / `↓` | 在搜索结果列表项中上下移动高亮光标，右侧预览面板自动联动刷新 |
| `Enter` | 执行选中项的主操作（应用启动 / 文件打开 / 剪贴板复制） |
| `Tab` | 极速模式 (⚡) 与智能模式 (✦) 快速平滑切换；选中文件夹条目时快速将其锁定为当前筛选作用域 |
| `Esc` | 依次执行：关闭录制/日历/位置浮层 → 关闭设置面板 → 清空搜索词 → 收起主窗口 |
| `Ctrl + K` | 呼出当前选中项的快速操作菜单 (ContextMenu / CommandMenu) |
| `Ctrl + P` | 快速固定 / 取消固定当前项到 Pinned 快捷访问栏 |
| `Ctrl + C` | 快速复制当前项完整磁盘路径或剪贴板文本内容 |
| `Ctrl + O` | 在 Windows 资源管理器中高亮定位选中文件 |
| `[自定义全局热键]` | （如 `Alt + C`、`Alt + T`）后台直接拦截，极速启动绑定的 App/脚本或前置激活已有窗口 |
