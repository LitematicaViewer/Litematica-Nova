"""内容物列表视图（显示方式「列表」）：行高、缩略图列、文本对齐与自由排序拖拽。"""

from __future__ import annotations

import random
import time
import uuid
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from PySide6.QtCore import QEvent, QModelIndex, QObject, QPoint, QPointF, QMimeData, Qt
from PySide6.QtGui import (
    QColor,
    QDrag,
    QDragEnterEvent,
    QDragMoveEvent,
    QDropEvent,
    QFontMetrics,
    QMouseEvent,
    QPainter,
    QPalette,
    QPixmap,
)
from PySide6.QtWidgets import (
    QAbstractItemView,
    QApplication,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QSizePolicy,
    QStyle,
    QStyledItemDelegate,
    QStyleOptionViewItem,
    QTableWidget,
    QTableWidgetItem,
    QWidget,
)

from litematicaba.ui.content_display.list_table.drag_logic import (
    MIME_FREE_SORT_ROW_INDEX,
    apply_move_in_order,
    dest_insert_index_from_pos,
    placeholder_row_from_dest,
)
from litematicaba.ui.table.metro10_list_profile import (
    METRO10_LIST_TABLE_TOKENS,
    METRO_LIST_FLAT_TABLE_QSS,
    apply_metro_list_flat_chrome_flags,
    content_list_row_height_px,
    is_metro_list_table_theme,
)
from litematicaba.ui.table.minecraft_list_profile import (
    MINECRAFT_LIST_TABLE_TOKENS,
    apply_minecraft_content_list_chrome_flags,
    apply_minecraft_list_table_header_chrome,
)
from litematicaba.ui.theme import normalize_theme_id
from litematicaba.ui.themes.base import ui_dir

_M10 = METRO10_LIST_TABLE_TOKENS
_METRO_ROW_H = _M10.row_height
_METRO_THUMB_PX = _M10.thumb_px
_METRO_THUMB_COL_W = _M10.thumb_col_w
_SORT_FREE = "自由排序"
_SORT_FREE_LOCKED = "自由排序（锁定）"
_DRAG_SORT_HEADER_SEMANTIC = "拖拽以自由排序"
_DRAG_SORT_COL_WIDTH = _M10.drag_col_w
_DRAG_HINT_DRAW_W = _M10.drag_hint_draw_w
_DRAG_HINT_DRAW_H = _M10.drag_hint_draw_h
_CONTENT_LIST_MIN_HEIGHT_PX = _M10.min_height
_METRO_HOVER_BG = QColor(_M10.hover_bg_hex)
_METRO_SEL_BG = QColor(_M10.selected_bg_hex)
_METRO_TEXT_FG = QColor(_M10.text_fg_hex)
_MINECRAFT_LIST_HOVER_BG = QColor(MINECRAFT_LIST_TABLE_TOKENS.hover_bg_hex)
_MINECRAFT_LIST_TEXT_FG = QColor(MINECRAFT_LIST_TABLE_TOKENS.text_fg_hex)
_MINECRAFT_LIST_SEL_BG = QColor(MINECRAFT_LIST_TABLE_TOKENS.selected_bg_hex)
_METRO_TABLE_QSS = METRO_LIST_FLAT_TABLE_QSS
_TEXT_HEADERS = ("名称", "时间", "大小", "额外表头1", "额外表头2")


# 供属性页等「纯文本数据表」与内容物列表共用同一套 Metro 扁平表 QSS / 行高约定
METRO_LIST_TABLE_CHROME_QSS = METRO_LIST_FLAT_TABLE_QSS
METRO_LIST_SAMPLE_ROW_HEIGHT = _METRO_ROW_H
# 供资源管理等表格与内容列表对齐 Metro 行悬停/文本色（与 _MetroPlainDelegate 一致）
METRO_LIST_ROW_HOVER_COLOR = _METRO_HOVER_BG
METRO_LIST_ROW_SELECTED_COLOR = _METRO_SEL_BG
METRO_LIST_ROW_TEXT_COLOR = _METRO_TEXT_FG


