"""A/B：前台变化钩子（SetWinEventHook(EVENT_SYSTEM_FOREGROUND)）vs 200ms 轮询。

只换**一个**变量：进程启动时有没有 `ANYCAST_NO_FOREGROUND_HOOK=1`。
（配套驱动脚本 `tools/foreground_hook_ab_driver.py`：两臂各 N 轮、每轮重启应用。）

两段判据 —— 本机桌面会**每隔一两秒闪一下锁屏**，所以整轮必须能在极短窗口里跑完：

  阶段 A —— **钩子到底有没有即时送达**（最硬的证据，**完全不抓图**）
    一轮「抢走 → 抢回」压在 200ms 之内做完，数应用日志里的 `nudge_surface`：
      钩子版 → 每个方向各一次，N 轮就是 2N 次
      轮询版 → 下一次 tick 只看到「前后都是前台」→ 大约 0 次
    不看毫秒、不抓图，所以不怕窗口只有一两秒。

  阶段 B —— **擦净耗时分布**（用户可见的那个数字）
    抢走前台后尽快连采顶部亮像素占比，记「第一次变干净」的耗时。
    单采一次没有意义：轮询版耗时在 0~200ms 上近似均匀，必须看分布。
    ⚠️ 这一段要抓图，需要**连续几秒**可抓画面，本机不一定给得出。
    ⚠️ 「整条」标题栏**只在每轮第一次失去激活**时出现（同轮再切换只画窄的 ~12%），
       所以阶段 B 的样本数 = 轮数，天然凑不出分布 —— 主判据是阶段 A。

用法：python foreground_hook_ab.py [等待桌面可交互的秒数] [应用日志路径]

⚠️ 桌面锁屏时全部作废（不是「没生效」）：`OpenInputDesktop()` 返回 0、
`GetForegroundWindow()` 返回 0x0、抓图抛 `screen grab failed` / 返回 None。
本脚本每轮前复查，锁了就立刻收工并如实报告。
"""
import ctypes
import ctypes.wintypes as wt
import importlib.util
import os
import sys
import time

u = ctypes.windll.user32
k = ctypes.windll.kernel32
u.SetProcessDPIAware()

_HERE = os.path.dirname(os.path.abspath(__file__))

spec = importlib.util.spec_from_file_location("rp", os.path.join(_HERE, "rt_probe.py"))
rp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rp)

WAKE = "ctrl+alt+q"
# 阈值取 5：干净态实测 0.0~0.7%，「抢回激活」那条**窄的** ~10~12%，整条 ~74~99%。
# 取 15 会把窄的那条当成干净 → 起点没擦净就开始量，而且「一看到干净就退出」会
# 在第一采样（~25ms，标题栏还没画出来）就退出。取 5 才能把三者分开。
CLEAN = 5.0
ROUNDS = 12           # 阶段 A 的快速轮数
TRIALS = 8            # 阶段 B 的样本数
WAIT_DESKTOP = float(sys.argv[1]) if len(sys.argv) > 1 else 150.0
# 应用日志路径（给了就自己数 nudge_surface，省得靠外层猜）
LOG = sys.argv[2] if len(sys.argv) > 2 else None


def nudge_count():
    if not LOG:
        return None
    try:
        with open(LOG, encoding="utf-8", errors="replace") as f:
            return sum(1 for line in f if "nudge_surface" in line)
    except OSError:
        return None

u.GetTopWindow.argtypes = [wt.HWND]
u.GetTopWindow.restype = wt.HWND
u.GetWindow.argtypes = [wt.HWND, wt.UINT]
u.GetWindow.restype = wt.HWND
u.CreateWindowExW.restype = wt.HWND
u.CreateWindowExW.argtypes = [wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
                              ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                              wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p]


def rect(h):
    x = wt.RECT()
    u.GetWindowRect(h, ctypes.byref(x))
    return (x.left, x.top, x.right, x.bottom)


def desktop_interactive():
    """轻判据：**不抓图**。阶段 A 只需要这个。"""
    d = u.OpenInputDesktop(0, False, 0x0001)
    if not d:
        return False
    u.CloseDesktop(d)
    return u.GetForegroundWindow() != 0


def grabbable(h):
    """重判据：真抓得到画面才算数。阶段 B 需要。"""
    return desktop_interactive() and band(h) is not None


def wait_for(pred, limit):
    t0 = time.time()
    while time.time() - t0 < limit:
        if pred():
            return True
        time.sleep(0.35)
    return False


