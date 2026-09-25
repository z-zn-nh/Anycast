//! 用户配置（config.json）读写。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}
fn default_wake_hotkey() -> String {
    "Alt+Space".into()
}
fn default_mode() -> String {
    "fast".into()
}
fn default_theme() -> String {
    "dark".into()
}
fn default_opacity() -> String {
    "balanced".into()
}
fn default_view_mode() -> String {
    "list".into()
}
fn default_width() -> u32 {
    860
}
fn default_height() -> u32 {
    560
}
fn default_clip_limit() -> u32 {
    500
}
fn default_content_max_kb() -> u32 {
    512
}

fn default_ai_backend_mode() -> String {
    "auto".into()
}

fn default_jev_endpoint() -> String {
    "https://api.typesafe.ai/v1/systemone".into()
}

fn default_jev_model() -> String {
    "jev-latest".into()
}

fn default_excluded() -> Vec<String> {
    [
        "node_modules",
        ".git",
        "target",
        "dist",
        ".next",
        "__pycache__",
        ".cache",
        ".vs",
        ".idea",
        "obj",
        "$RECYCLE.BIN",
        "System Volume Information",
        "AppData",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// 默认索引根目录：桌面、文档、下载、图片、视频
pub fn default_index_roots() -> Vec<String> {
    let mut roots = Vec::new();
    if let Some(dirs) = directories::UserDirs::new() {
        for p in [
            dirs.desktop_dir(),
            dirs.document_dir(),
            dirs.download_dir(),
            dirs.picture_dir(),
            dirs.video_dir(),
        ]
        .into_iter()
        .flatten()
        {
            roots.push(p.to_string_lossy().to_string());
        }
    }
    roots
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    // 常规
    #[serde(default = "default_wake_hotkey")]
    pub wake_hotkey: String,
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default = "default_true")]
    pub hide_on_blur: bool,
    #[serde(default = "default_mode")]
    pub default_mode: String,
    #[serde(default = "default_true")]
    pub double_click_launch: bool,
    // 外观
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_opacity")]
    pub acrylic_opacity: String,
    #[serde(default = "default_true")]
    pub gpu_blur: bool,
    #[serde(default = "default_true")]
    pub rim_light: bool,
    #[serde(default = "default_view_mode")]
    pub view_mode: String,
    #[serde(default)]
    pub pinned_collapsed: bool,
    /// 首次启动是否已完成「常用应用预置到置顶」（设计稿 65.4 置顶空状态）。
    /// 置为 true 后即使用户手动清空全部置顶，也不会再次灌入。
    #[serde(default)]
    pub pins_seeded: bool,
    #[serde(default)]
    pub filter_shelf_open: bool,
    #[serde(default = "default_width")]
    pub window_width: u32,
    #[serde(default = "default_height")]
    pub window_height: u32,
    // 快捷直达
    #[serde(default = "default_true")]
    pub hotkeys_master_enabled: bool,
    // 搜索与索引
    #[serde(default = "default_index_roots")]
    pub index_roots: Vec<String>,
    #[serde(default = "default_excluded")]
    pub excluded_dirs: Vec<String>,
    #[serde(default = "default_true")]
    pub incremental_index: bool,
    #[serde(default = "default_true")]
    pub content_index_enabled: bool,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default = "default_true")]
    pub exclude_build_caches: bool,
    #[serde(default = "default_content_max_kb")]
    pub content_max_kb: u32,
    // 剪贴板
    #[serde(default = "default_true")]
    pub clipboard_enabled: bool,
    #[serde(default = "default_clip_limit")]
    pub clipboard_limit: u32,
    #[serde(default = "default_true")]
    pub clipboard_dedupe: bool,
    #[serde(default = "default_true")]
    pub clipboard_ignore_password_managers: bool,
    #[serde(default)]
    pub clipboard_mask_sensitive: bool,
    // AI
    #[serde(default)]
    pub ai_enabled: bool,
    #[serde(default = "default_true")]
    pub ai_lazy_load: bool,
    #[serde(default = "default_true")]
    pub ai_intent_parsing: bool,
    // AI —— 判断模型后端（开发文档 Phase 2）
    /// `"auto"` | `"rule"` | `"laya"` | `"jev"`
    ///
    /// `auto` = 逐级升级：规则 → 本地模型 →（若开云端回退）云端，任一级成功即止。
    #[serde(default = "default_ai_backend_mode")]
    pub ai_backend_mode: String,
    /// 本地置信度低于阈值时是否升级到云端。**默认关**（§5.6）。
    #[serde(default)]
    pub ai_cloud_fallback: bool,
    /// Jev 端点完整 URL。默认官方端点；托管端点见 `decision::jev::HOSTED_ENDPOINT`。
    #[serde(default = "default_jev_endpoint")]
    pub ai_jev_endpoint: String,
    /// API Key。⚠️ 明文存本地配置；环境变量 `TYPESAFE_API_KEY` / `JEV_API_KEY` 优先级更高。
    #[serde(default)]
    pub ai_jev_api_key: String,
    #[serde(default = "default_jev_model")]
    pub ai_jev_model: String,
    /// 显式代理（如 `http://127.0.0.1:7897`）。留空 = 跟随系统代理。
    #[serde(default)]
    pub ai_jev_proxy: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        serde_json::from_str("{}").expect("default settings")
    }
}

/// 应用数据目录：%APPDATA%\Anycast\data
pub fn data_dir() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("", "", "Anycast") {
        return dirs.data_dir().to_path_buf();
    }
    std::env::temp_dir().join("Anycast")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn db_path() -> PathBuf {
    data_dir().join("anycast.db")
}

pub fn load_settings() -> AppSettings {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("配置文件解析失败，使用默认配置: {e}");
                AppSettings::default()
            }
        },
        Err(_) => AppSettings::default(),
    }
}

pub fn save_settings(settings: &AppSettings) -> Result<()> {
    save_to(settings, &config_path())
}

fn save_to(settings: &AppSettings, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建目录 {parent:?}"))?;
    }
    let text = serde_json::to_string_pretty(settings)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text)?;
    if std::fs::rename(&tmp, path).is_err() {
        // 某些环境下 rename 会失败（如目录联结），退化为直接覆盖写入
        std::fs::write(path, &text)?;
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(())
}