def uses_content_list_sample_row_height(theme_id: str) -> bool:
    """与 ``ContentListTableWidget._render_rows`` 中固定行高路径一致。

    仅 ``QTDefault`` 使用 ``resizeRowToContents``；其它主题采用 ``content_list_row_height_px``（Minecraft **44**，其余非默认 **60**）。
    """
    return normalize_theme_id(theme_id) != "QTDefault"


def _qt_list_thumb_px() -> int:
    app = QApplication.instance()
    if app is None:
        return 32
    st = app.style()
    pm = QStyle.PixelMetric
    for name in ("PM_ListViewIconSize", "PM_LargeIconSize", "PM_SmallIconSize"):
        if hasattr(pm, name):
            v = st.pixelMetric(getattr(pm, name))
            if v > 0:
                return int(v)
    return 32


def _exampic_paths() -> list[Path]:
    d = ui_dir() / "resources" / "exampic"
    if not d.is_dir():
        return []
    return sorted(d.glob("*.png"))


def format_size_bytes_display(bytes_value: int) -> str:
    return f"{int(bytes_value):,}"


def _alignment_for_column_header(header_name: str) -> Qt.AlignmentFlag:
    if header_name == "大小":
        return Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter
    return Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter


@dataclass(frozen=True)
class ContentRow:
    thumb_path: Path
    name: str
    sort_ts: float
    time_display: str
    size_display: str
    size_sort_key: int
    extra1: str
    extra2: str


def generate_sample_content_rows(n: int = 20) -> list[ContentRow]:
    rng = random.Random()
    pics = _exampic_paths() or [Path()]
    now = time.time()
    start = now - 5 * 365 * 86400
    rows: list[ContentRow] = []
    for i in range(n):
        path = pics[i % len(pics)]
        ts = rng.uniform(start, now)
        sz_key = rng.randint(256, 9_999_999_999)
        rows.append(
            ContentRow(
                thumb_path=path,
                name=str(uuid.uuid4()),
                sort_ts=ts,
                time_display=datetime.fromtimestamp(ts).strftime("%Y-%m-%d %H:%M"),
                size_display=format_size_bytes_display(sz_key),
                size_sort_key=sz_key,
                extra1="" if rng.random() < 0.25 else "".join(rng.choices("abcdefghijklmnopqrstuvwxyz", k=rng.randint(3, 14))),
                extra2="",
            )
        )
    return rows


class _DragSortHintWidget(QWidget):
    def __init__(self, table: "ContentListTableWidget", row_index: int) -> None:
        super().__init__(table)
        self._table = table
        self._row_index = row_index
        self._press_pos: QPointF | None = None
        self.setCursor(Qt.CursorShape.OpenHandCursor)

    def paintEvent(self, event):  # type: ignore[override]
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setBrush(QColor(0x66, 0x66, 0x66))
        painter.setPen(Qt.PenStyle.NoPen)
        w, h = self.width(), self.height()
        draw_w = min(_DRAG_HINT_DRAW_W, w)
        draw_h = min(_DRAG_HINT_DRAW_H, h)
        ox = (w - draw_w) / 2.0
        oy = (h - draw_h) / 2.0
        aw, ah, margin = max(1, draw_w - 2), max(1, draw_h - 2), 1
        dot_r = max(1.0, min(aw, ah) / 8.0)
        for ri in range(3):
            for ci in range(2):
                cx = ox + margin + aw * (ci + 0.5) / 2
                cy = oy + margin + ah * (ri + 0.5) / 3
                painter.drawEllipse(QPointF(cx, cy), dot_r, dot_r)

    def mousePressEvent(self, event: QMouseEvent) -> None:  # type: ignore[override]
        if event.button() == Qt.MouseButton.LeftButton:
            self._press_pos = event.position()
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event: QMouseEvent) -> None:  # type: ignore[override]
        if self._press_pos is None or not (event.buttons() & Qt.MouseButton.LeftButton):
            return super().mouseMoveEvent(event)
        if (event.position() - self._press_pos).manhattanLength() < QApplication.startDragDistance():
            return super().mouseMoveEvent(event)
        self._press_pos = None
        if self._table._sort_is_free():
            self._table._start_free_drag(self._row_index, self)


