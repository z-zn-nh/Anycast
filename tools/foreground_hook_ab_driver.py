"""A/B 驱动：两臂各跑 N 轮，每轮**重启应用**，一次拿全分布。

配套探针 `tools/foreground_hook_ab.py`（本脚本只 exec 它的**函数区**，
不跑它的 `__main__` 段）。

为什么要重启应用：实测「整条标题栏」只在**本轮第一次失去激活**时才画出来
（后续几次只画窄的那条 ~9%）。所以每个应用实例只能取到 1 个有效样本 ——
要分布就必须多次重启。

为什么要写成驱动脚本：本机桌面每隔一两秒闪一下锁屏，靠人工一条条发指令
会一直在「等解锁」上耗掉窗口；放在一个进程里连续跑，命中率高得多。

**自带的配置保护**（这是踩过坑加的）：
  1. 开跑前把整份 `config.json` 备份到日志目录，并把 `hide_on_blur` 置 false
     （否则窗口一失焦就被藏掉，阶段 B 什么都抓不到）；
  2. 不管中间发生什么（`finally`），收工时**先把应用杀干净、再整份写回备份**。
  ⚠️ 复位判据是**整份 config 逐字段 diff**，不是「我改的那个键变回来了」——
     实测 `taskkill /F` 是硬杀、应用不会在退出时回写配置，所以残留是**运行期**
     写进去的（探针点过 UI 就会改 `pinned_collapsed` / `filter_shelf_open` /
     窗口宽高），只复位一个键 = 「配置已复位」是假的。

用法：python foreground_hook_ab_driver.py [每臂轮数] [日志目录]
"""
import json
import os
import shutil
import subprocess
import sys
import time

_HERE = os.path.dirname(os.path.abspath(__file__))
_ROOT = os.path.dirname(_HERE)

# 只 exec 探针的**函数区**（`# ---- 准备` 之前），拿到 u / band / activate 这些；
# 整个 import 会连脚本主体（阶段 A/B）一起跑掉。
# ⚠️ `ns` 是**独立命名空间**，看不到本模块的全局变量 —— 探针的函数区靠 `__file__`
# 定位 `rt_probe.py`，所以必须把 `__file__` 注进去（跟真正 `import` 一样），
# 否则 exec 时 NameError: name '__file__' is not defined。
_PROBE = os.path.join(_HERE, "foreground_hook_ab.py")
src = open(_PROBE, encoding="utf-8").read()
ns = {"__file__": _PROBE, "__name__": "foreground_hook_ab"}
exec(src.split("# ---------------------------------------------------------------- 准备")[0], ns)
u = ns["u"]
band = ns["band"]
rect = ns["rect"]
activate = ns["activate"]
steal = ns["steal"]
make_thief = ns["make_thief"]
wait_clean = ns["wait_clean"]
desktop_interactive = ns["desktop_interactive"]
CLEAN = ns["CLEAN"]
rp = ns["rp"]            # 上面那段 exec 里已经加载好了，别重复加载

APP = os.path.join(_ROOT, "target", "debug", "anycast.exe")
CWD = _ROOT
# ⚠️ 别用 `os.environ["APPDATA"]` —— 从 Git Bash / 计划任务启动时它可能是**空的**
# （`std::env::var` 对空串返回 Ok("")，Windows 侧读配置也会跟着错位）。
# 与 BASE_ENV 用同一个字面量，并允许用环境变量覆盖。
ROAMING = os.environ.get("ANYCAST_APPDATA") or r"C:\Users\30130\AppData\Roaming"
CONFIG = os.path.join(ROAMING, "Anycast", "data", "config.json")
BASE_ENV = {
    **os.environ,
    "APPDATA": ROAMING,
    "LOCALAPPDATA": r"C:\Users\30130\AppData\Local",
    "ProgramData": r"C:\ProgramData",
    "PUBLIC": r"C:\Users\Public",
    "RUST_LOG": "debug",
}
ROUNDS = 12
N = int(sys.argv[1]) if len(sys.argv) > 1 else 3
LOGDIR = sys.argv[2] if len(sys.argv) > 2 else os.path.join(_HERE, "..", "target", "hook_ab")
LOGDIR = os.path.abspath(LOGDIR)
WAIT_DESKTOP = 600.0


