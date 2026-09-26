"""跑一条命令，同时盯住 Windows **提交余量**；低于阈值就终止 rustc。

为什么需要这个：
    本机提交上限约 39.7G（`D:\\pagefile.sys` 没扩容），而 rustc 编 `anycast`
    这一个 crate 需要约 7G 峰值余量。余量不足时 rustc 报
    `LLVM ERROR: out of memory` → abort（`0xc0000409`），并留下**残缺产物** ——
    后续会伪装成「can't find crate for std」这类假错误，得手工清理 `target/`。

    与其让它撞上限、把整机拖进换页地狱，不如**主动止损**：宁可中断。

用法：
    python tools/build_guard.py --min 0.5 -- cargo build
    python tools/build_guard.py --min 0.5 -- cargo test --no-run
    CARGO_INCREMENTAL=0 python tools/build_guard.py --min 0.5 -- cargo build

退出码与 cargo 一致；被 guard 主动终止时额外打印一行说明。
"""
import argparse
import ctypes
import subprocess
import sys
import threading
import time

G = 2 ** 30


class _Mem(ctypes.Structure):
    _fields_ = [
        ("dwLength", ctypes.c_ulong),
        ("dwMemoryLoad", ctypes.c_ulong),
        ("ullTotalPhys", ctypes.c_ulonglong),
        ("ullAvailPhys", ctypes.c_ulonglong),
        ("ullTotalPageFile", ctypes.c_ulonglong),
        ("ullAvailPageFile", ctypes.c_ulonglong),
        ("ullTotalVirtual", ctypes.c_ulonglong),
        ("ullAvailVirtual", ctypes.c_ulonglong),
        ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
    ]


def commit_free():
    """当前提交余量（字节）。"""
    m = _Mem()
    m.dwLength = ctypes.sizeof(m)
    ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(m))
    return m.ullAvailPageFile


def main():
    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("--min", type=float, default=0.5,
                    help="提交余量下限（GB），低于即终止 rustc（默认 0.5）")
    ap.add_argument("--interval", type=float, default=2.0, help="采样间隔秒（默认 2）")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()

    # 只剥掉**开头**的 `--`（argparse 的选项结束符），不要过滤内部的 `--`：
    # `cargo rustc -- -C debuginfo=0` 里那个 `--` 是语义的一部分，
    # 过滤掉会让命令变成非法的 `cargo rustc -C debuginfo=0`。
    cmd = list(args.cmd)
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        print(__doc__)
        return 2

    low = args.min * G
    peak_used = 0.0
    killed = threading.Event()
    done = threading.Event()

    def watch():
        nonlocal peak_used
        start_av = commit_free()
        while not done.is_set():
            av = commit_free()
            peak_used = max(peak_used, (start_av - av) / G)
            if av < low:
                killed.set()
                print(f"\n!! 提交余量 {av / G:.2f}G < {args.min}G —— 主动终止 rustc（止损）",
                      flush=True)
                subprocess.run(["taskkill", "/F", "/IM", "rustc.exe"], capture_output=True)
                return
            time.sleep(args.interval)

    print(f"[guard] 起始提交余量 {commit_free() / G:.2f}G，下限 {args.min}G")
    t = threading.Thread(target=watch, daemon=True)
    t.start()
    start = time.time()
    p = subprocess.run(cmd)
    done.set()
    t.join(timeout=3)
    print(f"\n[guard] 退出码 {p.returncode}  耗时 {time.time() - start:.0f}s  "
          f"编译期最低提交余量 {(commit_free()) / G:.2f}G（相对起始下降 {peak_used:.2f}G）")
    if killed.is_set():
        print("[guard] 本次是 guard 主动终止的，不是 rustc 自己 OOM")
    return p.returncode


if __name__ == "__main__":
    sys.exit(main())
