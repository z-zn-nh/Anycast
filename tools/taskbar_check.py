"""检查 Anycast 主窗口的 GWL_EXSTYLE，确认是否已退出任务栏 / Alt+Tab。

用法：先启动 target/debug/anycast.exe，再运行本脚本。
判定：WS_EX_TOOLWINDOW 置位且 WS_EX_APPWINDOW 清零 => 任务栏与 Alt+Tab 都不显示。
"""
import ctypes
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)

GWL_EXSTYLE = -20
GWL_STYLE = -16
WS_EX_TOOLWINDOW = 0x00000080
WS_EX_APPWINDOW = 0x00040000
WS_THICKFRAME = 0x00040000

user32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
user32.GetWindowLongW.restype = wintypes.LONG
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.IsWindowVisible.restype = wintypes.BOOL
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.restype = ctypes.c_int

hits = []


@ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
def enum_proc(hwnd, _lparam):
    buf = ctypes.create_unicode_buffer(512)
    user32.GetWindowTextW(hwnd, buf, 512)
    if "Anycast" in buf.value:
        hits.append(
            (
                hwnd,
                buf.value,
                user32.GetWindowLongW(hwnd, GWL_STYLE) & 0xFFFFFFFF,
                user32.GetWindowLongW(hwnd, GWL_EXSTYLE) & 0xFFFFFFFF,
                bool(user32.IsWindowVisible(hwnd)),
            )
        )
    return True


user32.EnumWindows(enum_proc, 0)

if not hits:
    print("未找到标题含 Anycast 的顶层窗口（程序没在跑？）")
else:
    for hwnd, title, style, ex, visible in hits:
        tool = bool(ex & WS_EX_TOOLWINDOW)
        app = bool(ex & WS_EX_APPWINDOW)
        print(f"HWND 0x{hwnd:08X}  标题={title!r}  可见={visible}")
        print(f"  GWL_STYLE   0x{style:08X}  WS_THICKFRAME={'ON' if style & WS_THICKFRAME else 'off'}")
        print(f"  GWL_EXSTYLE 0x{ex:08X}  WS_EX_TOOLWINDOW={'ON' if tool else 'off'}  WS_EX_APPWINDOW={'ON' if app else 'off'}")
        if tool and not app:
            print("  => 已退出任务栏与 Alt+Tab")
        else:
            print("  => 仍在任务栏（修复未生效或设了 ANYCAST_KEEP_TASKBAR=1）")
