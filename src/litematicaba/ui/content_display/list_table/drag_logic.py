"""列表模式拖拽逻辑（纯计算层）。"""

from __future__ import annotations

from typing import TypeVar

from PySide6.QtCore import QPoint
from PySide6.QtWidgets import QTableWidget

T = TypeVar("T")

# 自由排序拖拽协议：MIME 中存储源行索引（ascii 编码整数字符串）
MIME_FREE_SORT_ROW_INDEX = "application/x-litematicaba-free-sort-row"


def dest_insert_index_from_pos(table: QTableWidget, pos: QPoint) -> int | None:
    """根据 viewport 坐标计算插入位 r（插入到原第 r 行前；r==n 表示末尾）。"""
    n = table.rowCount()
    if n <= 0:
        return None
    idx = table.indexAt(pos)
    if idx.isValid():
        row = idx.row()
        rect = table.visualRect(idx)
        if pos.y() >= rect.center().y():
            return min(n, row + 1)
        return row
    r = table.rowAt(pos.y())
    if r >= 0:
        return r
    if pos.y() >= table.viewport().rect().bottom() - 2:
        return n
    return None


def placeholder_row_from_dest(src: int, r: int, n: int) -> int:
    """将目标插入位 r（原序列语义）映射为预览空位行号。"""
    if n <= 0:
        return 0
    if r >= n:
        return n - 1
    if r <= src:
        return r
    return r - 1


def apply_move_in_order(rows: list[T], src: int, r: int) -> list[T]:
    """返回新的重排结果：把 src 移到“插入原第 r 行前”的位置。"""
    n = len(rows)
    if src < 0 or src >= n or r < 0 or r > n or src == r:
        return list(rows)
    out = list(rows)
    item = out.pop(src)
    if r >= n:
        out.append(item)
    elif src < r:
        out.insert(r - 1, item)
    else:
        out.insert(r, item)
    return out

