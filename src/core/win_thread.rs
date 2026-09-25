//! 系统消息线程：全局热键（RegisterHotKey / WM_HOTKEY）与剪贴板监听（WM_CLIPBOARDUPDATE）。
//!
//! 独立 OS 线程维护一个 message-only 窗口与 GetMessageW 消息泵，
//! 前台任何全屏程序不受影响；命令通过 channel + WM_APP 唤醒消息投递。

use crate::core::hotkey::HotkeySpec;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage, RegisterClassW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE, WM_HOTKEY,
    WNDCLASSW,
};

const WM_ANYCAST_CMD: u32 = WM_APP + 1;
const CF_UNICODETEXT: u32 = 13;
pub const WAKE_HOTKEY_ID: i32 = 1;
pub const BINDING_ID_BASE: i32 = 0x1000;
const PROBE_ID: i32 = 0x7FF0;

pub enum SysCommand {
    SetWakeHotkey(Option<HotkeySpec>),
    /// 重新注册全部快捷直达绑定 (id, spec)
    SetBindings(Vec<(i32, HotkeySpec)>),
    /// 冲突探测：尝试注册再立即注销
    Probe(HotkeySpec, Sender<Result<(), String>>),
    SetClipboardListening(bool),
    Quit,
}

#[derive(Clone, Debug)]
pub enum SysEvent {
    WakeHotkey,
    BindingHotkey(i32),
    ClipboardText(String),
    /// 注册失败。`id` 是 `WAKE_HOTKEY_ID` 或某个绑定的 id。
    ///
    /// 为什么需要它：`RegisterHotKey` 失败以前只 `log::warn!`，用户完全看不到 ——
    /// 配置照存、界面上显示得好好的，热键就是不生效。而失败是**常态**：
    /// 保存时探测通过、之后别的程序把这个组合键抢走，或者开机时对方先启动。
    HotkeyRegisterFailed { id: i32, reason: String },
}

pub type SysHandler = Arc<dyn Fn(SysEvent) + Send + Sync>;

struct ThreadState {
    rx: Receiver<SysCommand>,
    handler: SysHandler,
    hwnd: HWND,
    wake: Option<HotkeySpec>,
    bindings: Vec<(i32, HotkeySpec)>,
    clipboard_listening: bool,
}

thread_local! {
    static STATE: RefCell<Option<ThreadState>> = const { RefCell::new(None) };
}

pub struct SystemBus {
    tx: Sender<SysCommand>,
    hwnd: AtomicIsize,
}

impl SystemBus {
    pub fn start(handler: SysHandler) -> Arc<SystemBus> {
        let (tx, rx) = unbounded::<SysCommand>();
        let bus = Arc::new(SystemBus { tx, hwnd: AtomicIsize::new(0) });
        let (ready_tx, ready_rx) = unbounded::<isize>();
        let bus_clone = Arc::clone(&bus);
        std::thread::Builder::new()
            .name("anycast-sysbus".into())
            .spawn(move || run_thread(rx, handler, ready_tx))
            .expect("spawn sysbus");
        if let Ok(h) = ready_rx.recv() {
            bus_clone.hwnd.store(h, Ordering::SeqCst);
        }
        bus
    }

    fn post(&self, cmd: SysCommand) {
        let _ = self.tx.send(cmd);
        let h = self.hwnd.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_ANYCAST_CMD, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn set_wake_hotkey(&self, spec: Option<HotkeySpec>) {
        self.post(SysCommand::SetWakeHotkey(spec));
    }

    pub fn set_bindings(&self, bindings: Vec<(i32, HotkeySpec)>) {
        self.post(SysCommand::SetBindings(bindings));
    }

    pub fn set_clipboard_listening(&self, on: bool) {
        self.post(SysCommand::SetClipboardListening(on));
    }

    /// 探测热键是否已被其他程序占用（阻塞等待，最长 1s）
    pub fn probe(&self, spec: HotkeySpec) -> Result<(), String> {
        let (tx, rx) = unbounded();
        self.post(SysCommand::Probe(spec, tx));
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap_or_else(|_| Err("探测超时".into()))
    }

    pub fn quit(&self) {
        self.post(SysCommand::Quit);
    }
}

fn run_thread(rx: Receiver<SysCommand>, handler: SysHandler, ready: Sender<isize>) {
    unsafe {
        let hinst = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default();
        let class_name = w!("AnycastSystemBusWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinst),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                log::error!("创建系统消息窗口失败: {e}");
                let _ = ready.send(0);
                return;
            }
        };
        STATE.with(|s| {
            *s.borrow_mut() = Some(ThreadState {
                rx,
                handler,
                hwnd,
                wake: None,
                bindings: Vec::new(),
                clipboard_listening: false,
            })
        });
        let _ = ready.send(hwnd.0 as isize);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        STATE.with(|s| {
            if let Some(st) = s.borrow_mut().take() {
                unregister_all(&st);
                if st.clipboard_listening {
                    let _ = RemoveClipboardFormatListener(st.hwnd);
                }
            }
        });
    }
}

unsafe fn unregister_all(st: &ThreadState) {
    if st.wake.is_some() {
        let _ = UnregisterHotKey(Some(st.hwnd), WAKE_HOTKEY_ID);
    }
    for (id, _) in &st.bindings {
        let _ = UnregisterHotKey(Some(st.hwnd), *id);
    }
}

