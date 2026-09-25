# Windows 本地统一入口 — 项目基础设计文档

## 1. 项目概述

### 1.1 项目定位

本项目是一款面向 Windows 的 **Local-first Search & Action Platform（本地优先搜索与操作平台）**。

它不是单纯的启动器，也不是单纯的文件搜索工具，而是将以下内容统一到一个快速入口：

- 应用程序
- 文件
- 文件夹
- 文件内容
- 剪贴板历史
- 收藏 / 固定项目
- 最近使用项目
- 快捷操作
- **快捷直达（专属全局热键直接绑定启动 App / 脚本 / 常用文件）**
- AI / 语义搜索

用户通过一个全局搜索窗口即可完成：

> **搜索 → 定位 → 打开 → 操作**

AI 搜索属于增强能力，而不是产品的核心依赖。

------

# 2. 核心设计理念

## 2.1 Local-first

默认情况下：

**不依赖网络、不启动 AI、不调用云端服务。**

用户按下快捷键后，应尽可能在极短时间内出现搜索窗口，并立即可以进行本地搜索。

默认搜索应该具有类似 Everything / Raycast / TinyCast 的即时响应体验。

------

## 2.2 Fast-first

搜索模式默认：

> ⚡ 极速搜索

主要用于：

- 应用
- 文件
- 文件夹
- 收藏
- 剪贴板
- 最近项目

要求：

- 极低延迟
- 不等待 AI
- 不等待网络
- 不加载大型模型

------

## 2.3 AI as an Enhancement

AI 搜索作为第二种搜索方式：

> ✦ 智能搜索

它可以理解自然语言，并进行：

- 语义搜索
- 文件内容理解
- 自然语言条件解析
- 模糊意图识别
- RAG
- AI 辅助结果排序

例如用户输入：

> 找一下我昨天修改的 EasyNote 文档

AI 可以理解：

```
关键词：EasyNote
类型：文件
修改时间：昨天
```

再例如：

> 找一下记录 EasyNote 数据怎么存的文件

即使文件名没有 `EasyNote`，也可以通过文件内容语义搜索找到相关文件。

------

# 3. UI 参考方向

## 3.1 核心参考

UI 整体体验参考：

**TinyCast**

重点参考：

- 极简搜索入口
- 居中浮动窗口
- 快速唤起
- 搜索框作为核心交互
- 信息层级简洁
- 圆角窗口
- 现代 Windows 风格
- 轻量动画
- 键盘操作优先
- 搜索结果快速切换
- 不占据整个桌面

但是：

> **不要复制 TinyCast UI。**

本项目应该在 TinyCast 的产品形态基础上进行扩展。

------

# 4. 视觉设计方向

整体视觉关键词：

```
Windows 11
TinyCast
Raycast
Arc
Linear
Acrylic
Glass
Blur
Minimal
Modern
Premium
```

视觉应该偏：

> **现代、克制、轻量、具有高级感**

而不是传统：

- Qt 默认控件
- Windows Forms
- 老式 Windows 软件
- 大量按钮
- 大量边框
- 密集的信息面板

------

# 5. Acrylic / Glass 材质

UI 使用现代半透明材质。

目标效果：

```
┌─────────────────────────────────────┐
│                                     │
│     半透明 / 模糊桌面背景           │
│                                     │
│   ┌─────────────────────────────┐   │
│   │ 🔍 搜索应用、文件……         │   │
│   ├─────────────────────────────┤   │
│   │ 结果                         │   │
│   │                              │   │
│   │ 📦 Visual Studio Code       │   │
│   │ 📄 EasyNote                 │   │
│   │ 📁 Projects                 │   │
│   └─────────────────────────────┘   │
│                                     │
└─────────────────────────────────────┘
```

重点：

- Acrylic / Glass
- 背景模糊
- 半透明
- 柔和阴影
- 轻微高光
- 圆角
- 层次感

但必须注意：

> **视觉效果不能以严重影响性能为代价。**

不要每一帧都进行昂贵的全窗口实时 Blur。

------

# 6. Haze 风格

用户希望视觉上具有类似：

> **Haze / Acrylic Material**

的现代材质感觉。

但项目采用：

> **Rust + Slint**

因此不要强制依赖 Android Compose 的 Haze。

应该在 Slint 中实现类似效果：

```
Acrylic Material
        │
        ├── Transparency
        ├── Blur
        ├── Noise
        ├── Highlight
        ├── Shadow
        └── Border
```

最终目标是：

> **视觉效果类似 Haze，而不是技术上必须使用 Haze。**

