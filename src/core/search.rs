//! 搜索引擎：ISearchProvider 契约、极速搜索（应用 + FTS5 文件名 + 剪贴板）、
//! 智能搜索（自然语言意图解析 + 正文全文检索 + 可插拔向量后端）。

use crate::core::storage::{AppRecord, ClipRecord, EntryRecord, FileRecord, Storage};
use crate::models::{IntentChip, ItemType, SearchItemModel, SearchMode, SearchRequest, SearchResponse, SearchScopeFilter};
use chrono::{Datelike, Local, TimeZone};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// 统一搜索接口（UI 不依赖具体实现）
pub trait SearchProvider: Send + Sync {
    fn search(&self, req: &SearchRequest) -> SearchResponse;
}

/// 向量嵌入后端契约（bge-small-zh / sqlite-vec 的接入点，当前为空实现）
pub trait EmbeddingBackend: Send + Sync {
    fn is_ready(&self) -> bool;
    fn embed(&self, text: &str) -> Option<Vec<f32>>;
    fn name(&self) -> &'static str;
}

pub struct NoopEmbedding;

impl EmbeddingBackend for NoopEmbedding {
    fn is_ready(&self) -> bool {
        false
    }
    fn embed(&self, _text: &str) -> Option<Vec<f32>> {
        None
    }
    fn name(&self) -> &'static str {
        "none"
    }
}

// ---------------------------------------------------------------------------
// 通用转换与格式化
// ---------------------------------------------------------------------------

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

pub fn fmt_relative(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let now = now_ts();
    let diff = now - ts;
    if diff < 60 {
        return "刚刚".into();
    }
    if diff < 3600 {
        return format!("{}m前", diff / 60);
    }
    let today = Local::now().date_naive();
    let day = Local.timestamp_opt(ts, 0).single().map(|d| d.date_naive()).unwrap_or(today);
    let days = (today - day).num_days();
    if days == 0 {
        return format!("{}h前", diff / 3600);
    }
    if days == 1 {
        return "昨天".into();
    }
    if days < 7 {
        return format!("{days}天前");
    }
    if day.year() == today.year() {
        return format!("{:02}-{:02}", day.month(), day.day());
    }
    format!("{}-{:02}-{:02}", day.year(), day.month(), day.day())
}

pub fn fmt_size(bytes: i64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", b / 1024.0 / 1024.0 / 1024.0)
    }
}

fn short_dir(path: &str) -> String {
    let p = Path::new(path);
    let Some(parent) = p.parent() else { return path.to_string() };
    let comps: Vec<String> = parent.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    if comps.len() <= 3 {
        return parent.to_string_lossy().to_string();
    }
    let tail = &comps[comps.len() - 2..];
    format!("{}\\…\\{}", comps[0], tail.join("\\"))
}

pub fn file_badge(ext: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "文件夹";
    }
    match ext {
        "md" | "txt" | "doc" | "docx" | "pdf" | "rtf" | "odt" | "one" | "wps" | "epub" => "文档",
        "xls" | "xlsx" | "csv" => "表格",
        "ppt" | "pptx" => "演示",
        "yml" | "yaml" | "json" | "toml" | "ini" | "cfg" | "env" | "xml" | "conf" => "配置",
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "java" | "kt" | "c" | "cpp" | "h" | "cs" | "swift"
        | "rb" | "php" | "lua" | "sh" | "ps1" | "bat" | "cmd" | "slint" | "vue" | "html" | "css" | "sql" => "代码",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "heic" => "图片",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "视频",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => "音频",
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "iso" => "压缩包",
        "exe" | "msi" | "lnk" => "程序",
        "" => "文件",
        _ => "文件",
    }
}

pub fn file_icon(ext: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder";
    }
    match file_badge(ext, false) {
        "代码" | "配置" => "fileCode",
        "图片" => "image",
        "视频" | "音频" => "media",
        "压缩包" => "archive",
        "程序" => "app",
        _ => "fileText",
    }
}

