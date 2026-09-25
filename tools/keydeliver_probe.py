"""决定性实验：组合键被别的进程用 RegisterHotKey 占住后，
按键还会不会**送达**前台窗口（WM_KEYDOWN / WM_SYSKEYDOWN）？

为什么必须做这个实验：
    冲突 H 的「保存时探测」那一半，走的是**界面录制**这条路 ——
    `gui/mod.rs` 的 `on_key` → `finish_recording` → `set_wake_hotkey`。
    而录制是靠 Slint 的窗口按键回调驱动的（全仓 `grep SetWindowsHookEx` 为空，
    没有低级键盘钩子），也就是说：**组合键必须以普通 WM_KEYDOWN 送进窗口**。

    可是「占住」用的偏偏是 `RegisterHotKey` —— 按 MSDN 的说法，
    命中热键时系统只给注册方发 WM_HOTKEY，**不再**给焦点窗口发按键消息。

    若真如此，UI 录制就永远收不到被占用的组合键 →
    「点键帽 → 按 Ctrl+Shift+F9 → 看有没有报错」这套测试**根本不成立**：
    配置确实不会变，但原因是「应用压根没收到键」，不是「探测拦住了」。
    那是**假阴性**，比没测还危险。

    所以先把这个前提单独验掉，再决定怎么测保存路径。

做法（同进程，两相）：
    相 A（对照）：没人占 → 发键 → 窗口应该收到
    相 B（实验）：外部进程占住 → 发键 → 窗口还收得到吗？

用法：python tools/keydeliver_probe.py [--combo Ctrl+Shift+F9]
退出码：0 = 两相都收到（UI 测试有效）；3 = 相 B 收不到（UI 测试无效，须换法）
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
user32.SetProcessDPIAware()

# 必须显式声明签名：不声明时 ctypes 把句柄当 c_int，64 位下
# `GetModuleHandleW` 返回的 0x140000000 会直接 OverflowError。
kernel32.GetModuleHandleW.restype = ctypes.c_void_p
kernel32.GetModuleHandleW.argtypes = [wt.LPCWSTR]
user32.LoadCursorW.restype = ctypes.c_void_p
user32.LoadCursorW.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
user32.CreateWindowExW.restype = wt.HWND
user32.CreateWindowExW.argtypes = [
    wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
    ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
    wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p,
]
user32.RegisterClassW.argtypes = [ctypes.c_void_p]

WM_KEYDOWN, WM_SYSKEYDOWN, WM_KEYUP, WM_SYSKEYUP = 0x0100, 0x0104, 0x0101, 0x0105
WM_DESTROY = 0x0002

WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_long, wt.HWND, ctypes.c_uint,
                             wt.WPARAM, wt.LPARAM)


class WNDCLASSW(ctypes.Structure):
    _fields_ = [("style", ctypes.c_uint), ("lpfnWndProc", WNDPROC),
                ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                ("hInstance", wt.HINSTANCE), ("hIcon", wt.HICON),
                ("hCursor", wt.HANDLE), ("hbrBackground", wt.HBRUSH),
                ("lpszMenuName", wt.LPCWSTR), ("lpszClassName", wt.LPCWSTR)]


def make_window(tag):
    """建一个可见窗口，把收到的按键记进列表。返回 (hwnd, events, keepalive)。"""
    events = []

    def proc(h, m, w, l):
        if m in (WM_KEYDOWN, WM_SYSKEYDOWN):
            events.append(("down", int(w)))
        elif m in (WM_KEYUP, WM_SYSKEYUP):
            events.append(("up", int(w)))
        return user32.DefWindowProcW(wt.HWND(h), ctypes.c_uint(m),
                                     wt.WPARAM(w), wt.LPARAM(l))

    cb = WNDPROC(proc)
    cls = f"KeyDeliverProbe_{tag}"
    wc = WNDCLASSW()
    wc.style = 0
    wc.lpfnWndProc = cb
    wc.hInstance = kernel32.GetModuleHandleW(None)
    wc.hCursor = user32.LoadCursorW(None, 32512)  # IDC_ARROW
    wc.hbrBackground = 6  # COLOR_WINDOW
    wc.lpszClassName = cls
    if not user32.RegisterClassW(ctypes.byref(wc)):
        err = ctypes.get_last_error()
        raise SystemExit(f"ERR: RegisterClassW 失败 err={err}")

    hwnd = user32.CreateWindowExW(0, cls, "KeyDeliverProbe", 0x10CF0000,
                                  100, 100, 420, 220, None, None,
                                  wc.hInstance, None)
    if not hwnd:
        raise SystemExit(f"ERR: CreateWindowExW 失败 err={ctypes.get_last_error()}")
    user32.ShowWindow(hwnd, 5)
    return hwnd, events, cb


def pump(seconds):
    """泵消息 —— 不泵的话 WndProc 一次都不会被调，必然误判成「收不到」。"""
    end = time.time() + seconds
    msg = wt.MSG()
    while time.time() < end:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))
        time.sleep(0.01)


def focus(hwnd):
    user32.SetForegroundWindow(hwnd)
    user32.SetFocus(hwnd)
    time.sleep(0.4)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--combo", default="Ctrl+Shift+F9")
    args = ap.parse_args()

    import hotkey_probe as hp

    hwnd, events, _keep = make_window(str(os.getpid()))
    print(f"探针窗口 hwnd=0x{hwnd:x}  combo={args.combo}\n")

    # ---- 相 A：对照（没人占）--------------------------------------------
    focus(hwnd)
    events.clear()
    print("[A] 对照：无人占用 → 发键 …")
    hp.send_combo_chord(args.combo)
    pump(1.2)
    got_a = [e for e in events if e[0] == "down"]
    print(f"    收到 keydown: {[hex(v) for _, v in got_a] or '无'}")

    # ---- 起占位进程 ------------------------------------------------------
    print(f"\n[占位] 起外部进程占住 {args.combo} …")
    holder = subprocess.Popen(
        [sys.executable, os.path.join(HERE, "hotkey_probe.py"),
         "hold", args.combo, "120"],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    time.sleep(2.5)
    mods, vk = hp.spec_of(args.combo)
    ok, err = hp.try_claim(mods, vk, "check")
    if ok:
        holder.terminate()
        print("    ❌ 没能占住 → 实验无效")
        return 2
    print(f"    ✅ 已占住（err={err}）")

    # ---- 相 B：实验（被占）----------------------------------------------
    focus(hwnd)
    events.clear()
    print(f"\n[B] 实验：已被占住 → 发键 …")
    hp.send_combo_chord(args.combo)
    pump(1.2)
    got_b = [e for e in events if e[0] == "down"]
    print(f"    收到 keydown: {[hex(v) for _, v in got_b] or '无'}")

    holder.terminate()
    try:
        holder.wait(timeout=5)
    except subprocess.TimeoutExpired:
        holder.kill()

    # ---- 判定 ------------------------------------------------------------
    print("\n── 判定 " + "─" * 46)
    print(f"相 A（无人占）收到按键 : {'是' if got_a else '否'}   ← 必须为「是」，否则是探针自己坏了")
    print(f"相 B（被占住）收到按键 : {'是' if got_b else '否'}")
    if not got_a:
        print("❌ 相 A 都没收到 → 探针实现有问题，结论不可信")
        return 2
    if got_b:
        print("✅ 被占住也收得到 → UI 录制路径可用于验证保存时探测")
        return 0
    print("⚠ 被占住时收不到按键 → 系统把该组合键吞给了注册方")
    print("  ⇒ 「点键帽 + 发被占组合键」这套 UI 测试**不成立**，须换验证方式")
    return 3


if __name__ == "__main__":
    sys.exit(main())