class _MetroContentListDelegate(QStyledItemDelegate):
    def __init__(self, table: "ContentListTableWidget", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._table = table

    def initStyleOption(self, option: QStyleOptionViewItem, index):  # type: ignore[override]
        super().initStyleOption(option, index)
        if self._table._column_is_text_data(index.column()):
            option.palette.setColor(QPalette.ColorRole.Text, _METRO_TEXT_FG)

    def paint(self, painter: QPainter, option: QStyleOptionViewItem, index):  # type: ignore[override]
        if not self._table._column_is_text_data(index.column()):
            return
        opt = QStyleOptionViewItem(option)
        self.initStyleOption(opt, index)
        row = index.row()
        sel = self._table.selectionModel().isRowSelected(row, QModelIndex())
        hr = self._table._hover_row
        painter.save()
        painter.setClipRect(opt.rect)
        if sel:
            painter.fillRect(opt.rect, _METRO_SEL_BG)
        elif hr is not None and row == hr:
            painter.fillRect(opt.rect, _METRO_HOVER_BG)
        painter.setPen(_METRO_TEXT_FG)
        tr = opt.rect.adjusted(4, 0, -4, 0)
        elided = QFontMetrics(opt.font).elidedText(opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width()))
        painter.drawText(tr, int(opt.displayAlignment), elided)
        painter.restore()