pub fn app_icon(name: &str, target: &str) -> &'static str {
    let n = format!("{} {}", name.to_lowercase(), target.to_lowercase());
    if n.contains("visual studio code") || n.contains("vscode") || n.contains("code.exe") {
        "vscode"
    } else if n.contains("docker") {
        "docker"
    } else if n.contains("edge") || n.contains("chrome") || n.contains("firefox") || n.contains("browser") || n.contains("brave") {
        "globe"
    } else if n.contains("terminal") || n.contains("powershell") || n.contains("cmd.exe") || n.contains("bash") || n.contains("wt.exe") {
        "terminal"
    } else if n.contains("explorer") || n.contains("资源管理器") {
        "folder"
    } else {
        "app"
    }
}

pub fn clip_kind(text: &str) -> &'static str {
    let t = text.trim();
    let lower = t.to_lowercase();
    if !t.contains(char::is_whitespace)
        && (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www."))
    {
        return "url";
    }
    let code_markers = ["npm ", "git ", "cargo ", "docker ", "pip ", "yarn ", "pnpm ", "curl ", "sudo ", "apt ", "brew "];
    if code_markers.iter().any(|m| lower.starts_with(m)) {
        return "code";
    }
    let symbols = t.chars().filter(|c| "{}[]();=<>$|&\\/".contains(*c)).count();
    let lines = t.lines().count();
    if symbols >= 4 || (lines >= 2 && t.lines().any(|l| l.starts_with("  ") || l.starts_with('\t'))) {
        return "code";
    }
    "text"
}

fn clip_badge(kind: &str) -> &'static str {
    match kind {
        "url" => "链接",
        "code" => "代码",
        _ => "文本",
    }
}

fn clip_icon(kind: &str) -> &'static str {
    match kind {
        "url" => "link",
        "code" => "terminal",
        _ => "clipboard",
    }
}

pub fn app_to_item(a: &AppRecord) -> SearchItemModel {
    let exe_name = Path::new(if a.target.is_empty() { &a.launch_path } else { &a.target })
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    SearchItemModel {
        id: format!("app:{}", a.launch_path.to_lowercase()),
        item_type: ItemType::App,
        title: a.name.clone(),
        subtitle: exe_name,
        full_path: if a.target.is_empty() { a.launch_path.clone() } else { a.target.clone() },
        icon_name: app_icon(&a.name, &a.target).into(),
        badge: "应用".into(),
        action_hint: "打开".into(),
        section: "应用".into(),
        ..Default::default()
    }
}

pub fn file_to_item(f: &FileRecord) -> SearchItemModel {
    let item_type = if f.is_dir { ItemType::Folder } else { ItemType::File };
    SearchItemModel {
        id: format!("{}:{}", if f.is_dir { "folder" } else { "file" }, f.path),
        item_type,
        title: f.name.clone(),
        subtitle: short_dir(&f.path),
        full_path: f.path.clone(),
        icon_name: file_icon(&f.ext, f.is_dir).into(),
        badge: file_badge(&f.ext, f.is_dir).into(),
        size_bytes: if f.is_dir { None } else { Some(f.size as u64) },
        modified_time: Some(f.mtime),
        action_hint: "打开".into(),
        section: "文件".into(),
        ..Default::default()
    }
}

pub fn clip_to_item(c: &ClipRecord) -> SearchItemModel {
    let title: String = c.content.trim().lines().next().unwrap_or("").chars().take(80).collect();
    SearchItemModel {
        id: format!("clip:{}", c.id),
        item_type: ItemType::Clipboard,
        title,
        subtitle: fmt_relative(c.created),
        full_path: c.content.clone(),
        icon_name: clip_icon(&c.kind).into(),
        badge: clip_badge(&c.kind).into(),
        is_pinned: c.pinned,
        modified_time: Some(c.created),
        action_hint: if c.kind == "url" { "打开".into() } else { "复制".into() },
        section: "剪贴板".into(),
        sub_type: c.kind.clone(),
        preview: c.content.chars().take(2000).collect(),
        ..Default::default()
    }
}

