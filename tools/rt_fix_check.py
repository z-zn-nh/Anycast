"""修复后的回归验证：
  1) 索引目录留空 -> 应被拒绝、配置不被清空
  2) 三个「有后端副作用但原本无反馈」的开关 -> 应出现 toast
  3) hotkeys_master_enabled -> toast（原有）
"""
import json
import sys
import time

sys.path.insert(0, r"D:\Anycast\tools")
import rt_probe as P

CFG = r"C:\Users\30130\AppData\Roaming\Anycast\data\config.json"
V = r"D:\Anycast\target\verify"


def cfg(k):
    return json.load(open(CFG, encoding="utf-8"))[k]


def clear_field(x, y, n=190):
    P.click(x, y)
    time.sleep(0.5)
    P.keys("end")
    time.sleep(0.25)
    for _ in range(n):
        P.keys("back")
    time.sleep(0.3)


def main():
    P.keys("ctrl+,")
    time.sleep(1.0)

    # ---------- 1. 空串保存索引目录 ----------
    orig = cfg("index_roots")
    P.click(160, 223); time.sleep(0.7)
    clear_field(450, 432)
    P.click(745, 432); time.sleep(0.8)
    P.grab(f"{V}/fix_empty_save.png")
    now = cfg("index_roots")
    print(f"[1] 空串保存 -> 条数 {len(now)}  未被清空: {now == orig}  "
          f"{'PASS' if now == orig else 'FAIL'}")

    # ---------- 2. 实时增量索引 ----------
    P.click(160, 223); time.sleep(0.6)
    P.click(736, 146); time.sleep(0.8)
    P.grab(f"{V}/fix_toast_incremental.png")
    print(f"[2] incremental_index -> {cfg('incremental_index')}")
    P.click(736, 146); time.sleep(0.7)

    # ---------- 3. 剪贴板监控 ----------
    P.click(160, 265); time.sleep(0.6)
    P.click(736, 149); time.sleep(0.8)
    P.grab(f"{V}/fix_toast_clipboard.png")
    print(f"[3] clipboard_enabled -> {cfg('clipboard_enabled')}")
    P.click(736, 149); time.sleep(0.7)

    # ---------- 4. 正文全文检索关闭 ----------
    P.click(160, 223); time.sleep(0.6)
    P.click(736, 204); time.sleep(1.0)
    P.grab(f"{V}/fix_toast_content_off.png")
    print(f"[4] content_index_enabled -> {cfg('content_index_enabled')}")
    P.click(736, 204); time.sleep(1.2)

    # ---------- 5. 全局热键服务 ----------
    P.click(160, 182); time.sleep(0.6)
    P.click(736, 141); time.sleep(0.8)
    P.grab(f"{V}/fix_toast_hotkeys.png")
    print(f"[5] hotkeys_master_enabled -> {cfg('hotkeys_master_enabled')}")
    P.click(736, 141); time.sleep(0.7)

    print("收尾状态：",
          {k: cfg(k) for k in ("index_roots", "incremental_index", "clipboard_enabled",
                               "content_index_enabled", "hotkeys_master_enabled")})


if __name__ == "__main__":
    main()
