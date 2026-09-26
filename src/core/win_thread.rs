//! 系统消息线程：全局热键（RegisterHotKey / WM_HOTKEY）与剪贴板监听（WM_CLIPBOARDUPDATE）。
//!
//! 独立 OS 线程维护一个 message-only 窗口与 GetMessageW 消息泵，
//! 前台任何全屏程序不受影响；命令通过 channel + WM_APP 唤醒消息投递。

use crate::core::hotkey::HotkeySpec;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW, PostQuitMessage,
    RegisterClassW, SetTimer, TranslateMessage, EVENT_SYSTEM_FOREGROUND, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_CLIPBOARDUPDATE, WM_HOTKEY, WM_TIMER, WNDCLASSW,
};

const WM_ANYCAST_CMD: u32 = WM_APP + 1;
const CF_UNICODETEXT: u32 = 13;
pub const WAKE_HOTKEY_ID: i32 = 1;
pub const BINDING_ID_BASE: i32 = 0x1000;
const PROBE_ID: i32 = 0x7FF0;
/// 投递自检的**对照**组合键。它只用来证明「本机此刻注入得动」。
/// 选 F13 是因为没有任何程序会响应它，注入它绝无副作用。
const VERIFY_CONTROL_ID: i32 = 0x7FF1;
const VERIFY_TIMER_ID: usize = 0xA1;
/// 单相等待上限。注入到 WM_HOTKEY 是毫秒级，500ms 已经非常宽松。
const VERIFY_WAIT_MS: u32 = 500;
/// 自检用的对照组合键（VK_F13 = 0x7C）。
const VERIFY_CONTROL_VK: u32 = 0x7C;

pub enum SysCommand {
    SetWakeHotkey(Option<HotkeySpec>),
    /// 重新注册全部快捷直达绑定 (id, spec)
    SetBindings(Vec<(i32, HotkeySpec)>),
    /// 冲突探测：尝试注册再立即注销
    Probe(HotkeySpec, Sender<Result<(), String>>),
    /// 投递自检：对**当前已注册的唤醒热键**真注入一次，看 `WM_HOTKEY` 到不到。
    ///
    /// 与 `Probe` 的区别是本质性的：`Probe` 只证明「注册得进去」，
    /// 这个证明「按键送得到」。后者才是用户能感知的那件事。
    VerifyWakeDelivery(Sender<WakeDelivery>),
    SetClipboardListening(bool),
    Quit,
}

/// 唤醒热键「注册成功之后，按键到底到不到」的实测结论。
///
/// 存在的理由：`RegisterHotKey` 成功只说明注册表里有了这一条。别的程序可以用
/// `SetWindowsHookEx(WH_KEYBOARD_LL)` 装一个全局低级键盘钩子，在回调里
/// `return 1` 把键**在投递之前**吃掉。钩子不占注册表，所以
/// `ERROR_HOTKEY_ALREADY_REGISTERED(1409)` 永远不会出现 —— 界面显示注册好了、
/// 按下去毫无反应，且没有任何错误可看。**唯一能发现它的办法就是真按一次。**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeDelivery {
    /// 注入后真收到了 `WM_HOTKEY`。
    Delivered,
    /// 对照组合键收到了、目标没收到 → 有程序用键盘钩子把它吞了。
    Swallowed,
    /// 连对照组合键都收不到 → 本机此刻的注入自检不成立，**不能**下结论。
    ///
    /// 这一档不能省：`Alt+Space` 家族本身就存在「合成按键打不进去」的情况
    /// （系统菜单加速键）。少了它，就会把「测不了」误报成「被占用」。
    Inconclusive,
    /// 当前没有已注册的唤醒热键。
    NotRegistered,
}

impl WakeDelivery {
    /// 给用户看的一句话。措辞刻意分开：只有 `Swallowed` 才允许说「被占用」。
    ///
    /// 写成**独立成句**的形式，这样既能接在「— 」后面，也能接在
    /// 「⚠ 唤醒快捷键 X 」后面，不必为两个入口各写一份文案（文案分叉 = 迟早不一致）。
    pub fn describe(self) -> &'static str {
        match self {
            WakeDelivery::Delivered => "按键投递正常",
            WakeDelivery::Swallowed => {
                "按键收不到 —— 有程序用低级键盘钩子占用了它。\
                 这种占用不占注册表，所以注册会成功、也不会报冲突，请换一个组合键"
            }
            WakeDelivery::Inconclusive => "无法判定 —— 本机的按键注入自检不成立（对照组合键也收不到）",
            WakeDelivery::NotRegistered => "当前未注册唤醒快捷键",
        }
    }
}