pub fn entry_to_item(e: &EntryRecord, section: &str) -> SearchItemModel {
    let item_type = ItemType::parse(&e.kind);
    let sub_type = if item_type == ItemType::Clipboard {
        match e.badge.as_str() {
            "链接" => "url",
            "代码" => "code",
            _ => "text",
        }
        .to_string()
    } else {
        String::new()
    };
    SearchItemModel {
        id: e.item_id.clone(),
        item_type,
        title: e.title.clone(),
        subtitle: if item_type == ItemType::Clipboard && e.last_used > 0 { fmt_relative(e.last_used) } else { e.subtitle.clone() },
        full_path: e.path.clone(),
        icon_name: e.icon.clone(),
        badge: e.badge.clone(),
        action_hint: if item_type == ItemType::Clipboard && sub_type != "url" { "复制".into() } else { "打开".into() },
        section: section.into(),
        sub_type,
        preview: if item_type == ItemType::Clipboard { e.path.clone() } else { String::new() },
        last_used: if e.last_used > 0 { Some(e.last_used) } else { None },
        ..Default::default()
    }
}

pub fn item_to_entry(item: &SearchItemModel) -> EntryRecord {
    EntryRecord {
        item_id: item.id.clone(),
        kind: item.item_type.as_str().into(),
        title: item.title.clone(),
        subtitle: item.subtitle.clone(),
        path: item.full_path.clone(),
        badge: item.badge.clone(),
        icon: item.icon_name.clone(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// 意图解析（智能模式）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Intent {
    pub keywords: Vec<String>,
    pub time_preset: String,
    pub time_range: Option<(i64, i64)>,
    pub time_label: String,
    pub type_category: String,
    pub type_label: String,
}

fn day_range(days_ago: i64) -> (i64, i64) {
    let today = Local::now().date_naive();
    let day = today - chrono::Duration::days(days_ago);
    let start = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap()).single().map(|d| d.timestamp()).unwrap_or(0);
    (start, start + 86_399)
}

/// 在小写文本中查找规则词：ASCII 词要求单词边界，中文直接子串匹配
fn find_rule_word(lower: &str, w: &str) -> Option<usize> {
    if w.is_ascii() {
        let w = w.trim();
        let bytes = lower.as_bytes();
        for (i, _) in lower.match_indices(w) {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after = i + w.len();
            let after_ok = after >= lower.len() || !bytes[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some(i);
            }
        }
        None
    } else {
        lower.find(w)
    }
}

pub fn parse_intent(query: &str) -> Intent {
    let mut intent = Intent::default();
    let mut text = query.to_string();
    let lower = text.to_lowercase();

    // 时间
    let time_rules: &[(&[&str], &str, &str)] = &[
        (&["今天", "today"], "today", "今天"),
        (&["昨天", "yesterday"], "yesterday", "昨天"),
        (&["前天"], "before_yesterday", "前天"),
        (&["最近三天", "近三天", "3天内", "三天内"], "3days", "近 3 天"),
        (&["上周", "上星期", "last week"], "last_week", "上周"),
        (&["本周", "这周", "这星期", "this week", "最近一周", "近一周", "7天内"], "7days", "本周"),
        (&["上个月", "上月", "last month"], "last_month", "上个月"),
        (&["本月", "这个月", "this month", "30天内", "最近一个月"], "30days", "本月"),
        (&["今年", "this year", "一年内"], "year", "今年"),
        (&["最近", "近期", "recent", "recently"], "7days", "最近"),
    ];
    for (words, preset, label) in time_rules {
        if intent.time_preset.is_empty() {
            if let Some((w, idx)) = words.iter().find_map(|w| find_rule_word(&lower, w).map(|i| (*w, i))) {
                match *preset {
                    "yesterday" => {
                        intent.time_preset = "range".into();
                        intent.time_range = Some(day_range(1));
                    }
                    "before_yesterday" => {
                        intent.time_preset = "range".into();
                        intent.time_range = Some(day_range(2));
                    }
                    "last_week" => {
                        let (s, _) = day_range(14);
                        let (_, e) = day_range(7);
                        intent.time_preset = "range".into();
                        intent.time_range = Some((s, e));
                    }
                    "last_month" => {
                        let (s, _) = day_range(60);
                        let (_, e) = day_range(30);
                        intent.time_preset = "range".into();
                        intent.time_range = Some((s, e));
                    }
                    p => intent.time_preset = p.into(),
                }
                intent.time_label = label.to_string();
                let end = idx + w.trim().len();
                text = format!("{} {}", &text[..idx], &text[end..]);
            }
        }
    }
    let lower = text.to_lowercase();

    // 类型
    let type_rules: &[(&[&str], &str, &str)] = &[
        (&["文件夹", "目录", "folder", "directory"], "folder", "文件夹"),
        (&["应用", "程序", "软件", "app "], "app", "应用"),
        (&["剪贴板", "剪切板", "复制过", "clipboard"], "clipboard", "剪贴板"),
        (&["图片", "照片", "截图", "视频", "音乐", "媒体", "image", "photo", "video"], "media", "图片与媒体"),
        (&["压缩包", "zip"], "archive", "压缩包"),
        (&["代码", "脚本", "源码", "配置文件", "工程文件", "code", "script"], "code", "代码"),
        (&["文档", "笔记", "markdown", "word", "pdf", "文章", "note", "doc "], "document", "文档"),
    ];
    for (words, cat, label) in type_rules {
        if intent.type_category.is_empty() {
            if let Some((w, idx)) = words.iter().find_map(|w| find_rule_word(&lower, w).map(|i| (*w, i))) {
                intent.type_category = cat.to_string();
                intent.type_label = label.to_string();
                let end = idx + w.trim().len();
                text = format!("{} {}", &text[..idx], &text[end..]);
            }
        }
    }

    // 填充词
    let fillers = [
        "找一下", "找到", "找出", "找找", "查找", "查一下", "搜索", "搜一下", "帮我", "请", "找", "搜",
        "修改过的", "修改的", "修改过", "修改", "编辑过的", "编辑的", "创建的", "打开过的", "用过的",
        "记录了", "记录", "关于", "有关", "怎么存的", "怎么", "什么", "哪个", "哪里", "在哪", "存在",
        "那个", "这个", "一下", "文件", "东西", "内容", "里面", "里", "中", "的", "了", "呢", "吗",
        "是", "我", "把", "个", "一个", "一份", "份", "有", "所有", "全部", "一些", "些",
        "find", "search", "the", "my", "for", "me", "a", "an", "file", "files", "about", "with", "that", "which",
    ];
    let mut cleaned = text.clone();
    for f in fillers {
        // 仅对英文做词边界处理；中文直接替换
        if f.is_ascii() {
            let lower_c = cleaned.to_lowercase();
            let mut out = String::new();
            let mut last = 0;
            for (i, _) in lower_c.match_indices(f) {
                let before_ok = i == 0 || !lower_c.as_bytes()[i - 1].is_ascii_alphanumeric();
                let after = i + f.len();
                let after_ok = after >= lower_c.len() || !lower_c.as_bytes()[after].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    out.push_str(&cleaned[last..i]);
                    out.push(' ');
                    last = after;
                }
            }
            out.push_str(&cleaned[last..]);
            cleaned = out;
        } else {
            cleaned = cleaned.replace(f, " ");
        }
    }
    let seps: &[char] = &[' ', ',', '，', '。', '.', '?', '？', '!', '！', ':', '：', ';', '；', '、', '"', '“', '”', '(', ')', '（', '）', '「', '」', '\'', '\t', '\n'];
    let mut keywords: Vec<String> = cleaned
        .split(seps)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    keywords.dedup();
    // 单个汉字过短时合并到上一个关键词（避免噪音）
    keywords.retain(|k| k.chars().count() >= 2 || k.chars().all(|c| c.is_ascii_alphanumeric()));
    if keywords.is_empty() {
        let q = query.trim();
        if !q.is_empty() && (intent.time_preset.is_empty() && intent.type_category.is_empty()) {
            keywords.push(q.to_string());
        }
    }
    intent.keywords = keywords;
    intent
}

