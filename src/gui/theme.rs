//! DWM 亚克力材质、圆角与深色模式注入；显示器居中定位。

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongW, GetWindowRect, IsWindowVisible, SetWindowLongW, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    WS_SYSMENU, WS_THICKFRAME,
};


#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttribData {
    attrib: u32,
    pv_data: *mut core::ffi::c_void,
    cb_data: usize,
}

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

type SetWindowCompositionAttributeFn =
    unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32;

/// 通过 user32!SetWindowCompositionAttribute 启用 DWM 亚克力模糊（Win10 1803+ / Win11 通用，
/// PowerToys Run、Flow Launcher 等启动器采用的同款方案）。`tint` 为 0xAABBGGRR。
pub fn apply_accent_acrylic(hwnd: HWND, enable: bool, tint: u32) -> bool {
    use windows::core::s;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    unsafe {
        let Ok(user32) = GetModuleHandleA(s!("user32.dll")) else { return false };
        let Some(proc_addr) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
            return false;
        };
        let set_attr: SetWindowCompositionAttributeFn = std::mem::transmute(proc_addr);
        let mut policy = AccentPolicy {
            accent_state: if enable { ACCENT_ENABLE_ACRYLICBLURBEHIND } else { ACCENT_DISABLED },
            accent_flags: 2,
            gradient_color: tint,
            animation_id: 0,
        };
        let mut data = WindowCompositionAttribData {
            attrib: WCA_ACCENT_POLICY,
            pv_data: &mut policy as *mut _ as *mut _,
            cb_data: std::mem::size_of::<AccentPolicy>(),
        };
        set_attr(hwnd, &mut data) != 0
    }
}

/// 为无边框窗口补回 DWM 准入所需的窗口样式。
///
/// 设计稿 65.2 核心硬伤一：Windows 11 22H2+ 的 DWM 系统级材质
/// （`DWMWA_SYSTEMBACKDROP_TYPE`）对窗口基础样式有严格准入条件。
/// Slint 声明 `no-frame: true` 时，winit 创建的是剥离了 `WS_CAPTION` /
/// `WS_THICKFRAME` 的裸窗口，**DWM 会对这类窗口拒绝注入 Mica / Acrylic 漫反射**，
/// 表现为 API 调用成功但毫无材质效果。
///
/// 处理：用 `SetWindowLongW` 补回 `WS_THICKFRAME`，再以 `SWP_FRAMECHANGED`
/// 触发 DWM 重新计算帧架构。
///
/// ---
/// ⚠️ 2026-09-24 实测纠正（截图取证，勿回退）：
///
/// 此前这里额外加了 `WS_CAPTION | WS_SYSMENU`，注释断言「只影响非客户区计算，
/// 配合 `no-frame` 不会真的画出标题栏」——**该断言是错的**。
/// 因为本窗口同时调用了 `DwmExtendFrameIntoClientArea(-1)` 把整个客户区并入
/// 帧扩展区，DWM 会把非客户区的**最小化 / 最大化 / 关闭三个按钮直接绘制在
/// 客户内容之上**：截图实测它们落在 (733, 781, 830)@y≈11，其中关闭按钮
/// 正好压在搜索栏的筛选漏斗图标上，叠成一个「漏斗套叉」的脏图。
///
/// 结论：DWM 材质准入**只需要 `WS_THICKFRAME`**，`WS_CAPTION` 是多余的，
/// 且是标题栏按钮的来源。这里改为只补 `WS_THICKFRAME`，并主动**清除**
/// `WS_CAPTION | WS_SYSMENU`（防止 winit 或系统在别处带进来）。
pub fn ensure_dwm_admissible(hwnd: HWND) -> bool {
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let wanted = WS_THICKFRAME.0;
        // 需要清掉的两个位：它们会让 DWM 画标题栏按钮
        let unwanted = WS_CAPTION.0 | WS_SYSMENU.0;
        let new_style = (style | wanted) & !unwanted;
        if new_style == style {
            return false; // 已具备且无多余位，无需改动
        }
        SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        log::info!("已校正 DWM 准入样式：0x{style:08x} -> 0x{new_style:08x}（仅保留 WS_THICKFRAME）");
        true
    }
}

