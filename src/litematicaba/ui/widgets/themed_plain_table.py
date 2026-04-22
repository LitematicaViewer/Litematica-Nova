"""与 UI 测试页 ``ContentListTableWidget`` 对齐的纯文本列表型 ``QTableWidget`` 主题样式。

Metro8 / Metro10：透明表头、无网格、行悬停/选中与 ``METRO10_LIST_TABLE_TOKENS``（``table_list_*``）一致。
Minecraft：斑马纹、行悬停/选中与 ``MINECRAFT_LIST_TABLE_TOKENS`` 一致（属性页区域表等）。
其它主题：Qt 默认表格绘制，行高 ``content_list_row_height_px``。
"""

from __future__ import annotations

from PySide6.QtCore import QEvent, QModelIndex, QObject, Qt
from PySide6.QtGui import QColor, QFontMetrics, QMouseEvent, QPainter, QPalette
from PySide6.QtWidgets import QStyledItemDelegate, QStyleOptionViewItem, QTableWidget, QWidget

from litematicaba.ui.content_display.list_table.view import METRO_LIST_TABLE_CHROME_QSS, is_metro_list_table_theme
from litematicaba.ui.table.metro10_list_profile import METRO10_LIST_TABLE_TOKENS, content_list_row_height_px
from litematicaba.ui.table.minecraft_list_profile import (
    MINECRAFT_LIST_TABLE_TOKENS,
    apply_minecraft_content_list_chrome_flags,
    apply_minecraft_list_table_header_chrome,
    is_minecraft_list_table_theme,
)
from litematicaba.ui.theme import normalize_theme_id
from litematicaba.ui.widgets.mcmeta_standard_table import apply_mcmeta_table_viewport_fill_below_items

_M10 = METRO10_LIST_TABLE_TOKENS
_METRO_HOVER_BG = QColor(_M10.hover_bg_hex)
_METRO_SEL_BG = QColor(_M10.selected_bg_hex)
_METRO_TEXT_FG = QColor(_M10.text_fg_hex)

_MC = MINECRAFT_LIST_TABLE_TOKENS
_MINECRAFT_HOVER_BG = QColor(_MC.hover_bg_hex)
_MINECRAFT_SEL_BG = QColor(_MC.selected_bg_hex)
_MINECRAFT_TEXT_FG = QColor(_MC.text_fg_hex)


def _plain_table_uses_row_hover_chrome(theme_id: str) -> bool:
    tid = normalize_theme_id(theme_id)
    return is_metro_list_table_theme(tid) or tid == "Minecraft"


