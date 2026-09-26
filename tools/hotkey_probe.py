"""唤醒热键真机验证探针。

要验的命题（Phase 0 阻塞项 G）：
    窗口隐藏到托盘后，按唤醒热键能不能真的把它唤回来。

为什么不能复用 rt_probe.py 的 keys()：
    那个函数第一步就 force_foreground(find_window()) —— 它自己会把窗口
    ShowWindow 出来。拿它测「热键能不能唤出窗口」等于**先把结论做出来再验证**，
    必然假阳性。本脚本一律不碰前台状态，只发键。

两条互补的证据：

  1) 注册归属（客观、不依赖窗口状态）
     从**本进程**去 RegisterHotKey 同一个组合键。若已被占用，系统返回
     ERROR_HOTKEY_ALREADY_REGISTERED = 1409。再配一个「明显没人用的组合键」
     做对照：它必须成功。对照失败 = 这套判据本身不成立（比如沙箱拦了注册），
     那就不能拿 1409 当证据。**有对照的探针才算探针。**

  2) 投递效果（行为）
     发 Alt+Space，看主窗口 IsWindowVisible / 是否前台怎么变。
     注意 IsWindowVisible 必须真读 —— 只看 GWL_EXSTYLE 会给假阳性
     （样式位对但窗口是隐藏的）。

用法：
  python tools/hotkey_probe.py claim            # 只做注册归属测试
  python tools/hotkey_probe.py send [combo]     # 只发一次键
  python tools/hotkey_probe.py run              # 完整流程（推荐）
"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import sys
import time

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
user32.SetProcessDPIAware()

WINDOW_TITLE = "Anycast"
BUS_CLASS = "AnycastSystemBusWindow"
CONFIG = r"C:\Users\30130\AppData\Roaming\Anycast\data\config.json"

MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN = 0x0001, 0x0002, 0x0004, 0x0008
MOD_NOREPEAT = 0x4000
ERROR_HOTKEY_ALREADY_REGISTERED = 1409

VK = {
    "ctrl": 0x11, "alt": 0x12, "shift": 0x10, "win": 0x5B,
    "space": 0x20, "enter": 0x0D, "esc": 0x1B, "tab": 0x09,
    "back": 0x08, "del": 0x2E, "insert": 0x2D,
    "up": 0x26, "down": 0x28, "left": 0x25, "right": 0x27,
    "home": 0x24, "end": 0x23, "pageup": 0x21, "pagedown": 0x22,
    ",": 0xBC, ".": 0xBE, ";": 0xBA, "/": 0xBF, "-": 0xBD, "=": 0xBB,
}
MOD_OF = {"ctrl": MOD_CONTROL, "alt": MOD_ALT, "shift": MOD_SHIFT, "win": MOD_WIN}


def vk_of(name):
    """按键名 → 虚拟键码。**所有发送函数必须共用这一个**。

    ⚠ 之前 `VK` 表里只有 f13/f14，`spec_of("Ctrl+Shift+F9")` 返回 vk=None，
    自检于是「解析不了 → 跳过」并返回 None，上层把它读成「注入不动」。
    **探针报 ❌ 时可能只是它不认识这个键，不是真的注入失败。**
    F1~F24 按 `0x70 + n - 1` 算（与应用的 `hotkey::vk_of_key` 一致）。
    """
    n = name.strip().lower()
    if not n:
        return None
    if n in VK:
        return VK[n]
    if n.startswith("f") and n[1:].isdigit():
        k = int(n[1:])
        if 1 <= k <= 24:
            return 0x70 + k - 1
    if len(n) == 1 and n.isascii() and n.isalnum():
        return ord(n.upper())
    return None


# ------------------------------------------------------------------ 窗口
def _enum(title=None, cls=None):
    out = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
    def cb(h, l):
        if cls:
            buf = ctypes.create_unicode_buffer(256)
            user32.GetClassNameW(h, buf, 256)
            if buf.value == cls:
                out.append(h)
        else:
            buf = ctypes.create_unicode_buffer(512)
            user32.GetWindowTextW(h, buf, 512)
            if buf.value == title:
                r = wt.RECT()
                user32.GetWindowRect(h, ctypes.byref(r))
                out.append((h, (r.right - r.left) * (r.bottom - r.top)))
        return True

    user32.EnumWindows(cb, 0)
    return out


def main_window():
    """面积最大的 Anycast 顶层窗口（隐藏 helper 只有 353x39）。"""
    ws = _enum(title=WINDOW_TITLE)
    if not ws:
        return None
    ws.sort(key=lambda w: w[1], reverse=True)
    return ws[0][0]


def bus_window():
    """注册热键的消息窗口。它和主窗口是**两个不同的 HWND**。"""
    ws = _enum(cls=BUS_CLASS)
    return ws[0] if ws else None


def pid_of(hwnd):
    p = wt.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
    return p.value


def snap(hwnd):
    return {
        "visible": bool(user32.IsWindowVisible(hwnd)),
        "iconic": bool(user32.IsIconic(hwnd)),
        "foreground": user32.GetForegroundWindow() == hwnd,
        "exstyle": user32.GetWindowLongW(hwnd, -20),
    }


def fmt(s):
    return (f"visible={str(s['visible']):5} iconic={str(s['iconic']):5} "
            f"fg={str(s['foreground']):5} exstyle=0x{s['exstyle'] & 0xFFFFFFFF:08X}")


# ------------------------------------------------------------------ 注册归属
def try_claim(mods, vk, tag):
    """本进程试注册；返回 (成功?, 错误码)。用 ID 0x4Axx 避开应用自己的 ID。"""
    hid = 0x4A00 + (hash(tag) & 0xFF)
    ok = user32.RegisterHotKey(None, hid, mods | MOD_NOREPEAT, vk)
    err = 0 if ok else kernel32.GetLastError()
    if ok:
        user32.UnregisterHotKey(None, hid)
    return bool(ok), err


def claim_test(specs):
    print("── 1. 注册归属（跨进程抢占）" + "─" * 30)
    print("   判据：别人能抢到 = 应用没占住；抢不到且 1409 = 应用占住了\n")

    # 对照组：先证明「这套判据本身有效」
    control_ok, control_err = try_claim(
        MOD_CONTROL | MOD_ALT | MOD_SHIFT, vk_of("F13"), "control")
    print(f"   对照 Ctrl+Alt+Shift+F13  → {'抢到' if control_ok else f'没抢到 err={control_err}'}")
    if not control_ok:
        print(f"   ⚠ 对照都没抢到（err={control_err}）→ 本环境不适合用 1409 下结论，")
        print("     下面的结果不能当证据。")
    print()

    rows = []
    for spec in specs:
        # ⚠️ 支持 4 元组：第 4 位 is_control=True 表示这一项是**对照**，
        # 期望结果是「能抢到」。以前对照项沿用被测项的判词，
        # 会把「抢到」打成「❌ 没占住（任何人都能抢）」—— 自己吓自己。
        name, mods, vk = spec[0], spec[1], spec[2]
        is_control = len(spec) > 3 and spec[3]
        ok, err = try_claim(mods, vk, name)
        if is_control:
            verdict = "✅ 对照：能抢到（判据有效）" if ok else f"⚠ 对照抢不到 err={err}（判据可疑）"
        elif ok:
            verdict = "❌ 没占住（任何人都能抢）"
        elif err == ERROR_HOTKEY_ALREADY_REGISTERED:
            verdict = "✅ 已被占用（应用注册成功）"
        else:
            verdict = f"⚠ 其他错误 err={err}"
        rows.append((name, ok, err, verdict))
        print(f"   {name:16} → err={err:<6} {verdict}")
    print()
    return rows, control_ok


# ------------------------------------------------------------------ 发键
def send_combo(combo):
    """发组合键。**不碰前台** —— 这是本脚本存在的理由。"""
    parts = [p.strip().lower() for p in combo.split("+") if p.strip()]
    mods = [p for p in parts if p in MOD_OF]
    rest = [p for p in parts if p not in MOD_OF]
    if not rest:
        raise SystemExit(f"ERR: {combo} 没有主键")
    main = vk_of(rest[0])
    if main is None:
        raise SystemExit(f"ERR: 未知主键 {rest[0]}")

    KEYUP = 0x0002
    for m in mods:
        user32.keybd_event(VK[m], 0, 0, 0)
        time.sleep(0.02)
    user32.keybd_event(main, 0, 0, 0)
    time.sleep(0.05)
    user32.keybd_event(main, 0, KEYUP, 0)
    time.sleep(0.02)
    for m in reversed(mods):
        user32.keybd_event(VK[m], 0, KEYUP, 0)
        time.sleep(0.02)


def send_combo_sendinput(combo):
    """SendInput + 真实扫描码。keybd_event 的扫描码恒为 0，
    而系统菜单加速键（Alt+Space）的判定很可能读扫描码 —— 换条路再试一次。"""
    parts = [p.strip().lower() for p in combo.split("+") if p.strip()]
    mods = [p for p in parts if p in MOD_OF]
    rest = [p for p in parts if p not in MOD_OF]
    main = vk_of(rest[0])
    if main is None:
        raise SystemExit(f"ERR: 未知主键 {rest[0]}")

    KEYEVENTF_KEYUP = 0x0002
    KEYEVENTF_EXTENDEDKEY = 0x0001
    INPUT_KEYBOARD = 1
    MAPVK_VK_TO_VSC = 0

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort),
                    ("dwFlags", ctypes.c_uint), ("time", ctypes.c_uint),
                    ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]

    class _INPUTunion(ctypes.Union):
        _fields_ = [("ki", KEYBDINPUT), ("pad", ctypes.c_byte * 32)]

    class INPUT(ctypes.Structure):
        _fields_ = [("type", ctypes.c_uint), ("u", _INPUTunion)]

    EXT = {0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22}

    def ev(vk, up):
        scan = user32.MapVirtualKeyW(vk, MAPVK_VK_TO_VSC)
        flags = (KEYEVENTF_KEYUP if up else 0)
        if vk in EXT:
            flags |= KEYEVENTF_EXTENDEDKEY
        inp = INPUT(type=INPUT_KEYBOARD)
        inp.u.ki = KEYBDINPUT(vk, scan, flags, 0, None)
        user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))

    seq = [(VK[m], False) for m in mods] + [(main, False)]
    time.sleep(0.02)
    seq += [(main, True)] + [(VK[m], True) for m in reversed(mods)]
    for vk, up in seq:
        ev(vk, up)
        time.sleep(0.03)


def send_combo_chord(combo):
    """把整个组合压进**一次** SendInput 调用（零间隔）。

    假设：keybd_event 版本在 Alt 与 Space 之间隔了 20ms，系统已经把 Alt 按下
    当成「进入菜单模式」，随后的 Space 被当菜单激活键吞掉。
    真人按键几乎是同时的。若零间隔能触发 Alt+Space，那之前测出的
    「Alt+Space 注入不了」就只是探针的时序伪影，不是系统限制。
    """
    parts = [p.strip().lower() for p in combo.split("+") if p.strip()]
    mods = [p for p in parts if p in MOD_OF]
    rest = [p for p in parts if p not in MOD_OF]
    main = vk_of(rest[0])
    if main is None:
        raise SystemExit(f"ERR: 未知主键 {rest[0]}")

    KEYEVENTF_KEYUP = 0x0002
    INPUT_KEYBOARD = 1

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort),
                    ("dwFlags", ctypes.c_uint), ("time", ctypes.c_uint),
                    ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]

    class _INPUTunion(ctypes.Union):
        _fields_ = [("ki", KEYBDINPUT), ("pad", ctypes.c_byte * 32)]

    class INPUT(ctypes.Structure):
        _fields_ = [("type", ctypes.c_uint), ("u", _INPUTunion)]

    def mk(vk, up):
        inp = INPUT(type=INPUT_KEYBOARD)
        inp.u.ki = KEYBDINPUT(vk, 0, KEYEVENTF_KEYUP if up else 0, 0, None)
        return inp

    seq = [mk(VK[m], False) for m in mods] + [mk(main, False), mk(main, True)]
    seq += [mk(VK[m], True) for m in reversed(mods)]
    arr = (INPUT * len(seq))(*seq)
    user32.SendInput(len(seq), arr, ctypes.sizeof(INPUT))


def park_foreground():
    """把前台让给桌面 —— 模拟「用户正在别的程序里按热键」。
    不这么做的话，主窗口本来就是前台，测不出「从别的程序唤出」。"""
    progman = user32.FindWindowW("Progman", None)
    if progman:
        user32.SetForegroundWindow(progman)
        time.sleep(0.4)
        return user32.GetForegroundWindow() != main_window()
    return False


# ------------------------------------------------------------------ 配置
def cfg_hotkey():
    try:
        with open(CONFIG, encoding="utf-8") as f:
            d = json.load(f)
        return d.get("wake_hotkey"), d.get("launch_at_startup")
    except Exception as e:
        return f"<读不到: {e}>", None


def spec_of(combo):
    parts = [p.strip().lower() for p in combo.split("+") if p.strip()]
    mods = 0
    vk = None
    for p in parts:
        if p in MOD_OF:
            mods |= MOD_OF[p]
        else:
            vk = vk_of(p)
    return mods, vk


# ------------------------------------------------------------------ 主流程
def cmd_run():
    combo_cfg, startup = cfg_hotkey()
    print(f"配置 wake_hotkey = {combo_cfg!r}   launch_at_startup = {startup}\n")

    main = main_window()
    bus = bus_window()
    if not main:
        raise SystemExit("ERR: 找不到 Anycast 主窗口，应用没在跑？")
    print(f"主窗口 HWND = {main}  (pid {pid_of(main)})")
    print(f"消息窗口 HWND = {bus}  (pid {pid_of(bus) if bus else '-'})")
    if bus and pid_of(bus) != pid_of(main):
        print("   ⚠ 两个窗口不属于同一进程，注册归属的判断要重看")
    print()

    combo = combo_cfg if isinstance(combo_cfg, str) else "Alt+Space"
    mods, vk = spec_of(combo)
    if vk is None:
        raise SystemExit(f"ERR: 解析不了热键 {combo!r}")

    # 先自检「注入在这台机器上到底行不行」。注意要分开两件事：
    #   · 环境能不能注入   → 用一个**空闲**组合键当对照
    #   · 目标组合键行不行 → 应用占着时抢不到，只能沿用先前的矩阵结论
    print("   对照（空闲组合键，验环境）：")
    env_ok = selfcheck("Ctrl+Shift+F13")
    if env_ok is not True:
        print("   ⚠ 连空闲组合键都注入不动 → 本环境做不了投递测试，中止。")
        return
    inj = selfcheck(combo)
    if inj is False:
        print("   ⚠ 本组合键注入触发不了注册热键，投递测试无意义，中止。")
        print("     换一个能注入的组合键（见 doc 里的可测性矩阵）。")
        return
    if inj == "held":
        print("     → 沿用先前矩阵的结论继续（该组合键在空闲时已验证可注入）")

    specs = [(combo, mods, vk)]
    specs.append(("Ctrl+Alt+Shift+F14", MOD_CONTROL | MOD_ALT | MOD_SHIFT, vk_of("F14"), True))
    claim_test(specs)

    print("── 2. 投递效果（全程不置前，两个方向都测）" + "─" * 18)
    import rt_probe

    # 方向 A：可见且前台 → 应当隐藏。这个方向没有歧义 ——
    # 只有热键真的到了，visible 才会 True → False。
    rt_probe.force_foreground(main)
    a0 = snap(main)
    print(f"\n   [A] 置前后 : {fmt(a0)}")
    if not (a0["visible"] and a0["foreground"]):
        print("       ⚠ 没能做到「可见且前台」，方向 A 测不了")
    else:
        send_combo(combo)
        time.sleep(1.0)
        a1 = snap(main)
        print(f"       按 {combo} 后 : {fmt(a1)}")
        print("       → " + ("✅ 隐藏成功（热键真的到了）" if not a1["visible"]
                             else "❌ 没隐藏"))

    # 方向 B：隐藏状态 → 应当唤出。**这才是用户实际的那条路**
    # （关掉窗口 = 隐藏到托盘，之后只能靠热键或托盘图标唤回）
    b0 = snap(main)
    if b0["visible"]:
        print("\n   [B] 窗口还可见，先手动隐藏再测")
        rt_probe.force_foreground(main)
        send_combo(combo)
        time.sleep(0.8)
        b0 = snap(main)
    print(f"\n   [B] 按前（应为隐藏）: {fmt(b0)}")
    parked = park_foreground()
    print(f"       让出前台            : {fmt(snap(main))}"
          + ("" if parked else "   ⚠ 没能让出前台"))
    send_combo(combo)
    time.sleep(1.2)
    b1 = snap(main)
    print(f"       按 {combo} 后 : {fmt(b1)}")
    if b1["visible"] and not b0["visible"]:
        print("       → ✅ 唤出成功" + ("（且抢到前台）" if b1["foreground"] else ""))
    elif b1["visible"]:
        print("       → ⚠ 可见了，但按前就是可见的，说明不了什么")
    else:
        print("       → ❌ 没唤出")
    print()


_SELFCHECK_N = [0]
# 注入方式可切换：keybd_event 与 SendInput 是两条不同的路径，
# 若某组合键只被其中一条触发得了，这个开关就是判据。
SENDER = [None]  # None = send_combo


def selfcheck(combo="Ctrl+Alt+Shift+F13"):
    """本探针的有效性检验：自己注册一个热键，注入它，看收不收得到 WM_HOTKEY。

    没有这一步的话，「发了键但窗口没反应」有两种解释 —— 应用有缺陷，
    或者注入的键压根触发不了注册热键。必须先把这个岔路堵掉。

    combo 要能换：Alt+Space 是**系统菜单键**，Ctrl+Alt+Shift+F13 不是。
    拿后者证明「注入可行」推不出前者也可行 —— 系统可能把 Alt+Space 截走了。
    所以验 Alt+Space 时必须用 Alt+Space 自己当对照。
    """
    print(f"── 0. 注入有效性自检（本进程自注册 {combo} 并注入）" + "─" * 10)
    WM_HOTKEY = 0x0312

    hinst = kernel32.GetModuleHandleW(None)
    # ⚠ 类名必须唯一。固定类名时第二次 RegisterClassW 返回
    # ERROR_CLASS_ALREADY_EXISTS(1410)，被下面的分支当成「跳过」→ 返回 None
    # → 被上层读成「触发不了」。实测表现为「一个进程里只有第一次的结果是真的，
    # 后面全是假 ❌」—— 差点据此得出「Alt+Space 注入不了」的错误结论。
    _SELFCHECK_N[0] += 1
    cls = f"ProbeSelfCheckWnd_{_SELFCHECK_N[0]}"

    WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_long, wt.HWND, ctypes.c_uint,
                                 wt.WPARAM, wt.LPARAM)
    got = []

    def _proc(h, m, w, l):
        if m == WM_HOTKEY:
            got.append(w)
        return user32.DefWindowProcW(wt.HWND(h), ctypes.c_uint(m),
                                     wt.WPARAM(w), wt.LPARAM(l))

    proc = WNDPROC(_proc)

    class WNDCLASS(ctypes.Structure):
        _fields_ = [("style", ctypes.c_uint), ("lpfnWndProc", WNDPROC),
                    ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                    ("hInstance", wt.HINSTANCE), ("hIcon", wt.HICON),
                    ("hCursor", wt.HANDLE), ("hbrBackground", wt.HBRUSH),
                    ("lpszMenuName", wt.LPCWSTR), ("lpszClassName", wt.LPCWSTR)]

    wc = WNDCLASS()
    wc.lpfnWndProc = proc
    wc.hInstance = hinst
    wc.lpszClassName = cls
    atom = user32.RegisterClassW(ctypes.byref(wc))
    if not atom:
        err = kernel32.GetLastError()
        print(f"   ⚠ RegisterClass 失败 err={err} → 自检跳过")
        return None

    HWND_MESSAGE = -3
    # ⚠ 必须给 argtypes —— 否则 ctypes 把 Python int 当**32 位**传，
    # HWND_MESSAGE(-3) 会被零扩展成 0x00000000FFFFFFFD 而不是
    # 0xFFFFFFFFFFFFFFFD，CreateWindowExW 直接返回 1400(无效句柄)。
    _cwex = user32.CreateWindowExW
    _cwex.argtypes = [wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
                      ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                      wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p]
    _cwex.restype = wt.HWND
    hwnd = _cwex(0, cls, cls, 0, 0, 0, 0, 0,
                 wt.HWND(HWND_MESSAGE), None, hinst, None)
    if not hwnd:
        print(f"   ⚠ CreateWindowEx 失败 err={kernel32.GetLastError()} → 自检跳过")
        return None

    hid = 0x4B01
    mods, vk = spec_of(combo)
    if vk is None:
        print(f"   ⚠ 解析不了 {combo!r} → 自检跳过")
        return None
    if not user32.RegisterHotKey(hwnd, hid, mods | MOD_NOREPEAT, vk):
        err = kernel32.GetLastError()
        if err == ERROR_HOTKEY_ALREADY_REGISTERED:
            # 别人（多半就是要测的应用）占着 —— 抢不到不等于注入不了。
            # 返回 'held' 而不是 False：把「抢不到」报成「注入不了」
            # 会把一个注册成功的证据说成失败，方向正好反了。
            print(f"   ○ {combo} 已被占用（err=1409）→ 自检做不了，"
                  f"但**这本身说明有人成功注册了它**")
            user32.DestroyWindow(hwnd)
            return "held"
        print(f"   ⚠ 自检热键注册失败 err={err} → 自检跳过")
        user32.DestroyWindow(hwnd)
        return None
    print(f"   已在本进程注册 {combo}，注入之…")

    (SENDER[0] or send_combo)(combo)

    # 泵消息直到超时
    class MSG(ctypes.Structure):
        _fields_ = [("hwnd", wt.HWND), ("message", ctypes.c_uint),
                    ("wParam", wt.WPARAM), ("lParam", wt.LPARAM),
                    ("time", wt.DWORD), ("pt_x", ctypes.c_long),
                    ("pt_y", ctypes.c_long)]

    msg = MSG()
    deadline = time.time() + 1.5
    while time.time() < deadline and not got:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.02)

    user32.UnregisterHotKey(hwnd, hid)
    user32.DestroyWindow(hwnd)

    if got:
        print(f"   ✅ 收到 WM_HOTKEY（id={got[0]}）→ 注入的键**能**触发注册热键")
        print("      → 那么应用那边没反应，就不是注入的问题")
    else:
        print("   ❌ 没收到 WM_HOTKEY → **注入的键触发不了注册热键**")
        print("      → 应用那边没反应不能算证据，得换注入方式（SendInput 带扫描码）")
    print()
    return bool(got)


def diag(combo="Alt+Space"):
    """决定性诊断：注册 combo 后注入，把收到的**所有**消息打出来。

    要区分两种可能：
      ① 热键表根本没匹配上（键被送到前台窗口当系统键了）→ 真实按键大概也一样
      ② 键被系统菜单抢先消费（出现 SC_KEYMENU / 进入菜单循环）
    只有看到键**去了哪**，才能说 Alt+Space 这个默认值靠不靠得住。
    """
    print(f"── 诊断：{combo} 注入后消息去向" + "─" * 28)
    NAMES = {0x0312: "WM_HOTKEY", 0x0100: "WM_KEYDOWN", 0x0101: "WM_KEYUP",
             0x0104: "WM_SYSKEYDOWN", 0x0105: "WM_SYSKEYUP",
             0x0112: "WM_SYSCOMMAND", 0x011F: "WM_ENTERMENULOOP",
             0x0120: "WM_EXITMENULOOP", 0x0006: "WM_ACTIVATE",
             0x0007: "WM_SETFOCUS", 0x0113: "WM_TIMER",
             0x0001: "WM_CREATE", 0x0002: "WM_DESTROY", 0x0014: "WM_ERASEBKGND",
             0x000F: "WM_PAINT", 0x0081: "WM_NCCALCSIZE", 0x0083: "WM_NCCALCSIZE"}
    SC = {0xF000: "SC_SIZE", 0xF010: "SC_MOVE", 0xF020: "SC_MINIMIZE",
          0xF030: "SC_MAXIMIZE", 0xF040: "SC_NEXTWINDOW", 0xF050: "SC_PREVWINDOW",
          0xF060: "SC_CLOSE", 0xF080: "SC_KEYMENU", 0xF090: "SC_RESTORE",
          0xF100: "SC_SCREENSAVE", 0xF120: "SC_TASKLIST"}
    log = []
    hinst = kernel32.GetModuleHandleW(None)
    _SELFCHECK_N[0] += 1
    cls = f"ProbeDiagWnd_{_SELFCHECK_N[0]}"

    WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_long, wt.HWND, ctypes.c_uint,
                                 wt.WPARAM, wt.LPARAM)

    def _proc(h, m, w, l):
        if m == 0x0312:
            log.append(("WM_HOTKEY", f"id={w}"))
        elif m == 0x0112:
            log.append(("WM_SYSCOMMAND", SC.get(w & 0xFFF0, hex(w))))
        elif m in (0x0104, 0x0105, 0x0100, 0x0101):
            log.append((NAMES[m], f"vk={w:#x}"))
        elif m == 0x011F:
            log.append(("WM_ENTERMENULOOP", ""))
        return user32.DefWindowProcW(wt.HWND(h), ctypes.c_uint(m),
                                     wt.WPARAM(w), wt.LPARAM(l))

    proc = WNDPROC(_proc)

    class WNDCLASS(ctypes.Structure):
        _fields_ = [("style", ctypes.c_uint), ("lpfnWndProc", WNDPROC),
                    ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                    ("hInstance", wt.HINSTANCE), ("hIcon", wt.HICON),
                    ("hCursor", wt.HANDLE), ("hbrBackground", wt.HBRUSH),
                    ("lpszMenuName", wt.LPCWSTR), ("lpszClassName", wt.LPCWSTR)]

    wc = WNDCLASS()
    wc.lpfnWndProc = proc
    wc.hInstance = hinst
    wc.lpszClassName = cls
    if not user32.RegisterClassW(ctypes.byref(wc)):
        print(f"   ⚠ RegisterClass 失败 err={kernel32.GetLastError()}")
        return

    _cwex = user32.CreateWindowExW
    _cwex.argtypes = [wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
                      ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                      wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p]
    _cwex.restype = wt.HWND
    # 普通可见窗口（不是 message-only）—— 要它当前台窗口才能看出键去了哪
    WS_OVERLAPPEDWINDOW = 0x00CF0000
    hwnd = _cwex(0x00000100, cls, "probe-diag", WS_OVERLAPPEDWINDOW,
                 120, 120, 320, 160, None, None, hinst, None)
    if not hwnd:
        print(f"   ⚠ CreateWindowEx 失败 err={kernel32.GetLastError()}")
        return
    user32.ShowWindow(hwnd, 5)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.4)

    hid = 0x4C01
    mods, vk = spec_of(combo)
    if not user32.RegisterHotKey(hwnd, hid, mods | MOD_NOREPEAT, vk):
        err = kernel32.GetLastError()
        print(f"   ○ 注册 {combo} 失败 err={err}"
              + ("（被应用占着）" if err == ERROR_HOTKEY_ALREADY_REGISTERED else ""))
        user32.DestroyWindow(hwnd)
        return
    fg = user32.GetForegroundWindow()
    print(f"   已注册 {combo}；本窗口 hwnd={hwnd}，前台 hwnd={fg}，"
          f"{'是前台' if fg == hwnd else '⚠ 不是前台'}")

    log.clear()
    (SENDER[0] or send_combo)(combo)

    class MSG(ctypes.Structure):
        _fields_ = [("hwnd", wt.HWND), ("message", ctypes.c_uint),
                    ("wParam", wt.WPARAM), ("lParam", wt.LPARAM),
                    ("time", wt.DWORD), ("pt_x", ctypes.c_long),
                    ("pt_y", ctypes.c_long)]

    msg = MSG()
    deadline = time.time() + 1.5
    while time.time() < deadline:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.02)

    user32.UnregisterHotKey(hwnd, hid)
    user32.DestroyWindow(hwnd)

    if not log:
        print("   ⚠ 一条消息都没收到（键没进这个窗口，也没触发热键）")
    else:
        for name, extra in log:
            print(f"   ← {name:18} {extra}")
    kinds = {n for n, _ in log}
    print()
    if "WM_HOTKEY" in kinds:
        print("   → 热键匹配成功")
    elif "WM_SYSCOMMAND" in kinds and any(
            e == "SC_KEYMENU" for n, e in log if n == "WM_SYSCOMMAND"):
        print("   → **系统菜单**抢走了（SC_KEYMENU）→ 热键表没赢")
    elif "WM_SYSKEYDOWN" in kinds:
        print("   → 键被当普通系统键送到前台窗口 → 热键表没匹配上")
    print()


def cmd_bindings(specs):
    """验证「快捷直达」绑定项。

    spec 形如 `<combo>|<期望已注册0/1>|<标记文件路径 或 ->`

    为什么要用**标记文件**当信号：绑定项的效果是「启动某个目标」，
    「某个窗口冒出来了」这种信号太软（窗口可能是本来就在的）。
    让靶子写一个标记文件，「文件出现」是确定性的、可轮询的、
    而且能区分**是哪一个绑定**被触发 —— 索引错位那类 bug 只有这样才看得出来。
    """
    main = main_window()
    if not main:
        raise SystemExit("ERR: 找不到 Anycast 主窗口，应用没在跑？")
    print(f"主窗口 HWND = {main}  (pid {pid_of(main)})\n")

    print("── 注册归属" + "─" * 44)
    print("   对照 Ctrl+Alt+Shift+F14 应当能抢到（证明判据有效）\n")
    ok, err = try_claim(MOD_CONTROL | MOD_ALT | MOD_SHIFT, vk_of("F14"), "control")
    # 注意方向：对照**应当抢得到**。抢不到说明判据本身失灵，
    # 那下面的 1409 就不能当证据 —— 但这里常被写反成「抢到 = 异常」。
    print(f"   对照 Ctrl+Alt+Shift+F14 → "
          + ("✅ 抢到（判据有效）" if ok else f"❌ 没抢到 err={err}（判据失灵，下面结论无效）"))
    print()
    reg_ok = True
    for combo, want, _marker in specs:
        mods, vk = spec_of(combo)
        ok, err = try_claim(mods, vk, combo)
        held = (not ok) and err == ERROR_HOTKEY_ALREADY_REGISTERED
        good = (held == bool(want))
        reg_ok &= good
        print(f"   {combo:18} 期望{'已注册' if want else '未注册'} → "
              f"{'已注册(1409)' if held else f'未注册(err={err})'} "
              + ("✅" if good else "❌ 不符"))
    print()

    print("── 触发效果（标记文件）" + "─" * 34)
    trig_ok = True
    for combo, want, marker in specs:
        if not marker or marker == "-":
            print(f"\n   {combo}：无标记文件，跳过触发测试")
            continue
        if os.path.exists(marker):
            os.remove(marker)
        before = snap(main)
        send_combo_chord(combo)
        appeared = False
        for _ in range(40):            # 最多等 4s：ShellExecute 是异步的
            time.sleep(0.1)
            if os.path.exists(marker):
                appeared = True
                break
        after = snap(main)
        if want:
            good = appeared
        else:
            good = not appeared
        trig_ok &= good
        print(f"\n   {combo} → {os.path.basename(marker)}")
        print(f"     期望{'出现' if want else '不出现'} → "
              f"{'出现了' if appeared else '没出现'} " + ("✅" if good else "❌ 不符"))
        print(f"     窗口 {fmt(before)} → {fmt(after)}")
        if appeared:
            try:
                with open(marker, encoding="utf-8", errors="replace") as f:
                    print(f"     内容: {f.read().strip()!r}")
            except OSError as e:
                print(f"     读不到内容: {e}")
    print()
    print("── 汇总" + "─" * 48)
    print(f"   注册归属: {'✅ 全部符合预期' if reg_ok else '❌ 有不符合'}")
    print(f"   触发效果: {'✅ 全部符合预期' if trig_ok else '❌ 有不符合'}")
    print()
    return reg_ok and trig_ok


def cmd_hold(combo, seconds=600):
    """占住一个组合键不放，模拟「别的程序已经占了这个键」。

    验证冲突提示需要这个：`RegisterHotKey` 的 1409 是跨进程的，
    所以外部占住之后，应用再注册同一个组合键必然失败。
    占住期间必须**泵消息** —— 光注册不泵的话，注册照样有效
    （注册表在系统里），但本进程收不到 WM_HOTKEY，容易被误判成没占住。
    """
    hinst = kernel32.GetModuleHandleW(None)
    _SELFCHECK_N[0] += 1
    cls = f"ProbeHoldWnd_{_SELFCHECK_N[0]}"

    WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_long, wt.HWND, ctypes.c_uint,
                                 wt.WPARAM, wt.LPARAM)

    def _proc(h, m, w, l):
        return user32.DefWindowProcW(wt.HWND(h), ctypes.c_uint(m),
                                     wt.WPARAM(w), wt.LPARAM(l))

    proc = WNDPROC(_proc)

    class WNDCLASS(ctypes.Structure):
        _fields_ = [("style", ctypes.c_uint), ("lpfnWndProc", WNDPROC),
                    ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                    ("hInstance", wt.HINSTANCE), ("hIcon", wt.HICON),
                    ("hCursor", wt.HANDLE), ("hbrBackground", wt.HBRUSH),
                    ("lpszMenuName", wt.LPCWSTR), ("lpszClassName", wt.LPCWSTR)]

    wc = WNDCLASS()
    wc.lpfnWndProc = proc
    wc.hInstance = hinst
    wc.lpszClassName = cls
    if not user32.RegisterClassW(ctypes.byref(wc)):
        print(f"ERR: RegisterClass 失败 err={kernel32.GetLastError()}", flush=True)
        return 1

    _cwex = user32.CreateWindowExW
    _cwex.argtypes = [wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
                      ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                      wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p]
    _cwex.restype = wt.HWND
    hwnd = _cwex(0, cls, cls, 0, 0, 0, 0, 0,
                 wt.HWND(-3), None, hinst, None)
    if not hwnd:
        print(f"ERR: CreateWindowEx 失败 err={kernel32.GetLastError()}", flush=True)
        return 1

    mods, vk = spec_of(combo)
    if vk is None:
        print(f"ERR: 解析不了 {combo!r}", flush=True)
        return 1
    if not user32.RegisterHotKey(hwnd, 0x5001, mods | MOD_NOREPEAT, vk):
        print(f"ERR: 占不住 {combo}，err={kernel32.GetLastError()}"
              + ("（已被占用）" if kernel32.GetLastError() == ERROR_HOTKEY_ALREADY_REGISTERED else ""),
              flush=True)
        return 1

    print(f"HELD {combo}", flush=True)
    print(f"（占住 {seconds}s，泵消息中；Ctrl+C 或杀掉本进程即释放）", flush=True)

    class MSG(ctypes.Structure):
        _fields_ = [("hwnd", wt.HWND), ("message", ctypes.c_uint),
                    ("wParam", wt.WPARAM), ("lParam", wt.LPARAM),
                    ("time", wt.DWORD), ("pt_x", ctypes.c_long),
                    ("pt_y", ctypes.c_long)]

    msg = MSG()
    deadline = time.time() + seconds
    while time.time() < deadline:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.05)
    user32.UnregisterHotKey(hwnd, 0x5001)
    user32.DestroyWindow(hwnd)
    print("RELEASED", flush=True)
    return 0


def cmd_claim():
    combo_cfg, _ = cfg_hotkey()
    combo = combo_cfg if isinstance(combo_cfg, str) else "Alt+Space"
    mods, vk = spec_of(combo)
    claim_test([(combo, mods, vk),
                ("Ctrl+Alt+Shift+F14", MOD_CONTROL | MOD_ALT | MOD_SHIFT, vk_of("F14"))])


def cmd_send(combo):
    main = main_window()
    if not main:
        raise SystemExit("ERR: 找不到主窗口")
    print("按前:", fmt(snap(main)))
    send_combo(combo)
    time.sleep(0.9)
    print("按后:", fmt(snap(main)))


def cmd_fg():
    """隐藏方向的测试：先把窗口**真正置前**，再按热键，看它会不会隐藏。

    为什么需要这个方向：唤醒方向的判据是「visible 变 True」或「前台抢过来」。
    但窗口本来就 visible 时，show_window() 走完也不改变 visible —— 判据分辨不出
    「热键没触发」和「触发了但只是重复显示」。隐藏方向没有这个歧义：
    只有热键真的到了，visible 才会 True → False。
    """
    main = main_window()
    if not main:
        raise SystemExit("ERR: 找不到主窗口")
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import rt_probe
    rt_probe.force_foreground(main)
    before = snap(main)
    print("置前后:", fmt(before))
    if not (before["visible"] and before["foreground"]):
        print("   ⚠ 没能同时做到「可见且前台」，本方向测不了")
        return
    combo_cfg, _ = cfg_hotkey()
    combo = combo_cfg if isinstance(combo_cfg, str) else "Alt+Space"
    send_combo(combo)
    time.sleep(1.0)
    after = snap(main)
    print(f"按 {combo} 后:", fmt(after))
    print("   → " + ("✅ 隐藏成功（热键真的到了）" if not after["visible"]
                     else "❌ 没隐藏"))


if __name__ == "__main__":
    argv = sys.argv[1:]
    if not argv or argv[0] == "run":
        cmd_run()
    elif argv[0] == "selfcheck":
        selfcheck(argv[1] if len(argv) > 1 else "Ctrl+Alt+Shift+F13")
    elif argv[0] == "diag":
        diag(argv[1] if len(argv) > 1 else "Alt+Space")
    elif argv[0] == "fg":
        cmd_fg()
    elif argv[0] == "hold":
        if len(argv) < 2:
            raise SystemExit("ERR: 用法 hold <combo> [秒数]")
        sys.exit(cmd_hold(argv[1], int(argv[2]) if len(argv) > 2 else 600))
    elif argv[0] == "claim":
        cmd_claim()
    elif argv[0] == "bindings":
        parsed = []
        for s in argv[1:]:
            parts = s.split("|")
            if len(parts) != 3:
                raise SystemExit(f"ERR: spec 要形如 <combo>|<0/1>|<marker>，收到 {s!r}")
            parsed.append((parts[0].strip(), int(parts[1]), parts[2].strip()))
        cmd_bindings(parsed)
    elif argv[0] == "send":
        cmd_send(argv[1] if len(argv) > 1 else "Alt+Space")
    else:
        print(__doc__)
