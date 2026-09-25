#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""Jev 探针 —— 用 Anycast 的真实查询场景测 Jev 的槽位解析效果与延迟。

背景见 doc/检索增强与判断模型接入开发文档.md §5.3（槽位设计）与 §5.8（语言策略）。
本脚本只做一件事：把真实查询喂给 Jev，看它能不能正确填出「类型 / 时间 / 是否在找文件」
三个槽位，并测出延迟。

用法
----
    # 官方端点（需 TypeSafe 排队申请，中国大陆需代理）
    set TYPESAFE_API_KEY=ts_...
    python tools/jev_probe.py --proxy http://127.0.0.1:7897

    # 第三方托管端点（jv_live_ key，免排队）
    set JEV_API_KEY=jv_live_...
    python tools/jev_probe.py --endpoint hosted --proxy http://127.0.0.1:7897

    # 只跑指定用例
    python tools/jev_probe.py --only zh1,en1

    # 列出全部用例
    python tools/jev_probe.py --list

    # 打印原始响应（排查用）
    python tools/jev_probe.py --raw

注意
----
* 默认**不使用**环境变量里的代理（避免被工具链注入的代理劫持）；要走代理请显式传 --proxy。
* 每个用例一次调用、并行问 3 个问题（Jev 官方建议批量），成本约 $0.00003/次。
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ENDPOINTS = {
    "official": ("https://api.typesafe.ai/v1/systemone", "TYPESAFE_API_KEY"),
    "hosted": ("https://jevtypesafeai.com/api/v1/decide", "JEV_API_KEY"),
}

MODEL = "jev-latest"

# ── 槽位问题定义（选项数量刻意压到 20 以内，见开发文档 §5.3）───────────────
TYPE_OPTIONS = {
    "document": "文档、笔记、说明、readme、md、pdf、word",
    "code": "源代码、脚本、配置、rs、py、js、json、yml、toml",
    "image": "图片、照片、截图、png、jpg、svg",
    "media": "音频、视频、mp3、mp4",
    "archive": "压缩包、zip、rar、7z",
    "executable": "可执行程序、exe、安装包",
    "folder": "文件夹、目录、项目根目录",
    "all": "无法判断类型，或用户不关心类型",
}

TIME_OPTIONS = {
    "today": "今天、刚刚、今天改的",
    "yesterday": "昨天",
    "this_week": "本周、这几天",
    "last_week": "上周",
    "this_month": "本月、这个月",
    "this_year": "今年",
    "older": "更早、很久以前",
    "any": "没有提到时间，或不限制时间",
}

LOCATION_OPTIONS = {
    "any": "没有提到位置",
    "current": "当前目录、这个文件夹里",
    "drive": "指定盘符，如 D 盘、E 盘",
    "common": "常用目录，如桌面、下载、文档、项目目录",
}

QUESTIONS = {
    "type": {
        "type": "choice",
        "instructions": "用户想找的东西属于哪一类？",
        "criteria": TYPE_OPTIONS,
    },
    "time": {
        "type": "choice",
        "instructions": "用户是否限定了文件的时间范围？",
        "criteria": TIME_OPTIONS,
    },
    "location": {
        "type": "choice",
        "instructions": "用户是否限定了查找的位置范围？",
        "criteria": LOCATION_OPTIONS,
    },
    "is_search": {
        "type": "noul",
        # 2026-09-24 首轮实测后改写。原措辞「是在搜索本机的文件或文件夹吗？」
        # 被模型理解成「这是不是一个明确的搜索指令」，导致 8/12 假阴性：
        #   docker -> 0.12、我的项目文件夹在哪 -> 0.29、那个写存储逻辑的文档 -> 0.30
        # 假阳性为 0（闲聊/生成任务都判对），说明模型只是保守，不是判错。
        # 改为正面描述「在找东西」，并把「只给一个词或文件名」显式写入以消除歧义。
        "instructions": (
            "用户是否在找本机的某个文件或文件夹？"
            "只要输入像是在找东西就算 —— 包括只给一个单词、一个文件名或一个名词短语。"
            "只有闲聊、问知识、要求生成内容才不算。"
        ),
    },
    "is_natural": {
        "type": "noul",
        # 首轮实测：英文短语被系统性低估（0.42~0.49），中文都在 0.60 以上。
        # 原措辞「是自然语言句子吗」把「名词短语」排除在外，
        # 但名词短语（the doc about storage logic）恰恰是最典型的搜索输入。
        # 改为强调「描述」而非「句子」。
        "instructions": (
            "用户是在用自然语言描述他想找的东西吗？"
            "整句、名词短语都算；只有一个孤立单词或纯文件名则不算。"
        ),
    },
}

