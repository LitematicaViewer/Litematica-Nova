from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from pathlib import Path

from litematicaba.core.config import user_data_dir


@dataclass(frozen=True, slots=True)
class TileLayoutRecord:
    tile_key: str
    col: int
    row: int
    span_cols: int
    span_rows: int


class ContentTileLayoutDB:
    def __init__(self, db_path: Path | None = None) -> None:
        self._db_path = db_path or (user_data_dir() / "content_tile_layout.sqlite3")
        self._init_db()

    def _connect(self) -> sqlite3.Connection:
        return sqlite3.connect(self._db_path)

    def _init_db(self) -> None:
        with self._connect() as conn:
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS content_tile_layout (
                    scope TEXT NOT NULL,
                    tile_key TEXT NOT NULL,
                    col INTEGER NOT NULL,
                    row INTEGER NOT NULL,
                    span_cols INTEGER NOT NULL,
                    span_rows INTEGER NOT NULL,
                    PRIMARY KEY (scope, tile_key)
                )
                """
            )
            conn.commit()

    def load_scope(self, scope: str) -> dict[str, TileLayoutRecord]:
        with self._connect() as conn:
            cur = conn.execute(
                """
                SELECT tile_key, col, row, span_cols, span_rows
                FROM content_tile_layout
                WHERE scope = ?
                """,
                (scope,),
            )
            rows = cur.fetchall()
        out: dict[str, TileLayoutRecord] = {}
        for tile_key, col, row, span_cols, span_rows in rows:
            out[str(tile_key)] = TileLayoutRecord(
                tile_key=str(tile_key),
                col=int(col),
                row=int(row),
                span_cols=int(span_cols),
                span_rows=int(span_rows),
            )
        return out

    def save_scope(self, scope: str, records: list[TileLayoutRecord]) -> None:
        keep_keys = {r.tile_key for r in records}
        with self._connect() as conn:
            if keep_keys:
                placeholders = ",".join("?" for _ in keep_keys)
                conn.execute(
                    f"""
                    DELETE FROM content_tile_layout
                    WHERE scope = ? AND tile_key NOT IN ({placeholders})
                    """,
                    (scope, *sorted(keep_keys)),
                )
            else:
                conn.execute("DELETE FROM content_tile_layout WHERE scope = ?", (scope,))
            conn.executemany(
                """
                INSERT INTO content_tile_layout (scope, tile_key, col, row, span_cols, span_rows)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(scope, tile_key) DO UPDATE SET
                    col = excluded.col,
                    row = excluded.row,
                    span_cols = excluded.span_cols,
                    span_rows = excluded.span_rows
                """,
                [(scope, r.tile_key, r.col, r.row, r.span_cols, r.span_rows) for r in records],
            )
            conn.commit()

    def clear_scope(self, scope: str) -> None:
        with self._connect() as conn:
            conn.execute("DELETE FROM content_tile_layout WHERE scope = ?", (scope,))
            conn.commit()
