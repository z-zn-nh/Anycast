"""验证冲突 H：热键注册失败时，用户到底看不看得见。

冲突 H 的原文主张（`doc/检索增强与判断模型接入开发文档.md` §6 Phase 0 第 2 条）：
    `save_hotkey` 保存时会 `probe_hotkey` 真去试注册，但 `set_wake_hotkey`
    **不探**；且 `win_thread.rs` 里 `RegisterHotKey` 真正失败时**只写日志**，
    用户看不到任何反馈，配置却照存 —— 界面显示「已设置」，实际不生效。

拆成两半验：
  H-1 保存时探测：`set_wake_hotkey` 要先 probe 再落盘。
  H-2 注册失败提示：`RegisterHotKey` 真失败时要有 toast。

────────────────────────────────────────────────────────────────────────
为什么 H-1 **不能**用「点键帽 + 发被占组合键」来验（重要，别再踩）
────────────────────────────────────────────────────────────────────────
    `tools/keydeliver_probe.py` 给出了决定性证据：组合键被别的进程用
    `RegisterHotKey` 占住后，发键时**修饰键照常送达，主键被系统吞掉**
    （实测 Ctrl、Shift 到达，F9 收不到）。

    而 Anycast 的录制走的是 Slint 窗口按键回调（全仓 `SetWindowsHookEx` 为空，
    没有低级键盘钩子）—— **主键收不到，录制就永远完不成**。
    于是「点键帽 → 按 Ctrl+Shift+F9 → 看有没有报错」会得到：
    配置没变（✅ 看着像通过）+ 也没有 toast（❗真正发生的事）。
    这是**假阴性**：通过的判据是「应用压根没收到键」，不是「探测拦住了」。

    更狠的推论：**被外部占用的组合键，用户根本无法在界面上录进去**，
    所以 H-1 那条「界面显示已设置、实际不生效」的路径，
    在**外部占用**这个成因下不可达。H-1 代码仍然正确（挡住竞态与自身占用），
    但它不是用户可见缺陷的主要来源 —— 主要来源是 H-2 的运行时失败。

────────────────────────────────────────────────────────────────────────
本脚本怎么做
────────────────────────────────────────────────────────────────────────
  wake 模式（默认，**有效**）：H-2 的唤醒热键那一半
      外部占住 X → 把 config.json 的 wake_hotkey 预置成 X → 起应用
      → `apply_wake_hotkey` 注册必然失败 → 应该弹 toast
    这条路径**不经过录制**，不碰那个吞键问题，判据是截图里的 toast 文案。

  record 模式：H-1 的界面路径，**带有效性守卫**
      先点键帽，比对「点前/点后」截图确认真的进了录制态；
      若主键被吞导致录制没完成，直接判「测试不成立」而不是判「通过」。

用法：
  python tools/conflict_probe.py                    # 跑 wake 模式
  python tools/conflict_probe.py --mode record      # 跑 record 模式
  python tools/conflict_probe.py --mode both
产出：默认在 %TEMP%\\anycast_conflict_probe
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

PY = sys.executable


def _roaming():
    """定位 Roaming 目录。

    ⚠️ 不要写 `os.environ.get("APPDATA") or os.path.expandvars("%APPDATA%")`：
    从脚本/Bash 启动时 `APPDATA` **可能整个不存在**（本机实测如此），
    `expandvars` 对未知变量原样保留，于是得到字面量 `%APPDATA%\\...` 的假路径。
    见 skill `windows-launch-env-gotchas`。
    """
    v = os.environ.get("APPDATA", "").strip()
    if v and os.path.isdir(v):
        return v
    return os.path.join(os.path.expanduser("~"), "AppData", "Roaming")


APPDATA = _roaming()
CONFIG = os.path.join(APPDATA, "Anycast", "data", "config.json")


def cfg_read():
    with open(CONFIG, encoding="utf-8") as f:
        return json.load(f)


def cfg_write(d):
    with open(CONFIG, "w", encoding="utf-8") as f:
        json.dump(d, f, ensure_ascii=False, indent=2)


def kill_app():
    subprocess.run(["taskkill", "/F", "/IM", "anycast.exe"], capture_output=True)
    time.sleep(0.8)


def start_app(app):
    env = dict(os.environ)
    env["APPDATA"] = APPDATA
    env["ProgramData"] = r"C:\ProgramData"
    env["PUBLIC"] = r"C:\Users\Public"
    return subprocess.Popen([os.path.abspath(app)], stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, env=env)


def wait_window(rt_probe, timeout=30.0):
    t0 = time.time()
    while time.time() - t0 < timeout:
        h = rt_probe.find_window()
        if h:
            return h
        time.sleep(0.4)
    return None


def img_diff(p1, p2):
    """两图差异像素占比。用来判断「点键帽之后界面到底变没变」。"""
    from PIL import Image
    import numpy as np
    a = np.asarray(Image.open(p1).convert("L"), dtype=np.int16)
    b = np.asarray(Image.open(p2).convert("L"), dtype=np.int16)
    if a.shape != b.shape:
        return 1.0
    return float((abs(a - b) > 12).mean())


class Holder:
    """外部进程占住一个组合键（RegisterHotKey）。"""

    def __init__(self, combo, seconds=300):
        self.combo = combo
        self.proc = subprocess.Popen(
            [PY, os.path.join(HERE, "hotkey_probe.py"), "hold", combo, str(seconds)],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)

    def verify(self):
        import hotkey_probe as hp
        time.sleep(2.5)
        mods, vk = hp.spec_of(self.combo)
        ok, err = hp.try_claim(mods, vk, "check")
        return (not ok), err

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()


# ------------------------------------------------------------------ wake 模式
def mode_wake(args, out, rt_probe):
    """H-2：唤醒热键注册失败 → 必须有 toast。"""
    print("=" * 66)
    print(f"H-2 唤醒热键注册失败提示   占用组合键 = {args.combo}")
    print("=" * 66)

    kill_app()
    orig = cfg_read()
    backup = os.path.join(out, "config_backup.json")
    with open(backup, "w", encoding="utf-8") as f:
        json.dump(orig, f, ensure_ascii=False, indent=2)
    print(f"[0] 原 wake_hotkey = {orig.get('wake_hotkey')!r}（已备份到 {backup}）")

    holder = Holder(args.combo)
    try:
        ok, err = holder.verify()
        if not ok:
            print(f"    ❌ 没能占住 {args.combo} → 测试无意义，中止")
            return 2
        print(f"    ✅ 已占住（err={err}）")

        # 预置配置：唤醒热键指向那个被占的组合键
        d = cfg_read()
        d["wake_hotkey"] = args.combo
        cfg_write(d)
        print(f"[1] 已把 wake_hotkey 预置为 {args.combo}")

        print("[2] 启动应用 …")
        app = start_app(args.app)
        try:
            hwnd = wait_window(rt_probe)
            if not hwnd:
                print("    ❌ 等不到主窗口")
                return 2
            print(f"    ✅ 主窗口 hwnd=0x{hwnd:x}")

            # toast 只活 ~2600ms，连拍覆盖启动窗口
            shots = []
            for i in range(5):
                time.sleep(1.0)
                p = os.path.join(out, f"wake_{i}.png")
                rt_probe.grab(p)
                shots.append(p)
                print(f"    抓图 {i} @ {time.time():.1f}")

            print("[3] 应用日志：")
            app.terminate()
            try:
                so, _ = app.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                app.kill()
                so = ""
            for line in (so or "").splitlines()[-12:]:
                print("    |", line)
        finally:
            if app.poll() is None:
                app.terminate()
            kill_app()

        print(f"\n    截图：{[os.path.basename(s) for s in shots]}")
        print(f"    → 请人工确认其中一张含「唤醒快捷键 … 注册失败」toast")
        return 0
    finally:
        holder.stop()
        cfg_write(orig)
        print(f"[收尾] wake_hotkey 已还原为 {orig.get('wake_hotkey')!r}")


# ---------------------------------------------------------------- record 模式
def mode_record(args, out, rt_probe):
    """H-1：界面录制路径，带有效性守卫。"""
    print("=" * 66)
    print(f"H-1 界面录制路径（带守卫）   占用组合键 = {args.combo}")
    print("=" * 66)

    kill_app()
    cx, cy = (int(v) for v in args.click.split(","))
    orig = cfg_read()
    backup = os.path.join(out, "config_backup.json")
    with open(backup, "w", encoding="utf-8") as f:
        json.dump(orig, f, ensure_ascii=False, indent=2)
    print(f"[0] 原 wake_hotkey = {orig.get('wake_hotkey')!r}")

    import hotkey_probe as hp
    holder = Holder(args.combo)
    try:
        ok, err = holder.verify()
        if not ok:
            print(f"    ❌ 没能占住 {args.combo} → 测试无意义，中止")
            return 2
        print(f"    ✅ 已占住（err={err}）")

        app = start_app(args.app)
        try:
            hwnd = wait_window(rt_probe)
            if not hwnd:
                print("    ❌ 等不到主窗口")
                return 2
            print(f"[1] 主窗口 hwnd=0x{hwnd:x}")
            time.sleep(2.0)
            before = cfg_read()

            print("[2] Ctrl+, 打开设置 …")
            rt_probe.keys("ctrl+,")
            time.sleep(1.3)
            p1 = os.path.join(out, "rec_1_settings.png")
            rt_probe.grab(p1)

            print(f"[3] 点键帽 ({cx},{cy}) 进入录制 …")
            rt_probe.click(cx, cy)
            time.sleep(1.0)
            p2 = os.path.join(out, "rec_2_recording.png")
            rt_probe.grab(p2)

            # ---- 守卫：界面真的变了吗 ----
            d = img_diff(p1, p2)
            entered = d > 0.002
            print(f"    界面变化像素占比 = {d:.4f} → {'已进入录制态' if entered else '❌ 没进录制态（点击没打中）'}")
            if not entered:
                print("    ⚠ 判定不成立：点键帽这一步就失败了，后面的结论没意义")
                return 2

            print(f"[4] 发送 {args.combo} …")
            hp.send_combo_chord(args.combo)
            time.sleep(1.2)
            p3 = os.path.join(out, "rec_3_result.png")
            rt_probe.grab(p3)

            d3 = img_diff(p2, p3)
            after = cfg_read()
            b, a = before.get("wake_hotkey"), after.get("wake_hotkey")
            print(f"    发键后界面变化 = {d3:.4f}")

            print("\n── 判定 " + "─" * 44)
            if not entered:
                return 2
            if d3 < 0.002:
                print("⚠ 发键后界面**毫无变化** → 主键被系统吞了，应用没收到键。")
                print("  ⇒ 这条 UI 测试**不成立**（假阴性），不能据此判通过。")
                print("  依据见 tools/keydeliver_probe.py：被占组合键的主键不会送达窗口。")
                return 3
            if a == b:
                print(f"✅ wake_hotkey 未被改动（仍 {b!r}）")
            else:
                print(f"❌ wake_hotkey 被改成 {a!r}（原 {b!r}）→ 保存时探测没拦住")
            return 0 if a == b else 1
        finally:
            kill_app()
    finally:
        holder.stop()
        cfg_write(orig)
        print(f"[收尾] wake_hotkey 已还原为 {orig.get('wake_hotkey')!r}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", choices=["wake", "record", "both"], default="wake")
    ap.add_argument("--combo", default="Ctrl+Shift+F10")
    ap.add_argument("--click", default="712,148", help="唤醒热键键帽的窗口内逻辑坐标")
    ap.add_argument("--out", default=os.path.join(
        os.environ.get("TEMP", "."), "anycast_conflict_probe"))
    ap.add_argument("--app", default=os.path.join(HERE, "..", "target", "debug", "anycast.exe"))
    args = ap.parse_args()

    out = os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)

    import rt_probe

    rc = 0
    if args.mode in ("wake", "both"):
        rc = mode_wake(args, out, rt_probe) or rc
    if args.mode in ("record", "both"):
        rc = mode_record(args, out, rt_probe) or rc
    print(f"\n总退出码 {rc}；产出目录 {out}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