------

# 7. 主窗口

默认窗口应该是一个：

> **居中的浮动搜索面板**

而不是传统应用窗口。

特点：

- 无传统标题栏
- 无菜单栏
- 无最大化按钮
- 无最小化按钮
- 圆角
- 半透明
- Acrylic
- 阴影
- 居中显示
- 快速出现 / 消失

默认状态：

```
桌面

                ┌──────────────────────────┐
                │ 🔍 搜索应用、文件……      │
                │                          │
                │ 最近                     │
                │                           │
                │ 📁 Projects              │
                │ ◉ Visual Studio Code     │
                │ 📄 EasyNote.md           │
                └──────────────────────────┘
```

------

# 8. 搜索框

搜索框是整个产品最核心的 UI。

设计应该非常突出。

例如：

```
┌────────────────────────────────────────────┐
│ 🔍  搜索应用、文件、剪贴板……       ⚡ ▾    │
└────────────────────────────────────────────┘
```

右侧显示当前搜索模式。

默认：

> ⚡ 极速搜索

点击后可以切换：

```
⚡ 极速搜索
✦ 智能搜索
```

------

# 9. 搜索模式

## 9.1 极速搜索

```
⚡ 极速搜索
```

用于：

- App
- File
- Folder
- Clipboard
- Pin
- Recent

特点：

```
极低延迟
本地
无需 AI
无需网络
```

------

## 9.2 智能搜索

```
✦ 智能搜索
```

用于：

- 语义搜索
- 文件内容搜索
- 自然语言查询
- AI RAG
- 意图理解
- 智能排序

例如：

```
找一下昨天修改的 EasyNote 文件
```

结果：

```
✦ 智能匹配

EasyNote / docs / storage.md

匹配原因：
文件内容提到了 EasyNote 的本地存储机制，
并且文件修改时间为昨天。
```

------

# 10. 搜索结果设计

不要把产品设计成：

```
App
File
Folder
Clipboard
```

四个完全独立的搜索页面。

而应该：

> **统一搜索结果。**

例如用户搜索：

```
docker
```

结果可以是：

```
┌────────────────────────────────────────┐
│ ⚡ 极速搜索                            │
├────────────────────────────────────────┤
│                                        │
│ 应用                                   │
│ ◉ Docker Desktop                       │
│                                        │
│ 文件                                   │
│ 📄 docker-compose.yml                  │
│ 📄 Dockerfile                          │
│                                        │
│ 文件夹                                 │
│ 📁 Docker                              │
│                                        │
│ 剪贴板                                 │
│ 📋 docker compose up -d                │
│                                        │
└────────────────────────────────────────┘
```

这样才能真正形成：

> **统一入口**

------

# 11. 搜索结果卡片

每个结果应该具有：

```
Icon
Title
Subtitle
Type
Path / Metadata
Action
```

例如：

```
┌─────────────────────────────────────────┐
│  ◉  Visual Studio Code                  │
│     应用                                │
│                                  Enter  │
└─────────────────────────────────────────┘
```

文件：

```
┌─────────────────────────────────────────┐
│  📄  EasyNote.md                        │
│      E:\Projects\EasyNote\docs          │
└─────────────────────────────────────────┘
```

剪贴板：

```
┌─────────────────────────────────────────┐
│  📋  npm install react                  │
│      剪贴板 · 2小时前                   │
└─────────────────────────────────────────┘
```

------

# 12. 键盘优先

产品应该高度支持键盘。

例如：

```
↑ ↓
```

切换结果。

```
Enter
```

打开。

```
Esc
```

关闭。

```
Ctrl + Enter
```

执行特殊操作。

```
Ctrl + P
```

固定项目。

```
Ctrl + C
```

复制。

具体快捷键可以在后续开发阶段调整。

------

# 13. Pin / 收藏系统

项目需要一个统一的：

> **Pin System**

可以固定：

- App
- File
- Folder
- Clipboard
- Command
- URL
- Snippet

例如：

```
Pinned

◉ VS Code
📁 Projects
📄 EasyNote.md
📋 npm install
🌐 GitHub
```

Pin 不应该仅仅是“收藏文件”。

而是：

> **用户自己的快速操作面板。**

------

# 13.1 快捷直达系统 (Direct App & File Hotkey Binding)

除了在搜索框中键入关键词或在置顶栏点击外，高频重度用户需要：

> **无需呼出搜索窗口，直接按下专属系统级全局快捷键，瞬间启动或激活特定 App、脚本或文件。**

### 核心能力设计：