# ── 测试用例：覆盖中文 / 英文 / 混合 / 纯关键词 / 非检索 ────────────────────
# expect 只写「必须判对」的槽位；没写的槽位不参与准确率统计（例如 zh2 的语义指代
# 本就无法由槽位模型表达，type 填 all 与 document 都不算错）。
# is_search / is_natural 为 noul 概率，判定阈值见 PASS_THRESHOLD。
CASES = [
    ("zh1", "找一下昨天改的 docker 配置", "中文·时间+类型",
     {"is_search": True, "type": "code", "time": "yesterday"}),
    ("zh2", "那个写存储逻辑的文档", "中文·语义指代",
     {"is_search": True, "type": "document"}),
    ("zh3", "我的项目文件夹在哪", "中文·文件夹",
     {"is_search": True, "type": "folder"}),
    ("zh4", "上周下载的那个压缩包", "中文·上周+压缩包",
     {"is_search": True, "type": "archive", "time": "last_week"}),
    ("en1", "files I modified yesterday", "英文·时间",
     {"is_search": True, "time": "yesterday"}),
    ("en2", "the doc about storage logic", "英文·语义",
     {"is_search": True, "type": "document"}),
    ("en3", "any png screenshots from last week", "英文·类型+时间",
     {"is_search": True, "type": "image", "time": "last_week"}),
    ("mix1", "找 Dockerfile 昨天改的", "中英混合",
     {"is_search": True, "type": "code", "time": "yesterday"}),
    ("kw1", "docker", "纯关键词",
     {"is_search": True, "is_natural": False}),
    ("kw2", "storage.rs", "纯关键词·文件名",
     {"is_search": True, "is_natural": False}),
    ("neg1", "今天天气怎么样", "非检索·闲聊",
     {"is_search": False}),
    ("neg2", "帮我写一段快排", "非检索·生成任务",
     {"is_search": False}),
]

# noul 概率的判定阈值：>= 阈值视为 True
PASS_THRESHOLD = 0.5


def call_jev(url, key, state, questions, proxy=None, timeout=30, model=MODEL):
    """返回 (响应 dict, 耗时毫秒)。失败抛异常。"""
    body = json.dumps(
        {"model": model, "state": state, "questions": questions}, ensure_ascii=False
    ).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            "Authorization": "Bearer " + key,
            "Content-Type": "application/json",
        },
        method="POST",
    )
    # 显式指定代理：传了就用，没传就禁用环境变量里的代理
    handlers = []
    if proxy:
        handlers.append(urllib.request.ProxyHandler({"http": proxy, "https": proxy}))
    else:
        handlers.append(urllib.request.ProxyHandler({}))
    opener = urllib.request.build_opener(*handlers)

    t0 = time.perf_counter()
    with opener.open(req, timeout=timeout) as resp:
        payload = json.loads(resp.read().decode("utf-8"))
    elapsed = (time.perf_counter() - t0) * 1000.0
    return payload, elapsed


def fmt_answer(name, ans):
    if not ans:
        return "    %-10s: <缺失>" % name
    t = ans.get("type")
    if t == "choice":
        return "    %-10s: %-10s conf=%.2f" % (name, ans.get("choice", "?"), ans.get("confidence", 0.0))
    if t == "score":
        return "    %-10s: %.2f       conf=%.2f" % (name, ans.get("score", 0.0), ans.get("confidence", 0.0))
    if t == "noul":
        return "    %-10s: %.2f" % (name, ans.get("noul", 0.0))
    return "    %-10s: %s" % (name, json.dumps(ans, ensure_ascii=False)[:80])


def judge(answers, expect):
    """逐槽位比对，返回 [(槽位, 期望, 实际, 是否判对)]。"""
    rows = []
    for name, want in expect.items():
        ans = answers.get(name) or {}
        t = ans.get("type")
        if t == "choice":
            got = ans.get("choice")
        elif t == "noul":
            raw = ans.get("noul", 0.0)
            got = raw >= PASS_THRESHOLD
        elif t == "score":
            got = ans.get("score", 0.0)
        else:
            got = None
        rows.append((name, want, got, got == want))
    return rows


