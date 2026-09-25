#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""Everything 集成诊断探针 —— 判断本机能否用 Everything 做文件名索引加速。

背景见 doc/检索增强与判断模型接入开发文档.md §4.3。

为什么要这个脚本
----------------
Everything 的 SDK **不是自包含的**：它只是一层 IPC 客户端，必须有一个正在运行的
Everything 主程序（或 Everything 服务）在后面应答，否则一切查询都返回
EVERYTHING_ERROR_IPC(2)。所以在决定「要不要接 Everything」之前，先跑这个脚本，
它会明确告诉你卡在哪一环。

用法
----
    python tools/everything_probe.py                    # 全量诊断
    python tools/everything_probe.py --query "*.rs"     # 顺带试一次真实查询
    python tools/everything_probe.py --dll D:\\path\\Everything64.dll
    python tools/everything_probe.py --no-http          # 跳过 HTTP server 探测

退出码
------
    0  全部就绪（能查到结果）
    1  部分可用（DLL 在，但主程序没跑）
    2  完全不可用（找不到 DLL）
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import socket
import struct
import sys
import urllib.error
import urllib.request

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# ── Everything SDK 常量（voidtools 官方头文件）──────────────────────────────
ERR_NAMES = {
    0: "OK",
    1: "ERROR_MEMORY（内存不足）",
    2: "ERROR_IPC（连不上 Everything 主程序 —— 最常见）",
    3: "ERROR_REGISTERCLASSEX",
    4: "ERROR_CREATEWINDOW",
    5: "ERROR_CREATETHREAD",
    6: "ERROR_INVALIDINDEX",
    7: "ERROR_INVALIDCALL",
    8: "ERROR_INVALIDREQUEST",
    9: "ERROR_INVALIDPARAMETER",
}

REQ_FILE_NAME = 0x00000001
REQ_PATH = 0x00000002
REQ_FULL_PATH = 0x00000004
REQ_EXTENSION = 0x00000008
REQ_SIZE = 0x00000010
REQ_DATE_CREATED = 0x00000020
REQ_DATE_MODIFIED = 0x00000040
REQ_DATE_ACCESSED = 0x00000080

SORT_DATE_MODIFIED_DESC = 14

# DLL 候选路径：本机已知位置优先（DeskBox 打包的那份），再是官方安装位置
DLL_CANDIDATES = [
    r"D:\DeskBox\EverythingSdk.dll",
    r"C:\Program Files\Everything\Everything64.dll",
    r"C:\Program Files (x86)\Everything\Everything32.dll",
    r"C:\Program Files\Everything\Everything32.dll",
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "Everything64.dll"),
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "EverythingSdk.dll"),
]


class FILETIME(ctypes.Structure):
    _fields_ = [("dwLowDateTime", wt.DWORD), ("dwHighDateTime", wt.DWORD)]


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wt.DWORD),
        ("cntUsage", wt.DWORD),
        ("th32ProcessID", wt.DWORD),
        ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
        ("th32ModuleID", wt.DWORD),
        ("cntThreads", wt.DWORD),
        ("th32ParentProcessID", wt.DWORD),
        ("pcPriClassBase", ctypes.c_long),
        ("dwFlags", wt.DWORD),
        ("szExeFile", wt.WCHAR * 260),
    ]


