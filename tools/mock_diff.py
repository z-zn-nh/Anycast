"""设计稿 vs 原生实现 的逐区域对照工具。

用法：
  python tools/mock_diff.py regions            # 输出若干区域的上下对照图
  python tools/mock_diff.py sample             # 采样窗口底色/壁纸透色
  python tools/mock_diff.py edges              # 扫描窗口右边缘，找滚动条

约定：
  设计稿 = index.html 在 1600x1000 视口下渲染，启动器窗口居中于 (370,220) 860x560（逻辑 px）
  原生   = win_shot 抓到的窗口位图 1290x840，即 860x560 逻辑 px @ 150% DPI
"""
import sys
from PIL import Image, ImageDraw

ROOT = r"D:\Anycast"
MOCK = f"{ROOT}\\target\\verify\\mock_full.png"
MOCK_ORIGIN = (370, 220)          # 设计稿中启动器窗口左上角（逻辑 px）
MOCK_SIZE = (860, 560)
NATIVE = f"{ROOT}\\target\\list_final.png"
NATIVE_SCALE = 1.5                # 物理 / 逻辑
OUT = f"{ROOT}\\target\\verify"


def load_pair(native_path=None):
    mock = Image.open(MOCK).convert("RGB").crop(
        (MOCK_ORIGIN[0], MOCK_ORIGIN[1],
         MOCK_ORIGIN[0] + MOCK_SIZE[0], MOCK_ORIGIN[1] + MOCK_SIZE[1]))
    nat = Image.open(native_path or NATIVE).convert("RGB")
    nat = nat.resize((MOCK_SIZE[0], MOCK_SIZE[1]), Image.LANCZOS)
    return mock, nat


def stack(name, box, zoom=4):
    """box = (x, y, w, h) 逻辑坐标；输出上=设计稿 下=原生 的对照图"""
    mock, nat = load_pair()
    x, y, w, h = box
    m = mock.crop((x, y, x + w, y + h)).resize((w * zoom, h * zoom), Image.NEAREST)
    n = nat.crop((x, y, x + w, y + h)).resize((w * zoom, h * zoom), Image.NEAREST)
    canvas = Image.new("RGB", (w * zoom, h * zoom * 2 + 8), (255, 0, 0))
    canvas.paste(m, (0, 0))
    canvas.paste(n, (0, h * zoom + 8))
    d = ImageDraw.Draw(canvas)
    d.text((4, 4), "MOCK", fill=(255, 80, 80))
    d.text((4, h * zoom + 12), "NATIVE", fill=(255, 80, 80))
    path = f"{OUT}\\cmp_{name}.png"
    canvas.save(path)
    print("wrote", path, canvas.size)


def sample():
    mock, nat = load_pair()
    print("=== 窗口底色采样（逻辑 px 坐标）===")
    pts = [("左上角内 8,8", 8, 8), ("顶部中央 430,30", 430, 30),
           ("左中 8,280", 8, 280), ("右下 852,552", 852, 552),
           ("内容区空白 700,470", 700, 470)]
    for label, x, y in pts:
        print(f"{label:20s} mock={mock.getpixel((x, y))}  native={nat.getpixel((x, y))}")
    print()
    print("=== 窗口水平扫描 y=300，每 60px 取样 ===")
    for x in range(0, 860, 60):
        print(f"x={x:4d}  mock={mock.getpixel((x, 300))}  native={nat.getpixel((x, 300))}")


def edges():
    mock, nat = load_pair()
    print("=== 原生窗口右边缘列扫描 y=300（x=830..859）===")
    for x in range(830, 860):
        print(f"x={x:3d}  {nat.getpixel((x, 300))}")
    print()
    print("=== 设计稿同区域 ===")
    for x in range(830, 860):
        print(f"x={x:3d}  {mock.getpixel((x, 300))}")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "regions"
    if cmd == "regions":
        stack("topright", (600, 0, 260, 52), zoom=5)
        stack("footer", (0, 520, 300, 40), zoom=5)
        stack("pinned", (0, 60, 340, 90), zoom=4)
        stack("row1", (0, 280, 460, 50), zoom=4)
        stack("rowsel", (0, 276, 860, 60), zoom=2)
    elif cmd == "sample":
        sample()
    elif cmd == "edges":
        edges()
