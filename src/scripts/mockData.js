/**
 * Anycast Mock Data Source (Concise & Minimal Text)
 * Clean paths, zero redundant descriptive prefixes
 */

const MockDatabase = {
  // Pinned Items (Fixed Quick Access)
  pinned: [
    {
      id: "pin-1",
      type: "app",
      title: "Visual Studio Code",
      shortTitle: "VS Code",
      subtitle: "Code.exe",
      badge: "应用",
      icon: "app",
      isPinned: true,
      lastUsed: "刚刚",
      action: "Enter",
      preview: {
        type: "app",
        version: "1.92.2",
        path: "C:\\Programs\\VSCode\\Code.exe"
      }
    },
    {
      id: "pin-2",
      type: "file",
      category: "文件",
      title: "Projects",
      subtitle: "D:\\Projects",
      badge: "文件夹",
      icon: "folder",
      isPinned: true,
      lastUsed: "10m前",
      action: "Enter",
      preview: {
        type: "folder",
        path: "D:\\Projects",
        size: "18.4 GB",
        modified: "今天 19:40"
      }
    },
    {
      id: "pin-3",
      type: "file",
      category: "文件",
      title: "EasyNote.md",
      subtitle: "docs\\EasyNote.md",
      badge: "文档",
      icon: "fileText",
      isPinned: true,
      lastUsed: "2h前",
      action: "Enter",
      preview: {
        type: "markdown",
        path: "E:\\Projects\\EasyNote\\docs\\EasyNote.md",
        size: "34.2 KB",
        modified: "今天 14:15",
        content: `# EasyNote\n- 本地优先 (Local-first)\n- SQLite FTS5 秒搜\n- 纯文本 Markdown`
      }
    },
    {
      id: "pin-4",
      type: "clipboard",
      category: "剪贴板",
      subType: "code",
      title: "npm install rust-analyzer",
      shortTitle: "npm install",
      subtitle: "1天前",
      badge: "代码",
      icon: "fileCode",
      isPinned: true,
      lastUsed: "昨天",
      action: "复制",
      preview: {
        type: "code",
        format: "bash",
        charCount: 26,
        time: "昨天 10:22",
        content: `npm install -g rust-analyzer`
      }
    },
    {
      id: "pin-5",
      type: "app",
      category: "应用",
      title: "Docker Desktop",
      shortTitle: "Docker",
      subtitle: "Docker.exe",
      badge: "应用",
      icon: "docker",
      isPinned: true,
      lastUsed: "3h前",
      action: "Enter",
      preview: {
        type: "app",
        version: "4.33.1",
        path: "C:\\Program Files\\Docker\\Docker Desktop.exe"
      }
    },
    {
      id: "pin-6",
      type: "app",
      category: "应用",
      title: "Windows Terminal",
      shortTitle: "Terminal",
      subtitle: "wt.exe",
      badge: "应用",
      icon: "terminal",
      isPinned: true,
      lastUsed: "4h前",
      action: "Enter",
      preview: {
        type: "app",
        version: "1.20.11781",
        path: "C:\\Users\\AppData\\Local\\Microsoft\\WindowsApps\\wt.exe"
      }
    },
    {
      id: "pin-7",
      type: "file",
      category: "文件",
      title: "architecture.md",
      shortTitle: "架构文档",
      subtitle: "doc\\architecture.md",
      badge: "文档",
      icon: "fileText",
      isPinned: true,
      lastUsed: "5h前",
      action: "Enter",
      preview: {
        type: "markdown",
        path: "D:\\Anycast\\doc\\architecture.md",
        size: "14.8 KB",
        modified: "今天 16:30",
        content: `# Anycast 架构蓝图\n- Slint UI 22 组件规范\n- USN Journal 增量文件索引`
      }
    }
  ],

  // Recent Items (Idle state list: 最近使用互通搜索结果)
  recent: [
    {
      id: "rec-1",
      type: "app",
      category: "应用",
      title: "Visual Studio Code",
      subtitle: "Code.exe",
      badge: "应用",
      action: "Enter",
      lastUsed: "刚刚",
      preview: {
        type: "app",
        version: "1.92.2",
        path: "C:\\Programs\\VSCode\\Code.exe"
      }
    },
    {
      id: "rec-2",
      type: "file",
      category: "文件",
      title: "architecture.md",
      subtitle: "docs\\architecture.md",
      badge: "文档",
      action: "Enter",
      lastUsed: "5m前",
      preview: {
        type: "markdown",
        path: "E:\\Projects\\EasyNote\\docs\\architecture.md",
        size: "15.8 KB",
        modified: "昨天 18:20",
        content: `# 架构概览\n1. Slint 前端层\n2. Rust Core 总线\n3. SQLite + FTS5`
      }
    },
    {
      id: "rec-3",
      type: "file",
      category: "文件",
      title: "Projects",
      subtitle: "D:\\Projects",
      badge: "文件夹",
      action: "Enter",
      lastUsed: "12m前",
      preview: {
        type: "folder",
        path: "D:\\Projects",
        size: "18.4 GB",
        modified: "今天 19:40"
      }
    },
    {
      id: "rec-4",
      type: "app",
      category: "应用",
      title: "Docker Desktop",
      subtitle: "Docker Desktop.exe",
      badge: "应用",
      action: "Enter",
      lastUsed: "30m前",
      preview: {
        type: "app",
        version: "4.33.0",
        path: "C:\\Program Files\\Docker\\Docker Desktop.exe"
      }
    },
    {
      id: "rec-5",
      type: "clipboard",
      category: "剪贴板",
      subType: "code",
      title: "docker compose up -d",
      subtitle: "1h前",
      badge: "代码",
      action: "复制",
      lastUsed: "1h前",
      preview: {
        type: "code",
        format: "bash",
        charCount: 21,
        time: "1小时前",
        content: `docker compose up -d`
      }
    },
    {
      id: "rec-6",
      type: "clipboard",
      category: "剪贴板",
      subType: "url",
      title: "https://slint.dev/docs",
      subtitle: "2h前",
      badge: "链接",
      action: "复制",
      lastUsed: "2h前",
      preview: {
        type: "url",
        time: "2小时前",
        content: `https://slint.dev/docs`
      }
    },
    {
      id: "rec-7",
      type: "app",
      category: "应用",
      title: "Microsoft Edge",
      subtitle: "msedge.exe",
      badge: "应用",
      action: "Enter",
      lastUsed: "3h前",
      preview: {
        type: "app",
        path: "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"
      }
    },
    {
      id: "rec-8",
      type: "clipboard",
      category: "剪贴板",
      subType: "text",
      title: "Windows 本地统一入口设计规范 v2",
      subtitle: "昨天",
      badge: "文本",
      action: "复制",
      lastUsed: "昨天",
      preview: {
        type: "text",
        time: "昨天 16:30",
        content: `Windows 本地统一入口设计规范 v2：基于 Slint 与 Windows 11 Fluent 亚克力视觉体系`
      }
    }
  ],

  // All Searchable Catalog Items (Fast Search Pool)
  allItems: [
    {
      id: "item-app-1",
      type: "app",
      category: "应用",
      title: "Docker Desktop",
      subtitle: "Docker Desktop.exe",
      badge: "应用",
      icon: "app",
      keywords: ["docker", "desktop", "container"],
      action: "Enter",
      preview: {
        type: "app",
        version: "4.33.0",
        path: "C:\\Program Files\\Docker\\Docker Desktop.exe"
      }
    },
    {
      id: "item-app-2",
      type: "app",
      category: "应用",
      title: "Visual Studio Code",
      subtitle: "Code.exe",
      badge: "应用",
      icon: "app",
      keywords: ["code", "vscode", "editor"],
      action: "Enter",
      preview: {
        type: "app",
        version: "1.92.2",
        path: "C:\\Programs\\VSCode\\Code.exe"
      }
    },
    {
      id: "item-file-1",
      type: "file",
      category: "文件",
      title: "docker-compose.yml",
      subtitle: "Infrastructure\\docker-compose.yml",
      badge: "配置",
      icon: "fileCode",
      keywords: ["docker", "compose", "yml"],
      action: "Enter",
      preview: {
        type: "code",
        path: "D:\\Infrastructure\\docker-compose.yml",
        size: "1.8 KB",
        modified: "09-18 16:30",
        content: `version: '3.8'\nservices:\n  postgres:\n    image: postgres:16-alpine\n    ports: ["5432:5432"]`
      }
    },
    {
      id: "item-file-2",
      type: "file",
      category: "文件",
      title: "Dockerfile",
      subtitle: "Infrastructure\\Dockerfile",
      badge: "构建",
      icon: "fileCode",
      keywords: ["docker", "dockerfile", "build"],
      action: "Enter",
      preview: {
        type: "code",
        path: "D:\\Infrastructure\\Dockerfile",
        size: "940 B",
        modified: "09-15 11:20",
        content: `FROM rust:1.80-slim as builder\nWORKDIR /app\nCOPY . .\nRUN cargo build --release`
      }
    },
    {
      id: "item-file-3",
      type: "file",
      category: "文件",
      title: "EasyNote.md",
      subtitle: "docs\\EasyNote.md",
      badge: "文档",
      icon: "fileText",
      keywords: ["easynote", "note", "markdown"],
      action: "Enter",
      preview: {
        type: "markdown",
        path: "E:\\EasyNote\\docs\\EasyNote.md",
        size: "34.2 KB",
        modified: "今天 14:15",
        content: `# EasyNote 本地知识库\n秒搜与本地优先架构。`
      }
    },
    {
      id: "item-file-4",
      type: "file",
      category: "文件",
      title: "storage.md",
      subtitle: "docs\\storage.md",
      badge: "文档",
      icon: "fileText",
      keywords: ["easynote", "storage", "sqlite", "fts5", "数据存储"],
      action: "Enter",
      preview: {
        type: "markdown",
        path: "E:\\EasyNote\\docs\\storage.md",
        size: "21.6 KB",
        modified: "昨天 17:30",
        content: `# 数据存储\n- 主表: notes\n- 全文索引: notes_fts USING fts5`
      }
    },
    {
      id: "item-folder-1",
      type: "file",
      category: "文件",
      title: "Docker",
      subtitle: "D:\\Dev\\Docker",
      badge: "文件夹",
      icon: "folder",
      keywords: ["docker", "dev"],
      action: "Enter",
      preview: {
        type: "folder",
        path: "D:\\Dev\\Docker",
        size: "320 MB",
        modified: "09-18 16:35"
      }
    },
    {
      id: "item-folder-2",
      type: "file",
      category: "文件",
      title: "Projects",
      subtitle: "D:\\Projects",
      badge: "文件夹",
      icon: "folder",
      keywords: ["projects", "code"],
      action: "Enter",
      preview: {
        type: "folder",
        path: "D:\\Projects",
        size: "18.4 GB",
        modified: "今天 19:40"
      }
    },
    {
      id: "item-clip-1",
      type: "clipboard",
      category: "剪贴板",
      subType: "code",
      title: "docker compose up -d",
      subtitle: "2h前",
      badge: "代码",
      icon: "fileCode",
      keywords: ["docker", "compose", "bash"],
      action: "复制",
      preview: {
        type: "code",
        format: "bash",
        charCount: 21,
        time: "2小时前",
        content: `docker compose up -d`
      }
    },
    {
      id: "item-clip-2",
      type: "clipboard",
      category: "剪贴板",
      subType: "code",
      title: "npm install react",
      subtitle: "3h前",
      badge: "代码",
      icon: "fileCode",
      keywords: ["npm", "react"],
      action: "复制",
      preview: {
        type: "code",
        format: "bash",
        charCount: 17,
        time: "3小时前",
        content: `npm install react react-dom`
      }
    },
    {
      id: "item-clip-3",
      type: "clipboard",
      category: "剪贴板",
      subType: "code",
      title: "{\"name\": \"EasyNote\"}",
      subtitle: "昨天",
      badge: "代码",
      icon: "fileCode",
      keywords: ["json", "easynote"],
      action: "复制",
      preview: {
        type: "code",
        format: "json",
        charCount: 32,
        time: "昨天 15:40",
        content: `{\n  "name": "EasyNote",\n  "version": "1.0.0"\n}`
      }
    },
    {
      id: "item-clip-4",
      type: "clipboard",
      category: "剪贴板",
      subType: "text",
      title: "git commit -m \"feat: 剪切板二级筛选与双视图模式\"",
      subtitle: "1h前",
      badge: "文本",
      icon: "clipboard",
      keywords: ["git", "commit", "feat"],
      action: "复制",
      preview: {
        type: "text",
        charCount: 42,
        time: "1小时前",
        content: `git commit -m "feat: 剪切板二级筛选与双视图模式"`
      }
    },
    {
      id: "item-clip-5",
      type: "clipboard",
      category: "剪贴板",
      subType: "url",
      title: "https://github.com/slint-ui/slint",
      subtitle: "4h前",
      badge: "链接",
      icon: "link",
      keywords: ["slint", "github", "url", "repo"],
      action: "打开",
      preview: {
        type: "url",
        time: "4小时前",
        content: `https://github.com/slint-ui/slint`
      }
    },
    {
      id: "item-clip-6",
      type: "clipboard",
      category: "剪贴板",
      subType: "text",
      title: "Slint 声明式 DSL 现代化界面规范备忘录",
      subtitle: "昨天",
      badge: "文本",
      icon: "clipboard",
      keywords: ["slint", "dsl", "规范", "备忘录"],
      action: "复制",
      preview: {
        type: "text",
        charCount: 22,
        time: "昨天 11:20",
        content: `Slint 声明式 DSL 现代化界面规范备忘录`
      }
    },
    {
      id: "item-clip-7",
      type: "clipboard",
      category: "剪贴板",
      subType: "url",
      title: "https://doc.rust-lang.org/book/",
      subtitle: "2天前",
      badge: "链接",
      icon: "link",
      keywords: ["rust", "doc", "book", "url"],
      action: "打开",
      preview: {
        type: "url",
        time: "2天前",
        content: `https://doc.rust-lang.org/book/`
      }
    }
  ],

  // AI Semantic Presets
  aiScenarios: [
    {
      query: "找一下昨天修改的 EasyNote 文件",
      intentChips: [
        { key: "关键词", val: "EasyNote" },
        { key: "类型", val: "文档" },
        { key: "修改", val: "昨天" }
      ],
      results: [
        {
          id: "ai-1",
          type: "file",
          title: "storage.md",
          subtitle: "docs\\storage.md",
          badge: "99% 匹配",
          icon: "fileText",
          reason: "内容包含存储机制，修改时间为昨天 17:30",
          preview: {
            type: "markdown",
            path: "E:\\EasyNote\\docs\\storage.md",
            size: "21.6 KB",
            modified: "昨天 17:30",
            content: `# 数据存储\n- 主表: notes\n- 全文索引: notes_fts`
          }
        },
        {
          id: "ai-2",
          type: "file",
          title: "architecture.md",
          subtitle: "docs\\architecture.md",
          badge: "94% 匹配",
          icon: "fileText",
          reason: "内容包含 EasyNote 技术栈架构设计",
          preview: {
            type: "markdown",
            path: "E:\\EasyNote\\docs\\architecture.md",
            size: "15.8 KB",
            modified: "昨天 18:20",
            content: `# 架构概览\nSlint + Rust Core + SQLite FTS5`
          }
        }
      ]
    }
  ],

  // App & File Global Hotkey Quick Launch Bindings (快捷直达热键绑定)
  hotkeys: [
    {
      id: "hk-1",
      title: "Visual Studio Code",
      type: "app",
      category: "应用",
      badge: "应用",
      path: "C:\\Program Files\\Microsoft VS Code\\Code.exe",
      hotkey: "Alt+C",
      enabled: true,
      launchMode: "activate",
      description: "一键呼出或激活代码编辑器"
    },
    {
      id: "hk-2",
      title: "Windows Terminal",
      type: "app",
      category: "应用",
      badge: "应用",
      path: "wt.exe",
      hotkey: "Alt+T",
      enabled: true,
      launchMode: "activate",
      description: "快速唤起终端 PowerShell 窗口"
    },
    {
      id: "hk-3",
      title: "Docker Desktop",
      type: "app",
      category: "应用",
      badge: "应用",
      path: "C:\\Program Files\\Docker\\Docker Desktop.exe",
      hotkey: "Alt+D",
      enabled: true,
      launchMode: "activate",
      description: "唤起 Docker 容器环境与面板"
    },
    {
      id: "hk-4",
      title: "工程项目目录",
      type: "folder",
      category: "文件",
      badge: "文件夹",
      path: "D:\\Projects",
      hotkey: "Alt+P",
      enabled: true,
      launchMode: "open",
      description: "在资源管理器中直接打开工程总目录"
    },
    {
      id: "hk-5",
      title: "项目架构设计文档",
      type: "file",
      category: "文件",
      badge: "文档",
      path: "D:\\Anycast\\doc\\architecture.md",
      hotkey: "Alt+N",
      enabled: false,
      launchMode: "open",
      description: "秒级打开核心系统设计文档"
    }
  ]
};
