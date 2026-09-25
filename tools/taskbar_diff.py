"""对照两张 PNG，输出差异像素的包围盒与数量（用于 A/B 取证）。

只支持本仓库 tools/taskbar_shot.py 产出的 PNG（8bit RGBA、filter 全 0、非隔行）。
这样解码只需 zlib + 去 filter 0，不必依赖 Pillow。

用法：
    python tools/taskbar_diff.py a.png b.png
"""
import struct
import sys
import zlib


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "不是 PNG"
    pos = 8
    idat = b""
    w = h = None
    while pos < len(data):
        ln = struct.unpack(">I", data[pos:pos + 4])[0]
        tag = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + ln]
        if tag == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and ctype == 6 and interlace == 0, "仅支持 8bit RGBA 非隔行"
        elif tag == b"IDAT":
            idat += body
        pos += 12 + ln
    raw = zlib.decompress(idat)
    stride = w * 4
    rows = []
    for y in range(h):
        off = y * (stride + 1)
        assert raw[off] == 0, f"第 {y} 行 filter 不是 0，本工具不支持"
        rows.append(raw[off + 1:off + 1 + stride])
    return w, h, rows


def main():
    if len(sys.argv) < 3:
        print("用法: python tools/taskbar_diff.py a.png b.png")
        return
    wa, ha, ra = read_png(sys.argv[1])
    wb, hb, rb = read_png(sys.argv[2])
    print(f"A: {sys.argv[1]}  {wa}x{ha}")
    print(f"B: {sys.argv[2]}  {wb}x{hb}")
    if (wa, ha) != (wb, hb):
        print("尺寸不同，无法逐像素对照")
        return

    minx, miny, maxx, maxy = wa, ha, -1, -1
    diff = 0
    # 按列统计差异，便于定位（任务栏图标是横向排列的）
    col_hits = {}
    for y in range(ha):
        ra_row, rb_row = ra[y], rb[y]
        if ra_row == rb_row:
            continue
        for x in range(wa):
            i = x * 4
            if ra_row[i:i + 4] != rb_row[i:i + 4]:
                diff += 1
                minx = min(minx, x)
                maxx = max(maxx, x)
                miny = min(miny, y)
                maxy = max(maxy, y)
                col_hits[x] = col_hits.get(x, 0) + 1

    total = wa * ha
    print(f"差异像素: {diff} / {total}  ({100.0 * diff / total:.3f}%)")
    if diff == 0:
        print("=> 两张图完全相同")
        return
    print(f"差异包围盒: x {minx}..{maxx}  y {miny}..{maxy}")

    # 把连续有差异的列合并成区间
    cols = sorted(col_hits)
    spans = []
    s = prev = cols[0]
    for c in cols[1:]:
        if c - prev > 8:
            spans.append((s, prev))
            s = c
        prev = c
    spans.append((s, prev))
    print("差异横向区间（宽度 >= 4px 的）:")
    for a, b in spans:
        if b - a >= 4:
            print(f"   x {a}..{b}   宽 {b - a + 1}px   该区差异像素 {sum(col_hits.get(c, 0) for c in range(a, b + 1))}")


main()
