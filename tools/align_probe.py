"""量测「元素背景框」与「内部墨迹」的左右内边距，判断文字是否真居中。
用法：import 或直接跑，改 REGIONS 表。坐标一律用**窗口内逻辑像素**，scale 只用于换算。
"""
from PIL import Image

def runs(img, y, x0, x1, base, thr, min_len):
    p = img.load(); out = []; cur = None
    for x in range(x0, x1):
        v = p[x, y]
        if max(abs(v[i] - base[i]) for i in range(3)) > thr:
            cur = [x, x] if cur is None else [cur[0], x]
        else:
            if cur and cur[1] - cur[0] + 1 >= min_len: out.append(tuple(cur))
            cur = None
    if cur and cur[1] - cur[0] + 1 >= min_len: out.append(tuple(cur))
    return out

def ink(img, x0, x1, y0, y1, thr):
    p = img.load(); lo = hi = None
    for x in range(x0, x1 + 1):
        if any(max(p[x, y]) > thr for y in range(y0, y1 + 1)):
            lo = x if lo is None else lo; hi = x
    return lo, hi

def probe(img, y, x0, x1, base_pt, scale=1.0, thr_bg=10, thr_ink=120,
          min_len=8, dy=15, pick=0):
    """pick: 取第几个背景段（0 = 最左）"""
    p = img.load(); base = p[base_pt[0], base_pt[1]]
    rs = runs(img, y, x0, x1, base, thr_bg, int(min_len * scale))
    if len(rs) <= pick: return None
    b0, b1 = rs[pick]
    lo, hi = ink(img, b0, b1, y - int(dy * scale), y + int(dy * scale), thr_ink)
    if lo is None: return dict(bg=(b0/scale, b1/scale), ink=None)
    return dict(bg=(round(b0/scale, 1), round(b1/scale, 1)),
                ink=(round(lo/scale, 1), round(hi/scale, 1)),
                L=round((lo - b0)/scale, 1), R=round((b1 - hi)/scale, 1),
                w=round((b1 - b0 + 1)/scale, 1))
