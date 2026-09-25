"""编译内存探针：跑一次 cargo build，同时采样 rustc 的提交量与系统提交余量。

背景：本机 `rustc-LLVM ERROR: out of memory` 的真实约束是**提交限制（commit limit）**，
不是物理内存。这个脚本用来量出「编译这个 crate 到底需要多少 GB 提交量」，
从而判断要腾出多少余量（或页面文件要配多大）才够。

用法：
    python tools/build_mem_probe.py                 # 默认 cargo build
    python tools/build_mem_probe.py -- cargo check   # 换命令
"""
import ctypes
import os
import subprocess
import sys
import time

psapi = ctypes.WinDLL("psapi")
k32 = ctypes.WinDLL("kernel32")


class PMC(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
    ]


class PERF(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("CommitTotal", ctypes.c_size_t),
        ("CommitLimit", ctypes.c_size_t),
        ("CommitPeak", ctypes.c_size_t),
        ("PhysicalTotal", ctypes.c_size_t),
        ("PhysicalAvailable", ctypes.c_size_t),
        ("SystemCache", ctypes.c_size_t),
        ("KernelTotal", ctypes.c_size_t),
        ("KernelPaged", ctypes.c_size_t),
        ("KernelNonpaged", ctypes.c_size_t),
        ("PageSize", ctypes.c_size_t),
        ("HandleCount", ctypes.c_ulong),
        ("ProcessCount", ctypes.c_ulong),
        ("ThreadCount", ctypes.c_ulong),
    ]


class PE32(ctypes.Structure):
    _fields_ = [
        ("dwSize", ctypes.c_ulong),
        ("cntUsage", ctypes.c_ulong),
        ("th32ProcessID", ctypes.c_ulong),
        ("th32DefaultHeapID", ctypes.c_void_p),
        ("th32ModuleID", ctypes.c_ulong),
        ("cntThreads", ctypes.c_ulong),
        ("th32ParentProcessID", ctypes.c_ulong),
        ("pcPriClassBase", ctypes.c_long),
        ("dwFlags", ctypes.c_ulong),
        ("szExeFile", ctypes.c_char * 260),
    ]


k32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
k32.CreateToolhelp32Snapshot.argtypes = [ctypes.c_ulong, ctypes.c_ulong]
k32.Process32First.argtypes = [ctypes.c_void_p, ctypes.POINTER(PE32)]
k32.Process32First.restype = ctypes.c_int
k32.Process32Next.argtypes = [ctypes.c_void_p, ctypes.POINTER(PE32)]
k32.Process32Next.restype = ctypes.c_int
k32.CloseHandle.argtypes = [ctypes.c_void_p]
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
GB = 1073741824


def snapshot():
    """返回 (rustc 提交量合计, 系统提交余量)，单位字节。"""
    snap = k32.CreateToolhelp32Snapshot(2, 0)
    e = PE32()
    e.dwSize = ctypes.sizeof(PE32)
    rustc_commit = 0
    if k32.Process32First(snap, ctypes.byref(e)):
        while True:
            name = e.szExeFile.decode("gbk", "ignore").lower()
            if name.startswith(("rustc", "link", "lld")):
                h = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, e.th32ProcessID)
                if h:
                    p = PMC()
                    p.cb = ctypes.sizeof(PMC)
                    if psapi.GetProcessMemoryInfo(h, ctypes.byref(p), p.cb):
                        rustc_commit += p.PagefileUsage
                    k32.CloseHandle(h)
            if not k32.Process32Next(snap, ctypes.byref(e)):
                break
    k32.CloseHandle(ctypes.c_void_p(snap))

    perf = PERF()
    perf.cb = ctypes.sizeof(PERF)
    psapi.GetPerformanceInfo(ctypes.byref(perf), perf.cb)
    headroom = (perf.CommitLimit - perf.CommitTotal) * perf.PageSize
    return rustc_commit, headroom


def main():
    argv = sys.argv[1:]
    cmd = argv[argv.index("--") + 1:] if "--" in argv else ["cargo", "build"]
    print(f"命令: {' '.join(cmd)}")
    print(f"{'秒':>4}  {'rustc提交':>10}  {'系统余量':>10}")
    print("-" * 32)

    # ⚠️ 必须重定向到文件，不能用 subprocess.PIPE：
    # cargo 的输出会超过 64KB 管道缓冲区，而本脚本只在进程结束后才读管道，
    # 于是 cargo 阻塞在 write 上、脚本阻塞在 poll 上 —— 经典管道死锁（实测卡死 16 分钟）。
    log_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "build_probe.log")
    log_path = os.path.abspath(log_path)
    log = open(log_path, "w", encoding="utf-8", errors="replace")
    proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT, text=True, errors="replace")

    peak_rustc = 0
    min_headroom = float("inf")
    t0 = time.time()
    while proc.poll() is None:
        rc, hr = snapshot()
        peak_rustc = max(peak_rustc, rc)
        min_headroom = min(min_headroom, hr)
        el = time.time() - t0
        if int(el) % 10 == 0:
            print(f"{el:>4.0f}  {rc / GB:>9.2f}G  {hr / GB:>9.2f}G", flush=True)
        time.sleep(1)

    log.close()
    with open(log_path, encoding="utf-8", errors="replace") as f:
        out = f.read()

    print("-" * 32)
    print(f"退出码           : {proc.returncode}")
    print(f"rustc 提交量峰值 : {peak_rustc / GB:.2f} GB   <<< 这就是需要腾出的余量")
    print(f"系统余量最低点   : {min_headroom / GB:.2f} GB")
    print(f"完整日志         : {log_path}")
    if proc.returncode != 0:
        tail = [ln for ln in out.splitlines() if "error" in ln.lower() or "LLVM" in ln]
        print("关键错误行:")
        for ln in tail[-6:]:
            print("   ", ln[:200])


if __name__ == "__main__":
    main()