unsafe fn register(hwnd: HWND, id: i32, spec: &HotkeySpec) -> Result<(), String> {
    match RegisterHotKey(Some(hwnd), id, spec.win32_modifiers(), spec.vk) {
        Ok(()) => Ok(()),
        Err(e) => {
            // ERROR_HOTKEY_ALREADY_REGISTERED = 1409
            if e.code().0 as u32 & 0xFFFF == 1409 {
                Err("该组合键已被系统或其他程序占用".into())
            } else {
                Err(format!("注册失败: {e}"))
            }
        }
    }
}

/// 处理挂起的命令。**返回待发事件，而不是自己回调 handler** ——
/// 本函数是在 `STATE.with(|s| s.borrow_mut())` 持有期间被调用的，
/// 在里面回调会让 handler 重入同一把 RefCell 借用。由 `wndproc` 在借用释放后再发。
unsafe fn handle_commands(st: &mut ThreadState) -> Vec<SysEvent> {
    let mut evs = Vec::new();
    while let Ok(cmd) = st.rx.try_recv() {
        match cmd {
            SysCommand::SetWakeHotkey(spec) => {
                if st.wake.is_some() {
                    let _ = UnregisterHotKey(Some(st.hwnd), WAKE_HOTKEY_ID);
                    st.wake = None;
                }
                if let Some(s) = spec {
                    match register(st.hwnd, WAKE_HOTKEY_ID, &s) {
                        Ok(()) => st.wake = Some(s),
                        Err(e) => {
                            // 注意旧键已经先注销了：注册失败意味着**唤醒热键彻底没了**，
                            // 不是「保持原样」。这个后果必须让用户知道。
                            log::warn!("唤醒热键注册失败: {e}");
                            evs.push(SysEvent::HotkeyRegisterFailed { id: WAKE_HOTKEY_ID, reason: e });
                        }
                    }
                }
            }
            SysCommand::SetBindings(list) => {
                for (id, _) in &st.bindings {
                    let _ = UnregisterHotKey(Some(st.hwnd), *id);
                }
                st.bindings.clear();
                for (id, spec) in list {
                    match register(st.hwnd, id, &spec) {
                        Ok(()) => st.bindings.push((id, spec)),
                        Err(e) => {
                            log::warn!("快捷直达热键 {id} 注册失败: {e}");
                            evs.push(SysEvent::HotkeyRegisterFailed { id, reason: e });
                        }
                    }
                }
            }
            SysCommand::Probe(spec, reply) => {
                let r = register(st.hwnd, PROBE_ID, &spec);
                if r.is_ok() {
                    let _ = UnregisterHotKey(Some(st.hwnd), PROBE_ID);
                }
                let _ = reply.send(r);
            }
            SysCommand::SetClipboardListening(on) => {
                if on && !st.clipboard_listening {
                    if AddClipboardFormatListener(st.hwnd).is_ok() {
                        st.clipboard_listening = true;
                    }
                } else if !on && st.clipboard_listening {
                    let _ = RemoveClipboardFormatListener(st.hwnd);
                    st.clipboard_listening = false;
                }
            }
            SysCommand::Quit => {
                PostQuitMessage(0);
            }
        }
    }
    evs
}

unsafe fn read_clipboard_text(hwnd: HWND) -> Option<String> {
    if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
        return None;
    }
    OpenClipboard(Some(hwnd)).ok()?;
    let mut out = None;
    if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
        let hmem = HGLOBAL(handle.0);
        let ptr = GlobalLock(hmem) as *const u16;
        if !ptr.is_null() {
            let max = GlobalSize(hmem) / 2;
            let mut len = 0usize;
            while len < max && *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            out = Some(String::from_utf16_lossy(slice));
            let _ = GlobalUnlock(hmem);
        }
    }
    let _ = CloseClipboard();
    out
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            let handler = STATE.with(|s| s.borrow().as_ref().map(|st| Arc::clone(&st.handler)));
            if let Some(h) = handler {
                if id == WAKE_HOTKEY_ID {
                    h(SysEvent::WakeHotkey);
                } else if id >= BINDING_ID_BASE {
                    h(SysEvent::BindingHotkey(id));
                }
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            let handler = STATE.with(|s| s.borrow().as_ref().map(|st| Arc::clone(&st.handler)));
            if let Some(h) = handler {
                // 部分程序写入剪贴板后延迟渲染，稍等再读
                std::thread::sleep(std::time::Duration::from_millis(30));
                if let Some(text) = read_clipboard_text(hwnd) {
                    if !text.trim().is_empty() {
                        h(SysEvent::ClipboardText(text));
                    }
                }
            }
            LRESULT(0)
        }
        WM_ANYCAST_CMD => {
            // 借用分开两次取：handle_commands 要 borrow_mut，回调要 borrow。
            // 合成一次写会 panic。事件在借用全部释放后才发。
            let evs = STATE.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    handle_commands(st)
                } else {
                    Vec::new()
                }
            });
            let handler = STATE.with(|s| s.borrow().as_ref().map(|st| Arc::clone(&st.handler)));
            if let Some(h) = handler {
                for ev in evs {
                    h(ev);
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