/// 把主窗口从任务栏与 Alt+Tab 中摘出去。
///
/// 设计文档第 7 节要求主窗口是「居中的浮动搜索面板，而不是传统应用窗口」。
/// 但 `no-frame: true` 只作用在 `GWL_STYLE`（标题栏 / 边框），而**任务栏按钮与
/// Alt+Tab 归属由 `GWL_EXSTYLE` 的 `WS_EX_TOOLWINDOW` / `WS_EX_APPWINDOW` 决定** ——
/// 项目此前从未碰过扩展样式，所以窗口在 Windows 眼里一直是个标准桌面应用，
/// 既进任务栏也进 Alt+Tab（任务栏按钮显示的正是 `icon: Icons.tray`，观感像凭空多出个 App）。
///
/// 处理：置 `WS_EX_TOOLWINDOW`、清 `WS_EX_APPWINDOW`。
/// ⚠️ 任务栏只在窗口**可见性变化**时重新评估，单纯改样式不会让按钮立刻消失，
/// 因此需要 hide → show 一次；用 `SW_SHOWNOACTIVATE` 避免抢焦点
/// （前置聚焦由 `launcher::force_foreground` 单独负责）。
///
/// 副作用（预期内）：窗口不再出现在 Alt+Tab，唤醒方式只剩托盘图标与全局热键。
/// 只写扩展样式位，**不做 hide/show**（无闪烁）。
///
/// 从 `hide_from_taskbar` 里拆出来的理由：那里的 `SW_HIDE + SW_SHOWNOACTIVATE`
/// 是为了逼任务栏**重新评估**这个窗口，首次显示用一次就够，代价是一次可见闪烁；
/// 而「每次显示都补一遍样式」需要的是无闪烁版本。
///
/// ⚠️ **幂等且必须可重复调用**：已符合要求时直接返回 `false`，不做任何事。
/// 一次 `hide()` → `show()` 循环之后 winit 会把窗口属性重置回默认值 ——
/// `WS_EX_TOOLWINDOW` 被换成 `WS_EX_APPWINDOW`（任务栏按钮复活），
/// 所以这个校正不能只做一次。
pub fn enforce_taskbar_exclusion(hwnd: HWND) -> bool {
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let new_ex = (ex | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0;
        if new_ex == ex {
            return false; // 已是工具窗口样式，无需改动
        }
        SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
        log::info!(
            "已校正窗口扩展样式：0x{ex:08x} -> 0x{new_ex:08x}（WS_EX_TOOLWINDOW，退出任务栏与 Alt+Tab）"
        );
        true
    }
}

