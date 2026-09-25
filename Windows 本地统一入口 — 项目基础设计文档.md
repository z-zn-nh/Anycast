# Windows 本地统一入口 — 项目基础设计文档

> ## 修订记录
>
> **2026-09-24 修订** —— 依据《`doc/检索增强与判断模型接入开发文档.md`》§3 的冲突清单，
> 对下列条目作出修订（用户已裁决「按建议改」）。原条款以 `~~删除线~~` 或「原」标注保留，
> 便于回溯：
>
> | 条目 | 修订内容 |
> | :--- | :--- |
> | §2.1 | 明确「默认走**本地**判断模型，云端增强可选且默认关闭」，Local-first 原则**保留** |
> | §7 | 补充「唤醒路径只剩托盘 + 全局热键」及热键注册失败时的提示要求 |
> | §11 | 补充右键操作菜单（含终端打开、用…打开） |
> | §12 | 补充右键菜单与终端相关快捷键 |
> | §20 | AI 层拆为「本地判断层（默认）+ 本地向量层（可选）+ 云端增强层（可选，默认关）」 |
> | §25 | AI 分组改为本地判断模型为主，新增 Laya / Jev 条目 |
> | §26 | Phase 顺序调整：**检索质量（FTS5 + 中文分词 + 排序）提到判断模型接入之前** |
> | §27 | 补充「判断模型不得阻塞首屏」的性能前提 |
>
> 术语：本文中 **「判断模型」** 指 Jev 式的 System One 模型（只输出 Choice / Score / Noul
> 三种强类型 + 置信概率，不生成文本）。默认实现为**开源的 Laya**，云端 Jev 为可选替代。

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

> **2026-09-24 修订 —— 本条原则保留，仅补充「智能增强」的边界**
>
> 原条款只写了「不启动 AI」，未区分 AI 是本地还是云端。现明确：
>
> 1. **默认走本地判断模型**（开源 Laya，Apache 2.0，权重可离线部署）。
>    「智能搜索」的意图解析、路由、重排全部在本地完成，**不产生任何网络请求**。
> 2. **云端增强（Jev）为可选能力，默认关闭**。开启后仅作用于「✦ 智能搜索」路径；
>    极速搜索路径**永远零网络**。
> 3. 云端增强可一键全局关闭，关闭后功能**不降级为不可用**，只是回退为纯本地判断。
> 4. 云端调用失败 / 超时**自动降级**到本地判断或本地极速搜索，不得弹出错误阻断用户。
>
> 即：**Local-first by default，cloud-enhanced on demand。**

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

> **2026-09-24 修订 —— 补充「唤起与关闭」的明确要求**
>
> 原条款未规定窗口的唤起与关闭方式，现补充：
>
> | 行为 | 要求 |
> | :--- | :--- |
> | **唤起** | 全局热键（默认 `Alt+Space`）+ 托盘图标。**必须在任意前台页面上都能唤起**，不依赖当前焦点所在程序 |
> | **关闭** | **点击窗口自身以外的任意区域即关闭**（失焦隐藏）。窗口显示后 600 ms 内为宽限期，避免唤起瞬间被抢焦点导致自关 |
> | **抑制** | 弹出原生对话框（文件选择等）或执行需切换前台的测试操作期间，必须**抑制**失焦关闭 |
>
> ⚠️ **唤醒路径已收窄**：窗口已于 2026-09-24 置 `WS_EX_TOOLWINDOW`，
> **不再出现在任务栏与 Alt+Tab**。因此唤醒只剩「托盘图标」与「全局热键」两条路。
>
> **要求**：全局热键注册失败（被输入法 / 通讯软件 / 游戏加速器等占用）时，
> **必须给出显式提示**并说明可改用托盘图标，不得静默失败让用户「找不到窗口」。
>
> ⚠️ 失焦关闭基于「前台窗口比对」。应用内右键菜单与设置面板是同窗口绘制，不受影响；
> 但若将来引入**原生 Shell 右键菜单**（独立 popup 窗口），会立即误触发关闭，须复用抑制标志。

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

> **2026-09-24 修订 —— 新增「右键操作菜单」规格**
>
> 原条款只描述了结果卡片的静态字段，未规定右键菜单。现补充（`ui/components/context_menu.slint` 已实现前 6 项）：
>
> | 菜单项 | 适用类型 | 说明 | 实现 |
> | :--- | :--- | :--- | :--- |
> | 立即打开 | 全部 | 默认动作，等同 Enter | 已有 |
> | 在资源管理器中显示 | 文件 / 文件夹 | 定位到所在目录 | 已有 |
> | 进入此文件夹查找 | 文件夹 | 把搜索范围限定到该目录 | 已有 |
> | 复制完整路径 / 复制内容 | 全部 | 剪贴板项复制内容 | 已有 |
> | 固定到置顶 | 全部 | 加入置顶展架 | 已有 |
> | 从最近记录中移除 | 全部 | 危险操作，红色 | 已有 |
> | **在终端打开（cmd）** | 文件 / 文件夹 | 在工作目录启动 `cmd.exe /K cd /d "<dir>"` | **新增** |
> | **在 PowerShell 打开** | 文件 / 文件夹 | `pwsh.exe -NoExit -Command Set-Location -LiteralPath '<dir>'` | **新增** |
> | **在 Windows Terminal 打开** | 文件 / 文件夹 | `wt.exe -d "<dir>"`；不存在则隐藏该项 | **新增** |
> | **用…打开** | 文件 | 调起系统「打开方式」对话框（ShellExecute verb `openas`） | **新增** |
>
> 规则：
> - **文件取父目录，文件夹取自身**作为工作目录；
> - 终端进程须显式设置工作目录，避免窗口闪退；
> - 需探测 `pwsh.exe` / `wt.exe` 是否存在，缺失则隐藏对应项（`cmd.exe` 恒在）。

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

