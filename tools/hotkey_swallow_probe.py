"""检测「注册成功但按键被第三方低级键盘钩子吞掉」这一类全局热键冲突。

背景（2026-09-25 实测）：
    PI-Desktop 的 host-core 用 SetWindowsHookExW(WH_KEYBOARD_LL) 独占 Alt+Space
    （`crates/host-core/src/keyboard.rs`，命中时 `return 1` 且不调 CallNextHookEx）。
    结果是 **RegisterHotKey 照样返回成功、GetLastError() == 0**，
    但 WM_HOTKEY 永远不来 —— Anycast 只看 1409 的冲突检测对这类冲突完全失明。

为什么必须带判据对照：
    「注册成功 + 收不到 WM_HOTKEY」有两种解释：
      ① 真有第三方钩子在吞键
      ② 探针自己的消息泵/注册写错了
    对照组（一个必然空闲的组合键，默认 Ctrl+Alt+Shift+F13）必须收到 WM_HOTKEY，
    否则本探针的结论一律作废。

用法：
    E:\\Python\\python.exe tools\\hotkey_swallow_probe.py                 # 默认测 Alt+Space
    E:\\Python\\python.exe tools\\hotkey_swallow_probe.py --combo ctrl+alt+p
    E:\\Python\\python.exe tools\\hotkey_swallow_probe.py --no-inject     # 只注册不发键

⚠️ 会**真的注入按键**。若目标组合键被别的程序接管，那个程序会真的响应
   （例如 Alt+Space 会弹出 PI-Desktop 的插件启动器）。跑完请自行关掉它，
   或再按一次同样的组合键让它 toggle 回去。
⚠️ 必须用系统 Python（E:\\Python\\python.exe）。托管 Python 也可以跑，
   但本项目其它探针需要 PIL，统一用系统 Python 省得记两套。
"""

from __future__ import annotations

import argparse
import ctypes
import sys
import time
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)

WM_HOTKEY = 0x0312
PM_REMOVE = 1

MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN = 0x0001, 0x0002, 0x0004, 0x0008
MOD_NOREPEAT = 0x4000

VK_NAMES = {
    "space": 0x20, "enter": 0x0D, "esc": 0x1B, "tab": 0x09,
    "left": 0x25, "up": 0x26, "right": 0x27, "down": 0x28,
    "f13": 0x7C, "f14": 0x7D,
}
VK_MODS = {"alt": 0x12, "ctrl": 0x11, "shift": 0x10, "win": 0x5B}


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD),
                ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.POINTER(wintypes.ULONG))]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("padding", ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wintypes.DWORD), ("u", _INPUTUNION)]


class MSG(ctypes.Structure):
    _fields_ = [("hwnd", wintypes.HWND), ("message", wintypes.UINT),
                ("wParam", wintypes.WPARAM), ("lParam", wintypes.LPARAM),
                ("time", wintypes.DWORD), ("pt_x", wintypes.LONG), ("pt_y", wintypes.LONG)]


# 一律显式声明 argtypes/restype：漏掉在 64 位下会把指针按 int 截断
user32.RegisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int, wintypes.UINT, wintypes.UINT]
user32.RegisterHotKey.restype = wintypes.BOOL
user32.UnregisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int]
user32.UnregisterHotKey.restype = wintypes.BOOL
user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
user32.SendInput.restype = wintypes.UINT
user32.PeekMessageW.argtypes = [ctypes.POINTER(MSG), wintypes.HWND, wintypes.UINT,
                                wintypes.UINT, wintypes.UINT]
user32.PeekMessageW.restype = wintypes.BOOL


def parse_combo(text: str) -> tuple[int, int, list[int]]:
    """'alt+space' -> (mods, vk, [修饰键 vk...])，修饰键顺序固定，便于零间隔一次发完。"""
    mods, vk, order = 0, None, []
    for part in [p.strip().lower() for p in text.split("+") if p.strip()]:
        if part in ("alt", "ctrl", "control", "shift", "win", "meta"):
            key = "ctrl" if part == "control" else ("win" if part == "meta" else part)
            mods |= {"alt": MOD_ALT, "ctrl": MOD_CONTROL,
                     "shift": MOD_SHIFT, "win": MOD_WIN}[key]
            order.append(VK_MODS[key])
        elif part in VK_NAMES:
            vk = VK_NAMES[part]
        elif len(part) == 1 and part.isascii():
            vk = ord(part.upper())
        else:
            raise SystemExit("认不出的键：%r" % part)
    if vk is None:
        raise SystemExit("组合键里缺少主键")
    return mods, vk, order