def main():
    ap = argparse.ArgumentParser(description="Jev 槽位解析探针")
    ap.add_argument("--endpoint", choices=sorted(ENDPOINTS), default="official",
                    help="official=TypeSafe 官方；hosted=第三方托管（免排队）")
    ap.add_argument("--key", help="直接给 API Key（否则读环境变量）")
    ap.add_argument("--proxy", help="代理地址，如 http://127.0.0.1:7897")
    ap.add_argument("--timeout", type=float, default=30.0, help="单次请求超时秒数")
    ap.add_argument("--only", help="只跑指定用例，逗号分隔，如 zh1,en1")
    ap.add_argument("--list", action="store_true", help="列出全部用例后退出")
    ap.add_argument("--raw", action="store_true", help="打印原始响应 JSON")
    ap.add_argument("--model", default=MODEL, help="模型名，生产建议锁定版本号")
    ap.add_argument("--report", help="把结果写成 markdown 报告到指定路径")
    args = ap.parse_args()

    if args.list:
        print("可用用例：")
        for cid, q, note, _ in CASES:
            print("  %-6s %-32s %s" % (cid, q, note))
        return 0

    url, env_name = ENDPOINTS[args.endpoint]
    key = args.key or os.environ.get(env_name, "").strip()
    if not key:
        print("缺少 API Key。", file=sys.stderr)
        print("  请设置环境变量 %s，或用 --key 传入。" % env_name, file=sys.stderr)
        print("  申请：https://jevtypesafeai.com/zh/get-jev", file=sys.stderr)
        return 2

    cases = CASES
    if args.only:
        want = {s.strip() for s in args.only.split(",") if s.strip()}
        cases = [c for c in CASES if c[0] in want]
        if not cases:
            print("--only 没有匹配到任何用例，用 --list 查看。", file=sys.stderr)
            return 2

    print("端点   : %s" % url)
    print("模型   : %s" % args.model)
    print("代理   : %s" % (args.proxy or "（不使用代理）"))
    print("用例数 : %d" % len(cases))
    print("=" * 72)

    lat = []
    errors = 0
    results = []
    for cid, query, note, expect in cases:
        print("\n[%s] %s   （%s）" % (cid, query, note))
        try:
            resp, ms = call_jev(url, key, query, QUESTIONS, args.proxy, args.timeout, args.model)
        except urllib.error.HTTPError as e:
            errors += 1
            detail = ""
            try:
                detail = e.read().decode("utf-8", "replace")[:300]
            except Exception:
                pass
            print("    HTTP %s %s  %s" % (e.code, e.reason, detail))
            results.append({"cid": cid, "query": query, "note": note,
                            "ms": None, "usage": {}, "answers": {},
                            "rows": [], "error": "HTTP %s %s %s" % (e.code, e.reason, detail)})
            continue
        except Exception as e:
            errors += 1
            print("    调用失败: %s: %s" % (type(e).__name__, e))
            results.append({"cid": cid, "query": query, "note": note,
                            "ms": None, "usage": {}, "answers": {},
                            "rows": [], "error": "%s: %s" % (type(e).__name__, e)})
            continue

        lat.append(ms)
        usage = resp.get("usage", {}) or {}
        print("    延迟 %.0f ms | tokens in=%s out=%s"
              % (ms, usage.get("input_tokens", "?"), usage.get("output_tokens", "?")))
        answers = resp.get("answers", {}) or {}
        for name in ("is_search", "is_natural", "type", "time", "location"):
            print(fmt_answer(name, answers.get(name)))

        rows = judge(answers, expect)
        bad = [r for r in rows if not r[3]]
        if rows:
            if bad:
                print("    判定 : %d/%d 正确，错在 %s"
                      % (len(rows) - len(bad), len(rows),
                         ", ".join("%s(期望 %s 得到 %s)" % (r[0], r[1], r[2]) for r in bad)))
            else:
                print("    判定 : %d/%d 全部正确" % (len(rows), len(rows)))

        if args.raw:
            print("    raw: %s" % json.dumps(resp, ensure_ascii=False)[:1200])

        results.append({"cid": cid, "query": query, "note": note, "ms": ms,
                        "usage": usage, "answers": answers, "rows": rows, "error": None})

    # ── 汇总统计 ──────────────────────────────────────────────────────────
    def group_of(cid):
        for p in ("zh", "en", "mix", "kw", "neg"):
            if cid.startswith(p):
                return p
        return "other"

    stats = {}
    for r in results:
        if r["error"]:
            continue
        g = group_of(r["cid"])
        s = stats.setdefault(g, [0, 0])
        for row in r["rows"]:
            s[1] += 1
            if row[3]:
                s[0] += 1

    print("\n" + "=" * 72)
    if lat:
        lat_sorted = sorted(lat)
        n = len(lat_sorted)
        print("成功 %d / 失败 %d" % (n, errors))
        print("延迟   min %.0f ms | p50 %.0f ms | max %.0f ms | 平均 %.0f ms"
              % (lat_sorted[0], lat_sorted[n // 2], lat_sorted[-1], sum(lat) / n))

        if stats:
            print("\n槽位准确率：")
            tot_ok = tot_all = 0
            for g in ("zh", "en", "mix", "kw", "neg"):
                if g in stats:
                    ok, all_ = stats[g]
                    tot_ok += ok
                    tot_all += all_
                    print("  %-4s %2d/%2d  %.0f%%" % (g, ok, all_, 100.0 * ok / all_))
            if tot_all:
                print("  %-4s %2d/%2d  %.0f%%" % ("合计", tot_ok, tot_all, 100.0 * tot_ok / tot_all))

        print("\n判读要点（人工核对）：")
        print("  · is_search 在 neg1/neg2 上应 < 0.5，否则会误触发模型")
        print("  · is_natural 在 kw1/kw2 上应 < 0.5，否则纯关键词也会调模型")
        print("  · zh* 用例的 type/time 是否填对 —— 这直接验证「中文查询」可用性")
        print("  · 对比 zh* 与 en* 的填对率，决定装英文根版还是多语言版")
    else:
        print("没有任何成功调用。失败 %d 次。" % errors)

    if args.report:
        write_report(args.report, url, args.model, args.proxy, results, stats, lat, errors)
        print("\n报告已写入 %s" % args.report)

    return 1 if errors and not lat else 0


def write_report(path, url, model, proxy, results, stats, lat, errors):
    """把本轮实测写成 markdown，便于贴进开发文档或对比多次结果。"""
    lines = []
    lines.append("# Jev 探针实测报告\n")
    lines.append("- 端点：`%s`" % url)
    lines.append("- 模型：`%s`" % model)
    lines.append("- 代理：%s" % (proxy or "（未使用）"))
    lines.append("- 用例：成功 %d / 失败 %d" % (len(lat), errors))
    if lat:
        ls = sorted(lat)
        n = len(ls)
        lines.append("- 延迟：min %.0f ms / p50 %.0f ms / max %.0f ms / 平均 %.0f ms"
                     % (ls[0], ls[n // 2], ls[-1], sum(ls) / n))
    lines.append("")
    lines.append("## 槽位准确率\n")
    lines.append("| 分组 | 判对 | 总数 | 准确率 |")
    lines.append("| --- | --- | --- | --- |")
    tot_ok = tot_all = 0
    for g in ("zh", "en", "mix", "kw", "neg"):
        if g in stats:
            ok, all_ = stats[g]
            tot_ok += ok
            tot_all += all_
            lines.append("| %s | %d | %d | %.0f%% |" % (g, ok, all_, 100.0 * ok / all_))
    if tot_all:
        lines.append("| **合计** | **%d** | **%d** | **%.0f%%** |"
                     % (tot_ok, tot_all, 100.0 * tot_ok / tot_all))
    lines.append("")
    lines.append("## 逐用例明细\n")
    for r in results:
        lines.append("### `%s` %s" % (r["cid"], r["query"]))
        lines.append("")
        if r["error"]:
            lines.append("- **调用失败**：%s" % r["error"])
            lines.append("")
            continue
        lines.append("- 延迟 %.0f ms，tokens in=%s out=%s"
                     % (r["ms"], r["usage"].get("input_tokens", "?"),
                        r["usage"].get("output_tokens", "?")))
        a = r["answers"]
        parts = []
        for name in ("is_search", "is_natural", "type", "time", "location"):
            ans = a.get(name)
            if not ans:
                continue
            t = ans.get("type")
            if t == "choice":
                parts.append("%s=`%s`(%.2f)" % (name, ans.get("choice"), ans.get("confidence", 0.0)))
            elif t == "noul":
                parts.append("%s=%.2f" % (name, ans.get("noul", 0.0)))
            elif t == "score":
                parts.append("%s=%.2f" % (name, ans.get("score", 0.0)))
        lines.append("- 输出：" + "，".join(parts))
        if r["rows"]:
            lines.append("")
            lines.append("| 槽位 | 期望 | 实际 | 结果 |")
            lines.append("| --- | --- | --- | --- |")
            for name, want, got, ok in r["rows"]:
                lines.append("| %s | %s | %s | %s |" % (name, want, got, "✅" if ok else "❌"))
        lines.append("")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
