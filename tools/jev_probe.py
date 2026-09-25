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
        "instructions": "这个输入是在搜索本机的文件或文件夹吗？（而不是闲聊、提问或执行命令）",
    },
    "is_natural": {
        "type": "noul",
        "instructions": "这个输入是自然语言句子吗？（而不是单纯的关键词）",
    },
}

# ── 测试用例：覆盖中文 / 英文 / 混合 / 纯关键词 / 非检索 ────────────────────
CASES = [
    ("zh1", "找一下昨天改的 docker 配置", "中文·时间+类型"),
    ("zh2", "那个写存储逻辑的文档", "中文·语义指代"),
    ("zh3", "我的项目文件夹在哪", "中文·文件夹"),
    ("zh4", "上周下载的那个压缩包", "中文·上周+压缩包"),
    ("en1", "files I modified yesterday", "英文·时间"),
    ("en2", "the doc about storage logic", "英文·语义"),
    ("en3", "any png screenshots from last week", "英文·类型+时间"),
    ("mix1", "找 Dockerfile 昨天改的", "中英混合"),
    ("kw1", "docker", "纯关键词"),
    ("kw2", "storage.rs", "纯关键词·文件名"),
    ("neg1", "今天天气怎么样", "非检索·闲聊"),
    ("neg2", "帮我写一段快排", "非检索·生成任务"),
]


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
    args = ap.parse_args()

    if args.list:
        print("可用用例：")
        for cid, q, note in CASES:
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
    for cid, query, note in cases:
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
            continue
        except Exception as e:
            errors += 1
            print("    调用失败: %s: %s" % (type(e).__name__, e))
            continue

        lat.append(ms)
        usage = resp.get("usage", {}) or {}
        print("    延迟 %.0f ms | tokens in=%s out=%s"
              % (ms, usage.get("input_tokens", "?"), usage.get("output_tokens", "?")))
        answers = resp.get("answers", {}) or {}
        for name in ("is_search", "is_natural", "type", "time", "location"):
            print(fmt_answer(name, answers.get(name)))
        if args.raw:
            print("    raw: %s" % json.dumps(resp, ensure_ascii=False)[:1200])

    print("\n" + "=" * 72)
    if lat:
        lat_sorted = sorted(lat)
        n = len(lat_sorted)
        print("成功 %d / 失败 %d" % (n, errors))
        print("延迟   min %.0f ms | p50 %.0f ms | max %.0f ms | 平均 %.0f ms"
              % (lat_sorted[0], lat_sorted[n // 2], lat_sorted[-1], sum(lat) / n))
        print("\n判读要点（人工核对）：")
        print("  · is_search 在 neg1/neg2 上应 < 0.5，否则会误触发模型")
        print("  · is_natural 在 kw1/kw2 上应 < 0.5，否则纯关键词也会调模型")
        print("  · zh* 用例的 type/time 是否填对 —— 这直接验证「中文查询」可用性")
        print("  · 对比 zh* 与 en* 的填对率，决定装英文根版还是多语言版")
    else:
        print("没有任何成功调用。失败 %d 次。" % errors)
    return 1 if errors and not lat else 0


if __name__ == "__main__":
    sys.exit(main())
