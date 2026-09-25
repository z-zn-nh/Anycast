//! 应用扫描：解析开始菜单 / 桌面的 .lnk 快捷方式与 .exe，生成应用目录（含拼音索引）。

use crate::core::storage::AppRecord;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// 取系统「已知文件夹」。比读环境变量可靠得多 —— 环境变量可能**整个缺失**，
/// 也可能**存在但为空串**（从脚本 / 计划任务 / 工具链启动时很常见）。
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let pw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = pw.to_string().ok();
        CoTaskMemFree(Some(pw.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// 读环境变量并**过滤空串**。
///
/// ⚠️ 踩过的坑：`std::env::var("ProgramData")` 对「已设置但为空」的变量返回 `Ok("")`，
/// 原实现没做判空，于是拼出 `Microsoft\Windows\Start Menu\Programs` 这个**相对路径**，
/// 而 `scan_apps()` 里 `!dir.exists()` 直接 continue —— 结果静默返回 0 个应用，
/// 日志只有一句「应用扫描完成：0 个」，极难定位。
/// 实测：从脚本环境启动（`ProgramData=` 为空）→ 0 个；补齐变量 → 136 个。
fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// 扫描目录列表：已知文件夹优先，环境变量兜底。
fn scan_dirs() -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_CommonStartMenu, FOLDERID_PublicDesktop, FOLDERID_StartMenu};

    const START_MENU: &str = "Microsoft\\Windows\\Start Menu\\Programs";
    let mut dirs = Vec::new();

    if let Some(d) = known_folder(&FOLDERID_CommonStartMenu) {
        dirs.push(d);
    } else if let Some(pd) = env_dir("ProgramData") {
        dirs.push(pd.join(START_MENU));
    }

    if let Some(d) = known_folder(&FOLDERID_StartMenu) {
        dirs.push(d);
    } else if let Some(ad) = env_dir("APPDATA") {
        dirs.push(ad.join(START_MENU));
    }

    if let Some(ud) = directories::UserDirs::new() {
        if let Some(d) = ud.desktop_dir() {
            dirs.push(d.to_path_buf());
        }
    }

    if let Some(d) = known_folder(&FOLDERID_PublicDesktop) {
        dirs.push(d);
    } else if let Some(public) = env_dir("PUBLIC") {
        dirs.push(public.join("Desktop"));
    }

    if dirs.is_empty() {
        log::warn!("未发现任何可扫描的应用目录：已知文件夹与环境变量均不可用，应用列表将为空");
    }
    dirs
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide_buf(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// 解析 .lnk：返回 (目标路径, 参数)
pub fn resolve_shortcut(lnk: &Path) -> Option<(String, String)> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        let wide = to_wide(&lnk.to_string_lossy());
        pf.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        let mut fd: WIN32_FIND_DATAW = std::mem::zeroed();
        link.GetPath(&mut buf, &mut fd, 0).ok()?;
        let target = from_wide_buf(&buf);
        let mut abuf = [0u16; 1024];
        let args = if link.GetArguments(&mut abuf).is_ok() { from_wide_buf(&abuf) } else { String::new() };
        Some((target, args))
    }
}

fn pinyin_of(name: &str) -> (String, String) {
    use pinyin::ToPinyin;
    let mut full = String::new();
    let mut initials = String::new();
    for (c, p) in name.chars().zip(name.to_pinyin()) {
        match p {
            Some(p) => {
                full.push_str(p.plain());
                initials.push_str(p.first_letter());
            }
            None => {
                if c.is_ascii_alphanumeric() {
                    let lc = c.to_ascii_lowercase();
                    full.push(lc);
                    initials.push(lc);
                }
            }
        }
    }
    (full, initials)
}

fn is_noise(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("uninstall")
        || n.contains("卸载")
        || n.contains("readme")
        || n.contains("release notes")
        || n.contains("license")
        || n.ends_with(" help")
        || n == "help"
}

/// 全量扫描应用，返回去重后的应用列表（已按名称排序）。
pub fn scan_apps() -> Vec<AppRecord> {
    let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let mut map: HashMap<String, AppRecord> = HashMap::new();

    for dir in scan_dirs() {
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(&dir).max_depth(4).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").trim().to_string();
            if stem.is_empty() || is_noise(&stem) {
                continue;
            }
            let (launch, target, args) = match ext.as_str() {
                "lnk" => {
                    let (target, args) = resolve_shortcut(path).unwrap_or_default();
                    let tl = target.to_lowercase();
                    // 过滤指向文档、网页、卸载器等的快捷方式
                    if !target.is_empty()
                        && !(tl.ends_with(".exe") || tl.ends_with(".bat") || tl.ends_with(".cmd") || tl.ends_with(".msc"))
                    {
                        continue;
                    }
                    if tl.contains("uninstall") || tl.contains("unins0") {
                        continue;
                    }
                    (path.to_string_lossy().to_string(), target, args)
                }
                "exe" => (path.to_string_lossy().to_string(), path.to_string_lossy().to_string(), String::new()),
                "url" | "appref-ms" => (path.to_string_lossy().to_string(), String::new(), String::new()),
                _ => continue,
            };
            let key = if target.is_empty() { launch.to_lowercase() } else { format!("{}|{}", target.to_lowercase(), args.to_lowercase()) };
            let (pinyin, initials) = pinyin_of(&stem);
            let rec = AppRecord { id: 0, name: stem, launch_path: launch, target, args, pinyin, initials };
            match map.get(&key) {
                Some(existing) if existing.name.len() <= rec.name.len() => {}
                _ => {
                    map.insert(key, rec);
                }
            }
        }
    }

    if com_ok {
        unsafe { CoUninitialize() };
    }
    let mut apps: Vec<AppRecord> = map.into_values().collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinyin_initials() {
        let (full, ini) = pinyin_of("微信 WeChat");
        assert!(full.starts_with("weixin"));
        assert!(ini.starts_with("wx"));
    }
}