def activate(h):
    """把前台交给**另一个进程**的窗口。

    ⚠️ 这比「把自己的窗口置前」难得多：后者永远允许，前者受前台锁限制。
    实测在桌面锁屏切换的缝隙里，`steal`（本进程窗口）100% 成功，
    而这一步会连续失败 —— 于是必须挂上**目标进程**的输入队列再试，并且重试。
    """
    tme = k.GetCurrentThreadId()
    t_app = u.GetWindowThreadProcessId(h, None)
    for _ in range(6):
        if u.GetForegroundWindow() == h:
            return True
        fg = u.GetForegroundWindow()
        tfg = u.GetWindowThreadProcessId(fg, None) if fg else 0
        attached = []
        for t in (tfg, t_app):
            if t and t != tme and t not in attached:
                u.AttachThreadInput(tme, t, True)
                attached.append(t)
        u.ShowWindow(h, 5)                 # SW_SHOW
        u.BringWindowToTop(h)
        u.SetForegroundWindow(h)
        for t in attached:
            u.AttachThreadInput(tme, t, False)
        if u.GetForegroundWindow() == h:
            return True
        time.sleep(0.03)
    return False


def make_thief():
    """8x8 的 STATIC 窗口，放屏幕左上角：故意做小，保证不可能压在取样带上。"""
    hinst = k.GetModuleHandleW(None)
    w = u.CreateWindowExW(0, "STATIC", "FocusThief", 0x00CF0000,
                          0, 0, 8, 8, None, None, hinst, None)
    if not w:
        raise SystemExit(f"建窗口失败 err={ctypes.get_last_error()}")
    u.ShowWindow(w, 5)
    return w


def steal(w):
    fg = u.GetForegroundWindow()
    tfg = u.GetWindowThreadProcessId(fg, None) if fg else 0
    tme = k.GetCurrentThreadId()
    if tfg and tfg != tme:
        u.AttachThreadInput(tme, tfg, True)
    u.BringWindowToTop(w)
    u.SetForegroundWindow(w)
    if tfg and tfg != tme:
        u.AttachThreadInput(tme, tfg, False)


_gdi = ctypes.windll.gdi32
_gdi.GetPixel.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
_gdi.GetPixel.restype = ctypes.c_ulong
u.GetDC.argtypes = [ctypes.c_void_p]
u.GetDC.restype = ctypes.c_void_p
u.ReleaseDC.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
_gdi.CreateCompatibleDC.argtypes = [ctypes.c_void_p]
_gdi.CreateCompatibleDC.restype = ctypes.c_void_p
_gdi.SelectObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
_gdi.SelectObject.restype = ctypes.c_void_p
_gdi.DeleteObject.argtypes = [ctypes.c_void_p]
_gdi.DeleteDC.argtypes = [ctypes.c_void_p]
_gdi.BitBlt.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                        ctypes.c_void_p, ctypes.c_int, ctypes.c_int, wt.DWORD]
_gdi.BitBlt.restype = wt.BOOL

SRCCOPY = 0x00CC0020


class _BMI(ctypes.Structure):
    _fields_ = [("biSize", wt.DWORD), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
                ("biPlanes", wt.WORD), ("biBitCount", wt.WORD), ("biCompression", wt.DWORD),
                ("biSizeImage", wt.DWORD), ("biXPelsPerMeter", ctypes.c_long),
                ("biYPelsPerMeter", ctypes.c_long), ("biClrUsed", wt.DWORD),
                ("biClrImportant", wt.DWORD)]


_gdi.CreateDIBSection.argtypes = [ctypes.c_void_p, ctypes.POINTER(_BMI), wt.UINT,
                                  ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p, wt.DWORD]
_gdi.CreateDIBSection.restype = ctypes.c_void_p

# 自己管一块 32bpp DIB，每帧只做一次 BitBlt 再稀疏读点。
# 为什么要自己写：`ImageGrab.grab` 一次 65~90ms，`GetPixel` 在屏幕 DC 上一次要 6ms
# 且要来回 120 次（实测一次采样 ~750ms）—— 两者都跟被测效应（~100ms）同量级或更差，
# 量出来的只会是「第一次采样落在什么时候」。自建 BitBlt 实测 **6ms 一次**（与面积无关），
# 时间分辨率才够。
_screen_dc = None
_mem_dc = None
_dib = None
_bits = None
_dib_w = _dib_h = 0


