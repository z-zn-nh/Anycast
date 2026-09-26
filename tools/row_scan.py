"""按行扫描截图，找出该行上所有「非背景」的水平区段。

用途：定位开关 / 按钮 / 输入框的**真实**横向位置，避免目测坐标点错相邻控件。
（截图 Read 回来是缩放过的，目测值会稳定偏到隔壁控件上。）

用法：
    python tools/row_scan.py <png> <display_y> [更多 y...]
显示坐标 = 截图里看到的坐标（1080 宽口径）。
"""
import sys
import importlib.util

spec = importlib.util.spec_from_file_location("pp", r"D:/Anycast/tools/png_probe.py")
pp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pp)

# 截图渲染口径：1080 显示宽 → 1290 物理宽
SCALE = 1290 / 1080.0


def px_at(buf, w, ch, x, y):
    return pp.px(buf, w, ch, x, y)


def scan_row(buf, w, ch, dy, tol=10, min_run=4):
    y = int(dy * SCALE)
    # 该行的背景 = 出现最多的颜色
    from collections import Counter
    c = Counter(px_at(buf, w, ch, x, y) for x in range(w))
    bg = c.most_common(1)[0][0]

    runs = []
    start = None
    for x in range(w):
        p = px_at(buf, w, ch, x, y)
        diff = abs(p[0] - bg[0]) + abs(p[1] - bg[1]) + abs(p[2] - bg[2])
        if diff > tol:
            if start is None:
                start = x
        else:
            if start is not None:
                if x - start >= min_run:
                    runs.append((start, x - 1))
                start = None
    if start is not None:
        runs.append((start, w - 1))

    out = []
    for a, b in runs:
        mid = (a + b) // 2
        p = px_at(buf, w, ch, mid, y)
        out.append((a, b, p))
    return bg, out


def main():
    path = sys.argv[1]
    ys = [float(v) for v in sys.argv[2:]]
    w, h, ch, buf = pp.read_png(path)
    print(f"# {path} {w}x{h}  (显示口径 1080x{int(h / SCALE)})")
    for dy in ys:
        bg, runs = scan_row(buf, w, ch, dy)
        print(f"\n--- 显示 y={dy:g} (物理 {int(dy * SCALE)})  背景 #{bg[0]:02x}{bg[1]:02x}{bg[2]:02x} ---")
        for a, b, p in runs:
            da, db = a / SCALE, b / SCALE
            print(f"  显示 x {da:7.1f}..{db:7.1f}  宽 {db - da:6.1f}  "
                  f"中心 {((da + db) / 2):7.1f}  色 #{p[0]:02x}{p[1]:02x}{p[2]:02x}  "
                  f"逻辑中心 {((da + db) / 2) * SCALE / 1.5:7.1f}")


if __name__ == "__main__":
    main()
