"""剪贴板监听的运行时验证：开关真的控制监听吗？

做法：
  1. 置 clipboard_enabled=True，写一段唯一文本到剪贴板 -> 等 -> 数 clipboard 表
  2. 置 clipboard_enabled=False，再写一段唯一文本 -> 等 -> 再数
  3. 复位 True，清掉测试写入的行
"""
import ctypes
import ctypes.wintypes as wt
import json
import sqlite3
import sys
import time
import uuid

sys.path.insert(0, r"D:\Anycast\tools")
import rt_probe as P

CFG = r"C:\Users\30130\AppData\Roaming\Anycast\data\config.json"
DB = r"C:\Users\30130\AppData\Roaming\Anycast\data\anycast.db"

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
CF_UNICODETEXT = 13
GMEM_MOVEABLE = 0x0002

# 64 位下必须声明原型，否则 HGLOBAL 被截成 32 位 -> GlobalLock 返回 0
kernel32.GlobalAlloc.restype = ctypes.c_void_p
kernel32.GlobalAlloc.argtypes = [wt.UINT, ctypes.c_size_t]
kernel32.GlobalLock.restype = ctypes.c_void_p
kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
user32.SetClipboardData.restype = ctypes.c_void_p
user32.SetClipboardData.argtypes = [wt.UINT, ctypes.c_void_p]
user32.OpenClipboard.argtypes = [ctypes.c_void_p]


def set_clip(text):
    """把 text 写入系统剪贴板。"""
    if not user32.OpenClipboard(None):
        raise RuntimeError("OpenClipboard 失败")
    try:
        user32.EmptyClipboard()
        data = text.encode("utf-16-le") + b"\x00\x00"
        h = kernel32.GlobalAlloc(GMEM_MOVEABLE, len(data))
        p = kernel32.GlobalLock(h)
        ctypes.memmove(p, data, len(data))
        kernel32.GlobalUnlock(h)
        user32.SetClipboardData(CF_UNICODETEXT, h)
    finally:
        user32.CloseClipboard()


def clip_count():
    c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        return c.execute("select count(*) from clipboard").fetchone()[0]
    finally:
        c.close()


def cfg_enabled():
    return json.load(open(CFG, encoding="utf-8"))["clipboard_enabled"]


def toggle_ui():
    """通过设置面板的开关切换 clipboard_enabled（外部改 config 不影响已运行进程）。"""
    P.click(160, 265)      # 左侧导航：剪贴板历史
    time.sleep(0.55)
    P.click(736, 149)      # 启用剪贴板监控
    time.sleep(0.8)


def set_enabled(v):
    for _ in range(3):
        if cfg_enabled() == v:
            return True
        toggle_ui()
    return cfg_enabled() == v


def main():
    tag_on = f"ANYCAST-TEST-ON-{uuid.uuid4().hex[:8]}"
    tag_off = f"ANYCAST-TEST-OFF-{uuid.uuid4().hex[:8]}"

    # --- 1) 开关 ON，复制应被记录 ---
    set_enabled(True)
    time.sleep(0.8)
    n0 = clip_count()
    set_clip(tag_on)
    time.sleep(1.6)
    n1 = clip_count()
    print(f"[ON ] 复制前 {n0} 条 -> 复制后 {n1} 条   {'PASS 已记录' if n1 > n0 else 'FAIL 未记录'}")

    # --- 2) 开关 OFF，复制不应被记录 ---
    set_enabled(False)
    time.sleep(0.8)
    n2 = clip_count()
    set_clip(tag_off)
    time.sleep(1.6)
    n3 = clip_count()
    print(f"[OFF] 复制前 {n2} 条 -> 复制后 {n3} 条   {'PASS 未记录' if n3 == n2 else 'FAIL 仍被记录'}")

    # --- 3) 复位 + 清理测试行 ---
    set_enabled(True)
    time.sleep(0.4)
    c = sqlite3.connect(DB)
    try:
        cur = c.execute("delete from clipboard where content like 'ANYCAST-TEST-%'")
        c.commit()
        print(f"清理测试行 {cur.rowcount} 条；当前 clipboard = "
              f"{c.execute('select count(*) from clipboard').fetchone()[0]}")
    finally:
        c.close()
    print("clipboard_enabled 已复位 =", json.load(open(CFG, encoding="utf-8"))["clipboard_enabled"])


if __name__ == "__main__":
    main()
