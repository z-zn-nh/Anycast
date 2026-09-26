"""Anycast 设置面板「运行时往返验证」探针。

为什么自己写而不用 ui_probe.py + win_shot.ps1：
  之前每次操作都要起一个 PowerShell 进程抓图，中间隔着一次进程切换，
  窗口会被浏览器/编辑器压住，win_shot 的 SetForegroundWindow 又抢不过前台锁定，
  结果 CopyFromScreen 抓到的是压在上面的别的窗口（实测抓到过浏览器和聊天窗口）。

本脚本把「置前 → 点击/按键 → 抓窗口区域」压进**同一个进程**，中间不让出焦点。
抓图用 PIL.ImageGrab 直接截屏幕矩形（窗口已被强制置前，所以截到的就是它）。

坐标一律用 **窗口内逻辑像素**（本机 150% DPI），脚本自己换算。

用法：
  python tools/rt_probe.py state                  # 窗口状态 + 配置快照
  python tools/rt_probe.py grab <out.png>         # 置前并抓窗口
  python tools/rt_probe.py click <lx> <ly> [out]  # 点击（可选顺带抓图）
  python tools/rt_probe.py wheel <delta> <lx> <ly> [out]  # 滚轮（delta>0 上滚）
  python tools/rt_probe.py keys <combo> [out]
  python tools/rt_probe.py type <text> [out]
  python tools/rt_probe.py grabdelay <ms> <out>    # 睡 ms 再抓（等异步结果）
  python tools/rt_probe.py cfg                    # 打印配置关键项
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
DPI = 1.5
CONFIG = r"C:\Users\30130\AppData\Roaming\Anycast\data\config.json"

MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
MOUSEEVENTF_WHEEL = 0x0800
KEYEVENTF_KEYUP = 0x0002
SWP_NOSIZE, SWP_NOMOVE, SWP_NOZORDER = 0x0001, 0x0002, 0x0004

VK = {
    "ctrl": 0x11, "alt": 0x12, "shift": 0x10, "win": 0x5B,
    "enter": 0x0D, "esc": 0x1B, "tab": 0x09, "space": 0x20,
    "back": 0x08, "del": 0x2E, "up": 0x26, "down": 0x28,
    "left": 0x25, "right": 0x27, "home": 0x24, "end": 0x23,
    ",": 0xBC, ".": 0xBE, ";": 0xBA, "/": 0xBF, "-": 0xBD, "=": 0xBB,
    "f1": 0x70, "f2": 0x71, "f3": 0x72, "f4": 0x73, "f5": 0x74,
}

user32.VkKeyScanW.argtypes = [ctypes.c_wchar]
user32.VkKeyScanW.restype = ctypes.c_short


def vk_for(main):
    """主键名 → (VK, 是否需要 Shift)。

    ⚠️ **单个字符绝不能拿 `ord()` 当 VK** —— 只有字母和数字恰好重合
    （`'a'`=0x41=VK_A、`'7'`=0x37=VK_7），**标点全部错位**，而且是错到
    **别的键**上、不报任何错：

        ord(",")=0x2C=VK_SNAPSHOT(PrintScreen)   真正的逗号是 0xBC(VK_OEM_COMMA)
        ord(".")=0x2E=VK_DELETE                  真正的句点是 0xBE
        ord("-")=0x2D=VK_INSERT                  真正的减号是 0xBD
        ord("[")=0x5B=VK_LWIN  ← 会把 Win 键按下去
        ord("\\")=0x5C=VK_RWIN

    症状极隐蔽：脚本「按键成功、无异常」，但应用里那个快捷键**就是没反应**，
    于是很容易反过来怀疑应用有 bug（本项目就这样误判过一次 `Ctrl+,`）。
    所以：先查表，查不到再问 `VkKeyScanW`（它会连 Shift 状态一起给出）。
    """
    if main in VK:
        return VK[main], False
    if len(main) == 1:
        res = user32.VkKeyScanW(main)
        if res == -1:
            raise SystemExit(f"ERR: 字符 {main!r} 在当前键盘布局下打不出来")
        return res & 0xFF, bool(res & 0x100)
    raise SystemExit(f"ERR: 未知按键 {main}")


# ---------------------------------------------------------------- 窗口
def all_windows():
    """枚举所有标题恰为 Anycast 的顶层窗口。"""
    out = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
    def cb(h, l):
        buf = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(h, buf, 512)
        if buf.value == WINDOW_TITLE:
            r = wt.RECT()
            user32.GetWindowRect(h, ctypes.byref(r))
            out.append((h, r.left, r.top, r.right - r.left, r.bottom - r.top,
                        bool(user32.IsWindowVisible(h)), bool(user32.IsIconic(h))))
        return True

    user32.EnumWindows(cb, 0)
    return out


def find_window():
    """挑主窗口：面积最大的那个（隐藏 helper 只有 353x39 且在 -32000）。"""
    ws = all_windows()
    if not ws:
        return None
    ws.sort(key=lambda w: w[3] * w[4], reverse=True)
    return ws[0][0]


def force_foreground(hwnd):
    """用 AttachThreadInput 绕过前台锁定，把窗口真正拉到最前。"""
    fg = user32.GetForegroundWindow()
    if fg == hwnd and not user32.IsIconic(hwnd):
        return True
    tid_fg = user32.GetWindowThreadProcessId(fg, None) if fg else 0
    tid_me = kernel32.GetCurrentThreadId()
    if tid_fg and tid_fg != tid_me:
        user32.AttachThreadInput(tid_me, tid_fg, True)
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, 9)          # SW_RESTORE
    else:
        user32.ShowWindow(hwnd, 5)          # SW_SHOW
    user32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE)
    user32.SetWindowPos(hwnd, -2, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE)
    user32.BringWindowToTop(hwnd)
    user32.SetForegroundWindow(hwnd)
    if tid_fg and tid_fg != tid_me:
        user32.AttachThreadInput(tid_me, tid_fg, False)
    time.sleep(0.35)
    return user32.GetForegroundWindow() == hwnd


def rect():
    hwnd = find_window()
    if not hwnd:
        raise SystemExit("ERR: 找不到 Anycast 主窗口")
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return hwnd, r


def to_screen(lx, ly):
    _, r = rect()
    return int(r.left + lx * DPI), int(r.top + ly * DPI)


# ---------------------------------------------------------------- 输入
def _key(vk, up=False):
    user32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


# --- SendInput + KEYEVENTF_UNICODE：唯一能把**中文**送进窗口的办法 ---
#
# `keybd_event` / `VkKeyScanW` 走的是「物理按键 → 键盘布局 → 字符」这条路，
# 而中文没有对应的物理键：`VkKeyScanW(ord('我'))` 返回 -1，
# 原来的实现 `& 0xFF` 会把它变成 0xFF（一个不存在的 VK），**静默什么都不输入**。
# 表现为「探针跑完没报错，但搜索框还是空的」，极容易误判成应用的问题。
#
# KEYEVENTF_UNICODE 直接送 UTF-16 码元，绕过键盘布局。
KEYEVENTF_UNICODE = 0x0004
INPUT_KEYBOARD = 1


class _KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wt.WORD),
        ("wScan", wt.WORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.c_void_p),
    ]


class _MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wt.LONG),
        ("dy", wt.LONG),
        ("mouseData", wt.DWORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.c_void_p),
    ]


class _HARDWAREINPUT(ctypes.Structure):
    _fields_ = [("uMsg", wt.DWORD), ("wParamL", wt.WORD), ("wParamH", wt.WORD)]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("ki", _KEYBDINPUT), ("mi", _MOUSEINPUT), ("hi", _HARDWAREINPUT)]


class _INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _INPUTUNION)]


def _send_unicode_unit(unit, up=False):
    flags = KEYEVENTF_UNICODE | (KEYEVENTF_KEYUP if up else 0)
    inp = _INPUT(type=INPUT_KEYBOARD)
    inp.u.ki = _KEYBDINPUT(0, unit, flags, 0, None)
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(_INPUT))


def _type_unicode(s):
    """按 UTF-16 码元逐个送；BMP 之外的字符要拆成代理对。"""
    for ch in s:
        code = ord(ch)
        units = [code] if code <= 0xFFFF else [0xD800 + ((code - 0x10000) >> 10), 0xDC00 + ((code - 0x10000) & 0x3FF)]
        for u in units:
            _send_unicode_unit(u)
            time.sleep(0.012)
            _send_unicode_unit(u, up=True)
            time.sleep(0.012)


def click(lx, ly, double=False):
    hwnd = find_window()
    force_foreground(hwnd)
    x, y = to_screen(lx, ly)
    user32.SetCursorPos(x, y)
    time.sleep(0.15)
    for _ in range(2 if double else 1):
        user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
        time.sleep(0.05)
        user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
        time.sleep(0.12)


def wheel(delta, lx, ly):
    """把光标放到窗口内逻辑坐标 (lx, ly) 再滚轮；delta>0 上滚。"""
    hwnd = find_window()
    force_foreground(hwnd)
    x, y = to_screen(lx, ly)
    user32.SetCursorPos(x, y)
    time.sleep(0.12)
    user32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, ctypes.c_long(delta).value, 0)
    time.sleep(0.25)


def keys(combo):
    parts = combo.lower().split("+")
    mods = [p for p in parts if p in ("ctrl", "alt", "shift", "win")]
    rest = [p for p in parts if p not in ("ctrl", "alt", "shift", "win")]
    if not rest:
        raise SystemExit("ERR: 无主键")
    main = rest[0]
    force_foreground(find_window())
    vk, need_shift = vk_for(main)
    if need_shift and "shift" not in mods:
        mods.append("shift")            # 大写字母 / 上档符号，布局要求的 Shift 不能省
    for m in mods:
        _key(VK[m])
    _key(vk)
    time.sleep(0.04)
    _key(vk, up=True)
    for m in reversed(mods):
        _key(VK[m], up=True)
    time.sleep(0.15)


def typetext(s):
    force_foreground(find_window())
    # ⚠️ **一律走 Unicode 通道**，不要用 keybd_event 打 ASCII。
    #
    # keybd_event 走的是「物理按键 → 键盘布局 → **输入法**」这条路。本机装着
    # 中文输入法，实测把 "abc123" 打成了「按不出23」；更坑的是击键可能被 IME
    # 吞进**未上屏的候选串** —— 界面上看着有内容（候选预览），
    # 但 TextInput 的 text 还是空，于是「保存」读到空串。
    # 这个假象骗过一次排查，一度误判成 Slint 的双向绑定失效。
    #
    # KEYEVENTF_UNICODE 直接送 UTF-16 码元，不经输入法，结果确定。
    _type_unicode(s)


# ---------------------------------------------------------------- 抓图
def grab(out):
    from PIL import ImageGrab
    hwnd = find_window()
    ok = force_foreground(hwnd)
    time.sleep(0.45)
    _, r = rect()
    bbox = (r.left, r.top, r.right, r.bottom)
    img = ImageGrab.grab(bbox=bbox, all_screens=True)
    img.save(out)
    print(f"grab fg={ok} bbox={bbox} size={img.size} -> {out}")
    return img


# ---------------------------------------------------------------- 配置
def cfg(keys=None):
    d = json.load(open(CONFIG, encoding="utf-8"))
    if keys:
        return {k: d.get(k) for k in keys}
    return d


def state():
    ws = all_windows()
    print(f"标题为 'Anycast' 的窗口 {len(ws)} 个：")
    for h, x, y, w, hh, vis, icon in ws:
        print(f"  hwnd=0x{h:x} pos=({x},{y}) size={w}x{hh} visible={vis} minimized={icon} "
              f"logical={w / DPI:.0f}x{hh / DPI:.0f}")
    hwnd = find_window()
    if hwnd:
        print(f"主窗口 hwnd=0x{hwnd:x} foreground={user32.GetForegroundWindow() == hwnd}")
    d = cfg()
    print("配置关键项：")
    for k in ("hide_on_blur", "theme", "view_mode",
              "filter_shelf_open", "window_width", "window_height"):
        print(f"  {k:22s}= {d.get(k)}")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "state"
    if cmd == "state":
        state()
    elif cmd == "grab":
        grab(sys.argv[2])
    elif cmd in ("click", "dblclick"):
        click(int(sys.argv[2]), int(sys.argv[3]), double=(cmd == "dblclick"))
        if len(sys.argv) > 4:
            grab(sys.argv[4])
        else:
            print(f"clicked logical({sys.argv[2]},{sys.argv[3]}) -> screen{to_screen(int(sys.argv[2]), int(sys.argv[3]))}")
    elif cmd == "wheel":
        wheel(int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]))
        if len(sys.argv) > 5:
            grab(sys.argv[5])
        else:
            print(f"wheel {sys.argv[2]}")
    elif cmd == "keys":
        keys(sys.argv[2])
        if len(sys.argv) > 3:
            grab(sys.argv[3])
        else:
            print(f"keys {sys.argv[2]}")
    elif cmd == "type":
        typetext(sys.argv[2])
        if len(sys.argv) > 3:
            grab(sys.argv[3])
        else:
            print("typed")
    elif cmd == "grabdelay":
        # 异步动作（如热键投递自检、后台索引）的结论不是点击后立刻出现的。
        # 先睡再抓，避免为了等结果把「点击」和「抓图」拆成两个进程 —— 那样中间
        # 焦点会跑掉，`hide_on_blur` 一开就抓到背后的窗口。
        time.sleep(int(sys.argv[2]) / 1000.0)
        grab(sys.argv[3])
    elif cmd == "cfg":
        print(json.dumps(cfg(sys.argv[2].split(",") if len(sys.argv) > 2 else None),
                         ensure_ascii=False, indent=2))
    else:
        raise SystemExit(__doc__)
