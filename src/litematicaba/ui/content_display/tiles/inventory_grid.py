from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable

from PySide6.QtCore import QPoint, QRect, Qt
from PySide6.QtGui import QColor, QImage, QMouseEvent, QPaintEvent, QPainter, QPainterPath, QPen, QPixmap
from PySide6.QtWidgets import QMenu, QWidget

from litematicaba.ui.content_display.tiles.tile import TileWidget

@dataclass(frozen=True, slots=True)
class GridPos:
    col: int
    row: int


class InventoryTile(TileWidget):
    def __init__(
        self,
        *,
        span_cols: int,
        span_rows: int,
        cell_px: int,
        gap_px: int,
        bg_color: str,
        label: str,
        draggable: bool,
        parent: QWidget | None,
    ) -> None:
        w = span_cols * cell_px + (span_cols - 1) * gap_px
        h = span_rows * cell_px + (span_rows - 1) * gap_px
        super().__init__(w, h, bg_color, label, parent=parent)
        self.span_cols = span_cols
        self.span_rows = span_rows
        self._dragging = False
        self._draggable = draggable
        self.layout_key: str | None = None
        self._press_offset = QPoint()
        self._last_valid_pos = QPoint()
        self.setCursor(Qt.CursorShape.OpenHandCursor if self._draggable else Qt.CursorShape.ArrowCursor)

    def set_last_valid_pos(self, p: QPoint) -> None:
        self._last_valid_pos = QPoint(p)

    def mousePressEvent(self, event: QMouseEvent) -> None:
        if event.button() == Qt.MouseButton.RightButton:
            parent = self.parentWidget()
            if isinstance(parent, InventoryGridWidget):
                parent.show_tile_context_menu(self, event.globalPosition().toPoint())
                event.accept()
                return
        if self._draggable and event.button() == Qt.MouseButton.LeftButton:
            self._dragging = True
            self._press_offset = event.position().toPoint()
            self.raise_()
            self.setCursor(Qt.CursorShape.ClosedHandCursor)
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event: QMouseEvent) -> None:
        if self._draggable and self._dragging and (event.buttons() & Qt.MouseButton.LeftButton):
            parent = self.parentWidget()
            if parent is None:
                return
            new_top_left = parent.mapFromGlobal(event.globalPosition().toPoint() - self._press_offset)
            self.move(new_top_left)
            event.accept()
            return
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event: QMouseEvent) -> None:
        if self._draggable and self._dragging and event.button() == Qt.MouseButton.LeftButton:
            self._dragging = False
            self.setCursor(Qt.CursorShape.OpenHandCursor)
            parent = self.parentWidget()
            if isinstance(parent, InventoryGridWidget):
                parent.snap_or_revert(self)
            event.accept()
            return
        super().mouseReleaseEvent(event)


