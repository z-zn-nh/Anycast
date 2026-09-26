"""读写 Windows 剪贴板（CF_UNICODETEXT），用于验证「双击条目 = 复制」这类行为。

为什么要单独写：内联 ctypes 很容易漏 `argtypes`/`restype`，
HGLOBAL 是**指针宽度**的句柄，按默认 `c_int` 传会在 64 位上被截断 → 直接段错误
（实测过：`GlobalLock` 不加 argtypes 必崩，且崩在 Python 里看不出原因）。

用法：
    python tools/clip_probe.py get
    python tools/clip_probe.py set <text>
"""
import ctypes
import sys

u = ctypes.windll.user32
k = ctypes.windll.kernel32

CF_UNICODETEXT = 13
GMEM_MOVEABLE = 0x0002

k.GlobalLock.argtypes = [ctypes.c_void_p]
k.GlobalLock.restype = ctypes.c_void_p
k.GlobalUnlock.argtypes = [ctypes.c_void_p]
k.GlobalAlloc.argtypes = [ctypes.c_uint, ctypes.c_size_t]
k.GlobalAlloc.restype = ctypes.c_void_p
u.GetClipboardData.argtypes = [ctypes.c_uint]
u.GetClipboardData.restype = ctypes.c_void_p
u.SetClipboardData.argtypes = [ctypes.c_uint, ctypes.c_void_p]
u.SetClipboardData.restype = ctypes.c_void_p


def get_text():
    if not u.OpenClipboard(None):
        raise SystemExit("ERR: OpenClipboard 失败（别的程序正占着剪贴板）")
    try:
        h = u.GetClipboardData(CF_UNICODETEXT)
        if not h:
            return None
        p = k.GlobalLock(h)
        if not p:
            return None
        try:
            return ctypes.c_wchar_p(p).value
        finally:
            k.GlobalUnlock(h)
    finally:
        u.CloseClipboard()


def set_text(text):
    data = text.encode("utf-16-le") + b"\x00\x00"
    h = k.GlobalAlloc(GMEM_MOVEABLE, len(data))
    if not h:
        raise SystemExit("ERR: GlobalAlloc 失败")
    p = k.GlobalLock(h)
    ctypes.memmove(p, data, len(data))
    k.GlobalUnlock(h)
    if not u.OpenClipboard(None):
        raise SystemExit("ERR: OpenClipboard 失败")
    try:
        u.EmptyClipboard()
        if not u.SetClipboardData(CF_UNICODETEXT, h):
            raise SystemExit("ERR: SetClipboardData 失败")
    finally:
        u.CloseClipboard()


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "get"
    if cmd == "get":
        print(repr(get_text()))
    elif cmd == "set":
        set_text(sys.argv[2])
        print("ok")
    else:
        raise SystemExit(__doc__)
