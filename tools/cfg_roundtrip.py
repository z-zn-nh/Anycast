"""设置项往返探针：点一个控件 → 报告 config.json 里哪个键变了。

用法：
    python tools/cfg_roundtrip.py "标签:x,y" ["标签2:x2,y2" ...]

坐标是**逻辑坐标**（Slint 口径）。每次点击后读 config.json 做差集，
所以「点下去没反应」和「点到了别的键」都能看出来 —— 不靠肉眼。
"""
import json
import subprocess
import sys
import time
from pathlib import Path

CONFIG = Path(r"C:/Users/30130/AppData/Roaming/Anycast/data/config.json")
PROBE = r"D:/Anycast/tools/rt_probe.py"
PY = r"E:/Python/python.exe"


def snap():
    for _ in range(3):
        try:
            return json.loads(CONFIG.read_text(encoding="utf-8"))
        except Exception:
            time.sleep(0.15)
    return {}


def click(x, y):
    subprocess.run([PY, PROBE, "click", str(int(x)), str(int(y))],
                   capture_output=True)
    # ⚠️ 1.0s 而不是 0.6s：某些开关（gpu_blur / rim_light）会连带重新应用窗口材质，
    # 落盘明显更慢。实测 0.6s 会读出「没变化」的假阴性，误判成开关没接线。
    time.sleep(1.0)


def main():
    specs = sys.argv[1:]
    if not specs:
        print(__doc__)
        return

    print(f"{'控件':22s} {'结果':46s} 变化")
    print("-" * 96)
    for spec in specs:
        label, coords = spec.split(":", 1)
        x, y = [float(v) for v in coords.split(",")]
        before = snap()
        click(x, y)
        after = snap()

        changed = {}
        for k in set(before) | set(after):
            if before.get(k) != after.get(k):
                changed[k] = (before.get(k), after.get(k))

        if not changed:
            verdict, note = "✗ 无变化", "点下去没反应 / 坐标不对"
        elif len(changed) == 1:
            k, (b, a) = next(iter(changed.items()))
            verdict, note = "✓", f"{k}: {b} → {a}"
        else:
            verdict = "⚠ 多处变化"
            note = "; ".join(f"{k}: {b} → {a}" for k, (b, a) in changed.items())

        print(f"{label:22s} {verdict:4s} {note:52s}")
        # 点回去，保持可重复（除了 string 类型）
        for k, (b, a) in changed.items():
            if isinstance(a, bool):
                click(x, y)
                break


if __name__ == "__main__":
    main()
