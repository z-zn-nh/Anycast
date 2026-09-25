"""索引增量对账 / 实时监控的端到端验证。

在一个隔离的临时目录树上，真跑 `anycast.exe` 的索引路径，断言：

  A. 首次扫描（空库）建全索引，排除目录与隐藏文件被跳过
  B. 无变更时对账几乎零工作（目录变化数 = 0）
  C. 改文件内容不会改变目录 mtime → 对账看不到（已知盲区），
     但 `--full` 全量校验能补回来
  D. 实时监控（notify）能接住 新增 / 删除 / 改名，
     且目录改名后 `pins` / `recent` 的引用被一起迁移

用法：
    python tools/index_reconcile_test.py [--keep]

`--keep` 保留临时目录便于人工检查。退出码 0 表示全部通过。
"""

import argparse
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "target", "debug", "anycast.exe")

PASS, FAIL = [], []


def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    mark = "PASS" if ok else "FAIL"
    print(f"  [{mark}] {name}" + (f"  —— {detail}" if detail else ""))


def run_scan(db, tree, *extra):
    out = subprocess.run(
        [EXE, "--index-scan", "--db", db, "--root", tree, *extra],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    return out.stdout + out.stderr


def parse(text, key):
    """命令行输出形如 `  索引文件        681 条`（空格分隔，无冒号）。"""
    for line in text.splitlines():
        s = line.strip()
        if s.startswith(key):
            return s[len(key):].strip()
    return None


def num(text, key):
    v = parse(text, key)
    if v is None:
        return None
    try:
        return int(v.split()[0])
    except ValueError:
        return None


def pair(text, key):
    """解析 `81 / 81` 这类成对数值。"""
    v = parse(text, key)
    if v is None:
        return (None, None)
    parts = [p for p in v.replace("/", " ").split() if p.isdigit()]
    return (int(parts[0]), int(parts[1])) if len(parts) >= 2 else (None, None)


def line_has(text, key, needle):
    for line in text.splitlines():
        if key in line and needle in line:
            return True
    return False


def duration_ms(text):
    """解析 Rust Duration 的 Debug 输出（如 `10.5459ms` / `1.0199124s`）。"""
    v = parse(text, "耗时")
    if not v:
        return None
    v = v.split()[0]
    for suffix, scale in (("ms", 1.0), ("µs", 0.001), ("ns", 1e-6), ("s", 1000.0)):
        if v.endswith(suffix):
            try:
                return float(v[: -len(suffix)]) * scale
            except ValueError:
                return None
    return None


def build_tree(tree):
    for d in range(1, 21):
        os.makedirs(os.path.join(tree, f"dir{d}", f"sub{d}"), exist_ok=True)
        for f in range(1, 6):
            with open(os.path.join(tree, f"dir{d}", f"file{f}.txt"), "w", encoding="utf-8") as fh:
                fh.write(f"content {d} {f}\n")
        for f in range(1, 3):
            with open(os.path.join(tree, f"dir{d}", f"sub{d}", f"s{f}.md"), "w", encoding="utf-8") as fh:
                fh.write(f"sub {d} {f}\n")
    # 排除目录 + 隐藏文件
    os.makedirs(os.path.join(tree, "node_modules", "pkg"), exist_ok=True)
    os.makedirs(os.path.join(tree, "dir1", "target"), exist_ok=True)
    with open(os.path.join(tree, "node_modules", "pkg", "index.js"), "w") as fh:
        fh.write("x")
    with open(os.path.join(tree, "dir1", "target", "a.o"), "w") as fh:
        fh.write("x")
    with open(os.path.join(tree, ".hidden.txt"), "w") as fh:
        fh.write("x")


def q(db, sql, args=()):
    c = sqlite3.connect(db)
    try:
        return c.execute(sql, args).fetchall()
    finally:
        c.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()

    if not os.path.exists(EXE):
        print(f"找不到 {EXE}，先 cargo build")
        return 2

    base = tempfile.mkdtemp(prefix="anycast_idx_")
    tree = os.path.join(base, "tree")
    db = os.path.join(base, "anycast.db")
    os.makedirs(tree)
    print(f"临时目录: {base}\n")

    try:
        build_tree(tree)

        # ---------------- A. 首次全量 ----------------
        print("A. 空库首次扫描")
        t0 = time.perf_counter()
        out = run_scan(db, tree)
        t_full = time.perf_counter() - t0
        files = num(out, "索引文件")
        visited, changed = pair(out, "目录访问/变化")
        # build_tree: 20 个 dir{d} × 5 文件 + 20 个 sub{d} × 2 文件 = 140 个文件
        #             root + 20 dir{d} + 20 sub{d}              =  41 个目录
        # 注意 CLI 的「索引文件」字段是 files 表的**总行数**（含目录行），不是纯文件数
        check("首次扫描索引到 181 行（140 文件 + 41 目录）", files == 181, f"{files} 条")
        check("丢弃条目为 0", num(out, "丢弃条目") == 0)
        check("一致性正常", line_has(out, "一致性", "正常"))
        check("首次扫描所有目录都判定为变化", visited == changed and (visited or 0) > 20,
              f"{visited} / {changed}")
        check(
            "排除目录 node_modules / target 未被索引",
            q(db, "SELECT COUNT(*) FROM files WHERE path LIKE '%node_modules%' OR path LIKE '%\\target\\%'")[0][0] == 0,
        )
        check(
            "隐藏文件未被索引",
            q(db, "SELECT COUNT(*) FROM files WHERE name LIKE '.%'")[0][0] == 0,
        )
        check(
            "索引根目录自身有行、且是目录",
            q(db, "SELECT COUNT(*) FROM files WHERE path = ? AND is_dir = 1", (tree,))[0][0] == 1,
        )
        check(
            "总行数 = 文件 + 目录",
            q(db, "SELECT COUNT(*) FROM files")[0][0] == 181,
            str(q(db, "SELECT COUNT(*) FROM files")[0][0]),
        )
        # 根行的 parent 指向索引范围之外，无从「对上」——只要求根以下的每一行都能对上
        check(
            "根以下所有子项的 parent 都能对上一个已索引的目录行",
            q(db, "SELECT COUNT(*) FROM files f WHERE f.path <> ? AND NOT EXISTS "
                  "(SELECT 1 FROM files p WHERE p.path = f.parent AND p.is_dir = 1)", (tree,))[0][0] == 0,
        )
        print(f"     首次耗时 进程内 {duration_ms(out):.0f} ms / 墙钟 {t_full*1000:.0f} ms（含启动与建库）")
        full_inner = duration_ms(out)

        # ---------------- B. 无变更 ----------------
        print("\nB. 无任何变更时对账")
        out = run_scan(db, tree)
        t_reconcile = duration_ms(out)
        _, changed = pair(out, "目录访问/变化")
        check("目录变化数为 0", changed == 0, str(parse(out, "目录访问/变化")))
        check("移除条目为 0", num(out, "移除条目") == 0)
        # 用进程内自报的耗时比较：墙钟被固定启动开销（进程拉起 + 建库）主导，没有意义
        check("耗时远低于首次全量", (t_reconcile or 9e9) * 10 < (full_inner or 0),
              f"进程内 {t_reconcile:.1f} ms vs 首次 {full_inner:.0f} ms")

        # ---------------- C. 改内容：对账看不到，--full 能补 ----------------
        print("\nC. 改写文件内容（目录 mtime 不变）")
        target = os.path.join(tree, "dir5", "file1.txt")
        with open(target, "w", encoding="utf-8") as fh:
            fh.write("UNIQUE_MARKER_CONTENT_XYZ\n")
        before = q(db, "SELECT mtime_ns FROM content_meta WHERE path = ?", (target,))
        out = run_scan(db, tree)
        after = q(db, "SELECT mtime_ns FROM content_meta WHERE path = ?", (target,))
        check("增量对账确实看不到内容改写（已知盲区）", before == after,
              "content_meta.mtime_ns 未变")
        out = run_scan(db, tree, "--full")
        after_full = q(db, "SELECT mtime_ns FROM content_meta WHERE path = ?", (target,))
        check("--full 全量校验补回了内容改写", before != after_full)
        check(
            "--full 后正文可检索到新内容",
            q(db, "SELECT COUNT(*) FROM content_fts WHERE content_fts MATCH ?", ('"UNIQUE_MARKER_CONTENT_XYZ"',))[0][0] == 1,
        )
        v, c = pair(out, "目录访问/变化")
        check("--full 模式下每个目录都被检查", v == c and (v or 0) > 20, f"{v} / {c}")

        # ---------------- D. 实时监控 ----------------
        print("\nD. 实时监控（notify）")
        pinned = os.path.join(tree, "dir4", "file1.txt")
        c = sqlite3.connect(db)
        c.execute(
            "INSERT INTO pins(item_id, kind, title, subtitle, path, badge, icon, sort) "
            "VALUES (?, 'file', 'file1.txt', '', ?, '', '', 1)",
            (f"file:{pinned}", pinned),
        )
        c.execute(
            "INSERT INTO recent(item_id, kind, title, subtitle, path, badge, icon, last_used, use_count) "
            "VALUES (?, 'file', 'file1.txt', '', ?, '', '', 1, 1)",
            (f"file:{pinned}", pinned),
        )
        c.commit()
        c.close()

        watch_secs = 30
        proc = subprocess.Popen(
            [EXE, "--index-watch", "--db", db, "--root", tree, "--seconds", str(watch_secs)],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
            encoding="utf-8", errors="replace",
        )
        time.sleep(6)  # 等首轮对账落定

        created = os.path.join(tree, "dir1", "created_by_test.txt")
        deleted = os.path.join(tree, "dir2", "file5.txt")
        renamed_from = os.path.join(tree, "dir3", "file1.txt")
        renamed_to = os.path.join(tree, "dir3", "renamed_by_test.txt")
        dir_from = os.path.join(tree, "dir4")
        dir_to = os.path.join(tree, "dir4_renamed")
        moved_in = os.path.join(tree, "dir8_moved_in")   # 从监控范围外移入
        moved_out = os.path.join(tree, "dir9")           # 移到监控范围外
        outside = os.path.join(base, "outside")
        os.makedirs(outside, exist_ok=True)
        os.makedirs(os.path.join(outside, "payload"))
        for i in range(3):
            with open(os.path.join(outside, "payload", f"p{i}.txt"), "w", encoding="utf-8") as fh:
                fh.write(f"payload {i}\n")

        with open(created, "w", encoding="utf-8") as fh:
            fh.write("created\n")
        os.remove(deleted)
        os.rename(renamed_from, renamed_to)
        os.rename(dir_from, dir_to)
        # 整棵目录从监控范围外搬进来：notify 只给一条 Create，子树要自己递归补
        os.rename(os.path.join(outside, "payload"), moved_in)
        time.sleep(4)

        # 整棵目录搬出监控范围：只会收到 From，没有 To → 超时后按删除清理
        os.rename(moved_out, os.path.join(outside, "dir9"))
        time.sleep(6.5)  # 越过 5 秒配对时限
        # 再触发一条任意事件，让悬空记录被扫掉
        with open(os.path.join(tree, "dir1", "trigger.txt"), "w", encoding="utf-8") as fh:
            fh.write("t\n")
        time.sleep(3)

        proc.wait(timeout=watch_secs + 30)
        watch_out = proc.stdout.read() if proc.stdout else ""
        print("     —— 监控进程输出（节选）——")
        for line in watch_out.splitlines():
            if any(k in line for k in ("索引", "移除", "错误", "监控")):
                print("       " + line.strip())

        check("新增文件已入索引", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (created,))[0][0] == 1)
        check("删除的文件已移出索引", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (deleted,))[0][0] == 0)
        check("改名后的新路径已入索引", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (renamed_to,))[0][0] == 1)
        check("改名前的旧路径已不存在", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (renamed_from,))[0][0] == 0)

        new_dir = os.path.join(dir_to, "file1.txt")
        check("目录改名：新目录行已入索引", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (dir_to,))[0][0] == 1)
        check("目录改名：旧目录行已不存在", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (dir_from,))[0][0] == 0)
        check("目录改名：子树整体迁移", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (new_dir,))[0][0] == 1)

        check("目录移入：目录行已入索引", q(db, "SELECT COUNT(*) FROM files WHERE path = ?", (moved_in,))[0][0] == 1)
        check(
            "目录移入：子树被递归补齐（不是只写目录行）",
            q(db, "SELECT COUNT(*) FROM files WHERE path LIKE ?", (moved_in + "\\%",))[0][0] == 3,
            str(q(db, "SELECT path FROM files WHERE path LIKE ?", (moved_in + "\\%",))),
        )
        check(
            "目录移出：悬空改名超时后被清理",
            q(db, "SELECT COUNT(*) FROM files WHERE path = ? OR path LIKE ?",
              (moved_out, moved_out + "\\%"))[0][0] == 0,
        )

        pins = q(db, "SELECT item_id, path FROM pins WHERE kind = 'file'")
        check(
            "目录改名：置顶引用被一起迁移",
            bool(pins) and pins[0][0] == f"file:{new_dir}" and pins[0][1] == new_dir,
            str(pins),
        )
        rec = q(db, "SELECT item_id, path FROM recent WHERE kind = 'file'")
        check(
            "目录改名：最近使用引用被一起迁移",
            bool(rec) and rec[0][0] == f"file:{new_dir}" and rec[0][1] == new_dir,
            str(rec),
        )
        check("监控期间丢弃条目为 0", num(watch_out, "丢弃条目") == 0)
        check(
            "监控结束后无孤儿正文",
            q(db, "SELECT COUNT(*) FROM content_meta m WHERE NOT EXISTS "
                  "(SELECT 1 FROM files f WHERE f.path = m.path)")[0][0] == 0,
        )

    finally:
        if a.keep:
            print(f"\n保留临时目录: {base}")
        else:
            shutil.rmtree(base, ignore_errors=True)

    print(f"\n===== 通过 {len(PASS)} / 失败 {len(FAIL)} =====")
    for f in FAIL:
        print(f"  失败: {f}")
    return 0 if not FAIL else 1


if __name__ == "__main__":
    sys.exit(main())
