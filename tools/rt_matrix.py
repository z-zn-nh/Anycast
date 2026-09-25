"""设置面板「开关往返」批量验证。

对每个开关做：切到所属页 -> 读 config -> 点一下 -> 读 config -> 再点一下 -> 读 config。
双向各验一次，且结束时回到初始状态（不留脏值）。

用法：python tools/rt_matrix.py [--commit] [--skip key1,key2]
  --commit  真正写入（默认 dry-run 只打印计划）
  --skip    跳过指定配置键，逗号分隔。用于排除有**系统副作用**的开关，
            例如 `launch_on_startup` 会写 HKCU\\...\\Run 注册表项。
            推荐：--commit --skip launch_on_startup
"""
import json
import sys
import time

sys.path.insert(0, r"D:\Anycast\tools")
import rt_probe as P

CFG = r"C:\Users\30130\AppData\Roaming\Anycast\data\config.json"

NAV = {"general": 98, "appearance": 140, "hotkeys": 182,
       "search": 223, "clipboard": 265, "ai": 306}
NAV_X = 160
SW_X = 736

# (配置键, 页面, 开关中心逻辑 y, 中文名)
SWITCHES = [
    ("launch_on_startup", "general", 204, "开机自启动"),
    ("hide_on_blur", "general", 256, "失去焦点时自动隐藏"),
    ("double_click_launch", "general", 406, "双击条目立即执行"),
    # 注：`minimize_to_tray`（原 general 页 y=504）已于 2026-09-24 裁决移除，
    #     它是 general 页最后一个开关，因此其余开关的 y 坐标不受影响。
    ("gpu_blur", "appearance", 204, "系统级 DWM 亚克力模糊"),
    ("rim_light", "appearance", 256, "边缘高光与微晶质感"),
    ("hotkeys_master_enabled", "hotkeys", 141, "全局热键直达服务"),
    ("incremental_index", "search", 146, "实时增量索引"),
    ("content_index_enabled", "search", 204, "文件正文全文检索 FTS5"),
    ("include_hidden", "search", 256, "搜索结果包含隐藏文件"),
    ("exclude_build_caches", "search", 352, "自动排除大型开发构建缓存"),
    ("clipboard_enabled", "clipboard", 149, "启用剪贴板监控"),
    ("clipboard_dedupe", "clipboard", 256, "相同内容自动合并置顶"),
    ("clipboard_ignore_password_managers", "clipboard", 356, "忽略密码管理器复制内容"),
    ("clipboard_mask_sensitive", "clipboard", 406, "银行卡与身份证脱敏展示"),
    ("ai_enabled", "ai", 146, "AI 语义增强模式"),
    ("ai_lazy_load", "ai", 256, "按需动态唤醒加载"),
    ("ai_intent_parsing", "ai", 352, "自然语言时间与类型解析"),
]


def read(key):
    return json.load(open(CFG, encoding="utf-8")).get(key)


def goto(page):
    P.click(NAV_X, NAV[page])
    time.sleep(0.55)


def main():
    commit = "--commit" in sys.argv
    skip = set()
    for i, a in enumerate(sys.argv):
        if a == "--skip" and i + 1 < len(sys.argv):
            skip = {s.strip() for s in sys.argv[i + 1].split(",") if s.strip()}
    items = [s for s in SWITCHES if s[0] not in skip]
    if skip:
        print(f"（已跳过 {len(skip)} 个：{', '.join(sorted(skip))}）")
    print(f"{'开关':30s} {'键':38s} {'初值':>6s} {'点后':>6s} {'再点':>6s}  结果")
    print("-" * 108)
    results = []
    for key, page, y, name in items:
        if not commit:
            print(f"{name:30s} {key:38s}   (dry-run)")
            continue
        goto(page)
        v0 = read(key)
        P.click(SW_X, y)
        time.sleep(0.7)
        v1 = read(key)
        P.click(SW_X, y)
        time.sleep(0.7)
        v2 = read(key)
        ok = (v0 != v1) and (v2 == v0) and isinstance(v1, bool)
        results.append((key, v0, v1, v2, ok))
        print(f"{name:30s} {key:38s} {str(v0):>6s} {str(v1):>6s} {str(v2):>6s}  "
              f"{'PASS' if ok else 'FAIL'}")
    if commit:
        bad = [r for r in results if not r[4]]
        print("-" * 108)
        print(f"通过 {len(results) - len(bad)}/{len(results)}")
        for key, v0, v1, v2, _ in bad:
            print(f"  FAIL {key}: {v0} -> {v1} -> {v2}")


if __name__ == "__main__":
    main()
