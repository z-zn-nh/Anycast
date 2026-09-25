/**
 * Anycast Prototype & Interaction Engine
 * Windows 11 Acrylic & Keyboard-first Reactive Launcher
 */

(function () {
  // Application State
  const state = {
    searchMode: "fast", // 'fast' | 'smart'
    searchQuery: "",
    selectedIndex: 0,
    currentResults: [],
    showPreview: false, // 遵循指示：先不考虑侧栏预览
    showContextMenu: false,
    contextMenuCoords: { x: 0, y: 0 },
    showSettings: false,
    settingsTab: "general",
    activeView: "prototype", // 'prototype' | 'design-system'
    theme: "dark",
    wallpaper: "user-current",
    acrylicOpacity: "balanced",
    activeFilter: "all", // 'all' | 'file' | 'app' | 'clipboard' (严格4类)
    clipboardSubFilter: "all", // 'all' | 'text' | 'code' | 'url'
    viewMode: localStorage.getItem("anycast_view_mode") || "list", // 'list' | 'grid'
    isPinnedCollapsed: localStorage.getItem("anycast_pinned_collapsed") === "true",
    showActionPalette: false,
    hotkeysMasterEnabled: true,
    recordingTarget: null,
    newHotkeyCombo: "Alt+K",
    isFilterShelfCollapsed: localStorage.getItem("anycast_filter_shelf_collapsed") === "false" ? false : true,
    searchScope: {
      time: "all",
      type: "all",
      location: "all",
      locationName: "",
      locationPath: ""
    },
    calViewYear: 2026,
    calViewMonth: 8, // 0-indexed: 8 is September
    calRange: {
      start: null,
      end: null,
      hover: null
    }
  };

  // DOM Elements
  const el = {
    canvas: document.getElementById("desktopCanvas"),
    searchWindow: document.getElementById("searchWindow"),
    searchBarWrapper: document.querySelector(".search-bar-wrapper"),
    searchInput: document.getElementById("searchInput"),
    searchClearBtn: document.getElementById("searchClearBtn"),
    toggleFilterShelfBtn: document.getElementById("toggleFilterShelfBtn"),
    searchFilterShelf: document.getElementById("searchFilterShelf"),
    resetShelfFilterBtn: document.getElementById("resetShelfFilterBtn"),
    searchBody: document.getElementById("searchBody"),
    aiIntentBar: document.getElementById("aiIntentBar"),
    aiChipsList: document.getElementById("aiChipsList"),
    pinnedShelfSection: document.getElementById("pinnedShelfSection"),
    pinnedTrackWrapper: document.getElementById("pinnedTrackWrapper"),
    pinnedGrid: document.getElementById("pinnedGrid"),
    categoryFilterBar: document.getElementById("categoryFilterBar"),
    categoryTabs: document.getElementById("categoryTabs"),
    clipboardSubFilterBar: document.getElementById("clipboardSubFilterBar"),
    clipboardSubFilterChips: document.getElementById("clipboardSubFilterChips"),
    viewModeToggleBtn: document.getElementById("viewModeToggleBtn"),
    viewModeIcon: document.getElementById("viewModeIcon"),
    viewModeLabel: document.getElementById("viewModeLabel"),
    launcherSettingsBtn: document.getElementById("launcherSettingsBtn"),
    resultPane: document.getElementById("resultPane"),
    paletteFooter: document.getElementById("paletteFooter"),
    footerStatusPill: document.getElementById("footerStatusPill"),
    footerItemCount: document.getElementById("footerItemCount"),
    signatureActionPill: document.getElementById("signatureActionPill"),
    footerPrimaryActionBtn: document.getElementById("footerPrimaryActionBtn"),
    footerActionLabel: document.getElementById("footerActionLabel"),
    footerActionsMenuBtn: document.getElementById("footerActionsMenuBtn"),
    actionPaletteModal: document.getElementById("actionPaletteModal"),
    actionItemOpenLabel: document.getElementById("actionItemOpenLabel"),
    previewPanel: document.getElementById("previewPanel"),
    previewContent: document.getElementById("previewContent"),
    contextMenu: document.getElementById("contextMenu"),
    settingsModal: document.getElementById("settingsModal"),
    toastContainer: document.getElementById("toastContainer"),
    designSystemView: document.getElementById("designSystemView"),
    themeToggleBtn: document.getElementById("themeToggleBtn"),
    wallpaperSelect: document.getElementById("wallpaperSelect"),
    acrylicOpacitySelect: document.getElementById("acrylicOpacitySelect"),
    customWallpaperInput: document.getElementById("customWallpaperInput"),
    viewToggleBtns: document.querySelectorAll("[data-view-target]")
  };

  // Toast Notification Manager
  function showToast(message, icon = "check") {
    const toast = document.createElement("div");
    toast.className = "toast-item";
    toast.innerHTML = `
      <span style="color: var(--accent-primary-hover); display: flex;">${Icons[icon] || Icons.check}</span>
      <span>${message}</span>
    `;
    el.toastContainer.appendChild(toast);
    setTimeout(() => {
      if (toast.parentNode) {
        toast.parentNode.removeChild(toast);
      }
    }, 2800);
  }

  // Set Search Mode (⚡ 极速 vs ✦ 智能)
  function setSearchMode(mode) {
    state.searchMode = mode;
    const modeIconEl = document.getElementById("searchModeIcon");
    if (mode === "smart") {
      el.searchWindow.classList.add("mode-smart");
      el.searchInput.placeholder = "智能搜索：支持自然语言、语义理解与意图识别……";
      if (el.searchModeName) el.searchModeName.textContent = "智能";
      if (modeIconEl) {
        modeIconEl.innerHTML = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z"/><path d="M5 3v4"/><path d="M19 17v4"/></svg>`;
      }
      showToast("已切换至 ✦ 智能搜索模式 (Tab 可切回极速)", "sparkles");
    } else {
      el.searchWindow.classList.remove("mode-smart");
      el.searchInput.placeholder = "搜索应用、文件、剪贴板……";
      if (el.searchModeName) el.searchModeName.textContent = "极速";
      if (modeIconEl) {
        modeIconEl.innerHTML = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>`;
      }
      showToast("已切换至 ⚡ 极速搜索模式 (Tab 可切至智能)", "check");
    }
    updateSearchResults();
  }

  // Double-Click System Launch Handler with Visual Toast Prompt
  function launchViaSystem(item, targetEl) {
    if (!item) return;

    if (targetEl) {
      targetEl.classList.remove("launching-pulse");
      void targetEl.offsetWidth; // Force CSS reflow to re-trigger animation
      targetEl.classList.add("launching-pulse");
    }

    if (item.type === "clipboard" || item.category === "剪贴板") {
      const textToCopy = item.preview?.content || item.title;
      copyToClipboard(textToCopy);
      showToast(`📋 [系统调用] 已复制到剪贴板并就绪: ${item.title.substring(0, 24)}...`, "copy");
    } else if (item.type === "app" || item.category === "应用") {
      showToast(`🚀 [系统调用] 已调用系统默认程序启动: ${item.title}`, "play");
    } else if (item.badge === "文件夹" || item.type === "folder") {
      showToast(`🚀 [系统调用] 已在资源管理器中打开文件夹: ${item.title}`, "folderOpen");
    } else {
      showToast(`🚀 [系统调用] 已使用系统关联程序打开: ${item.title}`, "play");
    }
  }

  // View Mode Switcher UI Updater (图标 vs 列表)
  function updateViewModeUI() {
    if (!el.viewModeToggleBtn) return;
    if (state.viewMode === "grid") {
      if (el.viewModeIcon) {
        el.viewModeIcon.innerHTML = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>`;
      }
      if (el.viewModeLabel) el.viewModeLabel.textContent = "列表";
      el.viewModeToggleBtn.title = "切换为单行列表视图";
    } else {
      if (el.viewModeIcon) {
        el.viewModeIcon.innerHTML = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/></svg>`;
      }
      if (el.viewModeLabel) el.viewModeLabel.textContent = "图标";
      el.viewModeToggleBtn.title = "切换为图标网格视图";
    }
  }

  function setViewMode(mode) {
    state.viewMode = mode;
    localStorage.setItem("anycast_view_mode", mode);
    updateViewModeUI();
    updateSearchResults();
    showToast(`已切换为${mode === "grid" ? "图标视图" : "列表视图"}`, mode === "grid" ? "grid" : "list");
  }

  // Render Pinned Quick Access Shelf (Fixed Icon Category, Single Row)
  function renderPinnedShelf(forceRebuild = false) {
    if (!el.pinnedShelfSection || !el.pinnedGrid) return;

    el.pinnedShelfSection.style.display = "block";
    el.pinnedShelfSection.classList.toggle("collapsed", state.isPinnedCollapsed);

    const toggleBtn = el.pinnedShelfSection.querySelector("#togglePinnedBtn");
    if (toggleBtn) {
      toggleBtn.title = state.isPinnedCollapsed ? "展开置顶快速访问" : "折叠置顶快速访问";
      toggleBtn.setAttribute("aria-label", state.isPinnedCollapsed ? "展开置顶快速访问" : "折叠置顶快速访问");
    }

    if (state.isPinnedCollapsed) {
      return;
    }

    if (el.pinnedGrid.children.length > 0 && !forceRebuild) {
      return;
    }

    const pinnedItems = MockDatabase.pinned;
    let html = "";
    pinnedItems.forEach((item, index) => {
      const glyph = getItemGlyph(item);
      html += `
        <div class="pinned-card" data-pin-index="${index}" title="${escapeHtml(item.title)} (${escapeHtml(item.subtitle || '')})">
          <button class="pinned-card-unpin-btn" data-unpin-index="${index}" title="取消置顶" aria-label="取消置顶">
            <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
          <div class="pinned-card-glyph">
            ${renderSquircleIcon(glyph, 34)}
          </div>
          <span class="pinned-card-title">${item.shortTitle || item.title}</span>
          <span class="pinned-card-badge">${item.badge || item.category || item.type}</span>
        </div>
      `;
    });
    el.pinnedGrid.innerHTML = html;

    // Attach click, double click, unpin, and contextmenu listeners for pinned cards
    el.pinnedGrid.querySelectorAll(".pinned-card").forEach((cardEl) => {
      const idx = parseInt(cardEl.dataset.pinIndex, 10);
      const item = pinnedItems[idx];
      const unpinBtn = cardEl.querySelector(".pinned-card-unpin-btn");
      if (unpinBtn) {
        unpinBtn.addEventListener("click", (e) => {
          e.stopPropagation();
          unpinItem(item);
        });
      }
      cardEl.addEventListener("click", () => {
        el.pinnedGrid.querySelectorAll(".pinned-card").forEach((c) => c.classList.remove("selected"));
        cardEl.classList.add("selected");
      });
      cardEl.addEventListener("dblclick", () => {
        launchViaSystem(item, cardEl);
      });
      cardEl.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        openContextMenu(e.clientX, e.clientY, item);
      });
    });
  }

  // Toggle Pin Status for an Item (Hover-based quick pin)
  function togglePin(item) {
    if (!item) return;
    const existingIndex = MockDatabase.pinned.findIndex(
      (p) => (item.id && p.id === item.id) || p.title === item.title
    );
    if (existingIndex >= 0) {
      MockDatabase.pinned.splice(existingIndex, 1);
      item.isPinned = false;
      showToast(`已从置顶移除: ${item.title || item.shortTitle}`, "check");
    } else {
      const newPinned = {
        ...item,
        id: item.id || `pin-${Date.now()}`,
        isPinned: true,
        shortTitle: item.shortTitle || item.title.split(" ")[0] || item.title
      };
      item.isPinned = true;
      MockDatabase.pinned.unshift(newPinned);
      showToast(`📌 已固定至置顶快速访问: ${item.title}`, "pin");
    }
    renderPinnedShelf(true);
    updateSearchResults();
  }

  // Unpin an Item from Pinned Shelf
  function unpinItem(item) {
    if (!item) return;
    const idx = MockDatabase.pinned.findIndex(
      (p) => (item.id && p.id === item.id) || p.title === item.title
    );
    if (idx >= 0) {
      MockDatabase.pinned.splice(idx, 1);
      item.isPinned = false;
      showToast(`已从置顶移除: ${item.title || item.shortTitle}`, "check");
      renderPinnedShelf(true);
      updateSearchResults();
    }
  }

  // Toggle Pinned Shelf Collapse / Expand (Clean, instant, no jitter or bottom toast popup)
  function togglePinnedShelf() {
    state.isPinnedCollapsed = !state.isPinnedCollapsed;
    localStorage.setItem("anycast_pinned_collapsed", state.isPinnedCollapsed);
    renderPinnedShelf();
  }

  // Highlight Query Text
  function highlightMatch(text, query) {
    if (!query || !text) return text;
    const regex = new RegExp(`(${query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
    return text.replace(regex, `<span class="highlight">$1</span>`);
  }

  // Get Glyph Icon with semantic placeholder SVGs (no rainbow candy colors)
  function getItemGlyph(item) {
    const title = (item.title || "").toLowerCase();
    const sub = (item.subtitle || "").toLowerCase();

    // Specific app placeholder icons
    if (title.includes("visual studio code") || title.includes("vscode") || title.includes("code.exe")) {
      return { type: "vscode", svg: Icons.vscode };
    }
    if (title.includes("docker")) {
      return { type: "docker", svg: Icons.docker };
    }
    if (title.includes("edge") || title.includes("chrome") || title.includes("browser") || title.includes("msedge")) {
      return { type: "globe", svg: Icons.globe };
    }
    if (title.includes("terminal") || title.includes("powershell") || title.includes("cmd") || title.includes("bash")) {
      return { type: "terminal", svg: Icons.terminal };
    }

    // Folders
    if (item.badge === "文件夹" || item.type === "folder" || sub.includes("文件夹")) {
      return { type: "folder", svg: Icons.folder };
    }

    // File types
    if (item.type === "file" || item.category === "文件") {
      if (title.endsWith(".yml") || title.endsWith(".yaml") || title.endsWith(".json") || title.endsWith(".toml") || title.endsWith(".rs") || title.endsWith(".js") || title.endsWith(".ts")) {
        return { type: "fileCode", svg: Icons.fileCode };
      }
      return { type: "fileText", svg: Icons.fileText };
    }

    // Clipboard types
    if (item.type === "clipboard" || item.category === "剪贴板") {
      if (item.subType === "url" || title.startsWith("http://") || title.startsWith("https://")) {
        return { type: "link", svg: Icons.link };
      }
      if (item.subType === "code" || title.includes("npm ") || title.includes("docker ") || title.includes("cargo ") || title.includes("git ")) {
        return { type: "terminal", svg: Icons.terminal };
      }
      return { type: "clipboard", svg: Icons.clipboard };
    }

    // General app fallback
    if (item.category === "应用" || item.type === "app") {
      return { type: "app", svg: Icons.app };
    }

    return { type: "fileText", svg: Icons.fileText };
  }

  // Render Squircle Icon Container (Tinycast / Raycast 28x28 Signature)
  function renderSquircleIcon(glyph, size = 28) {
    const iconSize = Math.round(size * 0.58);
    const radius = Math.round(size * 0.25);
    return `
      <div class="squircle-icon squircle-${glyph.type}" style="width: ${size}px; height: ${size}px; border-radius: ${radius}px;">
        <span style="display: flex; align-items: center; justify-content: center; width: ${iconSize}px; height: ${iconSize}px;">${glyph.svg}</span>
      </div>
    `;
  }

  // Normalize relative dates & timestamps to YYYY-MM-DD for precise range filtering
  function parseItemDate(item) {
    if (!item) return "2026-09-21";
    const str = `${item.lastUsed || ""} ${item.preview?.modified || ""} ${item.preview?.time || ""}`;
    const m = str.match(/\b(202\d[-/]\d{1,2}[-/]\d{1,2})\b/);
    if (m) {
      const parts = m[1].replace(/\//g, "-").split("-");
      return `${parts[0]}-${parts[1].padStart(2, "0")}-${parts[2].padStart(2, "0")}`;
    }
    if (str.includes("刚刚") || str.includes("m前") || str.includes("h前") || str.includes("今天")) {
      return "2026-09-21";
    }
    if (str.includes("昨天") || str.includes("1天前")) {
      return "2026-09-20";
    }
    if (str.includes("2天前")) {
      return "2026-09-19";
    }
    if (str.includes("3天前")) {
      return "2026-09-18";
    }
    if (str.includes("4天前")) {
      return "2026-09-17";
    }
    if (str.includes("5天前")) {
      return "2026-09-16";
    }
    if (str.includes("6天前")) {
      return "2026-09-15";
    }
    if (str.includes("7天前") || str.includes("1周前")) {
      return "2026-09-14";
    }
    return "2026-09-21";
  }

  // Notion-style Search Scope Matcher (时间范围、文件类型、索引位置)
  function itemMatchesScope(item) {
    if (!item) return false;
    const { time, type, location } = state.searchScope;

    // 1. Time Scope Filter
    if (time !== "all") {
      if (time === "range" && state.searchScope.customRange) {
        const { start, end } = state.searchScope.customRange;
        const itemDate = parseItemDate(item);
        if (start && itemDate < start) return false;
        if (end && itemDate > end) return false;
      } else {
        const timeStr = `${item.lastUsed || ""} ${item.preview?.modified || ""}`.toLowerCase();
        if (time === "today") {
          const isToday = timeStr.includes("刚刚") || timeStr.includes("m前") || timeStr.includes("h前") || timeStr.includes("今天") || timeStr.includes("分钟") || timeStr.includes("小时");
          if (!isToday) return false;
        } else if (time === "7days") {
          const isPast7Days = timeStr.includes("刚刚") || timeStr.includes("m前") || timeStr.includes("h前") || timeStr.includes("今天") || timeStr.includes("昨天") ||
            timeStr.includes("1天前") || timeStr.includes("2天前") || timeStr.includes("3天前") || timeStr.includes("4天前") || timeStr.includes("5天前") || timeStr.includes("6天前") || timeStr.includes("7天前") ||
            timeStr.includes("2026-09-2");
          if (!isPast7Days) return false;
        } else if (time === "30days") {
          const isPast30Days = !timeStr.includes("个月前") && !timeStr.includes("1年前") && !timeStr.includes("半年前");
          if (!isPast30Days) return false;
        } else if (time === "year") {
          const isPastYear = !timeStr.includes("2年前") && !timeStr.includes("3年前");
          if (!isPastYear) return false;
        }
      }
    }

    // 2. Type Scope Filter
    if (type !== "all") {
      if (type === "app") {
        if (item.category !== "应用" && item.type !== "app") return false;
      } else if (type === "document") {
        const isDoc = item.badge === "文档" || item.badge === "文件夹" || item.preview?.type === "markdown" ||
          (item.title && (item.title.endsWith(".md") || item.title.endsWith(".txt") || item.title.endsWith(".pdf") || item.title.endsWith(".docx") || item.title.endsWith(".xlsx")));
        if (!isDoc) return false;
      } else if (type === "code") {
        const isCode = item.badge === "代码" || item.badge === "配置" || item.subType === "code" || item.preview?.type === "code" ||
          (item.title && (item.title.endsWith(".yml") || item.title.endsWith(".yaml") || item.title.endsWith(".json") || item.title.endsWith(".rs") || item.title.endsWith(".js") || item.title.endsWith(".ts") || item.title.endsWith(".py") || item.title.endsWith(".toml") || item.title.endsWith(".slint")));
        if (!isCode) return false;
      } else if (type === "media") {
        const isMedia = item.badge === "图片" || item.badge === "媒体" || item.subType === "image" ||
          (item.title && (item.title.endsWith(".png") || item.title.endsWith(".jpg") || item.title.endsWith(".svg") || item.title.endsWith(".webp") || item.title.endsWith(".mp4")));
        if (!isMedia) return false;
      } else if (type === "clipboard") {
        if (item.category !== "剪贴板" && item.type !== "clipboard") return false;
      }
    }

    // 3. Location Scope Filter (支持驱动器分区、系统标准目录、特定工作区与自定义文件夹)
    if (location !== "all") {
      const fullPath = `${item.title || ""} ${item.subtitle || ""} ${item.preview?.path || ""}`.toLowerCase();
      const loc = location.toLowerCase();

      if (loc === "drive-c") {
        if (!fullPath.includes("c:") && !fullPath.includes("c:\\")) return false;
      } else if (loc === "drive-d") {
        if (!fullPath.includes("d:") && !fullPath.includes("d:\\")) return false;
      } else if (loc === "desktop") {
        const isDesktop = fullPath.includes("desktop") || fullPath.includes("桌面");
        if (!isDesktop) return false;
      } else if (loc === "downloads") {
        const isDownloads = fullPath.includes("download") || fullPath.includes("下载");
        if (!isDownloads) return false;
      } else if (loc === "documents") {
        const isDocs = fullPath.includes("document") || fullPath.includes("docs") || fullPath.includes("文档");
        if (!isDocs) return false;
      } else if (loc === "projects" || loc === "folder-projects") {
        const isProj = fullPath.includes("project") || fullPath.includes("dev") || fullPath.includes("projects");
        if (!isProj) return false;
      } else if (loc === "folder-anycast") {
        if (!fullPath.includes("anycast")) return false;
      } else if (loc === "folder-easynote") {
        if (!fullPath.includes("easynote")) return false;
      } else {
        // Custom folder path or name scoping
        const targetPath = (state.searchScope.locationPath || location).toLowerCase();
        const targetName = (state.searchScope.locationName || "").toLowerCase();
        const matchesPath = targetPath && fullPath.includes(targetPath);
        const matchesName = targetName && fullPath.includes(targetName);
        if (!matchesPath && !matchesName) return false;
      }
    }

    return true;
  }

  // Category Filter Matcher (严格精简 4 类：全部, 文件, 应用, 剪贴板 + 剪贴板二级子分类)
  function itemMatchesFilter(item, filter) {
    if (!filter || filter === "all") return true;
    if (filter === "app") return item.category === "应用" || item.type === "app";
    if (filter === "file") return item.category === "文件" || item.type === "file" || item.type === "folder";
    if (filter === "clipboard") {
      const isClip = item.category === "剪贴板" || item.type === "clipboard";
      if (!isClip) return false;
      if (state.clipboardSubFilter && state.clipboardSubFilter !== "all") {
        return item.subType === state.clipboardSubFilter;
      }
      return true;
    }
    return true;
  }

  // Update Category Tab Count Badges (Strictly 4 categories, taking search scope into account)
  function updateFilterCounts(candidateItems) {
    const scopeFiltered = candidateItems.filter(itemMatchesScope);
    const counts = {
      all: scopeFiltered.length,
      file: scopeFiltered.filter((i) => itemMatchesFilter(i, "file")).length,
      app: scopeFiltered.filter((i) => itemMatchesFilter(i, "app")).length,
      clipboard: scopeFiltered.filter((i) => i.category === "剪贴板" || i.type === "clipboard").length
    };
    const cAll = document.getElementById("countAll");
    const cFile = document.getElementById("countFile");
    const cApp = document.getElementById("countApp");
    const cClipboard = document.getElementById("countClipboard");
    if (cAll) cAll.textContent = counts.all;
    if (cFile) cFile.textContent = counts.file;
    if (cApp) cApp.textContent = counts.app;
    if (cClipboard) cClipboard.textContent = counts.clipboard;
  }

  // Set Category Filter
  function setCategoryFilter(filter) {
    state.activeFilter = filter;
    state.selectedIndex = 0;
    const tabs = el.categoryTabs ? el.categoryTabs.querySelectorAll(".category-tab") : [];
    tabs.forEach((tab) => {
      tab.classList.toggle("active", tab.dataset.filter === filter);
    });

    // Secondary sub-filter bar visibility for clipboard
    if (filter === "clipboard") {
      el.clipboardSubFilterBar?.classList.add("visible");
    } else {
      el.clipboardSubFilterBar?.classList.remove("visible");
    }

    updateSearchResults();
  }

  // Set Clipboard Secondary Sub-filter
  function setClipboardSubFilter(subFilter) {
    state.clipboardSubFilter = subFilter;
    state.selectedIndex = 0;
    if (el.clipboardSubFilterChips) {
      el.clipboardSubFilterChips.querySelectorAll(".sub-filter-chip").forEach((chip) => {
        chip.classList.toggle("active", chip.dataset.subfilter === subFilter);
      });
    }
    updateSearchResults();
    const labelMap = { all: "全部", text: "纯文本", code: "代码", url: "链接" };
    showToast(`剪贴板细分: ${labelMap[subFilter] || "全部"}`, "clipboard");
  }

  // Compute Search Results (最近使用和搜索结果互通，无搜索时使用最近使用，受搜索范围筛选约束)
  function computeResults() {
    const query = state.searchQuery.trim().toLowerCase();

    // 1. Idle State (Empty query) -> Use "最近使用" (Recent items)
    if (!query) {
      el.aiIntentBar.classList.remove("visible");
      
      // Update tab counts based on MockDatabase.recent
      updateFilterCounts(MockDatabase.recent);

      // Filter recent items according to scope and activeFilter
      const scopeRecent = MockDatabase.recent.filter(itemMatchesScope);
      const filteredRecent = scopeRecent.filter((item) => itemMatchesFilter(item, state.activeFilter));
      
      return filteredRecent.map((item) => ({
        ...item,
        section: "最近使用"
      }));
    }

    // 2. Smart / AI Search Mode
    if (state.searchMode === "smart") {
      el.aiIntentBar.classList.add("visible");
      
      // Match AI scenario or generic AI semantic mock
      const matchedScenario = MockDatabase.aiScenarios.find((s) =>
        s.query.toLowerCase().includes(query) || query.includes("easynote") || query.includes("文件") || query.includes("存")
      ) || MockDatabase.aiScenarios[0];

      // Render Intent Chips
      el.aiChipsList.innerHTML = matchedScenario.intentChips
        .map((c) => `<span class="ai-chip"><span class="chip-key">${c.key}:</span> ${c.val}</span>`)
        .join("");

      const aiPool = matchedScenario.results;
      updateFilterCounts(aiPool);

      const filteredAi = aiPool
        .filter(itemMatchesScope)
        .filter((item) => itemMatchesFilter(item, state.activeFilter));
      return filteredAi.map((r) => ({
        ...r,
        section: "智能匹配"
      }));
    }

    // 3. Fast Local Search Mode
    el.aiIntentBar.classList.remove("visible");
    const rawMatched = MockDatabase.allItems.filter((item) => {
      const matchTitle = item.title.toLowerCase().includes(query);
      const matchSub = item.subtitle && item.subtitle.toLowerCase().includes(query);
      const matchKw = item.keywords && item.keywords.some((k) => k.toLowerCase().includes(query));
      return matchTitle || matchSub || matchKw;
    });

    updateFilterCounts(rawMatched);

    const matched = rawMatched.filter(itemMatchesScope);
    const filtered = matched.filter((item) => itemMatchesFilter(item, state.activeFilter));

    // Group into sections: 应用 -> 文件 -> 剪贴板 (严格4类归并)
    const sectionOrder = ["应用", "文件", "剪贴板"];
    const grouped = [];

    sectionOrder.forEach((sec) => {
      const secItems = filtered.filter((i) => i.category === sec);
      secItems.forEach((i) => grouped.push({ ...i, section: sec }));
    });

    // In case any item didn't match sectionOrder
    filtered.forEach((i) => {
      if (!sectionOrder.includes(i.category)) {
        grouped.push({ ...i, section: i.category || "其他" });
      }
    });

    return grouped;
  }

  // Render List View Items HTML (Tinycast 42px 列表模式，带 28px Squircle 图标)
  function renderListItems(items, startIndex = 0) {
    let html = "";
    items.forEach((item, i) => {
      const index = startIndex + i;
      const isSelected = index === state.selectedIndex;
      const titleHighlighted = highlightMatch(item.title, state.searchQuery);
      const glyph = getItemGlyph(item);
      html += `
        <div class="search-item ${isSelected ? "selected" : ""}" data-index="${index}">
          ${renderSquircleIcon(glyph, 28)}
          <div class="item-main-col">
            <span class="item-title">${titleHighlighted}</span>
            <span class="item-badge">${item.badge || item.category || item.type}</span>
          </div>
          <div class="item-path-col">${item.subtitle || ""}</div>
          <div class="item-action-col">
            <span>${item.action || "打开"}</span>
          </div>
        </div>
      `;
    });
    return html;
  }

  // Render Grid View Items HTML (Tinycast 大图标卡片网格模式)
  function renderGridItems(items, startIndex = 0) {
    let html = `<div class="items-icon-grid">`;
    items.forEach((item, i) => {
      const index = startIndex + i;
      const isSelected = index === state.selectedIndex;
      const titleHighlighted = highlightMatch(item.title, state.searchQuery);
      const glyph = getItemGlyph(item);
      html += `
        <div class="icon-card-item ${isSelected ? "selected" : ""}" data-index="${index}" title="${escapeHtml(item.title)} (${escapeHtml(item.subtitle || '')})">
          <div class="icon-card-glyph">
            ${renderSquircleIcon(glyph, 38)}
          </div>
          <span class="icon-card-title">${titleHighlighted}</span>
          <span class="icon-card-badge">${item.badge || item.category || item.type}</span>
        </div>
      `;
    });
    html += `</div>`;
    return html;
  }

  // Render Search Results into DOM (Dual Views: Icon vs List, Collapsible Shelf Above Tabs)
  function updateSearchResults() {
    renderPinnedShelf();

    state.currentResults = computeResults();
    if (state.selectedIndex >= state.currentResults.length) {
      state.selectedIndex = Math.max(0, state.currentResults.length - 1);
    }

    el.searchClearBtn.classList.toggle("visible", state.searchQuery.length > 0);

    if (state.currentResults.length === 0) {
      el.resultPane.innerHTML = `
        <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; color: var(--text-tertiary); gap: 12px; padding: 40px 0;">
          <div style="width: 48px; height: 48px; opacity: 0.5;">${Icons.search}</div>
          <div style="font-size: var(--font-size-base);">未找到匹配 "${escapeHtml(state.searchQuery)}" 的项目</div>
          <div style="font-size: var(--font-size-xs);">尝试切换分类标签 或 按 Tab 切换至 ✦ 智能搜索模式</div>
        </div>
      `;
      return;
    }

    // Group into section headers
    const sections = [];
    state.currentResults.forEach((item) => {
      if (!sections.includes(item.section)) sections.push(item.section);
    });

    let html = "";
    let offset = 0;
    sections.forEach((sec) => {
      const secItems = state.currentResults.filter((r) => r.section === sec);
      html += `
        <div class="section-header">
          <span>${sec}</span>
        </div>
      `;

      if (state.viewMode === "grid") {
        html += renderGridItems(secItems, offset);
      } else {
        html += renderListItems(secItems, offset);
      }
      offset += secItems.length;
    });

    el.resultPane.innerHTML = html;

    // Attach Click, Double-click (System Launch with Prompt), and Contextmenu Listeners
    const itemEls = el.resultPane.querySelectorAll("[data-index]");
    itemEls.forEach((itemEl) => {
      const idx = parseInt(itemEl.dataset.index, 10);
      itemEl.addEventListener("click", () => {
        state.selectedIndex = idx;
        renderSelection();
      });
      // Double click triggers System Open with visual toast prompt
      itemEl.addEventListener("dblclick", () => {
        state.selectedIndex = idx;
        renderSelection();
        launchViaSystem(state.currentResults[idx], itemEl);
      });
      itemEl.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        state.selectedIndex = idx;
        renderSelection();
        openContextMenu(e.clientX, e.clientY);
      });
    });

    renderSelection();
  }

  // Update Item Selection Visuals & Scroll into View
  function renderSelection() {
    const itemEls = el.resultPane.querySelectorAll("[data-index]");
    itemEls.forEach((itemEl) => {
      const idx = parseInt(itemEl.dataset.index, 10);
      const isSelected = idx === state.selectedIndex;
      itemEl.classList.toggle("selected", isSelected);
      if (isSelected) {
        itemEl.scrollIntoView({ block: "nearest", behavior: "smooth" });
      }
    });

    // Update Tinycast Footer Action Label & Count dynamically
    const selectedItem = state.currentResults[state.selectedIndex];
    let actionLabel = "立即打开";
    if (selectedItem) {
      if (selectedItem.type === "clipboard" || selectedItem.category === "剪贴板") {
        actionLabel = "复制到剪贴板";
      } else if (selectedItem.type === "app" || selectedItem.category === "应用") {
        actionLabel = "打开应用";
      } else if (selectedItem.badge === "文件夹" || selectedItem.type === "folder") {
        actionLabel = "打开文件夹";
      } else {
        actionLabel = "打开文件";
      }
    }
    if (el.footerActionLabel) el.footerActionLabel.textContent = actionLabel;
    if (el.actionItemOpenLabel) el.actionItemOpenLabel.textContent = actionLabel;
    if (el.footerItemCount) el.footerItemCount.textContent = `${state.currentResults.length} 项就绪`;
  }

  // Update Preview Pane (Fixed split column: updates content without changing window geometry)
  function updatePreview(item) {
    if (!el.previewPanel || !el.previewContent) return;

    if (!item) {
      el.previewContent.innerHTML = `
        <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; color: var(--text-tertiary); font-size: 12px; gap: 6px; padding: 24px 0;">
          <span>无选中项目</span>
        </div>
      `;
      return;
    }

    // 1. Files & Code/Markdown Snippets
    if (item.type === "file" || (item.preview && (item.preview.type === "markdown" || item.preview.type === "code" || item.preview.content))) {
      el.previewContent.innerHTML = `
        <div class="preview-file-header">
          <div class="preview-file-title-row">
            <span class="preview-file-name">${escapeHtml(item.title)}</span>
            <span class="preview-file-badge">${escapeHtml(item.badge || item.type)}</span>
          </div>
          <div class="preview-file-path" title="${escapeHtml(item.preview?.path || item.subtitle || '')}">${escapeHtml(item.preview?.path || item.subtitle || '')}</div>
          <div class="preview-file-meta">
            ${item.preview?.size ? `<span>大小: ${item.preview.size}</span>` : ""}
            ${item.preview?.modified ? `<span>· 修改: ${item.preview.modified}</span>` : ""}
            ${item.preview?.time ? `<span>· 时间: ${item.preview.time}</span>` : ""}
          </div>
        </div>
        ${item.preview?.content ? `
          <div class="preview-code-block">${escapeHtml(item.preview.content)}</div>
        ` : `
          <div class="preview-details-card">
            <div class="preview-meta-row">
              <span class="preview-meta-label">文件类型</span>
              <span class="preview-meta-value">${escapeHtml(item.title.split('.').pop()?.toUpperCase() || "FILE")}</span>
            </div>
            <div class="preview-meta-row">
              <span class="preview-meta-label">路径</span>
              <span class="preview-meta-value">${escapeHtml(item.preview?.path || item.subtitle || "")}</span>
            </div>
          </div>
        `}
        <div class="preview-shortcuts-section">
          <div class="preview-shortcuts-title">可用快捷操作</div>
          <div class="preview-shortcut-row">
            <span>打开文件</span>
            <kbd>Enter</kbd>
          </div>
          <div class="preview-shortcut-row">
            <span>在文件夹中定位</span>
            <kbd>Ctrl+O</kbd>
          </div>
          <div class="preview-shortcut-row">
            <span>复制完整路径</span>
            <kbd>Ctrl+C</kbd>
          </div>
        </div>
      `;
      return;
    }

    // 2. Applications
    if (item.type === "app") {
      el.previewContent.innerHTML = `
        <div class="preview-file-header">
          <div class="preview-file-title-row">
            <span class="preview-file-name">${escapeHtml(item.title)}</span>
            <span class="preview-file-badge">${escapeHtml(item.badge || "应用")}</span>
          </div>
          <div class="preview-file-path" title="${escapeHtml(item.preview?.path || item.subtitle || '')}">${escapeHtml(item.preview?.path || item.subtitle || '')}</div>
        </div>
        <div class="preview-details-card">
          <div class="preview-meta-row">
            <span class="preview-meta-label">类型</span>
            <span class="preview-meta-value">本地应用程序</span>
          </div>
          ${item.preview?.version ? `
            <div class="preview-meta-row">
              <span class="preview-meta-label">版本</span>
              <span class="preview-meta-value">${escapeHtml(item.preview.version)}</span>
            </div>
          ` : ""}
          <div class="preview-meta-row">
            <span class="preview-meta-label">运行状态</span>
            <span class="preview-meta-value" style="color: #34d399;">就绪</span>
          </div>
          <div class="preview-meta-row">
            <span class="preview-meta-label">执行程序</span>
            <span class="preview-meta-value">${escapeHtml(item.subtitle || "")}</span>
          </div>
        </div>
        <div class="preview-shortcuts-section">
          <div class="preview-shortcuts-title">可用快捷操作</div>
          <div class="preview-shortcut-row">
            <span>立即启动</span>
            <kbd>Enter</kbd>
          </div>
          <div class="preview-shortcut-row">
            <span>在文件夹中定位</span>
            <kbd>Ctrl+O</kbd>
          </div>
          <div class="preview-shortcut-row">
            <span>固定 / 取消固定</span>
            <kbd>Ctrl+P</kbd>
          </div>
        </div>
      `;
      return;
    }

    // 3. Folders
    if (item.type === "folder") {
      el.previewContent.innerHTML = `
        <div class="preview-file-header">
          <div class="preview-file-title-row">
            <span class="preview-file-name">${escapeHtml(item.title)}</span>
            <span class="preview-file-badge">${escapeHtml(item.badge || "文件夹")}</span>
          </div>
          <div class="preview-file-path" title="${escapeHtml(item.preview?.path || item.subtitle || '')}">${escapeHtml(item.preview?.path || item.subtitle || '')}</div>
        </div>
        <div class="preview-details-card">
          <div class="preview-meta-row">
            <span class="preview-meta-label">类型</span>
            <span class="preview-meta-value">本地文件目录</span>
          </div>
          ${item.preview?.size ? `
            <div class="preview-meta-row">
              <span class="preview-meta-label">占用大小</span>
              <span class="preview-meta-value">${escapeHtml(item.preview.size)}</span>
            </div>
          ` : ""}
          ${item.preview?.modified ? `
            <div class="preview-meta-row">
              <span class="preview-meta-label">最近访问</span>
              <span class="preview-meta-value">${escapeHtml(item.preview.modified)}</span>
            </div>
          ` : ""}
        </div>
        <div class="preview-shortcuts-section">
          <div class="preview-shortcuts-title">可用快捷操作</div>
          <div class="preview-shortcut-row">
            <span>打开文件夹</span>
            <kbd>Enter</kbd>
          </div>
          <div class="preview-shortcut-row">
            <span>复制路径</span>
            <kbd>Ctrl+C</kbd>
          </div>
          <div class="preview-shortcut-row">
            <span>固定到快捷访问</span>
            <kbd>Ctrl+P</kbd>
          </div>
        </div>
      `;
      return;
    }

    // 4. Clipboard & Others
    el.previewContent.innerHTML = `
      <div class="preview-file-header">
        <div class="preview-file-title-row">
          <span class="preview-file-name">${escapeHtml(item.title)}</span>
          <span class="preview-file-badge">${escapeHtml(item.badge || "剪贴板")}</span>
        </div>
        <div class="preview-file-meta">
          ${item.preview?.time ? `<span>时间: ${item.preview.time}</span>` : ""}
          ${item.preview?.charCount ? `<span>· 字符: ${item.preview.charCount}</span>` : ""}
        </div>
      </div>
      ${item.preview?.content ? `
        <div class="preview-code-block">${escapeHtml(item.preview.content)}</div>
      ` : ""}
      <div class="preview-shortcuts-section">
        <div class="preview-shortcuts-title">可用快捷操作</div>
        <div class="preview-shortcut-row">
          <span>复制到剪贴板</span>
          <kbd>Enter</kbd>
        </div>
      </div>
    `;
  }

  // Execute Action for Selected Item (调用系统打开并弹出提示)
  function executeItemAction(item) {
    if (!item) return;
    const currentEl = el.resultPane.querySelector(`[data-index="${state.selectedIndex}"]`);
    launchViaSystem(item, currentEl);
  }

  // Pin / Unpin Item
  function togglePin(item) {
    if (!item) return;
    item.isPinned = !item.isPinned;
    showToast(item.isPinned ? `已固定到快速访问: ${item.title}` : `已取消固定: ${item.title}`, "pin");
    updateSearchResults();
  }

  // Copy Path
  function copyItemPath(item) {
    if (!item) return;
    const text = item.preview?.path || item.subtitle || item.title;
    copyToClipboard(text);
    showToast(`已复制路径: ${text}`, "copy");
  }

  function copyToClipboard(text) {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(text);
    }
  }

  function escapeHtml(str) {
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  // Context Menu Management (Unified Fluent Action Menu)
  function openContextMenu(x, y, targetItem = null) {
    const selectedItem = targetItem || state.currentResults[state.selectedIndex];
    if (!selectedItem) return;
    state.contextTargetItem = selectedItem;

    // Update dynamic labels in context menu
    const openLabel = document.getElementById("ctxOpenLabel");
    if (openLabel) {
      if (selectedItem.type === "clipboard" || selectedItem.category === "剪贴板") {
        openLabel.textContent = "复制到剪贴板";
      } else if (selectedItem.type === "app" || selectedItem.category === "应用") {
        openLabel.textContent = "立即启动";
      } else if (selectedItem.badge === "文件夹" || selectedItem.type === "folder") {
        openLabel.textContent = "打开文件夹";
      } else {
        openLabel.textContent = "立即打开";
      }
    }

    // Configure "进入此文件夹查找" in context menu
    const isFolder = selectedItem.badge === "文件夹" || selectedItem.type === "folder" ||
      selectedItem.preview?.type === "folder" ||
      (selectedItem.subtitle && (selectedItem.subtitle.includes(":\\") || selectedItem.subtitle.includes(":/"))) ||
      selectedItem.title.toLowerCase().includes("project");
    const ctxEnterFolder = document.getElementById("ctxEnterFolder");
    if (ctxEnterFolder) {
      ctxEnterFolder.style.display = isFolder ? "flex" : "none";
      const enterLabel = document.getElementById("ctxEnterFolderLabel");
      if (enterLabel) {
        enterLabel.textContent = `进入「${selectedItem.title}」查找`;
      }
    }

    const pinLabel = document.getElementById("ctxPinLabel");
    if (pinLabel) {
      const isPinned = MockDatabase.pinned.some((p) => (selectedItem.id && p.id === selectedItem.id) || p.title === selectedItem.title);
      pinLabel.textContent = isPinned ? "取消置顶" : "固定到置顶";
    }

    state.showContextMenu = true;

    if (typeof x === "number" && typeof y === "number") {
      el.contextMenu.style.left = `${Math.min(x, window.innerWidth - 260)}px`;
      el.contextMenu.style.top = `${Math.min(y, window.innerHeight - 280)}px`;
    } else {
      const selectedEl = el.resultPane.querySelector(".search-item.selected, .icon-card-item.selected");
      if (selectedEl) {
        const rect = selectedEl.getBoundingClientRect();
        el.contextMenu.style.left = `${Math.min(rect.left + 20, window.innerWidth - 260)}px`;
        el.contextMenu.style.top = `${Math.min(rect.bottom + 6, window.innerHeight - 280)}px`;
      } else {
        el.contextMenu.style.left = "50%";
        el.contextMenu.style.top = "50%";
      }
    }
    el.contextMenu.classList.add("visible");
  }

  function closeContextMenu() {
    state.showContextMenu = false;
    el.contextMenu?.classList.remove("visible");
  }

  // Enter specific folder to search within it
  function enterFolderSearch(folderItem) {
    if (!folderItem) return;
    const folderTitle = folderItem.title;
    const folderPath = folderItem.preview?.path || folderItem.subtitle || folderTitle;

    state.searchScope.location = "folder-" + (folderItem.id || folderTitle);
    state.searchScope.locationName = folderTitle;
    state.searchScope.locationPath = folderPath;

    // Ensure filter shelf is open so user sees current scope
    if (state.isFilterShelfCollapsed) {
      toggleFilterShelf(true);
    }

    closeAllFilterMenus();
    updateFilterShelfUI();
    updateSearchResults();

    // Focus search input
    el.searchInput.focus();

    showToast(`已进入文件夹「${folderTitle}」限定检索`, "folder");
  }

  function toggleContextMenu(x, y) {
    if (state.showContextMenu) {
      closeContextMenu();
    } else {
      openContextMenu(x, y);
    }
  }

  // Keyboard Event Handlers (Keyboard-first principle)
  function handleKeyDown(e) {
    // 0. Hotkey Recording Mode Interception
    if (state.recordingTarget) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        stopRecordingHotkey();
        showToast("已取消快捷键录制", "x");
        return;
      }
      const combo = parseKeyCombo(e);
      if (!combo) return; // Wait for non-modifier key
      e.preventDefault();
      e.stopPropagation();

      if (state.recordingTarget.type === "row") {
        const binding = (MockDatabase.hotkeys || []).find((h) => h.id === state.recordingTarget.id);
        if (binding) {
          const conflict = MockDatabase.hotkeys.find(
            (h) => h.id !== binding.id && h.hotkey.toLowerCase() === combo.toLowerCase()
          );
          if (conflict) {
            showToast(`⚠️ 快捷键 ${combo} 已被 "${conflict.title}" 绑定，请更换其他按键`, "lightning");
            return;
          }
          binding.hotkey = combo;
          showToast(`已更新快捷键: ${binding.title} -> ${combo}`, "check");
        }
      } else if (state.recordingTarget.type === "add") {
        state.newHotkeyCombo = combo;
        const preview = document.getElementById("newHotkeyKbdPreview");
        if (preview) preview.innerHTML = renderKbdCombo(combo);
        showToast(`已录制快捷键: ${combo}`, "check");
      }
      stopRecordingHotkey();
      return;
    }

    // 0.1 Global Hotkey Direct Dispatcher (模拟后台全局热键服务)
    if (state.hotkeysMasterEnabled !== false && !e.repeat && !state.recordingTarget) {
      const activeTag = document.activeElement ? document.activeElement.tagName.toLowerCase() : "";
      const isEditingInput = activeTag === "input" && document.activeElement !== el.searchInput;
      if (!isEditingInput) {
        const pressedCombo = parseKeyCombo(e);
        if (pressedCombo && (e.altKey || (e.ctrlKey && e.shiftKey))) {
          const matched = (MockDatabase.hotkeys || []).find(
            (h) => h.enabled && h.hotkey.toLowerCase() === pressedCombo.toLowerCase()
          );
          if (matched) {
            e.preventDefault();
            showToast(`🚀 [全局热键直达 ${matched.hotkey}] 正在唤醒启动: ${matched.title} (${matched.path})`, "lightning");
            return;
          }
        }
      }
    }

    // Esc: Close menu, modal, or clear search
    if (e.key === "Escape") {
      const openMenu = document.querySelector(".filter-dropdown-menu.visible");
      if (openMenu) {
        closeAllFilterMenus();
        e.preventDefault();
        return;
      }
      if (!state.isFilterShelfCollapsed) {
        toggleFilterShelf(false);
        e.preventDefault();
        return;
      }
      if (state.showContextMenu) {
        closeContextMenu();
        e.preventDefault();
        return;
      }
      if (state.showSettings) {
        closeSettingsModal();
        e.preventDefault();
        return;
      }
      if (state.searchQuery) {
        state.searchQuery = "";
        el.searchInput.value = "";
        updateSearchResults();
        e.preventDefault();
        return;
      }
      showToast("已按下 Esc (在真实应用中将极速收起居中搜索窗)", "eye");
      return;
    }

    // Tab: Enter Folder Scope if current item is folder, otherwise toggle search mode
    if (e.key === "Tab") {
      const curItem = state.currentResults[state.selectedIndex];
      const isFolder = curItem && (curItem.badge === "文件夹" || curItem.type === "folder" || curItem.preview?.type === "folder");
      if (isFolder) {
        e.preventDefault();
        enterFolderSearch(curItem);
        return;
      }
      e.preventDefault();
      setSearchMode(state.searchMode === "fast" ? "smart" : "fast");
      return;
    }

    // Alt+1..4: Fast Category Filter Switching (严格 4 类：全部 / 文件 / 应用 / 剪贴板)
    if (e.altKey && ["1", "2", "3", "4"].includes(e.key)) {
      e.preventDefault();
      const filterMap = {
        "1": "all",
        "2": "file",
        "3": "app",
        "4": "clipboard"
      };
      const nameMap = { all: "全部", file: "文件", app: "应用", clipboard: "剪贴板" };
      setCategoryFilter(filterMap[e.key]);
      showToast(`筛选: ${nameMap[filterMap[e.key]]}`, "check");
      return;
    }

    // Ctrl+1..4: Direct Pinned Item Launch (when in idle shelf)
    const isIdleAll = !state.searchQuery && state.activeFilter === "all";
    const pinnedCount = isIdleAll ? state.currentResults.filter((i) => i.isPinnedShelf).length : 0;

    if (e.ctrlKey && ["1", "2", "3", "4"].includes(e.key) && isIdleAll) {
      e.preventDefault();
      const pinIdx = parseInt(e.key, 10) - 1;
      if (state.currentResults[pinIdx]) {
        state.selectedIndex = pinIdx;
        renderSelection();
        executeItemAction(state.currentResults[pinIdx]);
      }
      return;
    }

    // Arrow Left / Right: Horizontal Navigation inside Pinned Shelf Grid or Icon Grid View
    if (e.key === "ArrowRight") {
      if (isIdleAll && state.selectedIndex < pinnedCount) {
        e.preventDefault();
        state.selectedIndex = (state.selectedIndex + 1) % pinnedCount;
        renderSelection();
        return;
      } else if (state.viewMode === "grid" && state.currentResults.length > 0) {
        e.preventDefault();
        state.selectedIndex = (state.selectedIndex + 1) % state.currentResults.length;
        renderSelection();
        return;
      }
    }

    if (e.key === "ArrowLeft") {
      if (isIdleAll && state.selectedIndex < pinnedCount) {
        e.preventDefault();
        state.selectedIndex = (state.selectedIndex - 1 + pinnedCount) % pinnedCount;
        renderSelection();
        return;
      } else if (state.viewMode === "grid" && state.currentResults.length > 0) {
        e.preventDefault();
        state.selectedIndex = (state.selectedIndex - 1 + state.currentResults.length) % state.currentResults.length;
        renderSelection();
        return;
      }
    }

    // Arrow Navigation Up / Down
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (state.currentResults.length > 0) {
        if (isIdleAll && state.selectedIndex < pinnedCount) {
          state.selectedIndex = pinnedCount; // jump from shelf down to recent list
        } else {
          state.selectedIndex = (state.selectedIndex + 1) % state.currentResults.length;
        }
        renderSelection();
      }
      return;
    }

    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (state.currentResults.length > 0) {
        if (isIdleAll && state.selectedIndex === pinnedCount) {
          state.selectedIndex = 0; // jump up from recent list into pinned shelf
        } else {
          state.selectedIndex = (state.selectedIndex - 1 + state.currentResults.length) % state.currentResults.length;
        }
        renderSelection();
      }
      return;
    }

    // Enter: Primary Action
    if (e.key === "Enter") {
      e.preventDefault();
      const selectedItem = state.currentResults[state.selectedIndex];
      if (selectedItem) {
        executeItemAction(selectedItem);
      }
      return;
    }

    // Ctrl+K: Toggle Context Action Menu
    if (e.ctrlKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      toggleContextMenu();
      return;
    }

    // Ctrl+O: Open in Explorer
    if (e.ctrlKey && e.key.toLowerCase() === "o") {
      e.preventDefault();
      const selectedItem = state.currentResults[state.selectedIndex];
      if (selectedItem) {
        showToast(`🚀 [系统调用] 已在资源管理器中定位: ${selectedItem.title}`, "folderOpen");
      }
      return;
    }

    // Ctrl+,: Preferences Modal
    if (e.ctrlKey && e.key === ",") {
      e.preventDefault();
      openSettingsModal();
      return;
    }

    // Ctrl+P: Toggle Pin
    if (e.ctrlKey && e.key.toLowerCase() === "p") {
      e.preventDefault();
      const selectedItem = state.currentResults[state.selectedIndex];
      if (selectedItem) togglePin(selectedItem);
      return;
    }

    // Ctrl+C: Copy Path / Content
    if (e.ctrlKey && e.key.toLowerCase() === "c" && document.activeElement !== el.searchInput) {
      e.preventDefault();
      const selectedItem = state.currentResults[state.selectedIndex];
      if (selectedItem) copyItemPath(selectedItem);
      return;
    }
  }

  // Hotkey & Key Combination Utilities
  function renderKbdCombo(combo) {
    if (!combo) return "";
    return combo
      .split("+")
      .map((k) => `<kbd class="hk-kbd">${escapeHtml(k.trim())}</kbd>`)
      .join(" + ");
  }

  function parseKeyCombo(e) {
    if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) {
      return null;
    }
    const parts = [];
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    if (e.metaKey) parts.push("Win");

    let key = e.key;
    if (key === " ") key = "Space";
    else if (key.length === 1) key = key.toUpperCase();
    parts.push(key);

    return parts.join("+");
  }

  // Start recording hotkey
  function startRecordingHotkey(type, id = null) {
    state.recordingTarget = { type, id };
    if (type === "row") {
      renderHotkeyBindings();
      showToast("正在录制快捷键，请在键盘上按下新的组合键 (按 Esc 取消)", "lightning");
    } else if (type === "add") {
      const box = document.getElementById("newHotkeyRecordBox");
      const preview = document.getElementById("newHotkeyKbdPreview");
      if (box && preview) {
        box.classList.add("recording");
        preview.innerHTML = `<span class="hk-kbd" style="color: #ef4444; font-weight: 600;">按下快捷键组合...</span>`;
      }
      showToast("正在录制快捷键，请在键盘上按下新的组合键 (按 Esc 取消)", "lightning");
    }
  }

  // Stop recording hotkey
  function stopRecordingHotkey() {
    if (!state.recordingTarget) return;
    const prevType = state.recordingTarget.type;
    state.recordingTarget = null;
    if (prevType === "row") {
      renderHotkeyBindings();
    } else if (prevType === "add") {
      const box = document.getElementById("newHotkeyRecordBox");
      const preview = document.getElementById("newHotkeyKbdPreview");
      if (box && preview) {
        box.classList.remove("recording");
        preview.innerHTML = renderKbdCombo(state.newHotkeyCombo || "Alt+K");
      }
    }
  }

  // Test Run a Hotkey
  function testRunHotkey(id) {
    const item = (MockDatabase.hotkeys || []).find((h) => h.id === id);
    if (!item) return;
    showToast(`🚀 [全局热键直达 ${item.hotkey}] 正在唤醒启动: ${item.title} (${item.path})`, "lightning");
  }

  // Toggle Hotkey Enabled
  function toggleHotkeyEnabled(id) {
    const item = (MockDatabase.hotkeys || []).find((h) => h.id === id);
    if (!item) return;
    item.enabled = !item.enabled;
    renderHotkeyBindings();
    showToast(`${item.title} 快捷直达已${item.enabled ? "启用" : "禁用"}`, item.enabled ? "check" : "x");
  }

  // Delete Hotkey Binding
  function deleteHotkeyBinding(id) {
    const idx = (MockDatabase.hotkeys || []).findIndex((h) => h.id === id);
    if (idx === -1) return;
    const removed = MockDatabase.hotkeys.splice(idx, 1)[0];
    renderHotkeyBindings();
    showToast(`已移除快捷直达绑定: ${removed.title}`, "trash");
  }

  // Render Hotkey Bindings List in Settings Modal
  function renderHotkeyBindings() {
    const container = document.getElementById("hotkeysListContainer");
    if (!container) return;

    const list = MockDatabase.hotkeys || [];
    if (list.length === 0) {
      container.innerHTML = `
        <div style="text-align: center; padding: 24px 10px; color: var(--text-tertiary); font-size: var(--font-size-xs);">
          暂无快捷直达绑定项目，点击上方 "+ 添加绑定" 开始配置
        </div>
      `;
      return;
    }

    let html = "";
    list.forEach((hk) => {
      const glyph = getItemGlyph({
        type: hk.type,
        title: hk.title,
        subtitle: hk.path
      });
      const badgeText = hk.badge || (hk.type === "app" ? "应用" : hk.type === "folder" ? "文件夹" : "文档");
      const isRecordingThis = state.recordingTarget && state.recordingTarget.type === "row" && state.recordingTarget.id === hk.id;
      const kbdHtml = isRecordingThis
        ? `<span class="hk-kbd" style="color: #ef4444; font-weight: 600;">按下按键...</span>`
        : renderKbdCombo(hk.hotkey);

      html += `
        <div class="hotkey-binding-row ${hk.enabled ? "" : "disabled"}" data-hk-row-id="${hk.id}">
          <div class="hotkey-row-left">
            <div class="hotkey-glyph">
              ${renderSquircleIcon(glyph, 32)}
            </div>
            <div class="hotkey-item-info">
              <div class="hotkey-title-line">
                <span class="hotkey-name" title="${escapeHtml(hk.title)}">${escapeHtml(hk.title)}</span>
                <span class="hotkey-type-badge">${escapeHtml(badgeText)}</span>
              </div>
              <div class="hotkey-path-desc" title="${escapeHtml(hk.path)}">${escapeHtml(hk.path)}</div>
            </div>
          </div>
          <div class="hotkey-row-right">
            <span class="hotkey-badge ${isRecordingThis ? "recording" : ""}" data-hk-record="${hk.id}" title="点击重新录制快捷键">
              ${kbdHtml}
            </span>
            <button class="hotkey-action-btn hotkey-test-btn" data-hk-test="${hk.id}" title="测试运行此快捷键绑定">
              ${Icons.play}
            </button>
            <div class="switch-toggle ${hk.enabled ? "active" : ""}" data-hk-toggle="${hk.id}" title="${hk.enabled ? "点击禁用" : "点击启用"}">
              <div class="switch-toggle-knob"></div>
            </div>
            <button class="hotkey-action-btn hotkey-delete-btn" data-hk-del="${hk.id}" title="删除绑定">
              ${Icons.trash}
            </button>
          </div>
        </div>
      `;
    });

    container.innerHTML = html;

    // Attach row events
    container.querySelectorAll("[data-hk-record]").forEach((badge) => {
      badge.addEventListener("click", (e) => {
        e.stopPropagation();
        const id = badge.dataset.hkRecord;
        startRecordingHotkey("row", id);
      });
    });

    container.querySelectorAll("[data-hk-test]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const id = btn.dataset.hkTest;
        testRunHotkey(id);
      });
    });

    container.querySelectorAll("[data-hk-toggle]").forEach((sw) => {
      sw.addEventListener("click", (e) => {
        e.stopPropagation();
        const id = sw.dataset.hkToggle;
        toggleHotkeyEnabled(id);
      });
    });

    container.querySelectorAll("[data-hk-del]").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const id = btn.dataset.hkDel;
        deleteHotkeyBinding(id);
      });
    });
  }

  // Initialize Hotkey Panel Form & Controls
  function initHotkeyPanelEvents() {
    const addBtn = document.getElementById("addHotkeyBtn");
    const panel = document.getElementById("addHotkeyPanel");
    const closeBtn = document.getElementById("closeAddHotkeyBtn");
    const cancelBtn = document.getElementById("cancelAddHotkeyBtn");
    const saveBtn = document.getElementById("saveNewHotkeyBtn");
    const chooseRecentBtn = document.getElementById("chooseFromRecentBtn");
    const recordBox = document.getElementById("newHotkeyRecordBox");
    const masterToggle = document.getElementById("globalHotkeyMasterToggle");

    if (addBtn && panel) {
      addBtn.addEventListener("click", () => {
        const isHidden = panel.style.display === "none";
        panel.style.display = isHidden ? "block" : "none";
        if (isHidden) {
          document.getElementById("newHotkeyTitle")?.focus();
        }
      });
    }

    function hideAddPanel() {
      if (panel) panel.style.display = "none";
      stopRecordingHotkey();
    }

    closeBtn?.addEventListener("click", hideAddPanel);
    cancelBtn?.addEventListener("click", hideAddPanel);

    recordBox?.addEventListener("click", (e) => {
      e.stopPropagation();
      startRecordingHotkey("add");
    });

    chooseRecentBtn?.addEventListener("click", () => {
      const candidates = MockDatabase.recent.filter(
        (r) => !MockDatabase.hotkeys.some((h) => h.title === r.title || (r.preview?.path && h.path === r.preview.path))
      );
      const chosen = candidates[0] || MockDatabase.recent[0];
      if (chosen) {
        const titleInput = document.getElementById("newHotkeyTitle");
        const pathInput = document.getElementById("newHotkeyPath");
        const typeSelect = document.getElementById("newHotkeyType");
        if (titleInput) titleInput.value = chosen.title;
        if (pathInput) pathInput.value = chosen.preview?.path || chosen.subtitle || chosen.title;
        if (typeSelect) {
          typeSelect.value = chosen.type === "app" ? "app" : chosen.badge === "文件夹" ? "folder" : "file";
        }
        showToast(`已从最近记录填入: ${chosen.title}`, "check");
      }
    });

    saveBtn?.addEventListener("click", () => {
      const titleInput = document.getElementById("newHotkeyTitle");
      const pathInput = document.getElementById("newHotkeyPath");
      const typeSelect = document.getElementById("newHotkeyType");

      const title = titleInput?.value.trim();
      const path = pathInput?.value.trim();
      const type = typeSelect?.value || "app";
      const hotkey = state.newHotkeyCombo || "Alt+K";

      if (!title) {
        showToast("请输入绑定目标名称", "x");
        titleInput?.focus();
        return;
      }
      if (!path) {
        showToast("请输入程序或文件路径", "x");
        pathInput?.focus();
        return;
      }

      // Check conflict
      const conflict = MockDatabase.hotkeys.find((h) => h.hotkey.toLowerCase() === hotkey.toLowerCase());
      if (conflict) {
        showToast(`⚠️ 快捷键 ${hotkey} 与 "${conflict.title}" 冲突，请更换组合键`, "lightning");
        return;
      }

      const newBinding = {
        id: "hk-" + Date.now(),
        title,
        type,
        category: type === "app" ? "应用" : "文件",
        badge: type === "app" ? "应用" : type === "folder" ? "文件夹" : "文档",
        path,
        hotkey,
        enabled: true,
        launchMode: type === "app" ? "activate" : "open",
        description: `全局直达 ${title}`
      };

      MockDatabase.hotkeys.push(newBinding);
      renderHotkeyBindings();
      hideAddPanel();
      if (titleInput) titleInput.value = "";
      if (pathInput) pathInput.value = "";
      showToast(`🎉 已成功绑定快捷键: ${hotkey} -> ${title}`, "check");
    });

    masterToggle?.addEventListener("click", () => {
      state.hotkeysMasterEnabled = masterToggle.classList.contains("active");
      showToast(state.hotkeysMasterEnabled ? "全局快捷直达服务已开启" : "全局快捷直达服务已暂停", "lightning");
    });
  }

  // Expandable Search Filter Shelf Management (单行极简、纯透明、Notion 风格下拉胶囊)
  function toggleFilterShelf(force) {
    state.isFilterShelfCollapsed = typeof force === "boolean" ? !force : !state.isFilterShelfCollapsed;
    localStorage.setItem("anycast_filter_shelf_collapsed", String(state.isFilterShelfCollapsed));

    if (el.searchFilterShelf) {
      el.searchFilterShelf.classList.toggle("collapsed", state.isFilterShelfCollapsed);
    }
    if (el.toggleFilterShelfBtn) {
      el.toggleFilterShelfBtn.classList.toggle("open", !state.isFilterShelfCollapsed);
    }
    if (state.isFilterShelfCollapsed) {
      closeAllFilterMenus();
    }
  }

  function closeAllFilterMenus() {
    document.querySelectorAll(".filter-dropdown-menu").forEach((m) => m.classList.remove("visible"));
    document.querySelectorAll(".filter-pill").forEach((p) => p.classList.remove("open"));
  }

  // Calendar Helpers & Formatting
  function formatShortDate(dateStr) {
    if (!dateStr) return "";
    const parts = dateStr.split("-");
    return `${parseInt(parts[1], 10)}月${parseInt(parts[2], 10)}日`;
  }

  function formatPillDate(dateStr) {
    if (!dateStr) return "";
    const parts = dateStr.split("-");
    return `${parseInt(parts[1], 10)}/${parseInt(parts[2], 10)}`;
  }

  function calculateDaysBetween(startStr, endStr) {
    if (!startStr || !endStr) return 1;
    const d1 = new Date(startStr);
    const d2 = new Date(endStr);
    return Math.round(Math.abs((d2 - d1) / (1000 * 60 * 60 * 24))) + 1;
  }

  // Render Fluent Calendar Grid & Date Range Selection
  function renderCalendarGrid() {
    const grid = document.getElementById("calDaysGrid");
    const monthTitle = document.getElementById("calMonthTitle");
    const summary = document.getElementById("calRangeSummary");
    const applyBtn = document.getElementById("calApplyBtn");
    if (!grid) return;

    if (monthTitle) {
      monthTitle.textContent = `${state.calViewYear}年 ${state.calViewMonth + 1}月`;
    }

    grid.innerHTML = "";

    const year = state.calViewYear;
    const month = state.calViewMonth;

    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const firstDayIndex = (new Date(year, month, 1).getDay() + 6) % 7; // Mon=0..Sun=6
    const prevMonthDays = new Date(year, month, 0).getDate();

    // Previous month padding
    for (let i = firstDayIndex - 1; i >= 0; i--) {
      const d = prevMonthDays - i;
      const cell = document.createElement("div");
      cell.className = "cal-day-cell other-month";
      cell.textContent = String(d);
      grid.appendChild(cell);
    }

    // Determine current effective range for rendering
    let effectiveStart = state.calRange.start;
    let effectiveEnd = state.calRange.end;

    if (effectiveStart && !effectiveEnd && state.calRange.hover) {
      if (state.calRange.hover < effectiveStart) {
        effectiveEnd = effectiveStart;
        effectiveStart = state.calRange.hover;
      } else {
        effectiveEnd = state.calRange.hover;
      }
    }

    // Current month days
    for (let d = 1; d <= daysInMonth; d++) {
      const cell = document.createElement("div");
      cell.className = "cal-day-cell";
      cell.textContent = String(d);

      const dateStr = `${year}-${String(month + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
      cell.dataset.date = dateStr;

      // Highlight today (2026-09-21)
      if (year === 2026 && month === 8 && d === 21) {
        cell.classList.add("today");
      }

      // Range highlight classes
      if (effectiveStart && dateStr === effectiveStart) {
        cell.classList.add("range-start");
      }
      if (effectiveEnd && dateStr === effectiveEnd) {
        cell.classList.add("range-end");
      }
      if (effectiveStart && effectiveEnd && dateStr > effectiveStart && dateStr < effectiveEnd) {
        cell.classList.add("in-range");
      }

      // Click event
      cell.addEventListener("click", (e) => {
        e.stopPropagation();
        handleCalendarDayClick(dateStr);
      });

      // Hover event
      cell.addEventListener("mouseenter", () => {
        if (state.calRange.start && !state.calRange.end) {
          state.calRange.hover = dateStr;
          renderCalendarGrid();
        }
      });

      grid.appendChild(cell);
    }

    // Trailing padding to fill complete weeks
    const totalCells = firstDayIndex + daysInMonth;
    const remainder = totalCells % 7;
    if (remainder !== 0) {
      const nextDays = 7 - remainder;
      for (let n = 1; n <= nextDays; n++) {
        const cell = document.createElement("div");
        cell.className = "cal-day-cell other-month";
        cell.textContent = String(n);
        grid.appendChild(cell);
      }
    }

    // Update footer summary
    if (summary) {
      if (state.calRange.start && state.calRange.end) {
        const days = calculateDaysBetween(state.calRange.start, state.calRange.end);
        summary.innerHTML = `<span class="active-summary">${formatShortDate(state.calRange.start)} ~ ${formatShortDate(state.calRange.end)} (共${days}天)</span>`;
      } else if (state.calRange.start) {
        summary.innerHTML = `<span class="active-summary">起点: ${formatShortDate(state.calRange.start)}，请选结束</span>`;
      } else {
        summary.innerHTML = `<span class="cal-status-text">点击选择起止日期范围</span>`;
      }
    }

    // Apply button state
    if (applyBtn) {
      applyBtn.disabled = !state.calRange.start;
    }
  }

  function handleCalendarDayClick(dateStr) {
    if (!state.calRange.start || (state.calRange.start && state.calRange.end)) {
      // Phase 1: Set start
      state.calRange.start = dateStr;
      state.calRange.end = null;
      state.calRange.hover = null;
    } else {
      // Phase 2: Set end
      if (dateStr < state.calRange.start) {
        state.calRange.end = state.calRange.start;
        state.calRange.start = dateStr;
      } else {
        state.calRange.end = dateStr;
      }
      state.calRange.hover = null;
    }
    renderCalendarGrid();
  }

  function updateFilterShelfUI() {
    const { time, type, location } = state.searchScope;
    const isFiltered = time !== "all" || type !== "all" || location !== "all";

    if (el.toggleFilterShelfBtn) {
      el.toggleFilterShelfBtn.classList.toggle("has-active-filter", isFiltered);
    }

    const timeLabels = { all: "时间", today: "今天", "7days": "7天内", "30days": "30天内", year: "1年内" };
    const typeLabels = { all: "类型", app: "应用", document: "文档", code: "代码", media: "媒体", clipboard: "剪贴板" };
    const locLabels = {
      all: "位置",
      "drive-c": "C: 盘",
      "drive-d": "D: 盘",
      desktop: "桌面",
      downloads: "下载",
      documents: "文档",
      projects: "工程",
      "folder-projects": "Projects",
      "folder-anycast": "Anycast",
      "folder-easynote": "EasyNote"
    };

    // Update Time Pill
    const pillTime = document.getElementById("pillFilterTime");
    if (pillTime) {
      const isPillActive = time !== "all";
      pillTime.classList.toggle("active", isPillActive);
      const label = pillTime.querySelector(".pill-label");
      if (label) {
        if (time === "range" && state.searchScope.customRange) {
          const { start, end } = state.searchScope.customRange;
          label.textContent = start === end ? formatPillDate(start) : `${formatPillDate(start)}-${formatPillDate(end)}`;
        } else {
          label.textContent = timeLabels[time] || "时间";
        }
      }
      const action = pillTime.querySelector(".pill-action");
      if (action) {
        action.innerHTML = isPillActive
          ? `<span class="pill-clear" title="清除时间筛选">×</span>`
          : `<svg class="pill-chevron" viewBox="0 0 24 24" width="9" height="9" fill="none" stroke="currentColor" stroke-width="2.2"><path d="m6 9 6 6 6-6"/></svg>`;
      }
    }

    // Update Type Pill
    const pillType = document.getElementById("pillFilterType");
    if (pillType) {
      const isPillActive = type !== "all";
      pillType.classList.toggle("active", isPillActive);
      const label = pillType.querySelector(".pill-label");
      if (label) label.textContent = typeLabels[type] || "类型";
      const action = pillType.querySelector(".pill-action");
      if (action) {
        action.innerHTML = isPillActive
          ? `<span class="pill-clear" title="清除类型筛选">×</span>`
          : `<svg class="pill-chevron" viewBox="0 0 24 24" width="9" height="9" fill="none" stroke="currentColor" stroke-width="2.2"><path d="m6 9 6 6 6-6"/></svg>`;
      }
    }

    // Update Location Pill
    const pillLoc = document.getElementById("pillFilterLocation");
    if (pillLoc) {
      const isPillActive = location !== "all";
      pillLoc.classList.toggle("active", isPillActive);
      const label = pillLoc.querySelector(".pill-label");
      if (label) {
        label.textContent = isPillActive ? (state.searchScope.locationName || locLabels[location] || location) : "位置";
      }
      if (isPillActive && state.searchScope.locationPath) {
        pillLoc.setAttribute("title", `当前限定位置: ${state.searchScope.locationPath}`);
      } else {
        pillLoc.setAttribute("title", "过滤检索位置或进入指定文件夹");
      }
      const action = pillLoc.querySelector(".pill-action");
      if (action) {
        action.innerHTML = isPillActive
          ? `<span class="pill-clear" title="清除位置筛选">×</span>`
          : `<svg class="pill-chevron" viewBox="0 0 24 24" width="9" height="9" fill="none" stroke="currentColor" stroke-width="2.2"><path d="m6 9 6 6 6-6"/></svg>`;
      }
    }

    // Update Checkmarks in Dropdown Menus
    document.querySelectorAll(".filter-dropdown-menu").forEach((menu) => {
      const wrap = menu.closest(".filter-dropdown-wrap");
      const group = wrap ? wrap.dataset.group : null;
      if (!group) return;
      const curVal = state.searchScope[group] || "all";
      menu.querySelectorAll(".filter-menu-item").forEach((item) => {
        item.classList.toggle("active", item.dataset.val === curVal);
      });
    });

    // Update Time Preset Chips active state
    document.querySelectorAll(".time-preset-chip").forEach((chip) => {
      chip.classList.toggle("active", time !== "range" && chip.dataset.val === time);
    });

    // Reset button visibility
    const resetBtn = document.getElementById("resetShelfFilterBtn");
    if (resetBtn) {
      resetBtn.classList.toggle("visible", isFiltered);
    }
  }

  function resetFilterShelf() {
    state.searchScope = { time: "all", type: "all", location: "all", locationName: "", locationPath: "" };
    state.calRange = { start: null, end: null, hover: null };
    delete state.searchScope.customRange;
    closeAllFilterMenus();
    renderCalendarGrid();
    updateFilterShelfUI();
    updateSearchResults();
    showToast("已重置所有检索筛选条件", "filter");
  }

  // Settings Modal Management
  function openSettingsModal() {
    state.showSettings = true;
    el.settingsModal.classList.add("visible");
    renderHotkeyBindings();

    // Two-way sync: Update Appearance tab controls to match current state
    const modalOpacity = document.getElementById("modalAcrylicOpacitySelect");
    if (modalOpacity) modalOpacity.value = state.acrylicOpacity || "balanced";
    const modalWallpaper = document.getElementById("modalWallpaperSelect");
    if (modalWallpaper) modalWallpaper.value = state.wallpaper || "user-current";
    const modalTheme = document.getElementById("modalThemeSelect");
    if (modalTheme) modalTheme.value = state.theme || "dark";
  }

  function closeSettingsModal() {
    state.showSettings = false;
    stopRecordingHotkey();
    el.settingsModal.classList.remove("visible");
  }

  // Switch View (Prototype vs Design System)
  function setView(viewName) {
    state.activeView = viewName;
    el.viewToggleBtns.forEach((btn) => {
      btn.classList.toggle("active", btn.dataset.viewTarget === viewName);
    });

    if (viewName === "design-system") {
      el.designSystemView.classList.add("active");
      el.searchWindow.style.display = "none";
    } else {
      el.designSystemView.classList.remove("active");
      el.searchWindow.style.display = "flex";
      el.searchInput.focus();
    }
  }

  // Window Sizing & Interactive Manual Resizing (Handles + Storage)
  function initResizeHandles() {
    const win = el.searchWindow;
    const body = el.searchBody;
    if (!win) return;

    const DEFAULT_WIDTH = 860;
    const DEFAULT_HEIGHT = 560;

    function applyDimensions(w, h) {
      win.style.setProperty("--launcher-width", `${w}px`);
      win.style.width = `${w}px`;
      win.style.setProperty("--launcher-height", `${h}px`);
      win.style.height = `${h}px`;
      if (body) {
        body.style.flex = "1";
        body.style.minHeight = "0";
        body.style.height = "auto";
      }
      localStorage.setItem("anycast_win_w", w);
      localStorage.setItem("anycast_win_h", h);
    }

    // Default to spacious 860×560 dimensions, auto-upgrade previous 740/490 cache
    let savedW = parseInt(localStorage.getItem("anycast_win_w"), 10);
    let savedH = parseInt(localStorage.getItem("anycast_win_h"), 10);
    if (!savedW || isNaN(savedW) || savedW < 680 || savedW === 740) savedW = DEFAULT_WIDTH;
    if (!savedH || isNaN(savedH) || savedH < 400 || savedH === 490) savedH = DEFAULT_HEIGHT;

    applyDimensions(savedW, savedH);

    // Interactive Drag-to-Resize on handles (.resize-handle)
    let isResizing = false;
    let resizeDir = "";
    let startX = 0;
    let startY = 0;
    let startW = 0;
    let startH = 0;

    const handles = win.querySelectorAll(".resize-handle");
    handles.forEach((handle) => {
      handle.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        isResizing = true;
        resizeDir = handle.dataset.direction || "se";
        startX = e.clientX;
        startY = e.clientY;
        const rect = win.getBoundingClientRect();
        startW = rect.width;
        startH = rect.height;
        document.body.style.userSelect = "none";
        document.body.style.cursor = resizeDir === "se" ? "nwse-resize" : resizeDir === "e" ? "ew-resize" : "ns-resize";
      });
    });

    window.addEventListener("mousemove", (e) => {
      if (!isResizing) return;
      let currentW = startW;
      let currentH = startH;

      if (resizeDir === "e" || resizeDir === "se") {
        const deltaX = (e.clientX - startX) * 2;
        currentW = Math.max(680, Math.min(window.innerWidth * 0.96, startW + deltaX));
      }

      if (resizeDir === "s" || resizeDir === "se") {
        const deltaY = (e.clientY - startY) * 2;
        currentH = Math.max(400, Math.min(window.innerHeight * 0.94, startH + deltaY));
      }

      applyDimensions(Math.round(currentW), Math.round(currentH));
    });

    window.addEventListener("mouseup", () => {
      if (isResizing) {
        isResizing = false;
        document.body.style.userSelect = "";
        document.body.style.cursor = "";
        showToast(`已调整窗口尺寸: ${win.offsetWidth}×${win.offsetHeight}`, "layers");
      }
    });

    // Connect top toolbar quick reset button
    document.getElementById("resetLauncherSizeBtn")?.addEventListener("click", () => {
      applyDimensions(DEFAULT_WIDTH, DEFAULT_HEIGHT);
      showToast(`已恢复为推荐尺寸 (${DEFAULT_WIDTH}×${DEFAULT_HEIGHT})`, "check");
    });
  }

  // Initialize Event Listeners
  function initEventListeners() {
    // Search input
    el.searchInput.addEventListener("input", (e) => {
      state.searchQuery = e.target.value;
      state.selectedIndex = 0;
      updateSearchResults();
    });

    el.searchClearBtn.addEventListener("click", () => {
      state.searchQuery = "";
      el.searchInput.value = "";
      state.selectedIndex = 0;
      el.searchInput.focus();
      updateSearchResults();
    });

    // Search Filter Icon Button (纯图标、无外框无底色)
    el.toggleFilterShelfBtn?.addEventListener("click", (e) => {
      e.stopPropagation();
      toggleFilterShelf();
    });

    // Reset Filter Button
    document.getElementById("resetShelfFilterBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      resetFilterShelf();
    });

    // Filter Dropdown Pills Click Handlers
    document.querySelectorAll(".filter-dropdown-wrap").forEach((wrap) => {
      const pill = wrap.querySelector(".filter-pill");
      const menu = wrap.querySelector(".filter-dropdown-menu");
      const group = wrap.dataset.group;

      menu?.addEventListener("click", (e) => {
        e.stopPropagation();
      });

      pill?.addEventListener("click", (e) => {
        e.stopPropagation();
        // If clicking on .pill-clear
        if (e.target.closest(".pill-clear")) {
          state.searchScope[group] = "all";
          if (group === "time") {
            state.calRange = { start: null, end: null, hover: null };
            delete state.searchScope.customRange;
            renderCalendarGrid();
          }
          if (group === "location") {
            state.searchScope.locationName = "";
            state.searchScope.locationPath = "";
          }
          closeAllFilterMenus();
          updateFilterShelfUI();
          updateSearchResults();
          const gMap = { time: "时间", type: "类型", location: "位置" };
          showToast(`已重置${gMap[group] || ""}筛选`, "filter");
          return;
        }

        // Toggle dropdown menu
        const isOpen = menu?.classList.contains("visible");
        closeAllFilterMenus();
        if (!isOpen) {
          menu?.classList.add("visible");
          pill.classList.add("open");
          if (group === "time") {
            renderCalendarGrid();
          }
          if (group === "location") {
            const locInput = document.getElementById("locSearchInput");
            if (locInput) {
              locInput.value = "";
              document.querySelectorAll("#locMenuScrollable .filter-menu-item").forEach((it) => (it.style.display = "flex"));
              document.querySelectorAll("#locMenuScrollable .loc-group-title, #locMenuScrollable .loc-divider").forEach((el) => (el.style.display = ""));
              setTimeout(() => locInput.focus(), 60);
            }
          }
        }
      });

      menu?.querySelectorAll(".filter-menu-item").forEach((item) => {
        item.addEventListener("click", (e) => {
          e.stopPropagation();
          const val = item.dataset.val;
          state.searchScope[group] = val;
          if (group === "location") {
            state.searchScope.locationName = item.dataset.name || item.querySelector("span").textContent.trim();
            state.searchScope.locationPath = item.dataset.path || "";
          }
          closeAllFilterMenus();
          updateFilterShelfUI();
          updateSearchResults();
          if (val === "all") {
            const gMap = { time: "时间", type: "类型", location: "位置" };
            showToast(`已重置${gMap[group] || ""}筛选`, "filter");
          } else {
            const displayName = item.dataset.name || item.querySelector("span").textContent.trim();
            showToast(`已限定位置: ${displayName}`, "folder");
          }
        });
      });
    });

    // Location Popover Search Box & Browse Custom Folder
    const locSearchInput = document.getElementById("locSearchInput");
    locSearchInput?.addEventListener("input", (e) => {
      const q = e.target.value.trim().toLowerCase();
      document.querySelectorAll("#locMenuScrollable .filter-menu-item").forEach((item) => {
        const text = (item.textContent || "").toLowerCase();
        const path = (item.dataset.path || "").toLowerCase();
        const match = !q || text.includes(q) || path.includes(q);
        item.style.display = match ? "flex" : "none";
      });
      document.querySelectorAll("#locMenuScrollable .loc-group-title, #locMenuScrollable .loc-divider").forEach((el) => {
        el.style.display = q ? "none" : "";
      });
    });

    locSearchInput?.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        const q = locSearchInput.value.trim();
        if (q) {
          e.preventDefault();
          state.searchScope.location = "custom";
          state.searchScope.locationName = q;
          state.searchScope.locationPath = q;
          closeAllFilterMenus();
          updateFilterShelfUI();
          updateSearchResults();
          showToast(`已限定在文件夹「${q}」中检索`, "folder");
        }
      }
    });

    document.getElementById("locBrowseFolderBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      const chosenFolder = {
        title: "Anycast",
        path: "D:\\Anycast"
      };
      state.searchScope.location = "folder-anycast";
      state.searchScope.locationName = chosenFolder.title;
      state.searchScope.locationPath = chosenFolder.path;
      closeAllFilterMenus();
      updateFilterShelfUI();
      updateSearchResults();
      showToast(`📂 [系统调用] 已选择并进入文件夹: ${chosenFolder.path}`, "folder");
    });

    // Calendar & Time Preset Handlers
    document.querySelectorAll(".time-preset-chip").forEach((chip) => {
      chip.addEventListener("click", (e) => {
        e.stopPropagation();
        const val = chip.dataset.val;
        state.searchScope.time = val;
        state.calRange = { start: null, end: null, hover: null };
        delete state.searchScope.customRange;
        closeAllFilterMenus();
        updateFilterShelfUI();
        updateSearchResults();
        renderCalendarGrid();
        showToast(val === "all" ? "已重置时间筛选" : `时间筛选: ${chip.textContent.trim()}`, "filter");
      });
    });

    document.getElementById("calPrevMonthBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      state.calViewMonth--;
      if (state.calViewMonth < 0) {
        state.calViewMonth = 11;
        state.calViewYear--;
      }
      renderCalendarGrid();
    });

    document.getElementById("calNextMonthBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      state.calViewMonth++;
      if (state.calViewMonth > 11) {
        state.calViewMonth = 0;
        state.calViewYear++;
      }
      renderCalendarGrid();
    });

    document.getElementById("calClearBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      state.calRange = { start: null, end: null, hover: null };
      if (state.searchScope.time === "range") {
        state.searchScope.time = "all";
        delete state.searchScope.customRange;
        updateFilterShelfUI();
        updateSearchResults();
      }
      renderCalendarGrid();
      showToast("已清空日期范围", "filter");
    });

    document.getElementById("calApplyBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      if (!state.calRange.start) return;
      if (!state.calRange.end) {
        state.calRange.end = state.calRange.start;
      }
      state.searchScope.time = "range";
      state.searchScope.customRange = {
        start: state.calRange.start,
        end: state.calRange.end
      };
      closeAllFilterMenus();
      updateFilterShelfUI();
      updateSearchResults();
      const days = calculateDaysBetween(state.calRange.start, state.calRange.end);
      showToast(`已应用日期范围: ${formatShortDate(state.calRange.start)} ~ ${formatShortDate(state.calRange.end)} (共${days}天)`, "check");
    });

    // Pinned Settings Button above search results
    el.launcherSettingsBtn?.addEventListener("click", openSettingsModal);

    // Pinned Shelf Collapse / Expand Toggle Button
    document.getElementById("togglePinnedBtn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      togglePinnedShelf();
    });

    // Pinned Section: 鼠标滚动左右切换 (仅针对置顶页，底部常规滚动)
    el.pinnedShelfSection?.addEventListener("wheel", (e) => {
      if (e.deltaY !== 0 && el.pinnedTrackWrapper) {
        e.preventDefault();
        el.pinnedTrackWrapper.scrollLeft += e.deltaY;
      }
    }, { passive: false });

    // View Mode Toggle Button (Icon vs List)
    el.viewModeToggleBtn?.addEventListener("click", () => {
      setViewMode(state.viewMode === "list" ? "grid" : "list");
    });

    // Category Filter Tabs
    if (el.categoryTabs) {
      el.categoryTabs.querySelectorAll(".category-tab").forEach((tab) => {
        tab.addEventListener("click", () => {
          setCategoryFilter(tab.dataset.filter);
        });
      });
    }

    // Clipboard Secondary Sub-filters
    if (el.clipboardSubFilterChips) {
      el.clipboardSubFilterChips.querySelectorAll(".sub-filter-chip").forEach((chip) => {
        chip.addEventListener("click", () => {
          setClipboardSubFilter(chip.dataset.subfilter);
        });
      });
    }

    // Draggable 2D Resize System
    initResizeHandles();

    // Global Keydown
    window.addEventListener("keydown", handleKeyDown);

    // Global click outside listeners
    window.addEventListener("click", (e) => {
      closeAllFilterMenus();
      if (state.showContextMenu && !el.contextMenu.contains(e.target)) {
        closeContextMenu();
      }
    });

    // Context menu item click actions
    document.getElementById("ctxOpen")?.addEventListener("click", () => {
      const target = state.contextTargetItem || state.currentResults[state.selectedIndex];
      if (target) executeItemAction(target);
      closeContextMenu();
    });
    document.getElementById("ctxFolder")?.addEventListener("click", () => {
      const target = state.contextTargetItem || state.currentResults[state.selectedIndex];
      if (target) {
        showToast(`🚀 [系统调用] 已在资源管理器中定位: ${target.title}`, "folderOpen");
      }
      closeContextMenu();
    });
    document.getElementById("ctxEnterFolder")?.addEventListener("click", () => {
      const target = state.contextTargetItem || state.currentResults[state.selectedIndex];
      if (target) enterFolderSearch(target);
      closeContextMenu();
    });
    document.getElementById("ctxCopyPath")?.addEventListener("click", () => {
      const target = state.contextTargetItem || state.currentResults[state.selectedIndex];
      if (target) copyItemPath(target);
      closeContextMenu();
    });
    document.getElementById("ctxPin")?.addEventListener("click", () => {
      const target = state.contextTargetItem || state.currentResults[state.selectedIndex];
      if (target) togglePin(target);
      closeContextMenu();
    });
    document.getElementById("ctxRemove")?.addEventListener("click", () => {
      const target = state.contextTargetItem || state.currentResults[state.selectedIndex];
      if (target) {
        showToast(`已从最近记录移除: ${target.title}`, "trash");
        const itemIdx = state.currentResults.findIndex((r) => r.id === target.id);
        if (itemIdx >= 0) state.currentResults.splice(itemIdx, 1);
        updateSearchResults();
      }
      closeContextMenu();
    });

    // Settings Modal
    document.getElementById("settingsBtn")?.addEventListener("click", openSettingsModal);
    document.getElementById("closeSettingsBtn")?.addEventListener("click", closeSettingsModal);
    el.settingsModal.addEventListener("click", (e) => {
      if (e.target === el.settingsModal) closeSettingsModal();
    });

    // Settings Navigation Tabs
    document.querySelectorAll(".settings-nav-item").forEach((btn) => {
      btn.addEventListener("click", () => {
        document.querySelectorAll(".settings-nav-item").forEach((b) => b.classList.remove("active"));
        btn.classList.add("active");
        const tab = btn.dataset.tab;
        document.querySelectorAll(".settings-tab-pane").forEach((pane) => {
          pane.style.display = pane.id === `tab-${tab}` ? "flex" : "none";
        });
        if (tab === "hotkeys") {
          renderHotkeyBindings();
        }
      });
    });

    // Initialize Hotkey Panel & Controls
    initHotkeyPanelEvents();

    // Click outside hotkey recording box cancels recording
    window.addEventListener("click", (e) => {
      if (state.recordingTarget && !e.target.closest(".hotkey-badge") && !e.target.closest("#newHotkeyRecordBox")) {
        stopRecordingHotkey();
      }
    });

    // Settings Maintenance Action Buttons
    document.getElementById("clearAppCacheBtn")?.addEventListener("click", () => {
      showToast("已清空本地运行时缩略图与临时缓存 (释放 14.8 MB)", "check");
    });
    document.getElementById("rebuildIndexBtn")?.addEventListener("click", () => {
      showToast("🚀 已触发全盘 NTFS USN 增量索引重新扫描", "database");
    });
    document.getElementById("clearClipboardBtn")?.addEventListener("click", () => {
      showToast("已清除所有未固定的历史剪贴板记录", "trash");
    });

    // Settings Switch Toggles (skip hotkey rows as they have dedicated data-hk-toggle handlers)
    document.querySelectorAll(".switch-toggle:not([data-hk-toggle])").forEach((sw) => {
      sw.addEventListener("click", () => {
        sw.classList.toggle("active");
      });
    });

    // View Switchers
    el.viewToggleBtns.forEach((btn) => {
      btn.addEventListener("click", () => setView(btn.dataset.viewTarget));
    });

    // Theme Toggle
    el.themeToggleBtn.addEventListener("click", () => {
      state.theme = state.theme === "dark" ? "light" : "dark";
      document.documentElement.setAttribute("data-theme", state.theme);
      el.themeToggleBtn.innerHTML = state.theme === "dark" ? Icons.sun : Icons.moon;
      const modalTheme = document.getElementById("modalThemeSelect");
      if (modalTheme) modalTheme.value = state.theme;
      showToast(`已切换至 ${state.theme === "dark" ? "深色" : "浅色"} Acrylic 材质`, state.theme === "dark" ? "sun" : "moon");
    });

    // Acrylic Opacity Preset Switcher
    if (el.acrylicOpacitySelect) {
      el.acrylicOpacitySelect.addEventListener("change", (e) => {
        state.acrylicOpacity = e.target.value;
        document.documentElement.setAttribute("data-acrylic-opacity", state.acrylicOpacity);
        const modalOpacity = document.getElementById("modalAcrylicOpacitySelect");
        if (modalOpacity) modalOpacity.value = state.acrylicOpacity;
        showToast(`透光度: ${e.target.options[e.target.selectedIndex].text}`, "layers");
      });
    }

    // Real Wallpaper Switcher
    if (el.wallpaperSelect) {
      el.wallpaperSelect.addEventListener("change", (e) => {
        state.wallpaper = e.target.value;
        el.canvas.style.backgroundImage = ""; // Clear inline override
        el.canvas.setAttribute("data-wallpaper", state.wallpaper);
        const modalWallpaper = document.getElementById("modalWallpaperSelect");
        if (modalWallpaper) modalWallpaper.value = state.wallpaper;
        showToast(`壁纸已切换: ${e.target.options[e.target.selectedIndex].text}`, "image");
      });
    }

    // Custom Wallpaper File Upload
    if (el.customWallpaperInput) {
      el.customWallpaperInput.addEventListener("change", (e) => {
        const file = e.target.files && e.target.files[0];
        if (!file) return;
        const reader = new FileReader();
        reader.onload = (evt) => {
          el.canvas.style.backgroundImage = `url("${evt.target.result}")`;
          showToast(`已载入本地壁纸: ${file.name}`, "image");
        };
        reader.readAsDataURL(file);
      });
    }

    // Settings Modal Appearance Tab Listeners & Two-Way Sync
    const modalAcrylicOpacitySelect = document.getElementById("modalAcrylicOpacitySelect");
    if (modalAcrylicOpacitySelect) {
      modalAcrylicOpacitySelect.addEventListener("change", (e) => {
        state.acrylicOpacity = e.target.value;
        document.documentElement.setAttribute("data-acrylic-opacity", state.acrylicOpacity);
        if (el.acrylicOpacitySelect) el.acrylicOpacitySelect.value = state.acrylicOpacity;
        showToast(`透光度已更新: ${e.target.options[e.target.selectedIndex].text}`, "layers");
      });
    }

    const modalWallpaperSelect = document.getElementById("modalWallpaperSelect");
    if (modalWallpaperSelect) {
      modalWallpaperSelect.addEventListener("change", (e) => {
        state.wallpaper = e.target.value;
        el.canvas.style.backgroundImage = "";
        el.canvas.setAttribute("data-wallpaper", state.wallpaper);
        if (el.wallpaperSelect) el.wallpaperSelect.value = state.wallpaper;
        showToast(`壁纸已同步: ${e.target.options[e.target.selectedIndex].text}`, "image");
      });
    }

    const modalCustomWallpaperInput = document.getElementById("modalCustomWallpaperInput");
    if (modalCustomWallpaperInput) {
      modalCustomWallpaperInput.addEventListener("change", (e) => {
        const file = e.target.files && e.target.files[0];
        if (!file) return;
        const reader = new FileReader();
        reader.onload = (evt) => {
          el.canvas.style.backgroundImage = `url("${evt.target.result}")`;
          showToast(`已载入自定义壁纸: ${file.name}`, "image");
        };
        reader.readAsDataURL(file);
      });
    }

    const modalThemeSelect = document.getElementById("modalThemeSelect");
    if (modalThemeSelect) {
      modalThemeSelect.addEventListener("change", (e) => {
        state.theme = e.target.value;
        document.documentElement.setAttribute("data-theme", state.theme);
        el.themeToggleBtn.innerHTML = state.theme === "dark" ? Icons.sun : Icons.moon;
        showToast(`已切换至 ${state.theme === "dark" ? "深色" : "浅色"} Acrylic 材质`, state.theme === "dark" ? "sun" : "moon");
      });
    }

    // Drag and Drop any local image directly onto desktop canvas
    window.addEventListener("dragover", (e) => {
      e.preventDefault();
    });

    window.addEventListener("drop", (e) => {
      e.preventDefault();
      if (e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files.length > 0) {
        const file = e.dataTransfer.files[0];
        if (file.type.startsWith("image/")) {
          const reader = new FileReader();
          reader.onload = (evt) => {
            el.canvas.style.backgroundImage = `url("${evt.target.result}")`;
            showToast(`已应用拖入的壁纸图片: ${file.name}`, "image");
          };
          reader.readAsDataURL(file);
        }
      }
    });

    // Demo Scenario Shortcuts
    document.querySelectorAll("[data-scenario]").forEach((btn) => {
      btn.addEventListener("click", () => {
        setView("prototype");
        const scenario = btn.dataset.scenario;
        if (scenario === "idle") {
          el.searchInput.value = "";
          state.searchQuery = "";
          setSearchMode("fast");
        } else if (scenario === "docker") {
          el.searchInput.value = "docker";
          state.searchQuery = "docker";
          setSearchMode("fast");
        } else if (scenario === "easynote") {
          el.searchInput.value = "EasyNote";
          state.searchQuery = "EasyNote";
          setSearchMode("fast");
        } else if (scenario === "ai-storage") {
          el.searchInput.value = "找一下昨天修改的 EasyNote 文件";
          state.searchQuery = "找一下昨天修改的 EasyNote 文件";
          setSearchMode("smart");
        }
        el.searchInput.focus();
        updateSearchResults();
      });
    });
  }

  // Initialize
  function init() {
    updateViewModeUI();
    initEventListeners();
    renderHotkeyBindings();
    renderCalendarGrid();
    updateFilterShelfUI();
    if (el.searchFilterShelf) {
      el.searchFilterShelf.classList.toggle("collapsed", state.isFilterShelfCollapsed);
    }
    if (el.toggleFilterShelfBtn) {
      el.toggleFilterShelfBtn.classList.toggle("open", !state.isFilterShelfCollapsed);
    }
    updateSearchResults();
    el.searchInput.focus();
  }

  // Start on DOM ready
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