/// 自检的两个阶段。对照先跑，通过了才轮到目标 ——
/// 顺序不能反，否则「目标收不到」无法归因。
#[derive(Clone, Copy, PartialEq, Eq)]
enum VerifyPhase {
    Control,
    Target,
}

struct VerifyRun {
    reply: Sender<WakeDelivery>,
    phase: VerifyPhase,
}

/// `WM_HOTKEY` 进来时自检该怎么认领它。抽成枚举是为了让「判定」与「动作」
/// 分成两次借用，避免同一把 `RefCell` 被同时可变借用。
enum VerifyStep {
    Unrelated,
    ControlPassed,
    TargetDelivered,
}

#[derive(Clone, Debug)]
pub enum SysEvent {
    WakeHotkey,
    BindingHotkey(i32),
    ClipboardText(String),
    /// 系统前台窗口变了（`EVENT_SYSTEM_FOREGROUND`）。
    ///
    /// 与 `WM_HOTKEY` 走的是**同一个 handler**，所以它同样在系统消息线程上发出 ——
    /// 消费方必须自己跨线程（`AppCore` 那一层已经统一转成
    /// `BackendNotification`，由 GUI 在 Slint 事件循环里收）。
    ///
    /// 存在的理由只有一个：把「本窗口激活态变了」的发现延迟从 200ms 轮询降到
    /// 「事件发生即回调」，好让 DWM 画上的浅色原生标题栏更快被擦掉。
    /// 轮询仍然保留作兜底（out-of-context 钩子在队列拥塞时会被系统丢弃）。
    ForegroundChanged,
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
    /// 正在跑的投递自检。同一时刻最多一个。
    verify: Option<VerifyRun>,
}

thread_local! {
    static STATE: RefCell<Option<ThreadState>> = const { RefCell::new(None) };
}

/// WinEvent 钩子回调专用的事件出口。
///
/// 为什么不直接用 `STATE` 里的 `handler`：钩子回调**可能在 `handle_commands`
/// 持有 `STATE` 可变借用期间被派发** —— `handle_commands` 里有 `SendInput`
/// （投递自检注入按键），而注入按键会改前台窗口，前台变化正是本钩子监听的事件。
/// 那时再 `STATE.with(|s| s.borrow())` 就是重入同一把 `RefCell` → panic。
/// 用独立 static 存取（`SysHandler` 是 `Send + Sync`），整条路径不碰 `RefCell`。
static EVENT_HANDLER: OnceLock<SysHandler> = OnceLock::new();

