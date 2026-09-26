"""抓取 Anycast 主窗口位图（带配置保护），用于「设计稿 vs 原生」逐区域对照。

用法：
  python tools/ui_shot.py <out.png> [cx cy cw ch] [zoom]
  python tools/ui_shot.py <out.png>                 # 整窗
  python tools/ui_shot.py <out.png> 0 40 460 60 4   # 裁「窗口内逻辑 px」区域并放大 4 倍

为什么要这个脚本（而不是 `win_shot.ps1` / `rt_probe.grab`）：
  1. **`hide_on_blur` 必须临时关掉**，否则一失焦窗口就隐藏、抓到的全是背后的东西；
     收工时**整份写回** config（判据是整份逐字段 diff 到零差异，不是只看那一个键）。
  2. 抓完把应用杀掉，避免留下一个「TOPMOST 已被摘掉」的实例（`force_foreground`
     内部 `SetWindowPos(HWND_TOPMOST)` → `(HWND_NOTOPMOST)` 会永久摘掉 `WS_EX_TOPMOST`，
     而应用自己的样式校正只补 `TOOLWINDOW`、不管 TOPMOST）。

⚠️ 窗口内逻辑 px = 物理 px / 1.5（本机 150% DPI）。裁剪一律按**逻辑 px** 传。
"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import shutil
import subprocess
import sys
import time

_HERE = os.path.dirname(os.path.abspath(__file__))
_ROOT = os.path.dirname(_HERE)
sys.path.insert(0, _HERE)
import importlib.util
_spec = importlib.util.spec_from_file_location("rp", os.path.join(_HERE, "rt_probe.py"))
rp = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(rp)

u = ctypes.windll.user32
k = ctypes.windll.kernel32
u.SetProcessDPIAware()

APP = os.path.join(_ROOT, "target", "debug", "anycast.exe")
ROAMING = r"C:\Users\30130\AppData\Roaming"
CONFIG = os.path.join(ROAMING, "Anycast", "data", "config.json")
BACKUP = os.path.join(_ROOT, "target", "verify", "config.before_shot.json")
DPI = 1.5

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(_ROOT, "target", "verify", "shot.png")
CROP = None
ZOOM = 1
KEYS = None
CLICK = None
args = sys.argv[2:]
i = 0
while i < len(args):
    if args[i] == "--keys":
        KEYS = args[i + 1]; i += 2
    elif args[i] == "--click":
        CLICK = tuple(int(v) for v in args[i + 1].split(",")); i += 2
    elif CROP is None and len(args) - i >= 4:
        CROP = tuple(int(v) for v in args[i:i + 4]); i += 4
        if i < len(args) and args[i].isdigit():
            ZOOM = int(args[i]); i += 1
    else:
        i += 1


def kill_app():
    subprocess.run(["taskkill", "/F", "/IM", "anycast.exe"],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.6)


def main():
    os.makedirs(os.path.dirname(BACKUP), exist_ok=True)
    shutil.copyfile(CONFIG, BACKUP)
    d = json.load(open(CONFIG, encoding="utf-8"))
    d["hide_on_blur"] = False
    json.dump(d, open(CONFIG, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
    print("配置已备份，hide_on_blur → false")

    kill_app()
    env = {**os.environ, "APPDATA": ROAMING,
           "LOCALAPPDATA": r"C:\Users\30130\AppData\Local",
           "ProgramData": r"C:\ProgramData", "PUBLIC": r"C:\Users\Public",
           "RUST_LOG": "info"}
    log = open(os.path.join(_ROOT, "target", "verify", "ui_shot_app.log"), "w", encoding="utf-8")
    p = subprocess.Popen([APP], cwd=_ROOT, env=env, stdout=log, stderr=log)
    try:
        h = None
        for _ in range(60):
            time.sleep(0.5)
            h = rp.find_window()
            if h:
                break
        if not h:
            raise SystemExit("ERR: 20s 内没找到主窗口")
        time.sleep(1.2)                      # 等首帧稳定（合成器过渡期会糊）
        # ⚠️ **不要**调 `force_foreground`：它内部 `SetWindowPos(HWND_TOPMOST)` →
        # `(HWND_NOTOPMOST)`，会 (a) 永久摘掉 `WS_EX_TOPMOST`，(b) **改变激活态**，
        # 而激活态一变 DWM 就把「原生标题栏」画到窗口上（doc §12.5.4），
        # 正好糊住搜索栏 —— 抓出来的图不能用。
        # 应用开机就显示窗口且本来就是 topmost，所以这里只需在「真的没显示」时补一次 SW_SHOW。
        if not u.IsWindowVisible(h):
            print("窗口不可见 → SW_SHOW 并等 nudge 擦净标题栏")
            u.ShowWindow(h, 5)               # SW_SHOW
            time.sleep(1.5)                  # 给 theme::nudge_surface 留出时间
        else:
            print("窗口已可见，保持激活态不动（避免 DWM 画标题栏）")
        time.sleep(0.3)

        # 可选：先做一次交互（打开设置页 / 点某个控件），再抓图
        if KEYS or CLICK:
            fg_before = u.GetForegroundWindow() == h
            print(f"交互前 foreground={fg_before}")
            if KEYS:
                rp.keys(KEYS)
            if CLICK:
                rp.click(*CLICK)
            time.sleep(0.9)
            print(f"交互后 foreground={u.GetForegroundWindow() == h}")
            time.sleep(0.4)

        from PIL import ImageGrab
        hwnd, r = rp.rect()
        img = ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom), all_screens=True)
        print(f"抓图 {img.size} (物理) = {img.size[0]/DPI:.0f}x{img.size[1]/DPI:.0f} (逻辑)")

        if CROP:
            cx, cy, cw, ch = CROP
            box = (int(cx * DPI), int(cy * DPI), int((cx + cw) * DPI), int((cy + ch) * DPI))
            img = img.crop(box)
            if ZOOM > 1:
                img = img.resize((img.width * ZOOM, img.height * ZOOM), 0)   # NEAREST
            print(f"裁剪逻辑 {CROP} → 输出 {img.size}")
        img.save(OUT)
        print("wrote", OUT)
    finally:
        kill_app()
        log.close()
        shutil.copyfile(BACKUP, CONFIG)
        same = open(BACKUP, "rb").read() == open(CONFIG, "rb").read()
        print("配置整份写回：", "零差异 ✅" if same else "⚠ 有差异，请人工核对")


main()
