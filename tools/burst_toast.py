"""一次性诊断脚本：验证「启动期热键注册失败」的 toast 是否会显示。

为什么需要它：`upgrade_in_event_loop` 在事件循环尚未启动时的行为（是排队到
事件循环启动后执行，还是当场同步执行）决定了启动期的通知会不会被 `with_gui`
的 `if let Some` 静默吞掉。截图时点靠 sleep 猜是不确定的 —— 这里用爆发式连拍
把时序不确定性消掉。

用法：python tools/burst_toast.py <组合键> [抓图间隔秒 ...]
"""
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rt_probe as rp  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(os.environ.get("TEMP", r"C:\Windows\Temp"), "toast_burst")


def main():
    combo = sys.argv[1] if len(sys.argv) > 1 else "Ctrl+Shift+F11"
    offsets = [float(x) for x in sys.argv[2:]] or [0.7, 1.3, 1.9, 2.5, 3.1, 3.8]
    os.makedirs(OUT_DIR, exist_ok=True)

    # 1) 先占住这个键。
    #    用线程级注册（hwnd=NULL）：热键注册是**系统全局**的，与 hwnd 无关，
    #    照样会让别的进程拿到 1409。故意不注销 —— 由进程退出统一释放，
    #    避免「注销掉自己」把测试前提破坏掉。
    import ctypes
    import ctypes.wintypes as wt
    user32 = ctypes.windll.user32
    user32.RegisterHotKey.argtypes = [wt.HWND, ctypes.c_int, wt.UINT, wt.UINT]
    user32.RegisterHotKey.restype = wt.BOOL
    mods, vk = rp_combo(combo)
    ok = user32.RegisterHotKey(None, 1, mods, vk)
    print(f"[hold] {combo} 抢占结果: {bool(ok)}  (True = 我们占住了，应用应拿到 1409)")

    # 2) 启动应用，环境变量显式给全（见 skill windows-launch-env-gotchas）
    env = dict(os.environ)
    env["APPDATA"] = r"C:\Users\30130\AppData\Roaming"
    env["ProgramData"] = r"C:\ProgramData"
    env["PUBLIC"] = r"C:\Users\Public"
    log_path = os.path.join(OUT_DIR, "app.log")
    logf = open(log_path, "w", encoding="utf-8", errors="replace")
    t0 = time.time()
    proc = subprocess.Popen(
        [os.path.join(ROOT, "target", "debug", "anycast.exe")],
        cwd=ROOT, env=env, stdout=logf, stderr=subprocess.STDOUT,
    )
    print(f"[app] 已启动 pid={proc.pid} t0={t0:.3f}")

    # 3) 爆发抓图
    shots = []
    for off in offsets:
        wait = t0 + off - time.time()
        if wait > 0:
            time.sleep(wait)
        p = os.path.join(OUT_DIR, f"t{off:.1f}.png")
        try:
            rp.grab(p)
            size = os.path.getsize(p)
        except Exception as e:  # noqa: BLE001
            size = f"ERR {e}"
        shots.append(p)
        print(f"[grab] +{off:.1f}s -> {p}  ({size})")

    # 4) 收尾
    time.sleep(0.3)
    try:
        subprocess.run(["taskkill", "/F", "/PID", str(proc.pid)], capture_output=True)
    except Exception:  # noqa: BLE001
        pass
    logf.close()

    print("\n=== 应用日志（只看关键行）===")
    with open(log_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            if any(k in line for k in ("热键", "toast", "Toast", "注册失败", "WARN", "ERROR")):
                print("  " + line.rstrip())
    print(f"\n抓图目录: {OUT_DIR}")


def rp_combo(combo):
    """借用 hotkey_probe 的解析（它已经处理过 Ctrl/Shift/Alt/Win 与 F 键）。"""
    import hotkey_probe as hp
    return hp.spec_of(combo)


if __name__ == "__main__":
    main()