/// 逼一次「窗口表面失效」——用来擦掉 DWM 在**激活态变化**时画上的浅色原生标题栏。
///
/// 现象（2026-09-25 实测）：窗口**可见**时，只要激活态一变，左上角就会压上一条浅色
/// 原生标题栏（标准字号写着窗口标题「Anycast」，高约 43 物理像素），**一直不消失**，
/// 直到窗口被 hide → show 一次。两个方向都会：**失去**激活画**整条**（顶带亮占比
/// 99.4%），**抢回**激活画**窄的一条**（约 60px，11.7%）。
///
/// 平时看不到（`hide_on_blur` 默认开，一失焦就藏），只有「窗口要留在屏幕上但焦点
/// 归别人」这条路径才暴露 —— 也就是 `suppress_blur` 期间（「快捷直达 → 测试」、
/// 唤醒键自检）。
///
/// ⚠️ 触发条件是**激活态变化**本身，与「测试」路径无关 —— 别把它和业务路径绑一起。
/// 把 `hide_on_blur` 临时设 `false` 就能反复制造这个状态（测完必须复位）。
///
/// 试过且**无效**的手段（别再重复走，全是实测）：
///   * `slint::Window::request_redraw()` —— 应用内重绘，无效；
///   * `SetWindowPos(SWP_FRAMECHANGED)` / `SetWindowPos(SWP_NOCOPYBITS)` —— 无效；
///   * `InvalidateRect` + `UpdateWindow`、`RedrawWindow(RDW_FRAME|RDW_ALLCHILDREN)` —— 无效；
///   * `DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED` —— 无效（该属性在 Win8+ 被忽略）；
///   * `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_NONE` —— 无效；
///   * 把帧扩展 `MARGINS` 收成 `(0,0,0,0)` —— 无效（**先改再触发**，不是事后改）；
///   * 是不是别的窗口压在它上面 —— 不是（z 序里本窗口始终最上）；
///   * 抓图假象 —— 不是（`PrintWindow` 画出同一条标题栏）；
///   * 窗口无响应被系统画「幽灵标题栏」—— 不是（`IsHungAppWindow` = false）。
///
/// **唯一有效的是真的改一次窗口尺寸**：DWM 会因此重算整个窗口（含帧）。
/// 实测 `+1px` 之后**立刻**还原即可（间隔 0 / 20 / 80 / 350 ms 都有效），
/// 顶带亮占比 11.2% → 0.3%。
///
/// ⚠️ 两次 `SetWindowPos` 必须**背靠背**发、中间不 `sleep`：这样窗口尺寸在同一次
/// 事件处理里就回到原值，`poll_tick` 的尺寸跟踪看不到中间态，
/// 不会把 `W x (H+1)` 当成用户缩放而写进配置（实测配置零差异）。
///
/// ⚠️ **必须从另一个线程发**（因此本函数会 `spawn` 一个短命线程）。
/// 在**本线程**（也就是 `poll_tick` 里）发同样两次调用，日志显示尺寸确实
/// `840 → 841 → 840`、两次都 `Ok`，但**标题栏不消失**；从外部进程发则有效。
/// 差别在 winit 的 `send_event`：事件处理器**已被借用**时（`poll_tick` 正是
/// 在 Slint 回调里跑）它会把 `WM_SIZE` 对应的 `Resized` **缓冲**起来，
/// 于是 Slint 那一次 resize 没有真正走完「重算表面 + 重绘」。
/// 换个线程发，消息由窗口消息队列在空闲时派发，处理器没被借用，路径就正常了。
pub fn nudge_surface(hwnd: HWND) {
    // `HWND` 不是 `Send`，按裸地址过线程，线程内立刻还原。
    let raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw as *mut _);
        unsafe {
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_err() {
                return;
            }
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            if w <= 0 || h <= 0 {
                return;
            }
            let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
            let _ = SetWindowPos(hwnd, None, 0, 0, w, h + 1, flags);
            let _ = SetWindowPos(hwnd, None, 0, 0, w, h, flags);
            log::debug!("nudge_surface(异线程)：{w}x{h} → +1 → 还原");
        }
    });
}

pub fn hide_from_taskbar(hwnd: HWND) -> bool {
    let changed = enforce_taskbar_exclusion(hwnd);
    if changed {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_HIDE);
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
    changed
}