def _ensure_dib(w, h):
    global _screen_dc, _mem_dc, _dib, _bits, _dib_w, _dib_h
    if _dib and _dib_w == w and _dib_h == h:
        return True
    if _dib:
        _gdi.DeleteObject(_dib)
        _gdi.DeleteDC(_mem_dc)
        _dib = _mem_dc = None
    if not _screen_dc:
        _screen_dc = u.GetDC(None)
        if not _screen_dc:
            return False
    _mem_dc = _gdi.CreateCompatibleDC(_screen_dc)
    if not _mem_dc:
        return False
    bmi = _BMI()
    bmi.biSize = ctypes.sizeof(_BMI)
    bmi.biWidth = w
    bmi.biHeight = -h                 # 负高度 = 自上而下
    bmi.biPlanes = 1
    bmi.biBitCount = 32
    bmi.biCompression = 0
    bits = ctypes.c_void_p()
    _dib = _gdi.CreateDIBSection(_screen_dc, ctypes.byref(bmi), 0, ctypes.byref(bits), None, 0)
    if not _dib:
        return False
    _gdi.SelectObject(_mem_dc, _dib)
    _bits = ctypes.cast(bits, ctypes.POINTER(ctypes.c_uint32))
    _dib_w, _dib_h = w, h
    return True


def band(h, ys=(8, 16, 24, 32), n=30):
    """窗口顶部带子里「亮度 >150」的稀疏采样点占比（%）。一次 ~7ms。

    判据沿用 §12.5.4：干净态 ~0.3%，整条标题栏 ~99%，「抢回激活」那条窄的 ~12%。
    所以 `CLEAN=5` 才把三者分开 —— 取 15 只认**整条**，会把窄的那条误当干净。
    """
    if not u.IsWindowVisible(h):
        return None
    l, t, r, b = rect(h)
    w = r - l
    if w <= 0 or b - t <= 0:
        return None
    if not _ensure_dib(w, 36):
        return None
    if not _gdi.BitBlt(_mem_dc, 0, 0, w, 36, _screen_dc, l, t, SRCCOPY):
        return None
    bright = total = 0
    for y in ys:
        row = y * w
        for i in range(n):
            x = (2 * i + 1) * w // (2 * n)
            c = _bits[row + x]
            rr, gg, bb = c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF
            if 0.299 * rr + 0.587 * gg + 0.114 * bb > 150:
                bright += 1
            total += 1
    return bright / total * 100


def wait_clean(h, limit=1.2):
    t0 = time.time()
    while time.time() - t0 < limit:
        v = band(h)
        if v is not None and v < CLEAN:
            return True
    return False


# ---------------------------------------------------------------- 准备
h = rp.find_window()
if not h:
    print("⚠ 找不到 Anycast 主窗口 —— 应用没在跑？")
    raise SystemExit(2)
print(f"Anycast hwnd=0x{h:x}  rect={rect(h)}  IsWindowVisible={bool(u.IsWindowVisible(h))}")
print(f"（`hide_on_blur` 已临时关掉，窗口会一直留在屏幕上；应用开机就显示窗口，"
      f"只有 `--silent` 才不显示）")

thief = make_thief()

# ---------------------------------------------------------------- 阶段 A
print(f"\n阶段 A：{ROUNDS} 轮「抢走→抢回」，每轮压进 200ms；数日志里的 nudge_surface")
print("（**不抓图**，所以能在极短的解锁窗口里跑完）")

# 先做一次**能力探测**：抢走+抢回都真的成功，才认为这个桌面「能配合」。
# 不做这一步的话，桌面处在锁屏切换的缝隙里时 `desktop_interactive()` 会假通过，
# 然后把 12 轮全烧光（实测过：12 轮全「切换没做成」）。
# 探测本身也会改前台、也会产生 nudge，所以计数起点放在探测**之后**。
def can_switch():
    steal(thief)
    a = u.GetForegroundWindow() == thief
    activate(h)
    return a and u.GetForegroundWindow() == h


if not wait_for(lambda: desktop_interactive() and can_switch(), WAIT_DESKTOP):
    print(f"⚠ 等了 {WAIT_DESKTOP:.0f}s，桌面始终没法完成一次「抢走+抢回」——"
          f"环境不允许，本轮作废（是「测不了」，不是「没生效」）")
    raise SystemExit(3)