1. **一对一全局热键映射**：
   - 允许用户为任意应用程序（如 Windows Terminal、VS Code、Chrome）、工程项目文件夹、Markdown 文档或自动化脚本（`.bat` / `.ps1`）分配独立专属热键（例如 `Alt + C` 直达控制台，`Alt + T` 直达任务管理器，`Alt + P` 直达项目工程）；
2. **后台静默拦截与极速调起**：
   - 由 Rust 后台 Daemon 通过 Windows `RegisterHotKey` API 注册系统全局监听；
   - 触发时直接通过 Windows `ShellExecuteExW` 调起进程，或通过 `SetForegroundWindow` 将已在后台运行的对应窗口平滑前置聚焦，0 延迟、0 唤窗损耗；
3. **设置中心可视化管理与交互录制**：
   - 在设置面板中提供独立的“⌨️ 快捷直达”页面；
   - 支持可视化绑定列表、3D 键帽展示、单项热键启用/禁用开关、一键测试运行（▶）与快速移除（🗑️）；
   - 支持交互式键盘录制：点击键帽即进入脉冲录制状态，直接按下目标物理组合键即刻完成捕获与持久化；
   - 智能按键冲突检测与建议机制（如提示系统常用键占用，推荐使用 `Alt + [Key]` 或 `Ctrl + Shift + [Key]` 组合）。

------

# 14. Clipboard History

增加剪贴板历史。

例如：

```
📋 Clipboard

npm install rust-analyzer

https://github.com/...

Hello World

图片

{
    "name": "EasyNote"
}
```

支持：

- 历史记录
- 搜索
- 删除
- Pin
- 快速复制
- 文本预览
- 图片预览

Pinned Clipboard：

```
📌 Pinned

API Endpoint
Git Commands
常用代码
```

------

# 15. Recent

显示最近使用内容。

例如：

```
最近

◉ Visual Studio Code
📄 EasyNote.md
📁 Projects
📋 npm install
```

根据使用频率自动排序。

------

# 16. 文件内容搜索

这是项目非常重要的能力。

不能只搜索：

```
文件名
```

还需要搜索：

```
文件内容
```

例如：

```
EasyNote 数据库存储在哪里？
```

能够找到：

```
E:\Projects\EasyNote\docs\architecture.md
```

即使：

```
文件名 = architecture.md
```

也可以通过内容找到。

------

# 17. 搜索架构

逻辑上分成：

```
                Search
                   │
          ┌────────┴────────┐
          │                 │
       Fast Search       AI Search
          │                 │
       Local Index       Embedding
          │                 │
       SQLite/FTS5       Vector Search
          │                 │
          └────────┬────────┘
                   │
                Ranking
                   │
                Results
```

核心接口设计：

```
ISearchProvider

├── FastSearchProvider
└── SemanticSearchProvider
```

UI 不应该依赖具体搜索实现。

这样以后即使更换 AI 模型，也不需要重新设计 UI。

------

# 18. 文件索引

不要每次搜索都扫描磁盘。

应该：

```
Windows File System
        │
        ▼
Incremental Index
        │
        ▼
SQLite
        │
        ├── File Metadata
        ├── Filename
        ├── Path
        ├── Modified Time
        └── Content Index
```

后续可以利用：

> Windows NTFS USN Journal

实现增量更新。

------

# 19. SQLite

MVP 阶段优先：

```
SQLite
```

承担：

- 文件索引
- 搜索索引
- Clipboard
- Pin
- Recent
- 用户配置
- 搜索历史

全文搜索：

```
SQLite FTS5
```

AI 向量搜索后续可以考虑：

```
sqlite-vec
```

MVP 不需要一开始就引入：

```
Qdrant
Milvus
Weaviate
```

避免架构过重。

------

# 20. AI / RAG

AI 不应该侵入基础搜索。

推荐：

```
User Query
    │
    ▼
Intent Understanding
    │
    ├── Keyword
    ├── File Type
    ├── Date
    ├── Location
    └── Semantic Meaning
           │
           ▼
      Vector Search
           │
           ▼
      Content Search
           │
           ▼
        Rerank
           │
           ▼
        Results
```

AI 是：

> **第二层搜索能力**

而不是基础搜索引擎。

------

# 21. 技术架构

最终选择：

> **Rust + Slint**

建议架构：

