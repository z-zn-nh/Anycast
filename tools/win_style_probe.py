"""读取 Anycast 主窗口的两层窗口样式位（GWL_STYLE / GWL_EXSTYLE）。

用途：验证「一次 hide() → show() 之后，winit 是否把窗口属性重置回默认值」。
样式位不是视觉判断 —— 读位比截图可靠，但**必须同时读 IsWindowVisible**：
样式位正确而窗口是隐藏的，就是假阳性。

关心的位：
    GWL_STYLE   WS_CAPTION / WS_SYSMENU  → 出现即 DWM 会在客户区上画 — □ ×
                WS_THICKFRAME            → DWM 材质准入所需，应常 ON
    GWL_EXSTYLE WS_EX_TOOLWINDOW         → 应常 ON（退出任务栏 / Alt+Tab）
                WS_EX_APPWINDOW          → 应常 OFF

用法：
    python tools/win_style_probe.py                     # 读一次
    python tools/win_style_probe.py --send Ctrl+Shift+Space   # 发键，再读
    python tools/win_style_probe.py --label 隐藏后      # 给输出加标注
"""
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

user32 = ctypes.windll.user32
user32.SetProcessDPIAware()

GWL_STYLE, GWL_EXSTYLE = -16, -20
WINDOW_TITLE = "Anycast"

STYLE_BITS = [
    ("WS_CAPTION", 0x00C00000, False),   # 期望 OFF
    ("WS_SYSMENU", 0x00080000, False),   # 期望 OFF
    ("WS_THICKFRAME", 0x00040000, True),  # 期望 ON
]
EX_BITS = [
    ("WS_EX_TOOLWINDOW", 0x00000080, True),   # 期望 ON
    ("WS_EX_APPWINDOW", 0x00040000, False),  # 期望 OFF
]

user32.GetWindowLongW.argtypes = [wt.HWND, ctypes.c_int]
user32.GetWindowLongW.restype = wt.LONG
user32.IsWindowVisible.argtypes = [wt.HWND]
user32.IsWindowVisible.restype = wt.BOOL
user32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.restype = ctypes.c_int


def enum_anycast():
    """标题为 Anycast 的顶层窗口，按面积降序（主窗口最大，隐藏 helper 只有 353x39）。"""
    out = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def proc(hwnd, _):
        buf = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(hwnd, buf, 512)
        if buf.value.strip() == WINDOW_TITLE:
            r = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(r))
            out.append(((r.right - r.left) * (r.bottom - r.top), hwnd))
        return True

    user32.EnumWindows(proc, 0)
    out.sort(reverse=True)
    return [h for _, h in out]


def read(hwnd):
    style = user32.GetWindowLongW(hwnd, GWL_STYLE) & 0xFFFFFFFF
    ex = user32.GetWindowLongW(hwnd, GWL_EXSTYLE) & 0xFFFFFFFF
    return {
        "visible": bool(user32.IsWindowVisible(hwnd)),
        "style": style,
        "ex": ex,
    }


def report(tag, s):
    print(f"[{tag}]  visible={s['visible']}  style=0x{s['style']:08X}  exstyle=0x{s['ex']:08X}")
    bad = []
    for name, bit, want in STYLE_BITS:
        got = bool(s["style"] & bit)
        ok = got == want
        print(f"    {name:<18}{'ON ' if got else 'OFF'}  期望{'ON ' if want else 'OFF'}"
              f"  {'✓' if ok else '✗ 偏离'}")
        if not ok:
            bad.append(name)
    for name, bit, want in EX_BITS:
        got = bool(s["ex"] & bit)
        ok = got == want
        print(f"    {name:<18}{'ON ' if got else 'OFF'}  期望{'ON ' if want else 'OFF'}"
              f"  {'✓' if ok else '✗ 偏离'}")
        if not ok:
            bad.append(name)
    return bad


def main():
    argv = sys.argv[1:]
    label = "当前"
    combo = None
    i = 0
    while i < len(argv):
        if argv[i] == "--label":
            label = argv[i + 1]
            i += 2
        elif argv[i] == "--send":
            combo = argv[i + 1]
            i += 2
        else:
            i += 1

    wins = enum_anycast()
    if not wins:
        raise SystemExit("ERR: 找不到标题为 Anycast 的顶层窗口（应用没跑？）")
    hwnd = wins[0]
    print(f"HWND 0x{hwnd:08X}（共 {len(wins)} 个 Anycast 顶层窗口，取面积最大者）")

    before = read(hwnd)
    bad_before = report(label, before)

    if combo:
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import hotkey_probe
        print(f"\n→ 发送 {combo}（不碰前台）")
        hotkey_probe.send_combo(combo)
        time.sleep(1.2)
        after = read(hwnd)
        bad_after = report(f"{label} → 发键后", after)
        print()
        # 结论只说「这次发键干了什么」，不猜 hide 还是 show ——
        # 脚本不读窗口状态机，硬猜会给出误导性结论。
        if not bad_before and not bad_after:
            print("结论：两次都符合期望 → 本次未复现样式丢失")
        elif bad_before and not bad_after:
            print("结论：发键后偏离消失 → 本次发键触发了样式校正")
            print("      若这次发键是「唤出」，说明 show 路径的校正生效（= 修复有效）")
        elif not bad_before and bad_after:
            print("结论：发键后新出现偏离 → 本次发键把样式弄丢了")
            print("      若这次发键是「隐藏」，属预期：hide 路径不校正，窗口反正不可见")
        else:
            print("结论：发键前后都偏离 → 偏离在本次发键之前就存在，与它无关")


if __name__ == "__main__":
    main()
