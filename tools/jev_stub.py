"""Jev 桩服务：本地替身，用于在没有 API Key 的情况下验证闸门 2/3 的**真实链路**。

为什么不直接打真端点：
  - 真端点要 Key、要花钱、要联网，且 p50 ≈ 900 ms
  - 这里要验的是**链路**（请求有没有发出去、响应能不能落到检索范围），
    不是模型的准确率 —— 准确率由 `tools/jev_probe.py` 负责

它会按请求体里**实际问到的问题**作答（只回问过的维度），
所以桩的响应形状与官方一致（§2.1.1），不需要为每个用例改桩。

用法：
  python tools/jev_stub.py --type code --time last_week --port 8931
  python tools/jev_stub.py --type executable --location drive-d
  python tools/jev_stub.py --is-search no          # 模拟「这是闲聊，不是搜索」

把 `ai_jev_endpoint` 指到 http://127.0.0.1:<port>/v1/systemone 即可。
收到的每个请求都会打到 stdout 与 --log 指定的文件。
"""
import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# 各维度的合法取值，与 src/core/decision/slots.rs 的选项表一致
VALID = {
    "type": ["document", "code", "image", "media", "archive", "executable", "folder", "all"],
    "time": ["today", "yesterday", "this_week", "last_week", "this_month", "this_year", "older", "any"],
    "location": ["any", "current", "drive", "common"],
}


def build_answers(spec, asked):
    """只为请求体里问到的问题作答 —— 与官方行为一致。"""
    out = {}
    for name in asked:
        if name == "is_search":
            out[name] = {"type": "noul", "noul": 0.95 if spec.is_search else 0.08}
        elif name == "is_natural":
            out[name] = {"type": "noul", "noul": 0.92 if spec.is_natural else 0.12}
        elif name in VALID:
            choice = getattr(spec, name)
            if choice is None:
                # 没指定就回该维度的「不限」值，等价于「模型没解析出来」
                choice = {"type": "all", "time": "any", "location": "any"}[name]
            out[name] = {"type": "choice", "choice": choice, "confidence": 0.93}
    return {"answers": out}


def make_handler(spec, log_path):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_POST(self):  # noqa: N802
            n = int(self.headers.get("Content-Length", 0))
            raw = self.rfile.read(n).decode("utf-8", "replace")
            auth = self.headers.get("Authorization", "")
            asked = []
            try:
                asked = list(json.loads(raw).get("questions", {}).keys())
            except json.JSONDecodeError as e:
                print(f"[stub] 请求体不是合法 JSON: {e}", file=sys.stderr)

            line = f"{self.command} {self.path} auth={auth[:28]!r} asked={asked}"
            print(f"[stub] {line}", flush=True)
            if log_path:
                with open(log_path, "a", encoding="utf-8") as f:
                    f.write(line + "\n" + raw + "\n")

            payload = json.dumps(build_answers(spec, asked)).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *args):
            pass  # 默认日志太吵，我们只打自己那行

    return Handler


def main():
    p = argparse.ArgumentParser(description="Jev 桩服务")
    p.add_argument("--port", type=int, default=8931)
    for dim, values in VALID.items():
        p.add_argument(f"--{dim}", choices=values, default=None, help=f"{dim} 槽位，取值 {values}")
    p.add_argument("--is-search", dest="is_search", action="store_true", default=True)
    p.add_argument("--no-is-search", dest="is_search", action="store_false")
    p.add_argument("--is-natural", dest="is_natural", action="store_true", default=True)
    p.add_argument("--no-is-natural", dest="is_natural", action="store_false")
    p.add_argument("--log", default=None, help="把收到的请求体追加写到这里")
    spec = p.parse_args()

    server = ThreadingHTTPServer(("127.0.0.1", spec.port), make_handler(spec, spec.log))
    print(f"[stub] 监听 http://127.0.0.1:{spec.port}/v1/systemone", flush=True)
    print(
        f"[stub] 固定答案 type={spec.type} time={spec.time} location={spec.location} "
        f"is_search={spec.is_search} is_natural={spec.is_natural}",
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("[stub] 退出", flush=True)


if __name__ == "__main__":
    main()