```
┌───────────────────────────────────────┐
│               Slint UI                │
│                                       │
│ Search / Result / Clipboard / Pin     │
└───────────────────┬───────────────────┘
                    │
                    ▼
┌───────────────────────────────────────┐
│             Application Core          │
│                                       │
│ Search Manager                        │
│ Clipboard Manager                     │
│ Pin Manager                           │
│ Recent Manager                        │
│ Command Manager                       │
│ Hotkey Direct Manager (App热键直达)   │
└───────────────────┬───────────────────┘
                    │
                    ▼
┌───────────────────────────────────────┐
│               Rust Core               │
│                                       │
│ File Index                            │
│ SQLite / FTS5                         │
│ Windows API (ShellExecute/DWM)        │
│ USN Journal                           │
│ Clipboard API                         │
│ Global Hotkey Engine (Win32 API)      │
└───────────────────┬───────────────────┘
                    │
                    ▼
┌───────────────────────────────────────┐
│             AI / Semantic             │
│                                       │
│ Embedding                             │
│ Vector Search                         │
│ RAG                                   │
│ Reranker                              │
└───────────────────────────────────────┘
```

------

# 22. Slint UI 组件规划

建议 Gemini 在设计 UI 时提前按照组件化思路设计。

核心组件：

```
AppWindow
SearchWindow
SearchBar
SearchModeSelector

SearchResultList
SearchResultItem

AppItem
FileItem
FolderItem
ClipboardItem
PinItem
RecentItem

SectionHeader

AcrylicSurface
GlassSurface

ContextMenu
CommandMenu
PreviewPanel

Toast
Tooltip
Dialog
SettingsPanel

HotkeyBindingCard       // 快捷直达已绑定卡片槽（3D键帽+幽灵动作）
HotkeyRecordModal       // 快捷键脉冲录制捕获浮层
SettingsExpander        // WinUI 3 风格组群折叠卡片容器
```

------

# 23. 动画设计

动画不能太多。

整体原则：

> **快、轻、克制。**

主要动画：

### 窗口出现

```
Opacity
0 → 1

Scale
0.97 → 1
```

### 搜索结果

轻微：

```
Opacity
Translate
```

### 搜索模式切换

```
Indicator slide
```

### Hover

```
Background
Opacity
Icon
```

避免：

- 大幅度缩放
- 弹跳
- 复杂粒子
- 持续动画
- 过度 Blur

产品应该更接近：

> TinyCast / Raycast / Linear

而不是：

> 炫技型 UI Demo

------

# 24. 产品窗口状态

需要设计至少三种状态。

### 空闲状态

```
┌────────────────────────────┐
│ 🔍 搜索应用、文件……        │
│                            │
│ 最近                         │
│ ◉ VS Code                   │
│ 📁 Projects                 │
│ 📄 EasyNote                 │
└────────────────────────────┘
```

### 搜索状态

```
┌────────────────────────────┐
│ 🔍 docker             ⚡    │
├────────────────────────────┤
│ 应用                        │
│ ◉ Docker Desktop            │
│                            │
│ 文件                        │
│ 📄 Dockerfile               │
│ 📄 docker-compose.yml       │
└────────────────────────────┘
```

### AI 搜索状态

```
┌────────────────────────────┐
│ ✦ 找一下昨天的 EasyNote 文件 │
├────────────────────────────┤
│ ✦ 智能匹配                  │
│                            │
│ 📄 architecture.md          │
│    昨天修改 · 内容匹配      │
│                            │
│ 📄 storage.md               │
│    内容语义匹配             │
└────────────────────────────┘
```

------

# 25. 设置页面

设置不要成为 MVP 的核心。

第一阶段只需要：

```
Settings

General (常规设置)
├── Global Wake Hotkey (唤醒搜索窗口，默认 Alt+Space)
├── Launch on Startup (开机自启)
└── Search Behavior (失焦自动隐藏、双击执行)

Hotkeys (快捷直达 - App与文件热键绑定)
├── Global Hotkey Engine Toggle (系统热键监听总开关)
├── Hotkey Bindings List (已绑定列表：App/文件/路径/3D键帽/独立开关/▶测试运行/🗑️删除)
├── + Add Hotkey Binding (从最近使用快速选取或手动浏览指定路径)
├── Interactive Key Recording Box (按键脉冲录制捕获与去抖持久化)
└── Conflict Detection & Recommendations (防冲突拦截与推荐组合键)

Appearance (外观设置)
├── Acrylic Opacity Presets (透光度档位)
├── Wallpaper Switcher (真实桌面/Win11官方/自定义壁纸)
└── Theme Mode (深色/浅色模式切换)

Search (搜索与索引)
├── Indexed Locations (NTFS USN 索引目录)
├── Excluded Locations (排除路径)
└── Search Mode (极速 / AI智能)

Clipboard (剪贴板历史)
├── Enable History (记录开关)
├── History Limit (容量上限)
└── Auto Delete (安全过期)

AI (AI语义引擎)
├── Enable AI Search (智能语义开关)
├── Model (本地量化模型 bge-small-zh)
├── sqlite-vec Extension (向量加速)
└── Lazy Loading (按需动态唤醒)
```