// ---------------------------------------------------------------------------
// 搜索引擎
// ---------------------------------------------------------------------------

pub struct SearchEngine {
    storage: Arc<Storage>,
    apps: RwLock<Vec<AppRecord>>,
    embedding: Box<dyn EmbeddingBackend>,
}

fn contains_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack.to_lowercase().find(needle)
}

fn name_score(name: &str, query: &str) -> f32 {
    let lower = name.to_lowercase();
    if lower == query {
        return 100.0;
    }
    if lower.starts_with(query) {
        return 85.0;
    }
    // 单词起始
    if lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(query))
    {
        return 70.0;
    }
    if lower.contains(query) {
        return 55.0 - (lower.len().min(60) as f32) * 0.2;
    }
    0.0
}

impl SearchEngine {
    pub fn new(storage: Arc<Storage>) -> SearchEngine {
        let apps = storage.load_apps();
        SearchEngine { storage, apps: RwLock::new(apps), embedding: Box::new(NoopEmbedding) }
    }

    pub fn set_apps(&self, apps: Vec<AppRecord>) {
        *self.apps.write() = apps;
    }

    pub fn app_count(&self) -> usize {
        self.apps.read().len()
    }

    pub fn embedding_name(&self) -> &'static str {
        self.embedding.name()
    }

    fn boost_by_usage(&self, item: &mut SearchItemModel) {
        if let Some((last, count)) = self.storage.recent_stats(&item.id) {
            let age_days = ((now_ts() - last).max(0) / 86_400) as f32;
            item.score += (count as f32 * 4.0).min(30.0) + (12.0 - age_days * 2.0).max(0.0);
            item.last_used = Some(last);
        }
        if self.storage.is_pinned(&item.id) {
            item.is_pinned = true;
            item.score += 3.0;
        }
    }

    fn search_apps(&self, query: &str, limit: usize) -> Vec<SearchItemModel> {
        let q = query.to_lowercase();
        let q_nospace: String = q.chars().filter(|c| !c.is_whitespace()).collect();
        let apps = self.apps.read();
        let mut out: Vec<SearchItemModel> = Vec::new();
        for a in apps.iter() {
            let mut score = name_score(&a.name, &q);
            if score == 0.0 && !q_nospace.is_empty() {
                if a.initials.starts_with(&q_nospace) {
                    score = 65.0;
                } else if a.pinyin.starts_with(&q_nospace) {
                    score = 60.0;
                } else if a.initials.contains(&q_nospace) || a.pinyin.contains(&q_nospace) {
                    score = 45.0;
                } else if contains_ci(&a.target, &q).is_some() {
                    score = 30.0;
                }
            }
            if score > 0.0 {
                let mut item = app_to_item(a);
                item.score = score;
                self.boost_by_usage(&mut item);
                out.push(item);
            }
        }
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit);
        out
    }

    fn search_files(&self, query: &str, scope: &SearchScopeFilter, limit: usize) -> Vec<SearchItemModel> {
        let q = query.to_lowercase();
        let recs = self.storage.search_files(query, scope, limit * 2).unwrap_or_default();
        let mut out: Vec<SearchItemModel> = recs
            .iter()
            .map(|f| {
                let mut item = file_to_item(f);
                item.score = name_score(&f.name, &q).max(20.0);
                if f.is_dir {
                    item.score += 2.0;
                }
                let age_days = ((now_ts() - f.mtime).max(0) / 86_400) as f32;
                item.score += (10.0 - age_days * 0.5).max(0.0);
                self.boost_by_usage(&mut item);
                item
            })
            .collect();
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit);
        out
    }

    fn search_clips(&self, query: &str, sub: &str, limit: usize) -> Vec<SearchItemModel> {
        let recs = self.storage.search_clips(query, sub, limit).unwrap_or_default();
        recs.iter()
            .map(|c| {
                let mut item = clip_to_item(c);
                item.score = 40.0 + (c.created % 1000) as f32 / 1000.0;
                item
            })
            .collect()
    }

    /// 空查询：混合流空闲推荐（常用应用 + 最近项目 + 剪贴板），自然饱满填充视口
    ///
    /// 修复历史缺陷：旧实现使用 `if items.is_empty()` 互斥分支，只要最近记录里
    /// 有任意 1~2 条（例如刚复制过剪贴板），就会整段跳过 136 个已扫描应用，
    /// 导致 560px 视口下方留下大片纯黑虚空。现改为分段混合流，各来源独立补足。
    fn idle_items(&self, scope: &SearchScopeFilter) -> Vec<SearchItemModel> {
        // ---- 1. 最近使用（来自 recent 表）----
        let mut recent: Vec<SearchItemModel> = self
            .storage
            .list_recent(60)
            .iter()
            .map(|e| entry_to_item(e, "最近使用"))
            .map(|mut i| {
                i.is_pinned = self.storage.is_pinned(&i.id);
                i
            })
            .collect();

        if scope.is_active() {
            // 范围筛选时，用文件索引的最新文件补充
            let files = self.storage.search_files("", scope, 40).unwrap_or_default();
            let extra: Vec<SearchItemModel> = files
                .iter()
                .map(file_to_item)
                .map(|mut i| {
                    i.section = "最近修改".into();
                    i
                })
                .collect();
            recent.retain(|i| self.item_in_scope(i, scope));
            recent.extend(extra);
        }

        // ---- 2. 剪贴板短期条目（最多 4 条，排在最近列表之前）----
        let clips: Vec<SearchItemModel> = self
            .storage
            .search_clips("", "all", 4)
            .unwrap_or_default()
            .iter()
            .map(|c| {
                let mut i = clip_to_item(c);
                i.section = "剪贴板".into();
                i
            })
            .filter(|i| !scope.is_active() || self.item_in_scope(i, scope))
            .collect();

        // ---- 3. 常用应用（补足到目标条数，杜绝空白）----
        // 目标：让默认 560px 视口（约 11~12 行）自然填满并略有溢出，避免死区。
        const TARGET_ROWS: usize = 14;
        let already = recent.len() + clips.len();
        let need_apps = TARGET_ROWS.saturating_sub(already).max(6);

        let apps: Vec<SearchItemModel> = {
            let guard = self.apps.read();
            guard
                .iter()
                .map(app_to_item)
                .filter(|i| !scope.is_active() || self.item_in_scope(i, scope))
                .take(need_apps)
                .map(|mut i| {
                    i.section = "常用应用".into();
                    i
                })
                .collect()
        };

        // ---- 4. 合并：应用兜底在前、剪贴板次之、最近使用殿后 ----
        // 应用列表是稳定的基础填充层；若应用扫描为空则退化为纯最近流。
        let mut items: Vec<SearchItemModel> = Vec::with_capacity(apps.len() + clips.len() + recent.len());
        items.extend(apps);
        items.extend(clips);
        items.extend(recent);

        // 去重：同一 id 只保留首次出现（应用层优先）
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        items.retain(|i| seen.insert(i.id.clone()));

        items
    }

    fn item_in_scope(&self, item: &SearchItemModel, scope: &SearchScopeFilter) -> bool {
        match scope.type_category.as_str() {
            "all" | "" => {}
            "app" => {
                if item.item_type != ItemType::App {
                    return false;
                }
            }
            "clipboard" => {
                if item.item_type != ItemType::Clipboard {
                    return false;
                }
            }
            "folder" => {
                if item.item_type != ItemType::Folder {
                    return false;
                }
            }
            cat => {
                if item.item_type != ItemType::File {
                    return false;
                }
                let ext = Path::new(&item.full_path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if !crate::core::storage::type_extensions(cat).map(|l| l.contains(&ext.as_str())).unwrap_or(true) {
                    return false;
                }
            }
        }
        if let Some(dir) = scope.custom_directory.as_ref().filter(|d| !d.is_empty() && scope.location_scope == "custom") {
            if !item.full_path.to_lowercase().starts_with(&dir.to_lowercase()) {
                return false;
            }
        }
        let ts = item.modified_time.or(item.last_used).unwrap_or(0);
        if let Some(lower) = scope.time_lower_bound(now_ts(), day_range(0).0) {
            if ts < lower {
                return false;
            }
        }
        true
    }

    fn fast(&self, req: &SearchRequest) -> (Vec<SearchItemModel>, Vec<IntentChip>) {
        let q = req.query.trim();
        let scope = &req.scope;
        let cat = scope.type_category.as_str();
        let mut items = Vec::new();
        if matches!(cat, "all" | "" | "app") {
            items.extend(self.search_apps(q, 12));
        }
        if !matches!(cat, "clipboard") {
            items.extend(self.search_files(q, scope, 50));
        }
        if matches!(cat, "all" | "" | "clipboard") {
            let sub = if req.category == "clipboard" { req.clip_sub.as_str() } else { "all" };
            items.extend(self.search_clips(q, sub, 30));
        }
        (items, Vec::new())
    }

    fn smart(&self, req: &SearchRequest) -> (Vec<SearchItemModel>, Vec<IntentChip>) {
        let intent = parse_intent(&req.query);
        let mut scope = req.scope.clone();
        if (scope.time_preset.is_empty() || scope.time_preset == "all") && !intent.time_preset.is_empty() {
            scope.time_preset = intent.time_preset.clone();
            if let Some((s, e)) = intent.time_range {
                scope.custom_start_time = Some(s);
                scope.custom_end_time = Some(e);
            }
        }
        if (scope.type_category.is_empty() || scope.type_category == "all") && !intent.type_category.is_empty() {
            scope.type_category = intent.type_category.clone();
        }
        let mut chips = Vec::new();
        if !intent.keywords.is_empty() {
            chips.push(IntentChip { key: "关键词".into(), val: intent.keywords.join(" · ") });
        }
        if !intent.type_label.is_empty() {
            chips.push(IntentChip { key: "类型".into(), val: intent.type_label.clone() });
        }
        if !intent.time_label.is_empty() {
            chips.push(IntentChip { key: "修改".into(), val: intent.time_label.clone() });
        }
        if let Some(d) = scope.custom_directory.as_ref().filter(|d| !d.is_empty()) {
            chips.push(IntentChip { key: "位置".into(), val: d.clone() });
        }
        if self.embedding.is_ready() {
            chips.push(IntentChip { key: "向量".into(), val: self.embedding.name().into() });
        }

        let mut merged: HashMap<String, SearchItemModel> = HashMap::new();
        let keywords: Vec<String> = if intent.keywords.is_empty() { vec![String::new()] } else { intent.keywords.clone() };
        for kw in &keywords {
            let sub_req = SearchRequest { query: kw.clone(), scope: scope.clone(), ..req.clone() };
            let (items, _) = self.fast(&sub_req);
            for mut it in items {
                it.section = "智能匹配".into();
                if !kw.is_empty() {
                    it.reason = format!("名称匹配「{kw}」");
                }
                merged
                    .entry(it.id.clone())
                    .and_modify(|e| {
                        e.score += it.score * 0.6;
                        if !kw.is_empty() {
                            e.reason = format!("{} · 「{kw}」", e.reason);
                        }
                    })
                    .or_insert(it);
            }
            // 正文全文检索
            if kw.chars().count() >= 2 {
                for hit in self.storage.search_content(kw, 40).unwrap_or_default() {
                    let Some(rec) = self.storage.file_by_path(&hit.path) else { continue };
                    let mut item = file_to_item(&rec);
                    if !self.item_in_scope(&item, &scope) {
                        continue;
                    }
                    item.section = "智能匹配".into();
                    item.score = 48.0;
                    let snippet = hit.snippet.replace(['\n', '\r'], " ").trim().chars().take(70).collect::<String>();
                    item.reason = format!("内容匹配「{kw}」: {snippet}");
                    item.preview = snippet.clone();
                    self.boost_by_usage(&mut item);
                    merged
                        .entry(item.id.clone())
                        .and_modify(|e| {
                            e.score += 25.0;
                            e.reason = format!("{} · 内容匹配", e.reason);
                        })
                        .or_insert(item);
                }
            }
        }
        let mut items: Vec<SearchItemModel> = merged.into_values().collect();
        for it in items.iter_mut() {
            if let Some(ts) = it.modified_time {
                if !intent.time_label.is_empty() {
                    it.reason = format!("{} · 修改于{}", it.reason, fmt_relative(ts));
                }
            }
            if it.reason.is_empty() {
                it.reason = "语义相关".into();
            }
        }
        items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        items.truncate(req.limit.max(20));
        (items, chips)
    }
}