n0 = nudge_count()
rounds = 0
too_slow = 0
for i in range(ROUNDS):
    if not desktop_interactive():
        print("  （桌面锁了，提前收工）")
        break
    t0 = time.time()
    steal(thief)
    a = u.GetForegroundWindow() == thief
    activate(h)
    b = u.GetForegroundWindow() == h
    dt = (time.time() - t0) * 1000
    if not (a and b):
        print(f"  第 {i+1:2d} 轮：切换没做成（抢走={a} 抢回={b}），作废")
        continue
    if dt >= 200:
        too_slow += 1
    rounds += 1
    print(f"  第 {i+1:2d} 轮：抢走+抢回共 {dt:4.0f} ms"
          f"{'  ← 超过 200ms，判据对这一轮不成立' if dt >= 200 else ''}")
time.sleep(0.6)          # 给日志落盘留点时间
n1 = nudge_count()
print(f"  → 有效轮数 {rounds}（其中 {too_slow} 轮超过 200ms）")
if n1 is not None and n0 is not None:
    print(f"  → **日志新增 nudge_surface = {n1 - n0}**"
          f"（钩子版应 ≈ {2 * (rounds - too_slow)}；轮询版应 ≈ 0）")
else:
    print(f"  → 没给日志路径，无法自动计数；请人工核对（预期 ≈ {2 * (rounds - too_slow)}）")

# ---------------------------------------------------------------- 阶段 B
print("\n阶段 B：擦净耗时分布（要连续几秒抓得到画面，本机不一定给得出）")
results = []
if wait_for(lambda: grabbable(h), min(WAIT_DESKTOP, 60.0)):
    for i in range(TRIALS):
        if not grabbable(h):
            print("  （画面又抓不到了，提前收工）")
            break
        if not activate(h) or not wait_clean(h):
            print(f"  第 {i+1} 轮：起点不干净，跳过")
            continue
        time.sleep(0.25)
        t0 = time.time()
        steal(thief)
        if u.GetForegroundWindow() != thief:
            print(f"  第 {i+1} 轮：没抢到前台，作废")
            continue
        # ⚠️ **不要**「一看到干净就退出」：第一采样（~25ms）往往落在标题栏画出来
        # **之前**，那时读数本来就低 —— 提前退出等于永远等不到标题栏。
        # 固定采 700ms 记整条时间序列，再回放找峰值与擦净时刻。
        series = []
        while time.time() - t0 < 0.7:
            v = band(h)
            if v is not None:
                series.append(((time.time() - t0) * 1000, v))
        if not series:
            print(f"  第 {i+1} 轮：一次都没采到，作废")
            continue
        ip = max(range(len(series)), key=lambda i: series[i][1])
        peak, t_peak = series[ip][1], series[ip][0]
        erase = None
        if peak >= 50:                       # 只有「整条」才算真出现过（窄的那条 ~12%）
            for (tt, vv) in series[ip:]:
                if vv < CLEAN:
                    erase = tt
                    break
        results.append((erase, len(series), peak, t_peak))
        print(f"  第 {i+1:2d} 轮：采样 {len(series):3d} 次   峰值 {peak:5.1f}%（t={t_peak:4.0f}ms）"
              f"   擦净 {'—（700ms 内没干净）' if erase is None else f'{erase:5.0f} ms'}"
              f"{'   ← 整条标题栏没出现，这轮不算' if peak < 50 else ''}")
else:
    print("  ⚠ 60s 内没能连续抓到画面 —— 这一段本机做不了，如实记为「未验证」")

valid = [(e, n) for (e, n, pk, _tp) in results if pk >= 50]
ok = [e for (e, n) in valid if e is not None]
print(f"\n阶段 B 样本 {len(results)}：**整条标题栏真的出现过**的 {len(valid)} 轮，"
      f"其中擦净 {len(ok)} / {len(valid)}")
if ok:
    s = sorted(ok)
    print(f"  最快 {s[0]:.0f} ms   中位 {s[len(s)//2]:.0f} ms   最慢 {s[-1]:.0f} ms")
    print(f"  ≤60ms：{sum(1 for x in ok if x <= 60)}/{len(valid)}")
if valid:
    print(f"  采样次数均值 {sum(n for (_, n) in valid)/len(valid):.0f}"
          f"（每次 = 1 次 BitBlt + 120 个稀疏采样点，约 7ms）")
u.DestroyWindow(thief)