------

# 26. MVP

第一版不要一次把所有功能做完。

### Phase 1

```
Global Hotkey
        ↓
Search Window
        ↓
App Search
        ↓
File Search
        ↓
Folder Search
```

### Phase 2

```
Clipboard History
Pin
Recent
Search Ranking
```

### Phase 3

```
File Content Search
FTS5
USN Journal
```

### Phase 4

```
AI Search
Semantic Search
Embedding
RAG
```

### Phase 5

```
Advanced Commands
Plugin System
More Windows Integration
```

------

# 27. 性能目标

项目定位是：

> **轻量级 Windows 常驻工具**

因此性能应该作为一级需求。

目标方向：

```
启动速度：极快

唤醒速度：极快

默认搜索：毫秒级响应

Idle RAM：尽量低

CPU：常驻接近 0%

AI：默认不启动

索引：后台低优先级运行
```

不要为了 UI 效果牺牲：

- 启动速度
- 内存
- 搜索速度
- 输入响应

------

# 28. Gemini UI 设计任务

这部分可以直接作为 Gemini 的任务说明：

> **你现在不是负责完整开发这个项目。**
>
> 你的第一阶段任务是：
>
> **设计整个 Windows Local-first Search & Action Platform 的 UI/UX。**
>
> UI 主要参考 TinyCast 的产品形态，同时吸收 Raycast、Arc、Linear、Windows 11 的现代设计语言。
>
> 不要复制任何现有产品。
>
> 重点设计：
>
> 1. 主搜索窗口
> 2. 空闲状态
> 3. 普通搜索状态
> 4. AI 搜索状态
> 5. App 搜索结果
> 6. File 搜索结果
> 7. Folder 搜索结果
> 8. Clipboard 搜索结果
> 9. Pin
> 10. Recent
> 11. Context Menu
> 12. Preview
> 13. Search Mode Selector
> 14. Settings
>
> UI 必须适合 Windows 桌面环境。
>
> 视觉方向：
>
> ```
> TinyCast
> Raycast
> Arc
> Linear
> Windows 11
> Acrylic
> Glass
> Blur
> Minimal
> Premium
> ```
>
> 使用现代 Acrylic / Glass Material。
>
> 视觉上可以参考 Haze 的材质表现，但本项目技术栈不是 Compose，因此不要直接依赖 Haze。
>
> UI 必须考虑：
>
> - 键盘操作
> - 快速搜索
> - 高信息密度但不拥挤
> - 轻量动画
> - Hover
> - Focus
> - Selection
> - Loading
> - Empty State
> - Error State
> - AI 搜索状态
>
> 最终需要形成一套完整的 UI Design System，而不是只设计一个搜索框。

------

# 29. 后续 Agent 开发原则

Gemini 完成 UI 设计后，再交给其他 Agent。

后续 Agent 必须遵守：

```
UI Design
     ↓
Slint Implementation
     ↓
Rust Core
     ↓
Windows Integration
     ↓
Search Engine
     ↓
Clipboard
     ↓
AI
```

不要反过来：

```
先写代码
↓
最后随便做 UI
```

------

# 30. 最重要的一条架构原则

这个项目最终应该保持：

```
                UI
                 │
                 ▼
        Application Layer
                 │
        ┌────────┴────────┐
        ▼                 ▼
   Fast Search         AI Search
        │                 │
        └────────┬────────┘
                 ▼
             Data Layer
                 │
        ┌────────┼────────┐
        ▼        ▼        ▼
      Files  Clipboard  SQLite
```

**UI 不应该知道底层搜索到底是怎么实现的。**

这样以后：

```
Everything
↓
自建索引
↓
SQLite
↓
FTS5
↓
Vector Search
↓
RAG
↓
本地模型
↓
云端模型
```

都可以替换，而不会破坏整个 UI。

------

## 最终产品一句话

> **一个以 TinyCast 为交互参考、以 Rust + Slint 为技术基础、以本地高速搜索为核心，并逐步加入文件内容搜索、剪贴板、收藏和 AI 语义搜索的 Windows 本地统一入口。**