"""验证「`--silent` 开机自启时，热键注册失败的提示会不会丢」。

为什么单独测：silent 模式下窗口全程不显示，Toast 画在看不见的地方 —— 这正是
`pending_toast` 要解决的那条路径。非 silent 场景（窗口可见）已验证正常。

关键点：**不能**用 rt_probe.keys()，它内部会 force_foreground 把窗口拉出来，
测试前提就没了。全局热键不依赖焦点，这里直接发 SendInput。
"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rt_probe as rp  # noqa: E402
import hotkey_probe as hp  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(os.environ.get("TEMP", r"C:\Windows\Temp"), "silent_toast")

user32 = ctypes.windll.user32
user32.RegisterHotKey.argtypes = [wt.HWND, ctypes.c_int, wt.UINT, wt.UINT]
user32.RegisterHotKey.restype = wt.BOOL


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD),
                ("time", wt.DWORD), ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]


class _U(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("pad", ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _U)]


def send_key(vk, up=False):
    inp = INPUT(type=1)
    inp.u.ki = KEYBDINPUT(wVk=vk, wScan=0, dwFlags=0x0002 if up else 0, time=0, dwExtraInfo=None)
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))


def tap_alt_space():
    """发 Alt+Space。不碰焦点 —— 应用已 RegisterHotKey 全局注册，系统会发 WM_HOTKEY。"""
    VK_MENU, VK_SPACE = 0x12, 0x20
    send_key(VK_MENU)
    time.sleep(0.03)
    send_key(VK_SPACE)
    time.sleep(0.04)
    send_key(VK_SPACE, up=True)
    send_key(VK_MENU, up=True)


def win_state():
    ws = rp.all_windows()
    if not ws:
        return "找不到窗口"
    ws.sort(key=lambda w: w[3] * w[4], reverse=True)
    _, l, t, w, h, vis, ico = ws[0]
    return f"rect=({l},{t},{w}x{h}) visible={vis} iconic={ico}"


def main():
    os.makedirs(OUT_DIR, exist_ok=True)

    # 1) 占住 Ctrl+Shift+F11，让应用的绑定注册必然失败（1409）
    mods, vk = hp.spec_of("Ctrl+Shift+F11")
    held = user32.RegisterHotKey(None, 1, mods, vk)
    print(f"[hold] Ctrl+Shift+F11 抢占: {bool(held)}")

    # 2) silent 启动
    env = dict(os.environ)
    env["APPDATA"] = r"C:\Users\30130\AppData\Roaming"
    env["ProgramData"] = r"C:\ProgramData"
    env["PUBLIC"] = r"C:\Users\Public"
    log_path = os.path.join(OUT_DIR, "app.log")
    logf = open(log_path, "w", encoding="utf-8", errors="replace")
    t0 = time.time()
    proc = subprocess.Popen(
        [os.path.join(ROOT, "target", "debug", "anycast.exe"), "--silent"],
        cwd=ROOT, env=env, stdout=logf, stderr=subprocess.STDOUT,
    )
    print(f"[app] --silent 启动 pid={proc.pid}")

    # 3) 等它完全起来，确认窗口确实是隐藏的
    time.sleep(4.0)
    print(f"[state] 启动 4.0s 后: {win_state()}   ← 期望 visible=False")

    # 4) 发唤醒热键把窗口叫出来，等 pending_toast 补显
    print("[act] 发送 Alt+Space 唤醒")
    tap_alt_space()
    for off in (1.0, 1.8):
        time.sleep(off if off == 1.0 else 0.8)
        p = os.path.join(OUT_DIR, f"after_{off:.1f}.png")
        print(f"[state] 唤醒后: {win_state()}")
        try:
            rp.grab(p)
            print(f"[grab] -> {p} ({os.path.getsize(p)} B)")
        except Exception as e:  # noqa: BLE001
            print(f"[grab] 失败: {e}")

    time.sleep(0.3)
    subprocess.run(["taskkill", "/F", "/PID", str(proc.pid)], capture_output=True)
    logf.close()

    print("\n=== 应用日志关键行 ===")
    with open(log_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            if any(k in line for k in ("热键", "注册失败", "WARN", "ERROR")):
                print("  " + line.rstrip())
    print(f"\n抓图目录: {OUT_DIR}")


if __name__ == "__main__":
    main()
