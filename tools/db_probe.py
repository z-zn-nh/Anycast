"""索引库诊断：表规模 / 空间分布 / FTS 膨胀 / 一致性。

用法：
    python tools/db_probe.py [db_path]

默认读 %APPDATA%\\Anycast\\data\\anycast.db 的**副本**（避免动到正在被应用占用的库）。
"""

import os
import sqlite3
import sys

DEFAULT = os.path.expandvars(r"%APPDATA%\Anycast\data\anycast.db")


def q1(cur, sql, *args):
    try:
        return cur.execute(sql, args).fetchone()
    except sqlite3.Error as e:
        return (f"<err {e}>",)


def scalar(cur, sql, *args):
    row = q1(cur, sql, *args)
    return row[0] if row else None


def mb(n):
    if not isinstance(n, (int, float)):
        return str(n)
    return f"{n / 1048576:.2f} MB"


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT
    if not os.path.exists(path):
        print(f"数据库不存在: {path}")
        return 1
    print(f"数据库: {path}")
    print(f"文件大小: {mb(os.path.getsize(path))}")
    wal = path + "-wal"
    if os.path.exists(wal):
        print(f"WAL     : {mb(os.path.getsize(wal))}")

    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    cur = conn.cursor()

    print("\n=== 表行数 ===")
    tables = [r[0] for r in cur.execute(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    ).fetchall()]
    for t in tables:
        n = scalar(cur, f"SELECT COUNT(*) FROM [{t}]")
        print(f"  {t:20s} {n}")

    print("\n=== 页面 / 空间 ===")
    pc = scalar(cur, "PRAGMA page_count")
    ps = scalar(cur, "PRAGMA page_size")
    fl = scalar(cur, "PRAGMA freelist_count")
    print(f"  page_count={pc}  page_size={ps}  合计={mb(pc * ps)}")
    print(f"  freelist={fl} 页 = {mb(fl * ps)}  ({fl / pc * 100:.1f}%)")
    print(f"  已用 = {mb((pc - fl) * ps)}")

    print("\n=== dbstat（各对象占用） ===")
    try:
        rows = cur.execute(
            "SELECT name, SUM(pgsize) AS sz, COUNT(*) AS pages FROM dbstat GROUP BY name ORDER BY sz DESC LIMIT 25"
        ).fetchall()
        for name, sz, pages in rows:
            print(f"  {name:28s} {mb(sz):>12s}  {pages} 页")
    except sqlite3.Error as e:
        print(f"  <dbstat 不可用: {e}>")

    print("\n=== files 表样本 ===")
    try:
        rows = cur.execute(
            "SELECT id, path, name, is_dir, size, mtime FROM files ORDER BY id LIMIT 10"
        ).fetchall()
        for r in rows:
            print(f"  {r}")
        if not rows:
            print("  (空表)")
    except sqlite3.Error as e:
        print(f"  <err {e}>")

    print("\n=== content_fts 样本 ===")
    try:
        rows = cur.execute("SELECT rowid, path, length(body) FROM content_fts LIMIT 10").fetchall()
        for r in rows:
            print(f"  {r}")
        if not rows:
            print("  (空表)")
        print(f"  正文总字节: {mb(scalar(cur, 'SELECT SUM(length(body)) FROM content_fts') or 0)}")
    except sqlite3.Error as e:
        print(f"  <err {e}>")

    print("\n=== kv ===")
    try:
        for r in cur.execute("SELECT key, substr(value,1,80) FROM kv LIMIT 20").fetchall():
            print(f"  {r}")
    except sqlite3.Error as e:
        print(f"  <err {e}>")

    print("\n=== 索引库配置 ===")
    for pragma in ("journal_mode", "synchronous", "auto_vacuum", "user_version", "mmap_size"):
        print(f"  {pragma} = {scalar(cur, f'PRAGMA {pragma}')}")

    conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
