"""设计稿（index.html 无头渲染）vs 原生 Slint 窗口 —— 逐地标几何对照。

用法：
    E:/Python/python.exe tools/design_diff.py target/verify/nat_after.png

前提：
  · target/verify/mock_full.png —— Edge 无头渲染 index.html 的整页截图（1600x1000）
  · 设计稿里启动器窗口的矩形 = MOCK_RECT（下面写死，改版式时重新标定）
  · 原生截图必须带 window 边界（tools/ui_shot.py 产出的整窗图），物理像素 = 逻辑 x SCALE

判据说明：
  两边都用「同一套像素谓词」提取地标，输出**逻辑像素**，直接比数值。
  设计稿的启动器预览框会因内容溢出触发 flex-shrink（行被压扁），
  所以**行距/行高类地标以 CSS 声明值为准**，本脚本只用于验证「结构位置」。
"""
import sys, os
from PIL import Image

SCALE = 1.5                      # 原生窗口 DPI 缩放
MOCK_RECT = (370, 220, 1230, 780)  # 设计稿里启动器窗口的 (left, top, right, bottom)


def load_mock():
    full = Image.open("target/verify/mock_full.png").convert("RGB")
    return full.crop(MOCK_RECT)


def bands(img, x0, x1, y0, y1, pred, scale=1.0, min_h=3, min_px=2):
    """返回 [(top, bottom, height, center)]，逻辑坐标。"""
    p = img.load()
    out, cur = [], None
    for y in range(y0, y1):
        n = sum(1 for x in range(x0, x1) if pred(p[x, y]))
        if n >= min_px:
            cur = [y, y] if cur is None else [cur[0], y]
        else:
            if cur:
                out.append(tuple(cur)); cur = None
    if cur:
        out.append(tuple(cur))
    res = []
    for a, b in out:
        if (b - a + 1) >= min_h * scale:
            res.append((a / scale, b / scale, (b - a + 1) / scale, (a + b) / 2 / scale))
    return res


def ink(thr):
    return lambda v: max(v) > thr


def sat():
    return lambda v: (max(v) - min(v)) > 45 and max(v) > 70


def show(title, mock_res, nat_res):
    print(f"\n=== {title} ===")
    print(f"  {'设计稿(logical)':<34} {'原生(logical)':<34} Δ中心")
    n = max(len(mock_res), len(nat_res))
    for i in range(n):
        m = mock_res[i] if i < len(mock_res) else None
        a = nat_res[i] if i < len(nat_res) else None
        ms = f"{m[0]:6.1f}..{m[1]:6.1f} h{m[2]:5.1f}" if m else " " * 21
        ns = f"{a[0]:6.1f}..{a[1]:6.1f} h{a[2]:5.1f}" if a else " " * 21
        d = f"{a[3]-m[3]:+6.1f}" if (m and a) else "   —"
        print(f"  {ms:<34} {ns:<34} {d}")


def main():
    nat_path = sys.argv[1] if len(sys.argv) > 1 else "target/verify/nat_after.png"
    mock = load_mock()
    nat = Image.open(nat_path).convert("RGB")
    print(f"设计稿窗口 {mock.size}  原生 {nat.size} (scale {SCALE})")
    print("（Δ中心 = 原生 - 设计稿，正数表示原生更低）")

    # 搜索栏图标
    show("搜索栏 magnifier（ink>110）",
         bands(mock, 10, 45, 0, 60, ink(110), 1.0),
         bands(nat, 15, 68, 0, 90, ink(110), SCALE))

    # 置顶架标题行
    show("置顶架标题「置顶」（ink>120）",
         bands(mock, 16, 120, 50, 85, ink(120), 1.0),
         bands(nat, 24, 180, 75, 128, ink(120), SCALE))

    # 置顶卡 squircle（彩色）
    show("置顶卡图标（sat）",
         bands(mock, 28, 62, 85, 135, sat(), 1.0),
         bands(nat, 42, 93, 128, 203, sat(), SCALE))

    # 分类栏活动胶囊内的图标+文字
    show("分类栏「全部」墨迹（ink>150）",
         bands(mock, 16, 90, 170, 215, ink(150), 1.0),
         bands(nat, 24, 135, 255, 323, ink(150), SCALE))

    # 结果区：段标题 + 首行图标
    show("结果区 section header（ink>120，低阈值）",
         bands(mock, 16, 120, 215, 245, ink(120), 1.0),
         bands(nat, 24, 180, 322, 368, ink(120), SCALE))

    show("结果行 squircle（sat）",
         bands(mock, 28, 62, 235, 520, sat(), 1.0),
         bands(nat, 42, 93, 352, 780, sat(), SCALE))

    # 底栏按钮
    show("底栏按钮墨迹（ink>140）",
         bands(mock, 16, 200, 520, 560, ink(140), 1.0),
         bands(nat, 24, 300, 780, 840, ink(140), SCALE))


if __name__ == "__main__":
    main()
