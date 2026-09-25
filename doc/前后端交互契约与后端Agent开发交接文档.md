# Anycast 前后端交互契约与后端 Agent 开发交接文档

> **对接目标**：面向负责 Rust 核心工程与后端服务实施的 **Backend Agent / Rust 工程师**  
> **前端基准**：基于 [`doc/前端开发规范与实施文档.md`](file:///d:/Anycast/doc/前端开发规范与实施文档.md) 及原型 [`d:/Anycast/index.html`](file:///d:/Anycast/index.html)  
> **技术栈环境**：Rust 1.75+、Tokio 异步运行时、Windows-sys / Windows-rs 原生 Win32 API、SQLite (FTS5 + sqlite-vec)

---

## 1. 系统分工边界与职责矩阵

本项目严格贯彻 **“UI 不知道搜索与系统的具体底层实现，后端不依赖特定 UI 框架”** 的解耦架构：

| 模块领域 | 前端职责 (Slint UI & Glue) | 后端职责 (Rust Core Backend) |
| :--- | :--- | :--- |
| **主窗口呈现** | 亚克力材质层叠、860×560 视口渲染、缩放手柄 | Win32 宿主窗体创建、DWM 透明穿透注入、失焦隐藏 |
| **搜索与输入** | 搜索词输入、Tab 键模式切换、微晶筛选抽屉展开 | 极速搜索引擎、USN Journal 毫秒监控、FTS5 全文索引 |
| **快捷直达 (App绑定)** | 3D 键帽展示、物理按键脉冲录制、绑定列表增删 | 全局 `RegisterHotKey` 监听、进程拉起与已有窗口前置聚焦 |
| **置顶展架** | 单行卡片轮播、横向滚轮映射、0 抖动折叠收拢 | 置顶数据加权、右键 Pin 状态持久化存储 |
| **剪贴板** | 历史列表展示、二级细分胶囊切换、条目预览 | Windows 剪贴板监听器、数据去重与脱敏、本地加密缓存 |
| **AI 语义搜索** | 自然语言解析提示条渲染、智能模式结果高亮 | 本地 `bge-small-zh` 模型按需加载、sqlite-vec 向量检索 |
| **偏好设置** | 纯双栏设置面板、开关滑动、壁纸与主题切换 | 配置文件读写（`config.json` / SQLite）、系统自启注入 |

---

## 2. 核心数据结构与 Serde 契约 (Data Models)

后端需定义并在前后端共享以下核心数据模型（建议放置于 `src/models/` 目录）：

### 2.1 搜索条目模型 (`SearchItemModel`)
```rust
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SearchItemModel {
    pub id: String,                  // 唯一标识 (UUID 或 路径哈希)
    pub item_type: ItemType,         // App | File | Folder | Clipboard | Pinned | Script
    pub title: String,               // 主标题 (应用名 / 文件名)
    pub subtitle: String,            // 副标题 (相对路径 / 修改时间)
    pub full_path: String,           // 绝对磁盘路径或完整剪贴板内容
    pub icon_name: String,           // 图标标识符 (用于 Slint 矢量图标映射)
    pub badge: String,               // 类型微标 (如 "EXE", "MD", "Rust", "2h前")
    pub is_pinned: bool,             // 是否已固定在置顶展架
    pub score: f32,                  // 匹配加权评分 (用于结果排序)
    pub size_bytes: Option<u64>,     // 文件大小 (字节)
    pub modified_time: Option<i64>,  // Unix 时间戳 (秒)
    pub action_hint: String,         // 默认操作提示 (如 "Enter 打开", "复制")
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ItemType {
    App,
    File,
    Folder,
    Clipboard,
    Command,
}
```

### 2.2 快捷直达全局绑定模型 (`HotkeyBindingModel`)
```rust
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HotkeyBindingModel {
    pub id: String,                  // 绑定唯一 UUID
    pub name: String,                // 规则名称 (如 "Windows Terminal", "工程文档")
    pub target_path: String,         // 目标 EXE 路径 / 文件夹路径 / 脚本路径
    pub hotkey: String,              // 规范化按键字符串 (如 "Alt+C", "Ctrl+Shift+T")
    pub modifiers: u32,              // Win32 MOD_ALT | MOD_CONTROL | MOD_SHIFT
    pub vk_code: u32,                // Win32 虚拟键码 (如 'C' = 0x43)
    pub item_type: String,           // "app" | "folder" | "file" | "script"
    pub enabled: bool,               // 独立启用/禁用开关
}
```

### 2.3 多维筛选范围模型 (`SearchScopeFilter`)
```rust
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchScopeFilter {
    pub time_preset: String,         // "all" | "today" | "3days" | "week" | "month" | "range"
    pub custom_start_time: Option<i64>, // 自定义起始时间戳
    pub custom_end_time: Option<i64>,   // 自定义结束时间戳
    pub type_category: String,       // "all" | "app" | "doc" | "code" | "image" | "media" | "archive"
    pub location_scope: String,      // "all" | "drives" | "projects" | "custom_path"
    pub custom_directory: Option<String>, // 指定文件夹路径 (如 "D:\Projects\Anycast")
}
```

---

## 3. 快捷直达引擎开发规范 (Direct Hotkey Daemon)

该模块是本次重构的核心新增系统，后端 Agent 需严格遵循以下机制开发：

### 3.1 监听与消息泵
1. **Windows API 注册**：
   - 使用 `windows-sys::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey`；
   - 格式：`RegisterHotKey(hwnd, atom_id, modifiers | MOD_NOREPEAT, vk_code)`；
   - 为每个绑定的 ID 分配唯一的 `atom_id`（建议映射在 `0x1000 ~ 0xBFFF` 用户热键段）；
2. **后台无窗消息循环**：
   - 在专用的 OS 线程中维护 `GetMessageW` 消息泵，实时拦截 `WM_HOTKEY`；
   - 收到消息后，匹配 `wParam (atom_id)`，异步触发执行动作，**完全不打断前台任何全屏程序或工作窗口**。

### 3.2 极速调起与前置激活 (Launch or Bring to Front)
当热键触发时，后端不得简单无脑调用 `std::process::Command` 启动新进程，而应执行**防多开与前置激活策略**：

```rust
pub fn execute_hotkey_action(target_path: &str) -> Result<(), String> {
    // 1. 检查目标是否已有活跃运行中的进程与主窗口
    if let Some(hwnd) = find_running_window_by_path(target_path) {
        unsafe {
            // 已在运行：将其平滑恢复并置顶前台
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
        return Ok(());
    }

    // 2. 未在运行：通过 ShellExecuteExW 极速拉起
    use std::os::windows::ffi::OsStrExt;
    let wide_path: Vec<u16> = std::ffi::OsStr::new(target_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_DEFAULT,
            hwnd: 0,
            lpVerb: std::ptr::null(),
            lpFile: wide_path.as_ptr(),
            lpParameters: std::ptr::null(),
            lpDirectory: std::ptr::null(),
            nShow: SW_SHOWNORMAL,
            ..std::mem::zeroed()
        };
        ShellExecuteExW(&mut info);
    }
    Ok(())
}
```

### 3.3 冲突检测与热键录制
1. **热键录制捕获**：
   - 当用户在前端进入“键帽脉冲录制”状态时，前端通过回调通知后端临时启动低级键盘钩子（`SetWindowsHookExW(WH_KEYBOARD_LL)`）捕获物理按键（如捕获到用户按下了 <kbd>Alt</kbd> + <kbd>C</kbd>）；
   - 录制完成后立即卸载钩子，避免长驻性能影响；
2. **冲突检测**：
   - 注册前尝试调用 `RegisterHotKey` 预检；若返回 `0` 且 `GetLastError() == ERROR_HOTKEY_ALREADY_REGISTERED`，向前端回传冲突警告（如已存在于微信、QQ 或系统占用），前端呈现场景提示。

---

## 4. 双搜索引擎与数据层实施规范

### 4.1 极速搜索：SQLite FTS5 + NTFS USN Journal
- **NTFS USN Journal 增量索引**：
  - 通过 `FSCTL_READ_USN_JOURNAL` 直接解析 NTFS 元数据日志，毫秒级捕获本地磁盘文件的创建、重命名、移动和删除；
  - 增量变动实时同步至本地 SQLite 数据库，**杜绝开机暴力遍历扫盘**；
- **SQLite FTS5 全文索引**：
  - 维护 `files_fts` 虚拟表，配置 `trigram` 或 `porter` 分词器，支持拼音首字母匹配、模糊中英文前缀搜索；
  - 极速搜索单次响应时延必须低于 **`5ms`**。

### 4.2 AI 智能语义搜索：bge-small-zh + sqlite-vec
- **轻量本地模型**：
  - 采用量化版 `bge-small-zh`（模型体积约 48MB，ONNX Runtime 或 Rust 原生 `candle` 运行）；
- **动态冷启动与懒加载**：
  - 默认空闲状态与极速搜索状态下，**模型完全不载入内存（0MB 内存占用）**；
  - 仅当用户按下 <kbd>Tab</kbd> 切换为智能模式，或查询中包含“昨天”、“上周”、“修改过的代码”等自然语言语义时，异步后台加载模型并调用 `sqlite-vec` 向量检索；
- **智能意图解析**：
  - 自动将自然语言查询解析为结构化过滤（如识别“找一下昨天修改的 EasyNote 文档” → `type: doc, time: yesterday, keyword: EasyNote`）。

---

## 5. 剪贴板历史服务 (`ClipboardDaemon`)

1. **系统监听**：
   - 通过 `AddClipboardFormatListener(hwnd)` 注册系统剪贴板监听，接收 `WM_CLIPBOARDUPDATE`；
2. **数据分类与处理**：
   - 文本与代码：捕获后经哈希去重存入 SQLite；
   - 图片：存入本地专用缓存目录（`~/.anycast/cache/clipboard/`），生成 64×64px 缩略图；
3. **隐私脱敏与过期**：
   - 自动检测常见密码格式并支持配置排除名单；
   - 超过配置上限（默认 500 条）或过期未固定的历史记录定时自动清理。

---

## 6. 前后端异步通信总线 (Slint - Tokio IPC)

前端 Slint 运行在主 UI 线程，后端 Tokio 运行在工作线程池。两者通过强类型通道解耦互通：

```rust
// 前端发往后端的事件命令
pub enum FrontendEvent {
    QueryChanged { query: String, mode: String, filter: SearchScopeFilter },
    ItemActivated { item_id: String },
    TogglePin { item_id: String },
    RegisterHotkey { binding: HotkeyBindingModel },
    UnregisterHotkey { binding_id: String },
    TestHotkeyAction { binding_id: String },
    SaveWindowSize { width: u32, height: u32 },
    ClearCache,
}

// 后端推向前台的状态通知
pub enum BackendNotification {
    SearchResultsReady { items: Vec<SearchItemModel>, elapsed_ms: u32 },
    RecentItemsReady { items: Vec<SearchItemModel> },
    HotkeyTriggered { binding_id: String, app_name: String },
    HotkeyConflictDetected { hotkey: String, reason: String },
    ClipboardItemAdded { item: SearchItemModel },
    ToastMessage { text: String, icon: String },
}
```

后端向 Slint 发送数据时，使用 `slint::invoke_from_event_loop` 确保线程安全无锁派发：
```rust
let handle = slint_app_window.as_weak();
tokio::spawn(async move {
    let results = search_engine.search(&query).await;
    handle.upgrade_in_event_loop(move |window| {
        window.set_search_results(results_into_slint_model(results));
    }).ok();
});
```

---

## 7. 联调验收测试基准

后端 Agent 交付时应针对以下核心用例提供全绿测试证明：

1. **用例 1：全局快捷键 App 直达**
   - 绑定 `Alt + C` 到 `cmd.exe`，最小化 Anycast；
   - 在 Windows 任意窗口下按下 <kbd>Alt+C</kbd>，控制台应在 **50ms 内立即弹出**；再次按下，已打开的控制台应立刻置顶激活。
2. **用例 2：毫秒级文件极速搜索**
   - 在搜索框连续高频输入（如 `docker`），每键入一个字符，结果列表在 **10ms 内丝滑更新**，无输入迟滞。
3. **用例 3：USN 增量感知**
   - 在桌面新建一个文本文件 `test_note.txt`，在 Anycast 中即刻输入 `test_note`，应直接命中，无需手动重建索引。
4. **用例 4：AI 语义懒加载与内存释放**
   - 启动初期常驻内存低于 **35MB**；切换至智能搜索并查询后，内存按需增加；闲置 5 分钟后自动释放模型权重。