def find_processes(names):
    """用 Toolhelp32 快照找进程，返回 [(pid, exe)]。避免依赖 tasklist 的参数怪癖。"""
    TH32CS_SNAPPROCESS = 0x00000002
    k = ctypes.windll.kernel32
    k.CreateToolhelp32Snapshot.restype = wt.HANDLE
    k.CreateToolhelp32Snapshot.argtypes = [wt.DWORD, wt.DWORD]
    k.Process32FirstW.argtypes = [wt.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    k.Process32FirstW.restype = wt.BOOL
    k.Process32NextW.argtypes = [wt.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    k.Process32NextW.restype = wt.BOOL
    k.CloseHandle.argtypes = [wt.HANDLE]

    snap = k.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if snap == wt.HANDLE(-1).value or snap is None:
        return []
    out = []
    pe = PROCESSENTRY32W()
    pe.dwSize = ctypes.sizeof(PROCESSENTRY32W)
    try:
        ok = k.Process32FirstW(snap, ctypes.byref(pe))
        while ok:
            exe = pe.szExeFile
            low = exe.lower()
            if any(n in low for n in names):
                out.append((pe.th32ProcessID, exe))
            ok = k.Process32NextW(snap, ctypes.byref(pe))
    finally:
        k.CloseHandle(snap)
    return out


def find_ipc_window():
    """Everything 用隐藏消息窗口做 IPC 锚点。窗口在 = 主程序在跑。"""
    u = ctypes.windll.user32
    u.FindWindowW.restype = wt.HWND
    u.FindWindowW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
    for cls in ("EVERYTHING_TASKBAR_NOTIFICATION", "EVERYTHING_IPC_WNDCLASS"):
        h = u.FindWindowW(cls, None)
        if h:
            return cls, h
    # 兜底：按标题枚举
    found = []
    CB = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)

    def cb(h, _):
        buf = ctypes.create_unicode_buffer(512)
        u.GetWindowTextW(h, buf, 512)
        if "everything" in buf.value.lower():
            found.append((h, buf.value))
        return True

    u.EnumWindows(CB(cb), 0)
    if found:
        return "标题匹配", found[0][0]
    return None, None


def probe_http(ports, timeout=0.6):
    """探测 Everything 内置 HTTP server（需用户在设置里手动开启）。"""
    hits = []
    for port in ports:
        # 先做一次 TCP 连通性，避免每个端口都等满超时
        s = socket.socket()
        s.settimeout(timeout)
        try:
            s.connect(("127.0.0.1", port))
        except Exception:
            s.close()
            continue
        s.close()
        url = "http://127.0.0.1:%d/?search=test&json=1&count=3" % port
        try:
            with urllib.request.urlopen(url, timeout=2.0) as r:
                body = r.read().decode("utf-8", "replace")
            try:
                j = json.loads(body)
                hits.append((port, "JSON 可用，totalResults=%s" % j.get("totalResults", "?")))
            except Exception:
                hits.append((port, "端口通但响应非 JSON（前 80 字符：%s）" % body[:80]))
        except urllib.error.HTTPError as e:
            hits.append((port, "HTTP %s" % e.code))
        except Exception as e:
            hits.append((port, "端口通但请求失败：%s" % e))
    return hits


def load_sdk(path):
    """加载 DLL 并绑定函数签名。返回 (dll, err)。"""
    try:
        E = ctypes.WinDLL(path)
    except OSError as e:
        return None, str(e)
    try:
        E.Everything_SetSearchW.argtypes = [wt.LPCWSTR]
        E.Everything_SetSearchW.restype = None
        E.Everything_SetMatchPath.argtypes = [wt.BOOL]
        E.Everything_SetMatchCase.argtypes = [wt.BOOL]
        E.Everything_SetMatchWholeWord.argtypes = [wt.BOOL]
        E.Everything_SetRegex.argtypes = [wt.BOOL]
        E.Everything_SetMax.argtypes = [wt.DWORD]
        E.Everything_SetOffset.argtypes = [wt.DWORD]
        E.Everything_SetRequestFlags.argtypes = [wt.DWORD]
        E.Everything_SetSort.argtypes = [wt.DWORD]
        E.Everything_QueryW.argtypes = [wt.BOOL]
        E.Everything_QueryW.restype = wt.BOOL
        E.Everything_GetNumResults.restype = wt.DWORD
        E.Everything_GetNumFileResults.restype = wt.DWORD
        E.Everything_GetNumFolderResults.restype = wt.DWORD
        E.Everything_GetLastError.restype = wt.DWORD
        E.Everything_GetMajorVersion.restype = wt.DWORD
        E.Everything_GetMinorVersion.restype = wt.DWORD
        E.Everything_GetBuildNumber.restype = wt.DWORD
        E.Everything_IsDBLoaded.restype = wt.BOOL
        E.Everything_IsFileResult.argtypes = [wt.DWORD]
        E.Everything_IsFileResult.restype = wt.BOOL
        E.Everything_IsFolderResult.argtypes = [wt.DWORD]
        E.Everything_IsFolderResult.restype = wt.BOOL
        E.Everything_GetResultFileNameW.argtypes = [wt.DWORD]
        E.Everything_GetResultFileNameW.restype = wt.LPCWSTR
        E.Everything_GetResultPathW.argtypes = [wt.DWORD]
        E.Everything_GetResultPathW.restype = wt.LPCWSTR
        E.Everything_GetResultFullPathNameW.argtypes = [wt.DWORD, wt.LPWSTR, wt.DWORD]
        E.Everything_GetResultFullPathNameW.restype = wt.DWORD
        E.Everything_GetResultSize.argtypes = [wt.DWORD, ctypes.POINTER(ctypes.c_ulonglong)]
        E.Everything_GetResultSize.restype = wt.BOOL
        E.Everything_GetResultDateModified.argtypes = [wt.DWORD, ctypes.POINTER(FILETIME)]
        E.Everything_GetResultDateModified.restype = wt.BOOL
        E.Everything_CleanUp.restype = None
    except AttributeError as e:
        return None, "DLL 缺少预期导出：%s" % e
    return E, None


def ft_to_str(ft):
    if not ft.dwLowDateTime and not ft.dwHighDateTime:
        return "(无)"
    v = (ft.dwHighDateTime << 32) | ft.dwLowDateTime
    if v < 116444736000000000:
        return "(无效)"
    import datetime
    secs = (v - 116444736000000000) / 10000000.0
    return datetime.datetime.fromtimestamp(secs).strftime("%Y-%m-%d %H:%M:%S")


def run_query(E, query, limit=5, sort_by_date=True):
    """跑一次真实查询，返回 (ok, err, rows, n_files, n_folders)。"""
    E.Everything_SetSearchW(query)
    E.Everything_SetMatchPath(False)
    E.Everything_SetMatchCase(False)
    E.Everything_SetMatchWholeWord(False)
    E.Everything_SetRegex(False)
    E.Everything_SetMax(limit)
    E.Everything_SetOffset(0)
    E.Everything_SetRequestFlags(
        REQ_FULL_PATH | REQ_SIZE | REQ_DATE_MODIFIED | REQ_EXTENSION
    )
    if sort_by_date:
        E.Everything_SetSort(SORT_DATE_MODIFIED_DESC)

    ok = bool(E.Everything_QueryW(True))
    err = E.Everything_GetLastError()
    if not ok:
        return False, err, [], 0, 0

    n = E.Everything_GetNumResults()
    nf = E.Everything_GetNumFileResults()
    nd = E.Everything_GetNumFolderResults()
    rows = []
    for i in range(min(n, limit)):
        buf = ctypes.create_unicode_buffer(1024)
        E.Everything_GetResultFullPathNameW(i, buf, 1024)
        size = ctypes.c_ulonglong(0)
        E.Everything_GetResultSize(i, ctypes.byref(size))
        ft = FILETIME()
        E.Everything_GetResultDateModified(i, ctypes.byref(ft))
        rows.append({
            "path": buf.value,
            "size": size.value,
            "modified": ft_to_str(ft),
            "is_folder": bool(E.Everything_IsFolderResult(i)),
        })
    return True, err, rows, nf, nd


def main():
    ap = argparse.ArgumentParser(description="Everything 集成诊断探针")
    ap.add_argument("--query", default="*.rs", help="试查询词（默认 *.rs）")
    ap.add_argument("--limit", type=int, default=5, help="最多列几条结果")
    ap.add_argument("--dll", help="手动指定 Everything DLL 路径")
    ap.add_argument("--no-http", action="store_true", help="跳过 HTTP server 探测")
    ap.add_argument("--http-ports", default="80,8080,8081",
                    help="要探测的 HTTP 端口，逗号分隔（默认 80,8080,8081）")
    args = ap.parse_args()

    print("=" * 72)
    print("Everything 集成诊断")
    print("=" * 72)

    # ── 1. 进程 ────────────────────────────────────────────────────────────
    procs = find_processes(["everything.exe"])
    if procs:
        print("[1/5] 进程        : 运行中 -> %s"
              % ", ".join("%s (pid %d)" % (e, p) for p, e in procs))
    else:
        print("[1/5] 进程        : 未运行")

    # ── 2. IPC 窗口 ────────────────────────────────────────────────────────
    cls, hwnd = find_ipc_window()
    if hwnd:
        print("[2/5] IPC 窗口    : 存在 (%s, hwnd=0x%X)" % (cls, hwnd))
    else:
        print("[2/5] IPC 窗口    : 不存在 —— 说明 Everything 主程序没在跑")

    # ── 3. 服务 ────────────────────────────────────────────────────────────
    svc = None
    try:
        import winreg
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                           r"SYSTEM\CurrentControlSet\Services\Everything")
        svc = winreg.QueryValueEx(k, "ImagePath")[0]
        print("[3/5] Everything 服务: 已安装 -> %s" % svc)
    except Exception:
        print("[3/5] Everything 服务: 未安装（Everything 服务用于无管理员权限建索引）")

    # ── 4. SDK DLL ─────────────────────────────────────────────────────────
    candidates = [args.dll] if args.dll else DLL_CANDIDATES
    found = []
    for p in candidates:
        if p and os.path.isfile(p):
            E, err = load_sdk(p)
            found.append((p, E, err))
    if not found:
        print("[4/5] SDK DLL     : 未找到")
        print("\n结论：完全不可用 —— 连 SDK DLL 都没有。")
        print("建议：到 https://www.voidtools.com/downloads/ 下载 Everything SDK，")
        print("      把 Everything64.dll 放到本脚本同目录，或安装 Everything 主程序。")
        return 2
    for p, E, err in found:
        mark = "可加载" if E else "加载失败(%s)" % err
        print("[4/5] SDK DLL     : %s  [%s]" % (p, mark))

    usable = [(p, E) for p, E, err in found if E]
    if not usable:
        print("\n结论：完全不可用 —— DLL 存在但加载失败。")
        return 2

    path, E = usable[0]
    print("      版本探测    : %s" % (
        "major=%s minor=%s build=%s"
        % (E.Everything_GetMajorVersion(), E.Everything_GetMinorVersion(),
           E.Everything_GetBuildNumber())
        if hwnd else "主程序未运行，版本号不可用"
    ))
    print("      索引已加载  : %s" % ("是" if E.Everything_IsDBLoaded() else "否"))

    # ── 5. 真实查询 ────────────────────────────────────────────────────────
    print("\n[5/5] 试查询      : %r" % args.query)
    ok, err, rows, nf, nd = run_query(E, args.query, args.limit)
    if ok:
        print("      结果        : 文件 %d 个 / 文件夹 %d 个（本次取前 %d 条）" % (nf, nd, len(rows)))
        for r in rows:
            kind = "DIR " if r["is_folder"] else "FILE"
            print("        %s  %10d  %s  %s" % (kind, r["size"], r["modified"], r["path"]))
    else:
        print("      结果        : 失败 err=%d %s" % (err, ERR_NAMES.get(err, "未知错误")))

    # ── 附：HTTP server ────────────────────────────────────────────────────
    http_hits = []
    if not args.no_http:
        ports = [int(x) for x in args.http_ports.split(",") if x.strip()]
        http_hits = probe_http(ports)
        print("\n[附] HTTP server  : %s" % (
            "；".join("端口 %d -> %s" % h for h in http_hits) if http_hits
            else "未开启（%s 端口均无响应）" % ",".join(str(p) for p in ports)))

    E.Everything_CleanUp()

    # ── 结论 ───────────────────────────────────────────────────────────────
    print("\n" + "=" * 72)
    print("结论与建议")
    print("=" * 72)
    if ok:
        print("Everything 就绪。Anycast 可以直接走 SDK 做文件名检索。")
        print("注意：Everything 只索引**文件名与路径**，不索引文件内容；")
        print("      内容检索仍需自建 FTS5 索引（见开发文档 §4.3）。")
        return 0

    print("SDK DLL 可用，但 Everything 主程序未运行，所以查询返回 IPC 错误。")
    print("这正是 §4.3 里那条结论的实测复现：SDK 只是 IPC 客户端，不是索引器。")
    print("")
    print("要让 Anycast 用上 Everything，需要二选一：")
    print("  A. 引导用户安装 Everything（voidtools 官方，Freeware）")
    print("     安装后 Everything 常驻托盘，SDK 立即可用；")
    print("     建议同时装 Everything 服务，免管理员权限也能索引。")
    print("  B. 保持自建索引兜底（当前 walkdir + 后续 FTS5）")
    print("     不依赖第三方常驻程序，但冷启动与增量成本更高。")
    print("")
    print("推荐：A + B 并存 —— Everything 存在则用，不存在则回落到自建索引。")
    print("      这也符合开发文档里 IndexProvider 双后端的定位。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