/// `EVENT_SYSTEM_FOREGROUND` 的回调，**在系统消息线程上被调用**。
///
/// 只做一件事：把事件转给 handler。两条禁令 ——
///   * **绝不能碰 `STATE`**（理由见 `EVENT_HANDLER` 的注释）；
///   * **不能做任何耗时的事**：回调是串行派发的，堵住它等于堵住之后所有前台通知。
///
/// 真正「本窗口是不是前台」的判断留给 GUI 侧做：这个回调拿到的 `hwnd` 是
/// **新的前台窗口**，抢焦点时它是对方，和我们要判的东西不是一回事。
unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // 当前只订阅了这一个事件，理论上收不到别的；仍然判一次 ——
    // 将来若把 eventmin/max 放宽，不至于静默串味。
    if event != EVENT_SYSTEM_FOREGROUND {
        return;
    }
    if let Some(h) = EVENT_HANDLER.get() {
        h(SysEvent::ForegroundChanged);
    }
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

    /// 唤醒热键投递自检（阻塞等待）。两相各 500ms 上限，留足余量给 3s。
    pub fn verify_wake_delivery(&self) -> WakeDelivery {
        let (tx, rx) = unbounded::<WakeDelivery>();
        self.post(SysCommand::VerifyWakeDelivery(tx));
        rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap_or(WakeDelivery::Inconclusive)
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
        // 钩子回调的出口先备好：`win_event_proc` 不经过 `STATE`，必须在这里单独登记。
        // `handler` 是 `Arc`，克隆一份进 static，另一份照旧进 `ThreadState`。
        if EVENT_HANDLER.set(Arc::clone(&handler)).is_err() {
            // 正常不会发生（`SystemBus::start` 全进程只调一次）。
            // 真发生了说明有两个消息线程，钩子事件会全归第一个 —— 值得留痕。
            log::warn!("WinEvent handler 已存在，前台变化通知可能落到旧的出口上");
        }
        STATE.with(|s| {
            *s.borrow_mut() = Some(ThreadState {
                rx,
                handler,
                hwnd,
                wake: None,
                bindings: Vec::new(),
                clipboard_listening: false,
                verify: None,
            })
        });
        // 前台窗口变化的**即时**通知，用来擦 DWM 画上的原生标题栏。
        //
        // `WINEVENT_OUTOFCONTEXT`：回调在本线程（消息线程）执行，所以本线程必须
        // 一直在泵消息 —— 下面那个 `GetMessageW` 循环就是。**不要**把这里改成
        // `WINEVENT_INCONTEXT`：那会要求一个 DLL，且回调在目标进程里跑。
        //
        // 装不上不算致命：GUI 侧的 200ms 轮询仍在跑，只是退回「最坏 200ms 才发现」，
        // 也就是装上之前的行为。所以这里只告警、不中断。
        //
        // `ANYCAST_NO_FOREGROUND_HOOK=1` 时**不装**，专供 A/B：要证明「钩子真把
        // 发现延迟降下来了」，就得让同一台机器上的两次测量**只差这一个变量**
        // （判据见 doc §12.5.5）。
        let hook = if std::env::var("ANYCAST_NO_FOREGROUND_HOOK").ok().as_deref() == Some("1") {
            log::info!("ANYCAST_NO_FOREGROUND_HOOK=1：不装前台变化钩子（A/B 对照），退回 200ms 轮询");
            HWINEVENTHOOK::default()
        } else {
            let h = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if h.is_invalid() {
                log::warn!("SetWinEventHook(EVENT_SYSTEM_FOREGROUND) 安装失败，前台变化退回 200ms 轮询");
            } else {
                log::info!("前台变化钩子已装上（EVENT_SYSTEM_FOREGROUND）：激活态变化即时通知");
            }
            h
        };
        let _ = ready.send(hwnd.0 as isize);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
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

/// 把一个组合键展开成「按下/抬起」序列。
///
/// ⚠️ **整串必须压进一次 `SendInput`**：分多次调用、或在修饰键与主键之间留间隔，
/// 系统会把修饰键的按下当成「进入菜单模式」，随后的主键被当菜单激活键吞掉。
/// 实测（见 skill `windows-global-hotkey-verification`）`Ctrl+Alt+Space` 与
/// `Alt+Tab` 都会因此**假失败**。顺序固定为「修饰键正序按下 → 主键按下/抬起 →
/// 修饰键逆序抬起」，抬起的顺序与按下严格相反，否则会留下卡住的修饰键。
fn key_sequence(spec: &HotkeySpec) -> Vec<(u16, bool)> {
    const ORDER: [(u32, u16); 4] = [
        (MOD_CONTROL.0, 0x11), // VK_CONTROL
        (MOD_ALT.0, 0x12),     // VK_MENU
        (MOD_SHIFT.0, 0x10),   // VK_SHIFT
        (MOD_WIN.0, 0x5B),     // VK_LWIN
    ];
    let mods: Vec<u16> = ORDER.iter().filter(|(flag, _)| spec.modifiers & flag != 0).map(|(_, vk)| *vk).collect();
    let main = spec.vk as u16;
    let mut seq: Vec<(u16, bool)> = mods.iter().map(|&vk| (vk, false)).collect();
    seq.push((main, false));
    seq.push((main, true));
    seq.extend(mods.iter().rev().map(|&vk| (vk, true)));
    seq
}

/// 注入一个组合键。返回是否整串都发出去了。
unsafe fn inject_hotkey(spec: &HotkeySpec) -> bool {
    let inputs: Vec<INPUT> = key_sequence(spec)
        .into_iter()
        .map(|(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
        .collect();
    let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize;
    sent == inputs.len()
}

/// 对照组合键：Ctrl+Alt+Shift+F13。
fn verify_control_spec() -> HotkeySpec {
    HotkeySpec { modifiers: MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0, vk: VERIFY_CONTROL_VK }
}

/// 收尾一次自检：停定时器、注销对照键、把结论回给调用方。
unsafe fn finish_verify(st: &mut ThreadState, outcome: WakeDelivery) {
    let _ = KillTimer(Some(st.hwnd), VERIFY_TIMER_ID);
    let _ = UnregisterHotKey(Some(st.hwnd), VERIFY_CONTROL_ID);
    if let Some(run) = st.verify.take() {
        let _ = run.reply.send(outcome);
    }
}

/// 处理挂起的命令。**返回待发事件，而不是自己回调 handler** ——
/// 本函数是在 `STATE.with(|s| s.borrow_mut())` 持有期间被调用的，
/// 在里面回调会让 handler 重入同一把 RefCell 借用。由 `wndproc` 在借用释放后再发。
///
/// ⚠️ 同理，自检**不能在这里泵消息**：泵消息会重入 `wndproc`，
/// 而那时 `STATE` 还被可变借用着 → panic。这里只负责「发键 + 起定时器」，
/// `WM_HOTKEY` / `WM_TIMER` 由主消息循环在借用释放后交回来。
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
            SysCommand::VerifyWakeDelivery(reply) => {
                // 上一次还没跑完又来一次：给「无法判定」，别把状态搅乱。
                if st.verify.is_some() {
                    let _ = reply.send(WakeDelivery::Inconclusive);
                    continue;
                }
                if st.wake.is_none() {
                    let _ = reply.send(WakeDelivery::NotRegistered);
                    continue;
                }
                // 对照键注册不上（几乎不会）：环境本身不成立，直接判「无法判定」。
                if register(st.hwnd, VERIFY_CONTROL_ID, &verify_control_spec()).is_err() {
                    let _ = reply.send(WakeDelivery::Inconclusive);
                    continue;
                }
                st.verify = Some(VerifyRun { reply, phase: VerifyPhase::Control });
                if inject_hotkey(&verify_control_spec()) {
                    SetTimer(Some(st.hwnd), VERIFY_TIMER_ID, VERIFY_WAIT_MS, None);
                } else {
                    // 连对照键都发不出去 → 这台机器此刻注入不了，不能下结论。
                    finish_verify(st, WakeDelivery::Inconclusive);
                }
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
            // 投递自检期间收到的热键消息**不能**当作用户按键派发出去，
            // 否则「测一下」会顺手把窗口显示/隐藏一次。先让自检认领它。
            // 借用与动作分两步：认领要 borrow_mut，收尾也要 borrow_mut，
            // 合成一步会同时持有两个可变借用。
            let claim = STATE.with(|s| {
                let mut guard = s.borrow_mut();
                let Some(st) = guard.as_mut() else { return VerifyStep::Unrelated };
                let Some(run) = st.verify.as_ref() else { return VerifyStep::Unrelated };
                match (run.phase, id) {
                    // 对照通过 → 轮到目标键
                    (VerifyPhase::Control, VERIFY_CONTROL_ID) => VerifyStep::ControlPassed,
                    // 目标键真收到了 → 按键投递正常
                    (VerifyPhase::Target, WAKE_HOTKEY_ID) => VerifyStep::TargetDelivered,
                    _ => VerifyStep::Unrelated,
                }
            });
            match claim {
                VerifyStep::Unrelated => {}
                VerifyStep::ControlPassed => {
                    STATE.with(|s| {
                        let mut guard = s.borrow_mut();
                        let Some(st) = guard.as_mut() else { return };
                        let _ = KillTimer(Some(st.hwnd), VERIFY_TIMER_ID);
                        let _ = UnregisterHotKey(Some(st.hwnd), VERIFY_CONTROL_ID);
                        if let Some(run) = st.verify.as_mut() {
                            run.phase = VerifyPhase::Target;
                        }
                        match st.wake {
                            Some(spec) if inject_hotkey(&spec) => {
                                SetTimer(Some(st.hwnd), VERIFY_TIMER_ID, VERIFY_WAIT_MS, None);
                            }
                            Some(_) => finish_verify(st, WakeDelivery::Inconclusive),
                            None => finish_verify(st, WakeDelivery::NotRegistered),
                        }
                    });
                    return LRESULT(0);
                }
                VerifyStep::TargetDelivered => {
                    STATE.with(|s| {
                        if let Some(st) = s.borrow_mut().as_mut() {
                            finish_verify(st, WakeDelivery::Delivered);
                        }
                    });
                    return LRESULT(0);
                }
            }
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
        WM_TIMER if wparam.0 == VERIFY_TIMER_ID => {
            // 该到的时候没到。对照相超时 = 测不了；目标相超时 = 被吞了
            // （目标相只在对照通过之后才会进入，所以这里归因是确定的）。
            STATE.with(|s| {
                let mut guard = s.borrow_mut();
                let Some(st) = guard.as_mut() else { return };
                let Some(phase) = st.verify.as_ref().map(|r| r.phase) else { return };
                let outcome = match phase {
                    VerifyPhase::Control => WakeDelivery::Inconclusive,
                    VerifyPhase::Target => WakeDelivery::Swallowed,
                };
                finish_verify(st, outcome);
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hotkey::parse_hotkey;
    use std::sync::atomic::AtomicUsize;

    fn spec(text: &str) -> HotkeySpec {
        parse_hotkey(text).expect("测试用的组合键必须可解析")
    }

    #[test]
    fn injection_sequence_is_one_shot_and_symmetric() {
        // 整串一次发完、修饰键逆序抬起。这是探针层面踩过的坑：
        // 拆成多次发送会让系统进「菜单模式」把主键吞掉，制造假失败。
        assert_eq!(
            key_sequence(&spec("Alt+Space")),
            vec![(0x12, false), (0x20, false), (0x20, true), (0x12, true)]
        );
        assert_eq!(
            key_sequence(&spec("Ctrl+Alt+T")),
            vec![(0x11, false), (0x12, false), (0x54, false), (0x54, true), (0x12, true), (0x11, true)]
        );
        // 抬起顺序必须是按下顺序的严格逆序，否则会留下卡住的修饰键
        let seq = key_sequence(&spec("Ctrl+Shift+Alt+P"));
        let downs: Vec<u16> = seq.iter().filter(|(_, up)| !up).map(|(vk, _)| *vk).collect();
        let mut ups: Vec<u16> = seq.iter().filter(|(_, up)| *up).map(|(vk, _)| *vk).collect();
        ups.reverse();
        assert_eq!(downs, ups);
    }

    #[test]
    fn only_swallowed_may_blame_the_key() {
        // 本项目的老规矩：每个失败都要能区分「环境不行」和「被测不行」。
        // 「被钩子吞了」和「这台机器测不了」是两句必须不同的话，
        // 而且只有前者允许说「被占用」。
        let swallowed = WakeDelivery::Swallowed.describe();
        let inconclusive = WakeDelivery::Inconclusive.describe();
        assert!(swallowed.contains("键盘钩子"), "被吞的说明要点出真因: {swallowed}");
        assert!(!inconclusive.contains("拦截") && !inconclusive.contains("占用"),
                "判不了的时候不能把责任推给组合键: {inconclusive}");
        assert_ne!(swallowed, inconclusive);
        assert_ne!(WakeDelivery::Delivered.describe(), swallowed);
    }

    #[test]
    fn control_combo_is_never_a_real_binding() {
        // 对照键必须是「没有程序会响应」的组合，否则自检本身就有副作用
        let c = verify_control_spec();
        assert_eq!(c.vk, 0x7C, "VK_F13");
        assert_eq!(c.modifiers, MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0);
        assert_ne!(VERIFY_CONTROL_ID, PROBE_ID);
        assert_ne!(VERIFY_CONTROL_ID, WAKE_HOTKEY_ID);
    }

    #[test]
    fn win_event_proc_forwards_only_the_subscribed_event() {
        // 钩子回调「串味」是最贵的一类错：把别的事件也转出去，GUI 会在无关的
        // 系统事件上白擦一次 —— 不报错、没日志，只是偶尔闪一下。
        // 用一个计数 handler 把方向钉住。
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        // 全进程只能设一次；用 get_or_init 保证重复执行也不会 panic。
        EVENT_HANDLER.get_or_init(move || {
            Arc::new(move |_ev: SysEvent| {
                counter.fetch_add(1, Ordering::SeqCst);
            })
        });
        unsafe {
            // 相邻的事件号（EVENT_SYSTEM_MENUSTART = 4）必须被丢掉
            win_event_proc(HWINEVENTHOOK::default(), EVENT_SYSTEM_FOREGROUND + 1, HWND::default(), 0, 0, 0, 0);
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0, "非订阅事件不应转发");
        unsafe {
            win_event_proc(HWINEVENTHOOK::default(), EVENT_SYSTEM_FOREGROUND, HWND::default(), 0, 0, 0, 0);
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1, "订阅事件应原样转发一次");
    }
}