class _MetroPlainDelegate(QStyledItemDelegate):
    def __init__(self, table: "ThemedPlainQTableWidget", parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._table = table

    def paint(self, painter: QPainter, option: QStyleOptionViewItem, index) -> None:  # type: ignore[override]
        opt = QStyleOptionViewItem(option)
        self.initStyleOption(opt, index)
        opt.palette.setColor(QPalette.ColorRole.Text, _METRO_TEXT_FG)
        row = index.row()
        sel = self._table.selectionModel().isRowSelected(row, QModelIndex())
        hr = self._table.hover_row
        painter.save()
        painter.setClipRect(opt.rect)
        if sel:
            painter.fillRect(opt.rect, _METRO_SEL_BG)
        elif hr is not None and row == hr:
            painter.fillRect(opt.rect, _METRO_HOVER_BG)
        painter.setPen(_METRO_TEXT_FG)
        tr = opt.rect.adjusted(4, 0, -4, 0)
        elided = QFontMetrics(opt.font).elidedText(
            opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
        )
        painter.drawText(tr, int(opt.displayAlignment), elided)
        painter.restore()


class _MinecraftPlainDelegate(QStyledItemDelegate):
    """Minecraft 纯文本表：与内容列表 delegate 同行选/悬停/斑马。"""

    def __init__(self, table: "ThemedPlainQTableWidget", parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._table = table

    def paint(self, painter: QPainter, option: QStyleOptionViewItem, index) -> None:  # type: ignore[override]
        opt = QStyleOptionViewItem(option)
        self.initStyleOption(opt, index)
        row = index.row()
        sel = self._table.selectionModel().isRowSelected(row, QModelIndex())
        hr = self._table.hover_row
        if sel:
            painter.save()
            painter.setClipRect(opt.rect)
            painter.fillRect(opt.rect, _MINECRAFT_SEL_BG)
            painter.setPen(_MINECRAFT_TEXT_FG)
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
            painter.fillRect(opt.rect, _MINECRAFT_HOVER_BG)
            painter.setPen(_MINECRAFT_TEXT_FG)
            tr = opt.rect.adjusted(4, 0, -4, 0)
            elided = QFontMetrics(opt.font).elidedText(
                opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
            )
            painter.drawText(tr, int(opt.displayAlignment), elided)
            painter.restore()
            return
        super().paint(painter, option, index)


class _ViewportHoverFilter(QObject):
    def __init__(self, table: "ThemedPlainQTableWidget") -> None:
        super().__init__(table)
        self._table = table

    def eventFilter(self, obj: QObject, event: QEvent) -> bool:  # type: ignore[override]
        if obj is self._table.viewport() and _plain_table_uses_row_hover_chrome(self._table.theme_id):
            if event.type() == QEvent.Type.MouseMove and isinstance(event, QMouseEvent):
                r = self._table.rowAt(int(event.position().y()))
                self._table.set_hover_row(r if r >= 0 else None)
            elif event.type() == QEvent.Type.Leave:
                self._table.set_hover_row(None)
        return super().eventFilter(obj, event)


class ThemedPlainQTableWidget(QTableWidget):
    """纯文本表格的主题封装；随全局主题切换调用 ``apply_theme``。"""

    def __init__(self, parent: QWidget | None = None, *, theme_id: str = "QTDefault") -> None:
        super().__init__(parent)
        self._theme_id = normalize_theme_id(theme_id)
        self._hover_row: int | None = None
        self._viewport_filter: QObject | None = None
        self.selectionModel().selectionChanged.connect(self._on_selection_changed)
        self._apply_chrome()

    @property
    def theme_id(self) -> str:
        return self._theme_id

    @property
    def hover_row(self) -> int | None:
        return self._hover_row

    def set_hover_row(self, row: int | None) -> None:
        if row == self._hover_row:
            return
        self._hover_row = row
        if _plain_table_uses_row_hover_chrome(self._theme_id):
            self.viewport().update()

    def _on_selection_changed(self) -> None:
        if _plain_table_uses_row_hover_chrome(self._theme_id):
            self.viewport().update()

    def apply_theme(self, theme_id: str) -> None:
        tid = normalize_theme_id(theme_id)
        if tid == self._theme_id:
            return
        self._theme_id = tid
        self._hover_row = None
        self._apply_chrome()

    def _teardown_metro(self) -> None:
        if self._viewport_filter is not None:
            self.viewport().removeEventFilter(self._viewport_filter)
            self._viewport_filter.deleteLater()
            self._viewport_filter = None
        self.setMouseTracking(False)
        self.viewport().setMouseTracking(False)

    def _apply_chrome(self) -> None:
        self._teardown_metro()
        if is_metro_list_table_theme(self._theme_id):
            self.setShowGrid(False)
            self.setAlternatingRowColors(False)
            self.setMouseTracking(True)
            self.viewport().setMouseTracking(True)
            self.setStyleSheet(METRO_LIST_TABLE_CHROME_QSS)
            self.setItemDelegate(_MetroPlainDelegate(self, self))
            filt = _ViewportHoverFilter(self)
            self.viewport().installEventFilter(filt)
            self._viewport_filter = filt
            apply_mcmeta_table_viewport_fill_below_items(self, self._theme_id)
        elif is_minecraft_list_table_theme(self._theme_id):
            apply_minecraft_content_list_chrome_flags(self)
            self.setItemDelegate(_MinecraftPlainDelegate(self, self))
            self.setMouseTracking(True)
            self.viewport().setMouseTracking(True)
            filt = _ViewportHoverFilter(self)
            self.viewport().installEventFilter(filt)
            self._viewport_filter = filt
            apply_mcmeta_table_viewport_fill_below_items(self, self._theme_id)
        else:
            self.setShowGrid(True)
            self.setAlternatingRowColors(False)
            self.setStyleSheet("")
            self.setItemDelegate(QStyledItemDelegate(self))
        apply_minecraft_list_table_header_chrome(self, self._theme_id)

    def sync_row_heights(self) -> None:
        """在数据行变更或主题切换后调用，使行高与 UI 测试页内容列表一致（``content_list_row_height_px``）。"""
        rh = content_list_row_height_px(self._theme_id)
        if rh is not None:
            for r in range(self.rowCount()):
                self.setRowHeight(r, rh)
        else:
            for r in range(self.rowCount()):
                self.resizeRowToContents(r)
