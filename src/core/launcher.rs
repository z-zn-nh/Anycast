//! 系统动作：启动 / 打开 / 定位 / 复制 / 已运行窗口前置 / 开机自启。

use anyhow::{anyhow, Result};
use std::path::Path;

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, LPARAM};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_MENU};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL,
};

const CF_UNICODETEXT: u32 = 13;

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 通过 ShellExecute 打开任意路径 / URL / 快捷方式
pub fn shell_open(target: &str) -> Result<()> {
    shell_open_with(target, "", "")
}

pub fn shell_open_with(target: &str, args: &str, verb: &str) -> Result<()> {
    let file = to_wide(target);
    let params = to_wide(args);
    let verb_w = to_wide(if verb.is_empty() { "open" } else { verb });
    let dir: Vec<u16> = Path::new(target)
        .parent()
        .map(|p| to_wide(&p.to_string_lossy()))
        .unwrap_or_else(|| vec![0]);
    let h = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(file.as_ptr()),
            if args.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) },
            if dir.len() > 1 { PCWSTR(dir.as_ptr()) } else { PCWSTR::null() },
            SW_SHOWNORMAL,
        )
    };
    if (h.0 as usize) <= 32 {
        return Err(anyhow!("ShellExecute 失败 (code {}): {}", h.0 as usize, target));
    }
    Ok(())
}

/// 在资源管理器中定位
pub fn reveal_in_explorer(path: &str) -> Result<()> {
    if Path::new(path).is_dir() {
        return shell_open(path);
    }
    shell_open_with("explorer.exe", &format!("/select,\"{path}\""), "open")
}

/// 写入系统剪贴板文本
pub fn set_clipboard_text(text: &str) -> Result<()> {
    unsafe {
        OpenClipboard(None)?;
        let result = (|| -> Result<()> {
            EmptyClipboard()?;
            let wide = to_wide(text);
            let bytes = wide.len() * 2;
            let hmem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let ptr = GlobalLock(hmem) as *mut u16;
            if ptr.is_null() {
                return Err(anyhow!("GlobalLock 失败"));
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hmem);
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0)))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

struct EnumCtx {
    target: String,
    found: Option<HWND>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    // 仅顶层主窗口（无 owner）
    if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
        if !owner.0.is_null() {
            return BOOL(1);
        }
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }
    if let Ok(proc_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(proc_handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(proc_handle);
        if ok {
            let exe = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if exe == ctx.target {
                ctx.found = Some(hwnd);
                return BOOL(0);
            }
        }
    }
    BOOL(1)
}

/// 查找已运行的、可执行文件路径等于 target 的顶层窗口
pub fn find_running_window_by_path(exe_path: &str) -> Option<HWND> {
    let mut ctx = EnumCtx { target: exe_path.to_lowercase(), found: None };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut EnumCtx as isize));
    }
    ctx.found
}

/// 强制前置窗口（绕过 SetForegroundWindow 限制：AttachThreadInput + 模拟 Alt 键）
pub fn force_foreground(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            return;
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let cur_thread = GetCurrentThreadId();
        if fg_thread != 0 && fg_thread != cur_thread {
            let _ = AttachThreadInput(cur_thread, fg_thread, true);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(cur_thread, fg_thread, false);
        }
        if GetForegroundWindow() != hwnd {
            // 模拟一次 Alt 键按下/释放以解除前台锁
            keybd_event(VK_MENU.0 as u8, 0, Default::default(), 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// 快捷直达：已运行则前置激活，否则启动。
pub fn activate_or_launch(target_path: &str, item_type: &str) -> Result<&'static str> {
    if item_type == "app" {
        let lower = target_path.to_lowercase();
        let exe = if lower.ends_with(".lnk") {
            crate::core::apps::resolve_shortcut(Path::new(target_path)).map(|(t, _)| t).unwrap_or_default()
        } else {
            target_path.to_string()
        };
        if !exe.is_empty() {
            if let Some(hwnd) = find_running_window_by_path(&exe) {
                force_foreground(hwnd);
                return Ok("activated");
            }
        }
    }
    shell_open(target_path)?;
    Ok("launched")
}

/// 开机自启（HKCU\...\Run）
pub fn set_launch_on_startup(enabled: bool) -> Result<()> {
    unsafe {
        let mut key = HKEY::default();
        let sub = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let err = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), None, KEY_SET_VALUE, &mut key);
        if err.is_err() {
            return Err(anyhow!("打开注册表 Run 键失败: {:?}", err));
        }
        let name = to_wide("Anycast");
        let result = if enabled {
            let exe = std::env::current_exe()?;
            let value = to_wide(&format!("\"{}\" --silent", exe.to_string_lossy()));
            let bytes: Vec<u8> = value.iter().flat_map(|c| c.to_le_bytes()).collect();
            let err = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes));
            if err.is_err() { Err(anyhow!("写入注册表失败: {:?}", err)) } else { Ok(()) }
        } else {
            let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            Ok(())
        };
        let _ = RegCloseKey(key);
        result
    }
}

/// 当前前台窗口所属进程的可执行文件名（用于忽略密码管理器等）
pub fn foreground_exe_name() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&full).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }
}