> **2026-09-24 修订 —— 补充右键菜单与终端相关快捷键**
>
> | 快捷键 | 行为 |
> | :--- | :--- |
> | `Shift + F10` / 右键 | 打开操作菜单（`Shift+F10` 保证纯键盘可用） |
> | `Ctrl + O` | 在资源管理器中显示 |
> | `Ctrl + T` | 在终端打开（默认终端，见设置） |
> | `Ctrl + Shift + T` | 在 PowerShell 打开 |
> | `Ctrl + Shift + O` | 用…打开（系统「打开方式」） |
>
> 注：终端类快捷键在**文件夹项**上作用于该目录，在**文件项**上作用于其父目录。

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

> **2026-09-24 修订 —— AI 层拆为三层，判断与向量解耦**
>
> 原条款把「意图理解 → 向量检索 → 重排」视为一条链，未区分本地与云端，
> 也未区分「判断」与「向量」。现明确拆为三层，**可独立开关**：
>
> | 层 | 职责 | 默认 | 实现 |
> | :--- | :--- | :--- | :--- |
> | **① 本地判断层** | 意图解析（槽位填充）、查询路由 | **开** | 开源 **Laya**（本地，Apache 2.0，无网络） |
> | **② 本地向量层** | 语义相似检索（内容理解） | 关 | `sqlite-vec` + `bge-small-zh` |
> | **③ 云端增强层** | 同上，但用云端模型 | **关** | TypeSafe **Jev**（可选替代 ①，非叠加） |
>
> **判断层不做重排打分**：官方 in-task 数据里 `intent and routing` 达 **0.991**，
> 而 `search relevance` 仅 **0.628**、`response quality scoring` 仅 **0.581**。
> 因此**排名一律交给本地排序算法**（前缀 > 词首 > 中间 + 频率/时间/类型权重）。
> 判断模型只负责「听懂」，不负责「排名」。
>
> **关键设计：判断模型只做 Choice / Score / Noul，不生成文本。**
>
> - **关键词不由判断模型生成**，必须留在本地（分词 + 拼音 + 索引召回），
>   否则会引入生成式幻觉 —— 而这正是判断模型存在的意义；
> - 判断模型只负责把「昨天」「文档」「项目」这类**语义槽位**映射到**预设枚举**上；
> - 因此**枚举必须离散化**，且候选数**控制在 20 个以内**（Laya 的实测甜点区；
>   其选项数超过 20 时准确率显著下降）。
>
> **重排与召回并行**：本地检索先出结果占据首屏，判断模型异步返回后**只做重排**，
> 不得阻塞首屏渲染（详见 §27）。
>
> 修订后的流程：
>
> ```
> User Query
>     │
>     ├──────────────► 本地极速检索（FTS5 + 分词）──► 首屏结果（毫秒级）
>     │                                                    │
>     └──► 本地判断层（Laya，异步）                         │
>              ├── 槽位填充（类型 / 时间 / 位置）            │
>              └── 路由（极速 / 智能 / 混合）                │
>                        │                                  │
>              本地排序算法排名（非判断模型）────────────────┴──► 最终结果
> ```
>
> **三级闸门**（「智能化调用，必要时才使用模型」）：
>
> | 闸门 | 触发条件 | 成本 | 行为 |
> | :-: | :--- | :--- | :--- |
> | 1 | 总是 | < 1 ms | 本地规则判定；纯关键词**直接出结果，不调模型** |
> | 2 | 规则判定为自然语言 | 200~460 ms | **先出本地结果**，异步解析槽位后二次检索 |
> | 3 | 用户显式开启且本地置信度低于阈值 | 70~500 ms | 云端模型；**超时 800 ms 放弃** |
>
> 硬性规则：首屏永远由本地检索提供，模型结果只做增量替换；
> 任何一级失败静默降级，不弹错误。

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
├── Enable AI Search (智能语义总开关)
│
├── ① 判断层 (默认仅规则，无网络)
│   ├── Backend Mode (自动 / 仅规则 / 本地模型 / 云端模型)
│   ├── Rule Engine (规则版槽位解析；零依赖，默认唯一启用项)
│   ├── Local Checkpoint (英文根版 421M / 多语言版 322M / 按查询语言自动)
│   ├── Model Path (权重目录，约 650MB)
│   ├── Preload (预加载，避免首次调用 7~10s 冷启动；默认关以省内存)
│   ├── Call Policy (触发策略：仅自然语言输入 / 每次智能搜索)
│   └── Confidence Threshold (置信度阈值，低于此值不自动采纳)
│
├── ② 本地向量层 (默认关闭)
│   ├── Model (本地量化模型 bge-small-zh)
│   ├── sqlite-vec Extension (向量加速)
│   └── Lazy Loading (按需动态唤醒)
│
└── ③ 云端增强层 (默认关闭)
    ├── Enable Cloud Decision (启用云端判断模型)
    ├── Escalate on Low Confidence (本地置信度低时升级到云端，默认关)
    ├── Timeout (超时上限，默认 800ms，超时即放弃不阻塞)
    ├── Jev API Key (凭据)
    ├── Proxy (代理地址，默认 http://127.0.0.1:7897)
    └── Fail-safe Degrade (失败自动降级到本地，默认开)

UI (终端与打开方式)
├── Default Terminal (cmd / PowerShell / Windows Terminal)
└── Show Open-With (在右键菜单显示「用…打开」)
```

> **2026-09-24 修订**：原 AI 分组只有「本地量化模型 / sqlite-vec / 懒加载」三项，
> 假定 AI 就等于向量检索。现按 §20 的三层拆分重构：
> 判断层（默认开，本地）与向量层（默认关，本地）解耦，
> 云端增强作为可选替代而非叠加。另新增「终端与打开方式」分组（对应 §11 右键菜单）。

------

# 26. MVP

第一版不要一次把所有功能做完。

> **2026-09-24 修订 —— Phase 顺序与内容调整**
>
> 两处调整：
>
> 1. **新增 Phase 0（阻塞项）**：全局唤醒热键与快捷直达的**端到端真机验证**。
>    这是所有「快捷键唤起 / 快捷键直达」体验的地基，未验证前其余功能都无处落地。
> 2. **Phase 3 补入「中文分词」与「排序模型」**。原 Phase 3 只列了 FTS5 与 USN Journal，
>    但真正决定「检索效果好不好」的是**中文分词质量**与**排序权重设计**，
>    而非索引结构本身。判断模型（Phase 4）**不提升准确率**，故不得替代这两项。
>
> ### Phase 0（前置阻塞）
>
> ```
> Global Hotkey 真机验证
>         ↓
> Hotkey Direct 真机验证
>         ↓
> 热键注册失败的显式提示
> ```
>
> ### Phase 1
>
> ```
> Global Hotkey
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
中文分词（trigram / jieba）
排序模型（前缀 > 词首 > 中间 + 频率 / 时间 / 类型权重）
USN Journal
```

### Phase 4

```
AI Search
Semantic Search
Embedding
RAG
```

> **2026-09-24 修订 —— Phase 4 内容细化**
>
> 「AI Search」过于笼统。本 Phase 拆为两件**独立**的事：
>
> - **4A 本地判断模型接入**（默认启用）：Laya 的意图解析 / 路由 / 重排。
>   需要先完成 ONNX 导出与 Rust 侧移植，并做温度校准；
> - **4B 语义检索**（默认关闭）：Embedding + 向量检索 + RAG，仍以本地 `bge-small-zh` 为主。
>
> 云端判断模型（Jev）作为 4A 的可选替代，**不单独占用 Phase**。
>
> ⚠️ **预期管理**：判断模型提升的是**响应速度与查询理解能力**，**不提升检索准确率**
> （官方与第三方基准均显示其准确率不高于大模型）。准确率只能靠 Phase 3 的检索链。

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

> **2026-09-24 修订 —— 补充判断模型的性能前提**
>
> 原条款只写「AI：默认不启动」，不足以约束引入判断模型后的行为。补充四条硬性前提：
>
> | # | 前提 | 理由 |
> | :-: | :--- | :--- |
> | 1 | **判断模型不得阻塞首屏**。本地检索先出结果并立即渲染，判断模型**异步**返回后只做重排 | 本地判断模型在 CPU 上单次约 **193~464 ms**（官方 T4 为 32.8 ms），远超「毫秒级」目标 |
> | 2 | **纯关键词输入永不触发判断模型**。仅当输入被**本地规则**判定为自然语言（含动词 / 时间词 / 疑问词）时才调用 | 省电、省内存、避免无谓延迟 |
> | 3 | **模型不得常驻内存**。默认懒加载、空闲后释放；仅在用户显式开启 Preload 时保留 | 多语言权重约 **650 MB**，违反「Idle RAM 尽量低」 |
> | 4 | **模型加载与索引同为后台低优先级**，且不得在窗口唤起路径上同步执行 | 避免首次调用 7~10 s 冷启动拖慢交互 |
>
> 另注：云端判断模型（Jev）端到端 70~500 ms，虽无本地冷启动问题，但**引入网络不确定性**，
> 且中国大陆需经代理。故默认关闭，且必须与本地路径并行、可超时降级。

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