fn category_of(item: &SearchItemModel) -> &'static str {
    match item.item_type {
        ItemType::App | ItemType::Command => "app",
        ItemType::File | ItemType::Folder => "file",
        ItemType::Clipboard => "clipboard",
    }
}

impl SearchProvider for SearchEngine {
    fn search(&self, req: &SearchRequest) -> SearchResponse {
        let started = std::time::Instant::now();
        let q = req.query.trim();
        let (mut items, chips) = if q.is_empty() {
            (self.idle_items(&req.scope), Vec::new())
        } else if req.mode == SearchMode::Smart {
            self.smart(req)
        } else {
            self.fast(req)
        };

        // 分类计数���分类过滤前）
        let mut counts = [0usize; 4];
        counts[0] = items.len();
        for it in &items {
            match category_of(it) {
                "file" => counts[1] += 1,
                "app" => counts[2] += 1,
                _ => counts[3] += 1,
            }
        }
        // 分类过滤 + 剪贴板二级
        if !req.category.is_empty() && req.category != "all" {
            items.retain(|it| category_of(it) == req.category);
            if req.category == "clipboard" && !req.clip_sub.is_empty() && req.clip_sub != "all" {
                items.retain(|it| it.sub_type == req.clip_sub);
            }
        }
        // 分组排序：最近使用/智能匹配保持得分序；极速模式按 应用 → 文件 → 剪贴板
        if !q.is_empty() && req.mode == SearchMode::Fast {
            let order = |s: &str| match s {
                "应用" => 0,
                "文件" => 1,
                "剪贴板" => 2,
                _ => 3,
            };
            items.sort_by(|a, b| {
                order(&a.section)
                    .cmp(&order(&b.section))
                    .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
            });
        }
        let limit = if req.limit == 0 { 80 } else { req.limit };
        items.truncate(limit);
        SearchResponse {
            items,
            elapsed_ms: started.elapsed().as_millis() as u32,
            intent_chips: chips,
            counts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_parsing() {
        let i = parse_intent("找一下昨天修改的 EasyNote 文档");
        assert_eq!(i.keywords, vec!["EasyNote".to_string()]);
        assert_eq!(i.time_preset, "range");
        assert_eq!(i.type_category, "document");
        let i = parse_intent("记录 EasyNote 数据怎么存的文件");
        assert!(i.keywords.iter().any(|k| k == "EasyNote"));
        assert!(i.keywords.iter().any(|k| k == "数据"));
    }

    #[test]
    fn clip_kinds() {
        assert_eq!(clip_kind("https://slint.dev/docs"), "url");
        assert_eq!(clip_kind("docker compose up -d"), "code");
        assert_eq!(clip_kind("{\n  \"name\": \"x\"\n}"), "code");
        assert_eq!(clip_kind("Windows 本地统一入口设计规范"), "text");
    }

    #[test]
    fn engine_search_roundtrip() {
        let storage = Arc::new(Storage::open_in_memory().unwrap());
        storage
            .upsert_files(&[FileRecord { path: "D:\\Infra\\docker-compose.yml".into(), name: "docker-compose.yml".into(), ext: "yml".into(), mtime: now_ts(), ..Default::default() }])
            .unwrap();
        storage.add_clip("docker compose up -d", "code", true, 50).unwrap();
        let engine = SearchEngine::new(storage);
        engine.set_apps(vec![AppRecord { name: "Docker Desktop".into(), launch_path: "C:\\d.lnk".into(), target: "C:\\Docker Desktop.exe".into(), pinyin: "dockerdesktop".into(), initials: "dd".into(), ..Default::default() }]);
        let resp = engine.search(&SearchRequest { query: "docker".into(), category: "all".into(), ..Default::default() });
        assert_eq!(resp.counts, [3, 1, 1, 1]);
        assert_eq!(resp.items[0].section, "应用");
        let resp = engine.search(&SearchRequest { query: "docker".into(), category: "clipboard".into(), ..Default::default() });
        assert_eq!(resp.items.len(), 1);
    }
}