def mk(vk: int, up: bool = False) -> INPUT:
    inp = INPUT(type=1)  # INPUT_KEYBOARD
    inp.u.ki = KEYBDINPUT(wVk=vk, wScan=0, dwFlags=2 if up else 0,
                          time=0, dwExtraInfo=None)
    return inp


def send_combo(mod_order: list[int], vk: int) -> int:
    """修饰键与主键压进**一次** SendInput（零间隔），逐键 sleep 会引入菜单模式伪影。"""
    seq = [mk(k) for k in mod_order] + [mk(vk), mk(vk, True)]
    seq += [mk(k, True) for k in reversed(mod_order)]
    arr = (INPUT * len(seq))(*seq)
    return user32.SendInput(len(seq), arr, ctypes.sizeof(INPUT))


def pump_for_hotkey(seconds: float = 1.0) -> bool:
    """泵本线程消息队列，看有没有 WM_HOTKEY 落到 RegisterHotKey(NULL, ...) 上。"""
    got, msg, t0 = False, MSG(), time.time()
    while time.time() - t0 < seconds:
        while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, PM_REMOVE):
            if msg.message == WM_HOTKEY:
                got = True
        time.sleep(0.02)
    return got


def trial(label: str, combo: str, inject: bool, hid: int) -> dict:
    mods, vk, order = parse_combo(combo)
    ok = user32.RegisterHotKey(None, hid, mods | MOD_NOREPEAT, vk)
    err = 0 if ok else ctypes.get_last_error()
    result = {"label": label, "combo": combo, "registered": bool(ok),
              "err": err, "wm_hotkey": None}
    try:
        if ok and inject:
            send_combo(order, vk)
            result["wm_hotkey"] = pump_for_hotkey()
    finally:
        if ok:
            user32.UnregisterHotKey(None, hid)   # 抢到了必须还回去
    return result


def main() -> int:
    ap = argparse.ArgumentParser(description="全局热键被钩子吞键的检测探针")
    ap.add_argument("--combo", default="alt+space", help="要检测的组合键，默认 alt+space")
    ap.add_argument("--control", default="ctrl+alt+shift+f13",
                    help="判据对照用的空闲组合键")
    ap.add_argument("--no-inject", action="store_true", help="只注册，不注入按键")
    args = ap.parse_args()

    inject = not args.no_inject
    print("判据对照（必须收到 WM_HOTKEY，否则本探针结论作废）")
    ctrl = trial("对照", args.control, inject, 0x4001)
    print("  %-10s 注册=%-4s err=%-4d WM_HOTKEY=%s"
          % (ctrl["combo"], ctrl["registered"], ctrl["err"], ctrl["wm_hotkey"]))
    if not ctrl["registered"] or ctrl["wm_hotkey"] is False:
        print("\n✗ 对照失败：本探针的消息泵或注册链路不成立，下面的结论不要采信。")
        return 2

    print("\n目标组合键")
    tgt = trial("目标", args.combo, inject, 0x4002)
    print("  %-10s 注册=%-4s err=%-4d WM_HOTKEY=%s"
          % (tgt["combo"], tgt["registered"], tgt["err"], tgt["wm_hotkey"]))

    print("\n结论")
    if not tgt["registered"]:
        if tgt["err"] == 1409:
            print("  · 注册被拒（1409）：有人用 RegisterHotKey 占住了 → 常规冲突，"
                  "Anycast 的检测能看见。")
        else:
            print("  · 注册失败 err=%d（非 1409），不是热键占用。" % tgt["err"])
    elif tgt["wm_hotkey"] is False:
        print("  · **注册成功但收不到 WM_HOTKEY**：有第三方低级键盘钩子在吞这个键。")
        print("    这类冲突不会产生 1409，只看 RegisterHotKey 的检测**看不见**它 ——")
        print("    对 Anycast 就是「设置里说注册好了，实际按下去没反应」。")
    else:
        print("  · 注册成功且按键能到：这个组合键当前可用。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
