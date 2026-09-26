#!/usr/bin/env python
"""扫 ui/ 下所有 Text 元素，列出字号与是否钉死了 height。

背景（doc §13.5 / §14.5 / §15）：设计稿用 CSS 的 `line-height` 精确控制行盒高度，
而 Slint 里 11~13px 文本的**自然行高 ≈1.5×**（11px → ≈16.5px）。

**真正会出问题的只有一种写法**：`Text` 里写了显式 `y:` 却没钉 `height:`。
这时墨迹中心 = `y + 自然行高/2`，比设计稿的 `y + line-height/2` 低
（11px 文本低 2.75px），而兄弟节点又不会跟着挪 → 只有这一行错位。

反之，`Text` 放在 `HorizontalLayout { alignment: center }` 或
`VerticalLayout` 里、且没有显式 `y:` 时，布局会把它撑满再按
`vertical-alignment` 对齐 → **不会**有这个问题，不用改。

用法：
    python tools/slint_text_audit.py          # 只列「有 y 无 height」的高危项
    python tools/slint_text_audit.py -a       # 列出全部有 font-size 的 Text
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SHOW_ALL = "-a" in sys.argv

RE_FONT = re.compile(r"font-size:\s*([\d.]+)px")
# 直接属性里找，允许与其它属性同行（`width: …; height: …;` 这种写法很常见）
RE_HEIGHT = re.compile(r"(?<![\w-])height:\s*([^;]+);")
RE_Y = re.compile(r"(?<![\w-])y:\s*([^;]+);")
RE_TEXT = re.compile(r"(?<![\w-])text:\s*(.+?);")


def own_props(body):
    """只取 Text 的**直接属性**行（与 `Text {` 同级的缩进），排除嵌套子元素的属性。

    否则 `Text { … if x : Rectangle { height: 3px } }` 里的 `height` 会被误当成
    这个 Text 的 height。
    """
    lines = [l for l in body.split("\n") if l.strip()]
    if not lines:
        return ""
    # base 取**第一条属性行**的缩进：不能取 min()，否则结尾那个 `}`（缩进更浅）
    # 会被当成基准，导致一条属性都收不进来。
    base = len(lines[0]) - len(lines[0].lstrip())
    keep = []
    for l in lines:
        if l.strip() == "}":
            break
        ind = len(l) - len(l.lstrip())
        if ind >= base:
            keep.append(l.strip())
        else:
            break
    return "\n".join(keep)


def blocks(src):
    """产出 (起始行号, 块文本)，块 = 从 `Text {` 到配对右括号。

    要覆盖三种写法：
      Text { ... }
      if cond : Text { ... }
      act := Text { ... }        ← 命名元素，最容易被漏掉
    """
    lines = src.split("\n")
    i = 0
    while i < len(lines):
        if re.match(r"\s*(if .+? :\s*)?(\w+\s*:=\s*)?Text\s*\{", lines[i]):
            depth = lines[i].count("{") - lines[i].count("}")
            body = []
            j = i + 1
            while j < len(lines) and depth > 0:
                body.append(lines[j])
                depth += lines[j].count("{") - lines[j].count("}")
                j += 1
            yield i + 1, "\n".join(body)
            i = j
        else:
            i += 1


def enclosing_layout(lines, idx):
    """从 idx（0 基）向上找最近的 `*Layout {` 祖先，返回它的名字。

    VerticalLayout 最危险：子级高度直接决定下一个兄弟的 y，所以 Text 的自然行高
    比设计稿的 line-height 大多少，下面的兄弟就低多少（§14.5 那一族）。
    HorizontalLayout 里只要 Text 没写 y，布局会按 vertical-alignment 对齐 → 安全。
    """
    depth = 0
    for i in range(idx - 1, -1, -1):
        line = lines[i]
        depth += line.count("}") - line.count("{")
        if depth < 0:
            depth = 0
            m = re.search(r"(\w*Layout)\s*\{", line)
            if m:
                return m.group(1)
    return "(顶层)"


def main():
    files = []
    for root, _, names in os.walk(os.path.join(ROOT, "ui")):
        for n in names:
            if n.endswith(".slint"):
                files.append(os.path.join(root, n))
    files.sort()

    print(f"{'文件':<38}{'行':<6}{'字号':<8}{'y':<9}{'height':<20}{'外层布局':<18}text")
    print("-" * 128)
    risky, vlayout = [], []
    for path in files:
        rel = os.path.relpath(path, ROOT).replace("\\", "/")
        with open(path, encoding="utf-8") as f:
            lines = f.read().split("\n")
        for lineno, body in blocks("\n".join(lines)):
            props = own_props(body)
            mf = RE_FONT.search(props)
            mh = RE_HEIGHT.search(props)
            my = RE_Y.search(props)
            mt = RE_TEXT.search(props)
            if not mf and not SHOW_ALL:
                continue
            fs = mf.group(1) if mf else "-"
            h = mh.group(1).strip() if mh else None
            y = my.group(1).strip() if my else None
            tv = (mt.group(1)[:26] if mt else "")
            lay = enclosing_layout(lines, lineno - 1)
            flag = ""
            if y and not h:
                flag = "  <== 有 y 无 height（会低 ~2.75px）"
                risky.append((rel, lineno, fs, y, tv))
            elif not h and lay == "VerticalLayout":
                flag = "  <== 在 VLayout 里且没钉 height"
                vlayout.append((rel, lineno, fs, tv))
            print(f"  {rel:<36}{lineno:<6}{fs:<8}{str(y):<9}{str(h):<20}{lay:<18}{tv}{flag}")

    print()
    if risky:
        print(f"⚠️ 共 {len(risky)} 处「有 y 无 height」——墨迹中心会低 ~2.75px，必须逐个对照设计稿：")
        for rel, lineno, fs, y, tv in risky:
            print(f"  {rel}:{lineno}  {fs}px  y:{y}  {tv}")
    else:
        print("「有 y 无 height」：0 处 ✅")
    print()
    if vlayout:
        print(f"ℹ️ 共 {len(vlayout)} 处在 VerticalLayout 里没钉 height（会把自己和下面的兄弟一起撑高）：")
        for rel, lineno, fs, tv in vlayout:
            print(f"  {rel}:{lineno}  {fs}px  {tv}")
    else:
        print("在 VerticalLayout 里且没钉 height：0 处 ✅")


if __name__ == "__main__":
    main()