class ThumbnailInventoryTile(InventoryTile):
    """带缩略图背景的大磁贴：背景图裁切填充，标题按亮度自动黑白。"""

    def __init__(
        self,
        *,
        span_cols: int,
        span_rows: int,
        cell_px: int,
        gap_px: int,
        label: str,
        thumb_path: str | Path | None,
        draggable: bool,
        parent: QWidget | None,
    ) -> None:
        super().__init__(
            span_cols=span_cols,
            span_rows=span_rows,
            cell_px=cell_px,
            gap_px=gap_px,
            bg_color="#fdfdfd",
            label=label,
            draggable=draggable,
            parent=parent,
        )
        self._thumb = QPixmap(str(thumb_path)) if thumb_path and Path(thumb_path).is_file() else QPixmap()
        self._set_auto_text_contrast()

    def _set_auto_text_contrast(self) -> None:
        luma = self._sample_luma()
        text_color = "#000000" if luma >= 0.60 else "#ffffff"
        self._label.setStyleSheet(f"background: transparent; border: none; color: {text_color};")

    def _sample_luma(self) -> float:
        if self._thumb.isNull():
            return 0.42
        img = self._thumb.toImage()
        if img.isNull():
            return 0.42
        sample = img.convertToFormat(QImage.Format.Format_ARGB32).scaled(
            1,
            1,
            Qt.AspectRatioMode.IgnoreAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        c = sample.pixelColor(0, 0)
        return (0.299 * c.red() + 0.587 * c.green() + 0.114 * c.blue()) / 255.0

    def paintEvent(self, event: QPaintEvent) -> None:
        p = QPainter(self)
        rect = self.rect()
        if self._effective_tid == "Fluent11":
            p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
            path = QPainterPath()
            path.addRoundedRect(
                float(rect.x()),
                float(rect.y()),
                float(rect.width() - 1),
                float(rect.height() - 1),
                10.0,
                10.0,
            )
            p.save()
            p.setClipPath(path)
            if not self._thumb.isNull():
                scaled = self._thumb.scaled(
                    rect.size(),
                    Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                    Qt.TransformationMode.SmoothTransformation,
                )
                sx = max(0, (scaled.width() - rect.width()) // 2)
                sy = max(0, (scaled.height() - rect.height()) // 2)
                src = QRect(sx, sy, rect.width(), rect.height())
                p.drawPixmap(rect, scaled, src)
            else:
                p.fillRect(rect, QColor("#fdfdfd"))
            if self._hovered:
                p.fillRect(rect, QColor(0, 0, 0, 18))
            p.restore()
            self._paint_tile_border(p, rect)
            return
        if not self._thumb.isNull():
            scaled = self._thumb.scaled(
                rect.size(),
                Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                Qt.TransformationMode.SmoothTransformation,
            )
            sx = max(0, (scaled.width() - rect.width()) // 2)
            sy = max(0, (scaled.height() - rect.height()) // 2)
            src = QRect(sx, sy, rect.width(), rect.height())
            p.drawPixmap(rect, scaled, src)
        else:
            p.fillRect(rect, QColor(self._bg_color))
        self._paint_tile_border(p, rect)


class InventoryGridWidget(QWidget):
    """简易网格背包控件：支持拖拽、吸附、碰撞/越界回弹。"""

    def __init__(
        self,
        *,
        cols: int,
        rows: int,
        cell_px: int = 48,
        gap_px: int = 4,
        draw_grid: bool = True,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.cols = cols
        self.rows = rows
        self.cell_px = cell_px
        self.gap_px = gap_px
        self.draw_grid = draw_grid
        w = cols * cell_px + (cols - 1) * gap_px
        h = rows * cell_px + (rows - 1) * gap_px
        self.setFixedSize(w, h)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self._tiles: list[InventoryTile] = []
        self._occ: list[list[InventoryTile | None]] = [[None for _ in range(cols)] for _ in range(rows)]
        self._tiles_draggable = True
        self._auto_place_preferred_cols = 9
        self._scroll_extent_pad_cols = 1
        self._scroll_extent_pad_rows = 1
        self._canvas_cols = cols
        self._canvas_rows = rows
        self._layout_changed_callback: Callable[[], None] | None = None

    def set_draw_grid(self, draw: bool) -> None:
        if self.draw_grid == draw:
            return
        self.draw_grid = draw
        self.update()

    def set_tiles_draggable(self, draggable: bool) -> None:
        self._tiles_draggable = draggable
        for tile in self._tiles:
            tile._draggable = draggable
            tile.setCursor(Qt.CursorShape.OpenHandCursor if draggable else Qt.CursorShape.ArrowCursor)

    def set_auto_place_preferred_cols(self, cols: int) -> None:
        self._auto_place_preferred_cols = max(1, int(cols))

    def set_layout_changed_callback(self, callback: Callable[[], None] | None) -> None:
        self._layout_changed_callback = callback

    def set_scroll_extent_padding(self, *, cols: int, rows: int) -> None:
        self._scroll_extent_pad_cols = max(0, int(cols))
        self._scroll_extent_pad_rows = max(0, int(rows))
        self._update_canvas_extent()

    def resize_grid(self, *, cols: int, rows: int) -> None:
        if cols == self.cols and rows == self.rows:
            return
        self.cols = cols
        self.rows = rows
        self._canvas_cols = cols
        self._canvas_rows = rows
        self._occ = [[None for _ in range(cols)] for _ in range(rows)]
        for tile in self._tiles:
            pos = self._pos_from_pixel(tile._last_valid_pos)
            if self._fits(pos, tile.span_cols, tile.span_rows):
                self._fill_occ(tile, pos)
                p = self._cell_top_left(pos)
                tile.move(p)
                tile.set_last_valid_pos(p)
        self._update_canvas_extent()
        self.update()

    def tiles(self) -> Iterable[InventoryTile]:
        return tuple(self._tiles)

    def _cell_top_left(self, pos: GridPos) -> QPoint:
        step = self.cell_px + self.gap_px
        return QPoint(pos.col * step, pos.row * step)

    def _pos_from_pixel(self, p: QPoint) -> GridPos:
        step = self.cell_px + self.gap_px
        return GridPos(col=int(round(p.x() / step)), row=int(round(p.y() / step)))

    def _fits(self, pos: GridPos, span_cols: int, span_rows: int) -> bool:
        return (
            0 <= pos.col
            and 0 <= pos.row
            and pos.col + span_cols <= self.cols
            and pos.row + span_rows <= self.rows
        )

    def _can_place(self, tile: InventoryTile, pos: GridPos) -> bool:
        if not self._fits(pos, tile.span_cols, tile.span_rows):
            return False
        for r in range(pos.row, pos.row + tile.span_rows):
            for c in range(pos.col, pos.col + tile.span_cols):
                other = self._occ[r][c]
                if other is not None and other is not tile:
                    return False
        return True

    def _clear_occ(self, tile: InventoryTile) -> None:
        for r in range(self.rows):
            for c in range(self.cols):
                if self._occ[r][c] is tile:
                    self._occ[r][c] = None

    def _tile_top_left(self, tile: InventoryTile) -> GridPos | None:
        min_r: int | None = None
        min_c: int | None = None
        for r in range(self.rows):
            for c in range(self.cols):
                if self._occ[r][c] is tile:
                    min_r = r if min_r is None else min(min_r, r)
                    min_c = c if min_c is None else min(min_c, c)
        if min_r is None or min_c is None:
            return None
        return GridPos(min_c, min_r)

    def _set_tile_at(self, tile: InventoryTile, pos: GridPos) -> None:
        self._fill_occ(tile, pos)
        p = self._cell_top_left(pos)
        tile.move(p)
        tile.set_last_valid_pos(p)

    def _snapshot_positions(self) -> dict[InventoryTile, GridPos]:
        out: dict[InventoryTile, GridPos] = {}
        for tile in self._tiles:
            pos = self._tile_top_left(tile)
            if pos is not None:
                out[tile] = pos
        return out

    def _restore_positions(self, positions: dict[InventoryTile, GridPos]) -> None:
        self._occ = [[None for _ in range(self.cols)] for _ in range(self.rows)]
        for tile, pos in positions.items():
            if self._fits(pos, tile.span_cols, tile.span_rows):
                self._set_tile_at(tile, pos)

    def _tiles_in_area(self, area: GridPos, span_cols: int, span_rows: int) -> list[InventoryTile]:
        out: list[InventoryTile] = []
        seen: set[int] = set()
        for r in range(area.row, area.row + span_rows):
            for c in range(area.col, area.col + span_cols):
                if r < 0 or c < 0 or r >= self.rows or c >= self.cols:
                    continue
                t = self._occ[r][c]
                if t is not None and id(t) not in seen:
                    seen.add(id(t))
                    out.append(t)
        return out

    def _try_shift_tile(self, tile: InventoryTile, delta_col: int, delta_row: int, visiting: set[int]) -> bool:
        tid = id(tile)
        if tid in visiting:
            return False
        visiting.add(tid)
        try:
            pos = self._tile_top_left(tile)
            if pos is None:
                return False
            target = GridPos(pos.col + delta_col, pos.row + delta_row)
            if not self._fits(target, tile.span_cols, tile.span_rows):
                return False
            self._clear_occ(tile)
            blockers = self._tiles_in_area(target, tile.span_cols, tile.span_rows)
            for b in blockers:
                if b is tile:
                    continue
                if not self._try_shift_tile(b, delta_col, delta_row, visiting):
                    self._fill_occ(tile, pos)
                    return False
            if not self._can_place(tile, target):
                self._fill_occ(tile, pos)
                return False
            self._set_tile_at(tile, target)
            return True
        finally:
            visiting.remove(tid)

    def _try_expand_with_push(self, tile: InventoryTile, pos: GridPos, new_cols: int, new_rows: int) -> bool:
        if self._can_place(tile, pos):
            return True
        old_cols, old_rows = tile.span_cols, tile.span_rows
        extra_right = max(0, new_cols - old_cols)
        extra_down = max(0, new_rows - old_rows)
        snapshot = self._snapshot_positions()
        if extra_right > 0:
            right_area = GridPos(pos.col + old_cols, pos.row)
            blockers = self._tiles_in_area(right_area, extra_right, new_rows)
            ok = True
            for b in blockers:
                if not self._try_shift_tile(b, extra_right, 0, set()):
                    ok = False
                    break
            if ok and self._can_place(tile, pos):
                return True
            self._restore_positions(snapshot)
        if extra_down > 0:
            down_area = GridPos(pos.col, pos.row + old_rows)
            blockers = self._tiles_in_area(down_area, new_cols, extra_down)
            ok = True
            for b in blockers:
                if not self._try_shift_tile(b, 0, extra_down, set()):
                    ok = False
                    break
            if ok and self._can_place(tile, pos):
                return True
            self._restore_positions(snapshot)
        return False

    def resize_tile_by_kind(self, tile: InventoryTile, size_kind: str) -> bool:
        size_map = {
            "小": (1, 1),
            "中": (2, 2),
            "宽": (4, 2),
            "大": (4, 4),
        }
        target = size_map.get(size_kind)
        if target is None:
            return False
        new_cols, new_rows = target
        pos = self._tile_top_left(tile)
        if pos is None:
            return False
        if tile.span_cols == new_cols and tile.span_rows == new_rows:
            return True
        old_cols, old_rows = tile.span_cols, tile.span_rows
        snapshot = self._snapshot_positions()
        self._clear_occ(tile)
        tile.span_cols, tile.span_rows = new_cols, new_rows
        w = new_cols * self.cell_px + (new_cols - 1) * self.gap_px
        h = new_rows * self.cell_px + (new_rows - 1) * self.gap_px
        tile.setFixedSize(w, h)
        if new_cols <= old_cols and new_rows <= old_rows:
            if self._can_place(tile, pos):
                self._set_tile_at(tile, pos)
                self._update_canvas_extent()
                self.update()
                if self._layout_changed_callback is not None:
                    self._layout_changed_callback()
                return True
            tile.span_cols, tile.span_rows = old_cols, old_rows
            tile.setFixedSize(
                old_cols * self.cell_px + (old_cols - 1) * self.gap_px,
                old_rows * self.cell_px + (old_rows - 1) * self.gap_px,
            )
            self._restore_positions(snapshot)
            tile.trigger_error_highlight()
            return False
        if self._try_expand_with_push(tile, pos, new_cols, new_rows):
            self._set_tile_at(tile, pos)
            self._update_canvas_extent()
            self.update()
            if self._layout_changed_callback is not None:
                self._layout_changed_callback()
            return True
        tile.span_cols, tile.span_rows = old_cols, old_rows
        tile.setFixedSize(
            old_cols * self.cell_px + (old_cols - 1) * self.gap_px,
            old_rows * self.cell_px + (old_rows - 1) * self.gap_px,
        )
        self._restore_positions(snapshot)
        tile.trigger_error_highlight()
        return False

    def show_tile_context_menu(self, tile: InventoryTile, global_pos: QPoint) -> None:
        menu = QMenu(self)
        act_resize = menu.addMenu("调整大小")
        for label in ("小", "中", "宽", "大"):
            a = act_resize.addAction(label)
            a.triggered.connect(lambda _checked=False, t=tile, s=label: self.resize_tile_by_kind(t, s))
        placeholder = menu.addAction("（稍后定义的其他选项，不可选中）")
        placeholder.setEnabled(False)
        menu.exec(global_pos)

    def _fill_occ(self, tile: InventoryTile, pos: GridPos) -> None:
        for r in range(pos.row, pos.row + tile.span_rows):
            for c in range(pos.col, pos.col + tile.span_cols):
                self._occ[r][c] = tile

    def _update_canvas_extent(self) -> None:
        if not self._tiles:
            target_cols = min(self.cols, max(1, 1 + self._scroll_extent_pad_cols))
            target_rows = min(self.rows, max(1, 1 + self._scroll_extent_pad_rows))
        else:
            max_right = 0
            max_bottom = 0
            for tile in self._tiles:
                pos = self._tile_top_left(tile)
                if pos is None:
                    continue
                max_right = max(max_right, pos.col + tile.span_cols)
                max_bottom = max(max_bottom, pos.row + tile.span_rows)
            target_cols = min(self.cols, max_right + self._scroll_extent_pad_cols)
            target_rows = min(self.rows, max_bottom + self._scroll_extent_pad_rows)
            target_cols = max(1, target_cols)
            target_rows = max(1, target_rows)
        self._canvas_cols = target_cols
        self._canvas_rows = target_rows
        w = target_cols * self.cell_px + (target_cols - 1) * self.gap_px
        h = target_rows * self.cell_px + (target_rows - 1) * self.gap_px
        self.setFixedSize(w, h)

    def _find_auto_pos(self, tile: InventoryTile) -> tuple[GridPos | None, str]:
        """自动放置：先前 N 列逐行；失败后全区域按列（竖向）扫描。"""
        preferred_cols = min(self._auto_place_preferred_cols, self.cols)
        max_c_pref = preferred_cols - tile.span_cols
        if max_c_pref >= 0:
            for r in range(0, self.rows - tile.span_rows + 1):
                for c in range(0, max_c_pref + 1):
                    p = GridPos(c, r)
                    if self._can_place(tile, p):
                        return p, "preferred_row_scan"
        max_c_all = self.cols - tile.span_cols
        max_r_all = self.rows - tile.span_rows
        if max_c_all >= 0 and max_r_all >= 0:
            for c in range(0, max_c_all + 1):
                for r in range(0, max_r_all + 1):
                    p = GridPos(c, r)
                    if self._can_place(tile, p):
                        return p, "fallback_col_scan"
        return None, "not_found"

    def add_tile(
        self,
        *,
        span_cols: int,
        span_rows: int,
        pos: GridPos | None = None,
        bg_color: str,
        label: str,
        layout_key: str | None = None,
    ) -> InventoryTile:
        tile = InventoryTile(
            span_cols=span_cols,
            span_rows=span_rows,
            cell_px=self.cell_px,
            gap_px=self.gap_px,
            bg_color=bg_color,
            label=label,
            draggable=self._tiles_draggable,
            parent=self,
        )
        target_pos, _auto_strategy = (pos, "explicit") if pos is not None else self._find_auto_pos(tile)
        if target_pos is None or not self._can_place(tile, target_pos):
            raise ValueError("tile overlap or out of bounds")
        self._tiles.append(tile)
        tile.layout_key = layout_key
        self._fill_occ(tile, target_pos)
        p = self._cell_top_left(target_pos)
        tile.move(p)
        tile.set_last_valid_pos(p)
        tile.show()
        self._update_canvas_extent()
        return tile

    def add_thumbnail_tile(
        self,
        *,
        span_cols: int,
        span_rows: int,
        pos: GridPos | None = None,
        label: str,
        thumb_path: str | Path | None,
        layout_key: str | None = None,
    ) -> ThumbnailInventoryTile:
        tile = ThumbnailInventoryTile(
            span_cols=span_cols,
            span_rows=span_rows,
            cell_px=self.cell_px,
            gap_px=self.gap_px,
            label=label,
            thumb_path=thumb_path,
            draggable=self._tiles_draggable,
            parent=self,
        )
        target_pos, _auto_strategy = (pos, "explicit") if pos is not None else self._find_auto_pos(tile)
        if target_pos is None or not self._can_place(tile, target_pos):
            raise ValueError("tile overlap or out of bounds")
        self._tiles.append(tile)
        tile.layout_key = layout_key
        self._fill_occ(tile, target_pos)
        p = self._cell_top_left(target_pos)
        tile.move(p)
        tile.set_last_valid_pos(p)
        tile.show()
        self._update_canvas_extent()
        return tile

    def snap_or_revert(self, tile: InventoryTile) -> None:
        self._clear_occ(tile)
        target = self._pos_from_pixel(tile.pos())
        if self._can_place(tile, target):
            p = self._cell_top_left(target)
            tile.move(p)
            tile.set_last_valid_pos(p)
            self._fill_occ(tile, target)
            self._update_canvas_extent()
            if self._layout_changed_callback is not None:
                self._layout_changed_callback()
            return
        tile.move(tile._last_valid_pos)
        self._fill_occ(tile, self._pos_from_pixel(tile._last_valid_pos))
        self._update_canvas_extent()

    def export_layout_records(self) -> list[tuple[str, int, int, int, int]]:
        out: list[tuple[str, int, int, int, int]] = []
        for tile in self._tiles:
            if not tile.layout_key:
                continue
            pos = self._tile_top_left(tile)
            if pos is None:
                continue
            out.append((tile.layout_key, pos.col, pos.row, tile.span_cols, tile.span_rows))
        return out

    def paintEvent(self, event: QPaintEvent) -> None:
        super().paintEvent(event)
        if not self.draw_grid:
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing, False)
        pen = QPen(QColor(0, 0, 0, 25))
        pen.setWidth(1)
        p.setPen(pen)
        step = self.cell_px + self.gap_px
        for r in range(self._canvas_rows):
            for c in range(self._canvas_cols):
                p.drawRect(c * step, r * step, self.cell_px, self.cell_px)