class _MinecraftContentListDelegate(QStyledItemDelegate):
    """Minecraft：黑底/斑马纹 + 行悬停 #2b2b2b、选中 ``table_list_selected_bg``（#3c3c3c）。"""

    def __init__(self, table: "ContentListTableWidget", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._table = table

    def paint(self, painter: QPainter, option: QStyleOptionViewItem, index) -> None:  # type: ignore[override]
        if not self._table._column_is_text_data(index.column()):
            return
        opt = QStyleOptionViewItem(option)
        self.initStyleOption(opt, index)
        opt.palette.setColor(QPalette.ColorRole.Text, _MINECRAFT_LIST_TEXT_FG)
        row = index.row()
        sel = self._table.selectionModel().isRowSelected(row, QModelIndex())
        hr = self._table._hover_row
        if sel:
            painter.save()
            painter.setClipRect(opt.rect)
            painter.fillRect(opt.rect, _MINECRAFT_LIST_SEL_BG)
            painter.setPen(_MINECRAFT_LIST_TEXT_FG)
            tr = opt.rect.adjusted(4, 0, -4, 0)
            elided = QFontMetrics(opt.font).elidedText(
                opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
            )
            painter.drawText(tr, int(opt.displayAlignment), elided)
            painter.restore()
            return
        if hr is not None and row == hr:
            painter.save()
            painter.setClipRect(opt.rect)
            painter.fillRect(opt.rect, _MINECRAFT_LIST_HOVER_BG)
            painter.setPen(_MINECRAFT_LIST_TEXT_FG)
            tr = opt.rect.adjusted(4, 0, -4, 0)
            elided = QFontMetrics(opt.font).elidedText(
                opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
            )
            painter.drawText(tr, int(opt.displayAlignment), elided)
            painter.restore()
            return
        super().paint(painter, option, index)


class ContentListTableWidget(QTableWidget):
    def __init__(self, rows: list[ContentRow], parent: QWidget | None = None, *, theme_id: str = "QTDefault") -> None:
        super().__init__(parent)
        self._original_rows = list(rows)
        self._theme_id_normalized = normalize_theme_id(theme_id)
        self._last_sort_label = "按名称"
        self._use_metro_sample = False
        self._thumb_px = _METRO_THUMB_PX
        self._thumb_col_w = _METRO_THUMB_COL_W
        self._hover_row: int | None = None
        self._drag_active = False
        self._drag_source_row: int | None = None
        self._drag_placeholder_row: int | None = None
        self._apply_list_metrics()
        self.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        self.verticalHeader().setVisible(False)
        self.setWordWrap(False)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.MinimumExpanding)
        self.setMinimumHeight(_CONTENT_LIST_MIN_HEIGHT_PX)
        self.viewport().installEventFilter(self)
        self.selectionModel().selectionChanged.connect(self._on_metro_selection_changed)
        self._apply_metro_table_chrome()
        self.apply_sort(self._last_sort_label)

    def _clear_drag_preview_state(self, *, restore_view: bool) -> None:
        self._drag_active = False
        self._drag_source_row = None
        self._drag_placeholder_row = None
        if restore_view and self._sort_is_free_draggable():
            self.apply_sort(_SORT_FREE)

    def eventFilter(self, obj: QObject, event: QEvent) -> bool:  # type: ignore[override]
        if obj is self.viewport() and (
            is_metro_list_table_theme(self._theme_id_normalized)
            or self._theme_id_normalized == "Minecraft"
        ):
            if event.type() == QEvent.Type.MouseMove and isinstance(event, QMouseEvent):
                self._set_metro_hover_row(self.rowAt(int(event.position().y())))
            elif event.type() == QEvent.Type.Leave:
                self._set_metro_hover_row(None)
        return super().eventFilter(obj, event)

    def _set_metro_hover_row(self, row: int | None) -> None:
        row = row if (row is not None and row >= 0) else None
        if not (
            is_metro_list_table_theme(self._theme_id_normalized)
            or self._theme_id_normalized == "Minecraft"
        ):
            return
        if row != self._hover_row:
            self._hover_row = row
            self._sync_cell_widget_row_backgrounds()

    def _on_metro_selection_changed(self) -> None:
        if is_metro_list_table_theme(self._theme_id_normalized) or self._theme_id_normalized == "Minecraft":
            self._sync_cell_widget_row_backgrounds()

    def _sync_cell_widget_row_backgrounds(self) -> None:
        if is_metro_list_table_theme(self._theme_id_normalized):
            self._metro_apply_row_appearance()
        elif self._theme_id_normalized == "Minecraft":
            self._minecraft_apply_row_appearance()

    def _metro_apply_row_appearance(self) -> None:
        if not is_metro_list_table_theme(self._theme_id_normalized):
            return
        selected_rows = {idx.row() for idx in self.selectionModel().selectedRows()}
        for r in range(self.rowCount()):
            is_sel = r in selected_rows
            is_hov = self._hover_row is not None and r == self._hover_row and not is_sel
            for c in ((0, 1) if self._sort_is_free_draggable() else (0,)):
                w = self.cellWidget(r, c)
                if w is None:
                    continue
                if is_sel:
                    w.setStyleSheet(f"background-color: {_METRO_SEL_BG.name()}; border: none;")
                elif is_hov:
                    w.setStyleSheet(f"background-color: {_METRO_HOVER_BG.name()}; border: none;")
                else:
                    w.setStyleSheet("background-color: transparent; border: none;")
        self.viewport().update()

    def _sort_is_free(self) -> bool:
        return self._last_sort_label in (_SORT_FREE, _SORT_FREE_LOCKED)

    def _sort_is_free_draggable(self) -> bool:
        return self._last_sort_label == _SORT_FREE

    def _column_is_text_data(self, col: int) -> bool:
        return col >= (2 if self._sort_is_free_draggable() else 1)

    def _setup_columns_for_current_sort(self) -> None:
        free_drag = self._sort_is_free_draggable()
        ncols = 7 if free_drag else 6
        self.setColumnCount(ncols)
        self.setHorizontalHeaderLabels(["", "", *_TEXT_HEADERS] if free_drag else ["", *_TEXT_HEADERS])
        h = self.horizontalHeader()
        if free_drag:
            hi0 = self.horizontalHeaderItem(0)
            if hi0 is not None:
                hi0.setToolTip(_DRAG_SORT_HEADER_SEMANTIC)
            h.setSectionResizeMode(0, QHeaderView.ResizeMode.Fixed)
            self.setColumnWidth(0, _DRAG_SORT_COL_WIDTH)
            h.setSectionResizeMode(1, QHeaderView.ResizeMode.Fixed)
            self.setColumnWidth(1, self._thumb_col_w)
            for c in range(2, 7):
                h.setSectionResizeMode(c, QHeaderView.ResizeMode.Stretch)
        else:
            h.setSectionResizeMode(0, QHeaderView.ResizeMode.Fixed)
            self.setColumnWidth(0, self._thumb_col_w)
            for c in range(1, 6):
                h.setSectionResizeMode(c, QHeaderView.ResizeMode.Stretch)
        self._apply_drag_drop_config()

    def _apply_drag_drop_config(self) -> None:
        accept = self._sort_is_free_draggable()
        self.setAcceptDrops(accept)
        self.viewport().setAcceptDrops(accept)

    def _dest_row_from_drop(self, event: QDropEvent) -> int | None:
        return dest_insert_index_from_pos(self, event.position().toPoint())

    def _move_row_logical(self, src: int, r: int) -> None:
        if not self._sort_is_free_draggable():
            return
        self._original_rows = apply_move_in_order(self._original_rows, src, r)
        self.apply_sort(_SORT_FREE)

    def _row_visual_height(self, row: int) -> int:
        h = self.rowHeight(row)
        if h > 0:
            return h
        fixed = content_list_row_height_px(self._theme_id_normalized)
        return fixed if fixed is not None else 28

    def _render_rows(self, rows: list[ContentRow], *, placeholder_row: int | None = None) -> None:
        self._hover_row = None
        self.clearContents()
        self.setRowCount(len(rows))
        for i, row in enumerate(rows):
            rh = content_list_row_height_px(self._theme_id_normalized)
            if rh is not None:
                self.setRowHeight(i, rh)
            if placeholder_row is not None and i == placeholder_row:
                self._fill_placeholder_row(i)
            else:
                self._fill_row(i, row)
        if not self._use_metro_sample:
            for r in range(self.rowCount()):
                self.resizeRowToContents(r)
        if placeholder_row is not None and 0 <= placeholder_row < self.rowCount():
            self.setRowHeight(placeholder_row, max(20, self._row_visual_height(placeholder_row)))
        if is_metro_list_table_theme(self._theme_id_normalized) or self._theme_id_normalized == "Minecraft":
            self._sync_cell_widget_row_backgrounds()

    def _minecraft_apply_row_appearance(self) -> None:
        if self._theme_id_normalized != "Minecraft":
            return
        selected_rows = {idx.row() for idx in self.selectionModel().selectedRows()}
        for r in range(self.rowCount()):
            is_sel = r in selected_rows
            is_hov = self._hover_row is not None and r == self._hover_row and not is_sel
            for c in ((0, 1) if self._sort_is_free_draggable() else (0,)):
                w = self.cellWidget(r, c)
                if w is None:
                    continue
                if is_sel:
                    w.setStyleSheet(
                        f"background-color: {_MINECRAFT_LIST_SEL_BG.name()}; border: none;"
                    )
                elif is_hov:
                    w.setStyleSheet(
                        f"background-color: {_MINECRAFT_LIST_HOVER_BG.name()}; border: none;"
                    )
                else:
                    w.setStyleSheet("background-color: transparent; border: none;")
        self.viewport().update()

    def _fill_placeholder_row(self, row_idx: int) -> None:
        for c in range(self.columnCount()):
            it = QTableWidgetItem("")
            it.setFlags(Qt.ItemFlag.NoItemFlags)
            it.setBackground(QColor(0xF2, 0xF2, 0xF2, 120))
            self.setItem(row_idx, c, it)
        ph = QWidget()
        ph.setStyleSheet("border: 1px dashed #8a8a8a; background: rgba(0,0,0,0);")
        self.setCellWidget(row_idx, 1 if self._sort_is_free_draggable() else 0, ph)

    def _ordered_rows_for_preview(self) -> tuple[list[ContentRow], int] | None:
        if not self._drag_active or self._drag_source_row is None or self._drag_placeholder_row is None:
            return None
        src, p = self._drag_source_row, self._drag_placeholder_row
        rows = list(self._original_rows)
        if src < 0 or src >= len(rows):
            return None
        moving = rows.pop(src)
        p = max(0, min(p, len(rows)))
        rows.insert(p, moving)
        return rows, p

    def _start_free_drag(self, src_row: int, drag_source: QWidget) -> None:
        if not self._sort_is_free_draggable() or src_row < 0 or src_row >= len(self._original_rows):
            return
        self._drag_active = True
        self._drag_source_row = src_row
        self._drag_placeholder_row = src_row
        row_rect = self.visualRect(self.model().index(src_row, 0))
        drag_pm = QPixmap()
        if row_rect.isValid():
            row_rect.setLeft(0)
            row_rect.setRight(self.viewport().width() - 1)
            drag_pm = self.viewport().grab(row_rect)
        preview = self._ordered_rows_for_preview()
        if preview is not None:
            rows, p = preview
            self._render_rows(rows, placeholder_row=p)
        mime = QMimeData()
        mime.setData(MIME_FREE_SORT_ROW_INDEX, str(src_row).encode("ascii"))
        drag = QDrag(drag_source)
        drag.setMimeData(mime)
        if not drag_pm.isNull():
            drag.setPixmap(drag_pm)
            drag.setHotSpot(QPoint(max(0, _DRAG_SORT_COL_WIDTH // 2), max(0, drag_pm.height() // 2)))
        drag.exec(Qt.DropAction.MoveAction)
        if self._drag_active:
            self._clear_drag_preview_state(restore_view=True)

    def dragEnterEvent(self, event: QDragEnterEvent) -> None:  # type: ignore[override]
        if self._sort_is_free_draggable() and event.mimeData().hasFormat(MIME_FREE_SORT_ROW_INDEX):
            event.acceptProposedAction()
        else:
            super().dragEnterEvent(event)

    def dragMoveEvent(self, event: QDragMoveEvent) -> None:  # type: ignore[override]
        if self._sort_is_free_draggable() and event.mimeData().hasFormat(MIME_FREE_SORT_ROW_INDEX):
            if self._drag_active and self._drag_source_row is not None:
                r = self._dest_row_from_drop(event)
                n = len(self._original_rows)
                if r is not None and 0 <= r <= n:
                    p = placeholder_row_from_dest(self._drag_source_row, r, n)
                    if p != self._drag_placeholder_row:
                        self._drag_placeholder_row = p
                        preview = self._ordered_rows_for_preview()
                        if preview is not None:
                            rows, pr = preview
                            self._render_rows(rows, placeholder_row=pr)
            event.acceptProposedAction()
        else:
            super().dragMoveEvent(event)

    def dropEvent(self, event: QDropEvent) -> None:  # type: ignore[override]
        if not self._sort_is_free_draggable() or not event.mimeData().hasFormat(MIME_FREE_SORT_ROW_INDEX):
            super().dropEvent(event)
            return
        try:
            src = int(bytes(event.mimeData().data(MIME_FREE_SORT_ROW_INDEX)).decode("ascii"))
        except ValueError:
            event.ignore()
            return
        r = self._dest_row_from_drop(event)
        n = len(self._original_rows)
        if r is None or src < 0 or src >= n:
            self._clear_drag_preview_state(restore_view=True)
            event.ignore()
            return
        if src == r:
            self._clear_drag_preview_state(restore_view=True)
            event.acceptProposedAction()
            return
        self._move_row_logical(src, r)
        self._clear_drag_preview_state(restore_view=False)
        event.acceptProposedAction()

    def _apply_list_metrics(self) -> None:
        if self._theme_id_normalized == "QTDefault":
            self._use_metro_sample = False
            self._thumb_px = _qt_list_thumb_px()
            self._thumb_col_w = max(self._thumb_px + 16, 48)
        else:
            self._use_metro_sample = True
            self._thumb_px = _METRO_THUMB_PX
            self._thumb_col_w = _METRO_THUMB_COL_W

    def apply_list_theme(self, theme_id: str) -> None:
        tid = normalize_theme_id(theme_id)
        if tid == self._theme_id_normalized:
            return
        self._theme_id_normalized = tid
        self._apply_list_metrics()
        self._apply_metro_table_chrome()
        self.apply_sort(self._last_sort_label)

    def _apply_metro_table_chrome(self) -> None:
        if is_metro_list_table_theme(self._theme_id_normalized):
            apply_metro_list_flat_chrome_flags(self)
            self.setItemDelegate(_MetroContentListDelegate(self, self))
        elif self._theme_id_normalized == "Minecraft":
            apply_minecraft_content_list_chrome_flags(self)
            apply_minecraft_list_table_header_chrome(self, self._theme_id_normalized)
            self.setItemDelegate(_MinecraftContentListDelegate(self, self))
        else:
            self.setShowGrid(True)
            self.setMouseTracking(False)
            self.viewport().setMouseTracking(False)
            self.setStyleSheet("")
            self.setItemDelegate(QStyledItemDelegate(self))

    def apply_sort(self, sort_label: str) -> None:
        self._last_sort_label = sort_label
        if sort_label in (_SORT_FREE, _SORT_FREE_LOCKED):
            ordered = list(self._original_rows)
        elif sort_label.startswith("按"):
            ordered = self._sorted_rows(sort_label[1:])
        else:
            ordered = list(self._original_rows)
        self._setup_columns_for_current_sort()
        self._render_rows(ordered)

    def set_rows(self, rows: list[ContentRow]) -> None:
        self._original_rows = list(rows)
        self.apply_sort(self._last_sort_label)

    def _sorted_rows(self, key_name: str) -> list[ContentRow]:
        key_map = {
            "名称": lambda r: r.name.lower(),
            "时间": lambda r: r.sort_ts,
            "大小": lambda r: r.size_sort_key,
            "额外表头1": lambda r: r.extra1.lower(),
            "额外表头2": lambda r: r.extra2.lower(),
        }
        key_fn = key_map.get(key_name)
        return list(self._original_rows) if key_fn is None else sorted(self._original_rows, key=key_fn)

    def _thumb_cell_margins(self) -> tuple[int, int, int, int]:
        if self._use_metro_sample:
            row_h = content_list_row_height_px(self._theme_id_normalized) or _METRO_ROW_H
            vm = max(0, (row_h - self._thumb_px) // 2)
            return (8, vm, 8, vm)
        return (8, 4, 8, 4)

    def _fill_row(self, row_idx: int, row: ContentRow) -> None:
        thumb_host = QWidget()
        lay = QHBoxLayout(thumb_host)
        lay.setContentsMargins(*self._thumb_cell_margins())
        lab = QLabel()
        lab.setFixedSize(self._thumb_px, self._thumb_px)
        lab.setAlignment(Qt.AlignmentFlag.AlignCenter)
        if row.thumb_path and row.thumb_path.is_file():
            pm = QPixmap(str(row.thumb_path))
            if not pm.isNull():
                lab.setPixmap(pm.scaled(self._thumb_px, self._thumb_px, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation))
        lay.addStretch(1)
        lay.addWidget(lab, 0, Qt.AlignmentFlag.AlignCenter)
        lay.addStretch(1)
        if is_metro_list_table_theme(self._theme_id_normalized) or self._theme_id_normalized == "Minecraft":
            thumb_host.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
            thumb_host.setStyleSheet("background-color: transparent; border: none;")
        texts = (row.name, row.time_display, row.size_display, row.extra1, row.extra2)
        if self._sort_is_free_draggable():
            drag = _DragSortHintWidget(self, row_idx)
            drag.setFixedWidth(_DRAG_SORT_COL_WIDTH)
            if is_metro_list_table_theme(self._theme_id_normalized) or self._theme_id_normalized == "Minecraft":
                drag.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
                drag.setStyleSheet("background-color: transparent; border: none;")
            self.setCellWidget(row_idx, 0, drag)
            self.setCellWidget(row_idx, 1, thumb_host)
            start_col = 2
        else:
            self.setCellWidget(row_idx, 0, thumb_host)
            start_col = 1
        for j, txt in enumerate(texts):
            it = QTableWidgetItem(txt)
            it.setTextAlignment(int(_alignment_for_column_header(_TEXT_HEADERS[j])))
            it.setToolTip(txt)
            self.setItem(row_idx, start_col + j, it)


__all__ = [
    "ContentListTableWidget",
    "ContentRow",
    "format_size_bytes_display",
    "generate_sample_content_rows",
]

