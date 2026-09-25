"""A/B 截图对照：裁出同一区域、放大、上下拼接成一张对比图。

本仓库做 UI 取证时经常要"改前 vs 改后"并排看，而任务栏图标这种差异只有 32px 宽，
不放大根本看不出来。这个脚本把两图同一区域裁出来、整数倍最近邻放大后上下拼在一起。

用法：
    python tools/png_ab.py a.png b.png out.png x0 x1 y0 y1 [放大倍数，默认 6]
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
        assert raw[off] == 0, f"第 {y} 行 filter 不是 0"
        rows.append(raw[off + 1:off + 1 + stride])
    return w, h, rows


def write_png(path, w, h, rows_rgba):
    raw = bytearray()
    for row in rows_rgba:
        raw.append(0)
        raw += row

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 6))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def crop_zoom(rows, x0, x1, y0, y1, zoom):
    """裁剪 + 最近邻放大，返回 RGBA 行列表。"""
    out = []
    for y in range(y0, y1):
        src = rows[y]
        seg = src[x0 * 4:x1 * 4]
        row = bytearray()
        for i in range(0, len(seg), 4):
            row += seg[i:i + 4] * zoom
        for _ in range(zoom):
            out.append(bytes(row))
    return out


def main():
    if len(sys.argv) < 8:
        print("用法: python tools/png_ab.py a.png b.png out.png x0 x1 y0 y1 [zoom=6]")
        return
    pa, pb, pout = sys.argv[1], sys.argv[2], sys.argv[3]
    x0, x1, y0, y1 = (int(v) for v in sys.argv[4:8])
    zoom = int(sys.argv[8]) if len(sys.argv) > 8 else 6

    wa, ha, ra = read_png(pa)
    wb, hb, rb = read_png(pb)
    if (wa, ha) != (wb, hb):
        print("两图尺寸不同")
        return
    x1 = min(x1, wa)
    y1 = min(y1, ha)

    ca = crop_zoom(ra, x0, x1, y0, y1, zoom)
    cb = crop_zoom(rb, x0, x1, y0, y1, zoom)

    cw = (x1 - x0) * zoom
    gap = 6 * zoom
    # 用中性灰做间隔带与边框，避免与任务栏内容混淆
    gap_row = bytes([90, 90, 90, 255]) * cw
    rows = ca + [gap_row] * gap + cb

    write_png(pout, cw, len(rows), rows)
    print(f"已保存 {pout}  ({cw}x{len(rows)})   上= {pa}   下= {pb}")
    print(f"裁剪区域 x {x0}..{x1} y {y0}..{y1}  放大 {zoom}x")


main()
