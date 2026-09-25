"""抓主屏底部「任务栏」条带并保存为 PNG，用于任务栏图标的 A/B 取证。

为什么不用窗口截图：任务栏是系统窗口，不属于目标进程；且窗口截图受遮挡与焦点影响。
这里直接 BitBlt 桌面 DC 的底部条带，不抢焦点、不受遮挡影响。

⚠️ 本机任务栏是**自动隐藏**的（工作区 == 整屏，`SPI_GETWORKAREA` 无预留高度）。
此时底部条带平时只是桌面内容，必须先 `--reveal` 把光标移到屏幕底边把任务栏唤出来。
`SetCursorPos` 不改变前台窗口，因此不会触发被测程序的 `hide_on_blur`。

用法：
    python tools/taskbar_shot.py out.png [条带高度，默认 110] [--reveal]
"""
import ctypes
import struct
import sys
import time
import zlib


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", ctypes.c_ulong), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
        ("biPlanes", ctypes.c_ushort), ("biBitCount", ctypes.c_ushort), ("biCompression", ctypes.c_ulong),
        ("biSizeImage", ctypes.c_ulong), ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long), ("biClrUsed", ctypes.c_ulong),
        ("biClrImportant", ctypes.c_ulong),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", ctypes.c_ulong * 3)]


def write_png(path, w, h, bgra):
    """bgra: top-down 的 BGRA 字节。手写 PNG，不依赖 Pillow。"""
    raw = bytearray()
    stride = w * 4
    for y in range(h):
        raw.append(0)  # filter type 0
        row = bytearray(bgra[y * stride:(y + 1) * stride])
        out = bytearray(len(row))
        out[0::4] = row[2::4]
        out[1::4] = row[1::4]
        out[2::4] = row[0::4]
        out[3::4] = row[3::4]
        raw += out

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 6))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    reveal = "--reveal" in sys.argv
    out = args[0] if args else "taskbar.png"
    strip = int(args[1]) if len(args) > 1 else 110

    user32 = ctypes.windll.user32
    gdi32 = ctypes.windll.gdi32

    user32.GetDC.restype = ctypes.c_void_p
    user32.GetDC.argtypes = [ctypes.c_void_p]
    gdi32.CreateCompatibleDC.restype = ctypes.c_void_p
    gdi32.CreateCompatibleDC.argtypes = [ctypes.c_void_p]
    gdi32.CreateCompatibleBitmap.restype = ctypes.c_void_p
    gdi32.CreateCompatibleBitmap.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    gdi32.SelectObject.restype = ctypes.c_void_p
    gdi32.SelectObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    gdi32.BitBlt.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                             ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    gdi32.GetDIBits.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint,
                                ctypes.c_void_p, ctypes.POINTER(BITMAPINFO), ctypes.c_uint]
    gdi32.DeleteObject.argtypes = [ctypes.c_void_p]
    gdi32.DeleteDC.argtypes = [ctypes.c_void_p]
    user32.ReleaseDC.argtypes = [ctypes.c_void_p, ctypes.c_void_p]

    user32.SetProcessDPIAware()
    w = user32.GetSystemMetrics(0)
    h = user32.GetSystemMetrics(1)
    y0 = max(0, h - strip)

    if reveal:
        # 把光标移到屏幕底边中央，等任务栏滑出（自动隐藏模式下必需）
        prev = ctypes.wintypes.POINT() if hasattr(ctypes, "wintypes") else None
        import ctypes.wintypes as wt
        prev = wt.POINT()
        user32.GetCursorPos(ctypes.byref(prev))
        user32.SetCursorPos(w // 2, h - 1)
        time.sleep(0.6)
        print(f"已唤起任务栏（光标 {prev.x},{prev.y} -> {w // 2},{h - 1}）")

    hdc = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, strip)
    gdi32.SelectObject(mem, bmp)
    SRCCOPY = 0x00CC0020
    gdi32.BitBlt(mem, 0, 0, w, strip, hdc, 0, y0, SRCCOPY)

    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = w
    bi.bmiHeader.biHeight = -strip  # 负值 = top-down
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    bi.bmiHeader.biCompression = 0  # BI_RGB
    buf = ctypes.create_string_buffer(w * strip * 4)
    gdi32.GetDIBits(mem, bmp, 0, strip, buf, ctypes.byref(bi), 0)

    write_png(out, w, strip, buf.raw)
    print(f"已保存 {out}  ({w}x{strip} @ y={y0})")

    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, hdc)


main()
