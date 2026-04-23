from __future__ import annotations

import sqlite3
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
DB_PATH = REPO_ROOT / ".tmp" / "assistant" / "lba_assistant_index.sqlite"


def main() -> int:
    if not DB_PATH.exists():
        print(f"missing db: {DB_PATH}")
        return 1
    if len(sys.argv) < 2:
        print("usage: py -3 scripts/query_assistant_index.py <query>")
        return 1

    query = " ".join(sys.argv[1:]).strip()
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    try:
        print("== symbols ==")
        for row in conn.execute(
            """
            SELECT s.path, s.line_start, s.kind, s.qualname
            FROM symbol_fts f
            JOIN symbols s ON s.id = f.rowid
            WHERE symbol_fts MATCH ?
            LIMIT 12
            """,
            (query,),
        ):
            print(f"{row['path']}:{row['line_start']} [{row['kind']}] {row['qualname']}")

        print("\n== anchors ==")
        for row in conn.execute(
            """
            SELECT path, anchor_type, anchor_key, line_no
            FROM anchors
            WHERE anchor_key LIKE ?
            ORDER BY path, line_no
            LIMIT 12
            """,
            (f"%{query}%",),
        ):
            print(f"{row['path']}:{row['line_no']} [{row['anchor_type']}] {row['anchor_key']}")

        print("\n== passages ==")
        for row in conn.execute(
            """
            SELECT p.path, p.line_start, p.line_end, substr(replace(p.content, char(10), ' '), 1, 180) AS snippet
            FROM passage_fts f
            JOIN passages p ON p.id = f.rowid
            WHERE passage_fts MATCH ?
            LIMIT 8
            """,
            (query,),
        ):
            print(f"{row['path']}:{row['line_start']}-{row['line_end']} {row['snippet']}")
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
