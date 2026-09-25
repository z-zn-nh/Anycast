"""在设置面板截图里自动定位 FluentSwitch 开关。

原理：开关是 40x20 的圆角胶囊，右边缘固定在内容卡片右侧。
只扫开关右侧的窄条（避开同一行的键帽 chip / 下拉框），
逐行判断「这一行在该窄条内是否有亮像素」，再按间隙聚类成一个个胶囊。

用法: python tools/find_switches.py <shot.png> [--x0 1108] [--x1 1130]
输出: 每个开关的逻辑坐标中心 (lx, ly) 与状态(on/off)
"""
import sys
from PIL import Image

SCALE = 1.5


def find(path, x0=1108, x1=1130, thr=60, gap=24, y0=80, y1=840):
    im = Image.open(path).convert("RGB")
    W, H = im.size

    def lum(p):
        return 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]

    lit = []
    for y in range(y0, min(y1, H)):
        if any(lum(im.getpixel((x, y))) > thr for x in range(x0, min(x1, W))):
            lit.append(y)

    groups, cur = [], []
    for y in lit:
        if cur and y - cur[-1] > gap:
            groups.append(cur)
            cur = []
        cur.append(y)
    if cur:
        groups.append(cur)

    out = []
    for g in groups:
        top, bot = g[0], g[-1]
        h = bot - top + 1
        if h < 12:
            continue
        cy = (top + bot) / 2
        # 判断 on/off：看胶囊中心行在整条 40px 宽度里亮像素的占比
        mid = int(cy)
        span = range(int(714 * SCALE), int(754 * SCALE))
        bright = sum(1 for x in span if lum(im.getpixel((x, mid))) > thr)
        state = "on " if bright > 20 else "off"
        out.append((734.0, cy / SCALE, state, h / SCALE, bright))
    return out


if __name__ == "__main__":
    path = sys.argv[1]
    args = sys.argv[2:]
    kw = {}
    for i in range(0, len(args) - 1, 2):
        kw[args[i].lstrip("-")] = int(args[i + 1])
    for lx, ly, st, h, br in find(path, **kw):
        print(f"logical ({lx:.0f}, {ly:.1f})  state={st} h={h:.0f} bright={br}")
