"""Anycast 原生窗口的输入探针：按逻辑坐标点击 / 发送按键。

为什么不用 drive.ps1：SendKeys 只能发按键，点不了控件；而且它依赖 Add-Type 编译，
在受限环境里会被拦。这里用纯 ctypes，坐标按 **窗口内逻辑像素** 给，
脚本自己换算成屏幕物理坐标（本机 150% DPI）。

用法：
  python tools/ui_probe.py rect                       # 打印窗口矩形与 DPI
  python tools/ui_probe.py click <lx> <ly>            # 在窗口内逻辑坐标点击
  python tools/ui_probe.py dblclick <lx> <ly>
  python tools/ui_probe.py keys ctrl+,                # 组合键（ctrl/alt/shift/win + 字符）
  python tools/ui_probe.py type "hello"               # 逐字符输入
  python tools/ui_probe.py shot <out.png>             # 抓窗口
"""
import ctypes
import ctypes.wintypes as wt
import sys
import time

user32 = ctypes.windll.user32
user32.SetProcessDPIAware()

WINDOW_TITLE = "Anycast"
DPI_SCALE = 1.5  # 本机 150%

MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
KEYEVENTF_KEYUP = 0x0002

VK = {
    "ctrl": 0x11, "alt": 0x12, "shift": 0x10, "win": 0x5B,
    "enter": 0x0D, "esc": 0x1B, "tab": 0x09, "space": 0x20,
    "back": 0x08, "del": 0x2E, "up": 0x26, "down": 0x28,
    "left": 0x25, "right": 0x27,
    ",": 0xBC, ".": 0xBE, ";": 0xBA, "/": 0xBF,
}


kernel32 = ctypes.windll.kernel32


def find_window():
    hwnd = user32.FindWindowW("Window Class", WINDOW_TITLE)
    if not hwnd:
        hwnd = user32.FindWindowW(None, WINDOW_TITLE)
    return hwnd


def force_foreground(hwnd):
    """把窗口拉到前台。

    直接调 SetForegroundWindow 常被 Windows 的前台锁定机制静默拒绝
    （调用进程不是当前前台进程时）。用 AttachThreadInput 挂到当前前台线程上
    再调，才能稳定生效。
    """
    if user32.GetForegroundWindow() == hwnd:
        return True
    fg = user32.GetForegroundWindow()
    tid_fg = user32.GetWindowThreadProcessId(fg, None) if fg else 0
    tid_me = kernel32.GetCurrentThreadId()
    if tid_fg:
        user32.AttachThreadInput(tid_me, tid_fg, True)
    user32.ShowWindow(hwnd, 9)          # SW_RESTORE
    user32.BringWindowToTop(hwnd)
    user32.SetForegroundWindow(hwnd)
    if tid_fg:
        user32.AttachThreadInput(tid_me, tid_fg, False)
    time.sleep(0.2)
    return user32.GetForegroundWindow() == hwnd


def status():
    hwnd = find_window()
    if not hwnd:
        return "窗口不存在"
    vis = bool(user32.IsWindowVisible(hwnd))
    fg = user32.GetForegroundWindow() == hwnd
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return (f"hwnd=0x{hwnd:x} visible={vis} foreground={fg} "
            f"rect={r.left},{r.top} size={r.right - r.left}x{r.bottom - r.top}")


def rect():
    hwnd = find_window()
    if not hwnd:
        raise SystemExit("ERR: 找不到窗口")
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return hwnd, r


def to_screen(lx, ly):
    _, r = rect()
    return int(r.left + lx * DPI_SCALE), int(r.top + ly * DPI_SCALE)


def _down_up(vk, up=False):
    user32.keybd_event(vk, 0, KEYEVENTF_KEYUP if up else 0, 0)


def click(lx, ly, double=False):
    hwnd, _ = rect()
    force_foreground(hwnd)
    x, y = to_screen(lx, ly)
    user32.SetCursorPos(x, y)
    time.sleep(0.12)
    for _ in range(2 if double else 1):
        user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
        time.sleep(0.04)
        user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
        time.sleep(0.10)


def keys(combo):
    """combo 形如 'ctrl+,' 或 'esc' 或 'ctrl+shift+t'"""
    parts = combo.lower().split("+")
    mods = [p for p in parts if p in ("ctrl", "alt", "shift", "win")]
    rest = [p for p in parts if p not in ("ctrl", "alt", "shift", "win")]
    main = rest[0] if rest else None
    if main is None:
        return
    hwnd, _ = rect()
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.25)
    for m in mods:
        _down_up(VK[m])
    vk = VK.get(main, ord(main.upper()) if len(main) == 1 else None)
    if vk is None:
        raise SystemExit(f"ERR: 未知按键 {main}")
    _down_up(vk)
    time.sleep(0.03)
    _down_up(vk, up=True)
    for m in reversed(mods):
        _down_up(VK[m], up=True)
    time.sleep(0.10)


def typetext(s):
    for ch in s:
        vk = VK.get(ch)
        if vk is None and len(ch) == 1:
            vk = user32.VkKeyScanW(ord(ch)) & 0xFF
        if vk:
            _down_up(vk)
            time.sleep(0.02)
            _down_up(vk, up=True)
            time.sleep(0.03)


def shot(out):
    import subprocess
    subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         f'& "D:\\Anycast\\tools\\win_shot.ps1" -title "Anycast" -out "{out}" -scale 1.0 -waitMs 700'],
        check=False)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "rect"
    if cmd == "rect":
        hwnd, r = rect()
        print(f"hwnd=0x{hwnd:x} rect={r.left},{r.top} size={r.right - r.left}x{r.bottom - r.top} "
              f"scale={DPI_SCALE} logical={(r.right - r.left) / DPI_SCALE:.0f}x{(r.bottom - r.top) / DPI_SCALE:.0f}")
    elif cmd == "click":
        click(int(sys.argv[2]), int(sys.argv[3]))
        print(f"clicked ({sys.argv[2]},{sys.argv[3]}) -> screen {to_screen(int(sys.argv[2]), int(sys.argv[3]))}")
    elif cmd == "dblclick":
        click(int(sys.argv[2]), int(sys.argv[3]), double=True)
        print("double clicked")
    elif cmd == "keys":
        keys(sys.argv[2])
        print(f"keys {sys.argv[2]}")
    elif cmd == "type":
        typetext(sys.argv[2])
        print("typed")
    elif cmd == "shot":
        shot(sys.argv[2])
        print("shot", sys.argv[2])
    else:
        raise SystemExit(__doc__)
