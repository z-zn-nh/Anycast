import sys
sys.path.insert(0, 'tools')
from png_probe import read_png, px

def scan_row(path, y, x0=0, x1=None, thr=14, bg=None):
    w, h, ch, buf = read_png(path)
    if x1 is None: x1 = w
    if bg is None: bg = (20, 23, 31)
    runs = []
    cur = None
    for x in range(x0, x1):
        p = px(buf, w, ch, x, y)
        d = abs(p[0]-bg[0]) + abs(p[1]-bg[1]) + abs(p[2]-bg[2])
        if d > thr:
            if cur is None: cur = [x, x, 0]
            cur[1] = x
            cur[2] = max(cur[2], max(p))
        else:
            if cur is not None:
                runs.append(cur); cur = None
    if cur is not None: runs.append(cur)
    print(f'--- scan y={y} ---')
    for a, b, pk in runs:
        print(f'  x {a:4d}..{b:4d} (w={b-a+1:3d}) center {(a+b)/2:7.1f}  peak {pk:3d}  color {px(buf,w,ch,(a+b)//2,y)}')

def scan_col(path, x, y0=0, y1=None, thr=14, bg=None):
    w, h, ch, buf = read_png(path)
    if y1 is None: y1 = h
    if bg is None: bg = (20, 23, 31)
    runs = []
    cur = None
    for y in range(y0, y1):
        p = px(buf, w, ch, x, y)
        d = abs(p[0]-bg[0]) + abs(p[1]-bg[1]) + abs(p[2]-bg[2])
        if d > thr:
            if cur is None: cur = [y, y, 0]
            cur[1] = y
            cur[2] = max(cur[2], max(p))
        else:
            if cur is not None:
                runs.append(cur); cur = None
    if cur is not None: runs.append(cur)
    print(f'--- scan x={x} ---')
    for a, b, pk in runs:
        print(f'  y {a:4d}..{b:4d} (h={b-a+1:3d}) center {(a+b)/2:7.1f}  peak {pk:3d}  color {px(buf,w,ch,x,(a+b)//2)}')

if __name__ == '__main__':
    path = sys.argv[1]
    kind = sys.argv[2]
    n = int(sys.argv[3])
    thr = int(sys.argv[4]) if len(sys.argv) > 4 else 14
    if kind == 'row': scan_row(path, n, thr=thr)
    else: scan_col(path, n, thr=thr)