def nudge_count(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return sum(1 for line in f if "nudge_surface" in line)
    except OSError:
        return None


def kill_app():
    # ⚠️ 别 `capture_output=True, text=True` —— 中文 Windows 上 taskkill 输出 GBK，
    # Python 默认按 UTF-8 解 → 读线程里 UnicodeDecodeError（不影响结果，但很吵）。
    subprocess.run(["taskkill", "/F", "/IM", "anycast.exe"],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def arm_config():
    """备份整份 config，并把 hide_on_blur 置 false。"""
    os.makedirs(LOGDIR, exist_ok=True)
    backup = os.path.join(LOGDIR, "config.before_hook_ab.json")
    shutil.copyfile(CONFIG, backup)
    d = json.load(open(CONFIG, encoding="utf-8"))
    d["hide_on_blur"] = False
    json.dump(d, open(CONFIG, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
    print(f"配置已备份到 {backup}，hide_on_blur → false", flush=True)
    return backup


def restore_config(backup):
    """应用必须**已经杀干净**，再整份写回。"""
    kill_app()
    time.sleep(1.0)
    shutil.copyfile(backup, CONFIG)
    same = open(backup, "rb").read() == open(CONFIG, "rb").read()
    print(f"\n配置整份写回：{'零差异 ✅' if same else '⚠ 仍有差异，请人工核对'}", flush=True)
    return same


def launch(arm, run):
    env = dict(BASE_ENV)
    env.update(arm["env"])
    logpath = os.path.join(LOGDIR, f"ab_{arm['tag']}_{run}.log")
    fh = open(logpath, "w", encoding="utf-8")
    p = subprocess.Popen([APP], cwd=CWD, env=env, stdout=fh, stderr=fh)
    return p, fh, logpath


def stop(p, fh):
    try:
        p.kill()
        p.wait(timeout=10)
    except Exception:
        pass
    try:
        fh.close()
    except Exception:
        pass
    time.sleep(0.8)


def first_steal_latency(h, thief):
    """量「本轮第一次失去激活」：整条标题栏出现 → 被擦净的耗时。"""
    if not activate(h):
        return None, "抢不到前台"
    if not wait_clean(h, 1.5):
        return None, "起点不干净"
    time.sleep(0.25)
    t0 = time.time()
    steal(thief)
    if u.GetForegroundWindow() != thief:
        return None, "没抢到前台"
    series = []
    while time.time() - t0 < 0.7:
        v = band(h)
        if v is not None:
            series.append(((time.time() - t0) * 1000, v))
    if not series:
        return None, "一次都没采到"
    ip = max(range(len(series)), key=lambda i: series[i][1])
    peak, t_peak = series[ip][1], series[ip][0]
    if peak < 50:
        return None, f"整条没出现（峰值 {peak:.0f}%）"
    for (tt, vv) in series[ip:]:
        if vv < CLEAN:
            return tt, f"峰值 {peak:.0f}%@{t_peak:.0f}ms，{len(series)} 次采样"
    return None, f"峰值 {peak:.0f}%@{t_peak:.0f}ms，700ms 内没擦净"


def main():
    backup = arm_config()
    try:
        run_arms()
    finally:
        restore_config(backup)


def run_arms():
    thief = make_thief()
    arms = [
        {"tag": "hook_on", "name": "钩子开", "env": {}},
        {"tag": "hook_off", "name": "钩子关（200ms 轮询）",
         "env": {"ANYCAST_NO_FOREGROUND_HOOK": "1"}},
    ]
    summary = {}
    for arm in arms:
        lat, nudges, notes = [], [], []
        for run in range(1, N + 1):
            print(f"\n=== {arm['name']} 第 {run}/{N} 轮 ===", flush=True)
            p, fh, logpath = launch(arm, run)
            time.sleep(6)
            if not ns["wait_for"](desktop_interactive, WAIT_DESKTOP):
                print("  ⚠ 等不到可交互桌面，跳过", flush=True)
                stop(p, fh)
                continue
            h = rp.find_window()
            if not h:
                print("  ⚠ 找不到主窗口，跳过", flush=True)
                stop(p, fh)
                continue
            if not ns["wait_for"](lambda: band(h) is not None and band(h) < CLEAN, 30.0):
                print(f"  ⚠ 窗口画面不干净（{band(h)}），跳过", flush=True)
                stop(p, fh)
                continue
            print(f"  窗口 0x{h:x} 顶部亮占比 {band(h):.1f}%", flush=True)

            # 阶段 B 先做：每个实例的**第一次失去激活**才有整条标题栏
            t, note = first_steal_latency(h, thief)
            print(f"  阶段 B（第一次失去激活）："
                  f"{'—' if t is None else f'{t:.0f} ms'}   {note}", flush=True)
            if t is not None:
                lat.append(t)
            notes.append(note)

            # 阶段 A：12 轮快速切换，数日志新增
            n0 = nudge_count(logpath)
            rounds = 0
            for i in range(ROUNDS):
                if not desktop_interactive():
                    break
                t0 = time.time()
                steal(thief)
                a = u.GetForegroundWindow() == thief
                activate(h)
                b = u.GetForegroundWindow() == h
                if not (a and b) or (time.time() - t0) * 1000 >= 200:
                    continue
                rounds += 1
            time.sleep(0.6)
            n1 = nudge_count(logpath)
            d = None if (n0 is None or n1 is None) else n1 - n0
            print(f"  阶段 A：有效轮数 {rounds}，日志新增 nudge_surface = {d}"
                  f"（钩子版应 ≈ {2*rounds}，轮询版应 ≈ 0）", flush=True)
            if d is not None:
                nudges.append((rounds, d))
            stop(p, fh)
        summary[arm["name"]] = (lat, nudges, notes)

    print("\n" + "=" * 60)
    for name, (lat, nudges, notes) in summary.items():
        print(f"\n【{name}】")
        if lat:
            s = sorted(lat)
            print(f"  擦净耗时 n={len(s)}：{['%.0f' % x for x in s]}  "
                  f"中位 {s[len(s)//2]:.0f} ms")
        else:
            print("  擦净耗时：无有效样本")
        for (r, d) in nudges:
            print(f"  阶段 A：{r} 轮 → {d} 次 nudge_surface"
                  f"（{'符合钩子版' if d >= 2*r*0.8 else '符合轮询版' if d <= max(2, r//4) else '介于两者之间'}）")
    u.DestroyWindow(thief)


main()