/// 应用 Windows 11 系统级亚克力背景、圆角与暗色模式。
pub fn apply_window_effects(hwnd: HWND, dark: bool, acrylic: bool) {
    unsafe {
        let dark_v: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_v as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );

        // ── 硬伤一修复：先补窗口样式，DWM 才受理系统材质 ──
        // 跳过样式补全的开关（用于 A/B 对照诊断）：ANYCAST_NO_STYLE=1
        if std::env::var("ANYCAST_NO_STYLE").is_err() {
            ensure_dwm_admissible(hwnd);
        }

        // ── 硬伤二/三修复：系统级 Acrylic 走默认路径 ──
        //
        // 此前的实现只在 ANYCAST_DWMSBT=1 时才启用系统材质，导致默认状态下
        // 既没有 DWM 模糊、又只有软件渲染器输出的半透明底色 —— 观感是
        // 「透光但无模糊」，与设计稿要求的 Windows 11 Acrylic 毛玻璃有明显差距。
        //
        // 现改为三层策略（对应设计稿 65.2 的三重阻断修复）：
        //   ⚠ 默认层：DWMSBT_TRANSIENTWINDOW + 客户区帧扩展
        //             → Windows 11 22H2+ 原生 Acrylic，本项目首选
        //   ⚠ 兜底层：SetWindowCompositionAttribute(ACRYLICBLURBEHIND)
        //             → 当系统「透明效果」被关闭或 DWM 接口异常时强制拉起
        //
        // 环境开关（用于逐层关闭做 A/B 诊断，正常使用无需设置）：
        //   ANYCAST_ACCENT=1   仅走兜底层（诊断用）
        //   ANYCAST_DWMSBT=1   仅走默认层（诊断用）
        //   ANYCAST_NO_ACRYLIC=1  完全禁用系统材质，退回纯半透明底色
        let accent_only = std::env::var("ANYCAST_ACCENT").ok().as_deref() == Some("1");
        let dwmsbt_only = std::env::var("ANYCAST_DWMSBT").ok().as_deref() == Some("1");
        let disabled = std::env::var("ANYCAST_NO_ACRYLIC").ok().as_deref() == Some("1");

        if disabled || !acrylic {
            // 关键：关闭时不能只是 return。DWM 材质是「注入」到窗口上的状态，
            // 早退并不会把它撤掉 —— 实测把「系统级 DWM 亚克力模糊」关掉后，
            // 整个窗口像素与开启时几乎完全一致（差异 0.15%，且全部来自开关旋钮本身），
            // 用户会以为这个开关坏了。
            // 这里显式把三层材质全部撤销，保证「关」这个动作当场可见。
            if !disabled {
                let none: i32 = 1; // DWMSBT_NONE
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    &none as *const i32 as *const _,
                    std::mem::size_of::<i32>() as u32,
                );
                let zero = MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 0, cyBottomHeight: 0 };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &zero);
                let _ = apply_accent_acrylic(hwnd, false, 0);
                log::info!("已撤销系统级 Acrylic 材质（gpu_blur=false）");
            }
            return;
        }

        let mut applied = false;

        if !accent_only {
            // 默认层：Windows 11 系统级 Acrylic（Transient Window 材质）
            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            match DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &DWMSBT_TRANSIENTWINDOW as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            ) {
                Ok(()) => {
                    applied = true;
                    log::info!("DWM 系统级 Acrylic 已启用（DWMSBT_TRANSIENTWINDOW）");
                }
                Err(e) => log::warn!("DWM 系统背景材质不可用（需 Windows 11 22H2+）: {e}"),
            }
        }

        if (!applied || dwmsbt_only) && !dwm_likely_disabled() {
            // 兜底层：Win10 1803+ / Win11 通用的 AccentPolicy 亚克力
            let tint: u32 = if dark { 0x0A000000 } else { 0x0AFFFFFF };
            if apply_accent_acrylic(hwnd, true, tint) {
                log::info!("已启用 AccentPolicy 亚克力兜底");
            } else {
                log::warn!("AccentPolicy 亚克力兜底不可用");
            }
        }
    }
}

/// 探测系统「透明效果」开关是否被关闭。
/// 关闭时 DWM 材质会无声失效，此时不应走 AccentPolicy 兜底（会变成不透明黑块）。
fn dwm_likely_disabled() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::core::w;
    unsafe {
        let mut value: u32 = 1;
        let mut size = std::mem::size_of::<u32>() as u32;
        let ok = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        );
        // 读不到（键不存在）时按「开启」处理，保持向后兼容
        ok.is_ok() && value == 0
    }
}

/// 鼠标所在显示器的工作区（物理像素）
/// 鼠标所在显示器的缩放因子（96 DPI = 1.0），不依赖 Slint 的 scale_factor（显示前后可能不准）
pub fn cursor_monitor_scale() -> f32 {
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
            return dx as f32 / 96.0;
        }
        1.0
    }
}

pub fn cursor_monitor_work_area() -> RECT {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1040 }
        }
    }
}
