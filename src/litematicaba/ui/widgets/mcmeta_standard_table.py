"""与「游戏资源版本」弹窗一致的 QTableWidget：首列 + 双操作列、主题行悬停、单元格底纹对齐。

供 ``McmetaVersionPickerDialog`` 与 UI 测试页共用；样式 QSS 见 ``themes/list_view_supplement``。"""
from __future__ import annotations

from PySide6.QtCore import QEvent, QObject, Qt, QTimer
from PySide6.QtGui import QColor, QCursor, QFontMetrics, QMouseEvent, QPainter, QPalette
from PySide6.QtWidgets import (
    QAbstractItemView,
    QApplication,
    QHBoxLayout,
    QHeaderView,
    QPushButton,
    QStyledItemDelegate,
    QStyleOptionViewItem,
    QTableWidget,
    QVBoxLayout,
    QWidget,
)

from litematicaba.ui.content_display.list_table.view import (
    METRO_LIST_ROW_HOVER_COLOR,
    METRO_LIST_ROW_TEXT_COLOR,
)
from litematicaba.ui.table.minecraft_list_profile import (
    MCMETA_VIEWPORT_FILL_BELOW_ITEMS_MINECRAFT_HEX,
    apply_minecraft_list_table_header_chrome,
)
from litematicaba.ui.theme import normalize_theme_id
from litematicaba.ui.themes.list_view_supplement import (
    mcmeta_table_row_hover_bg,
    mcmeta_table_row_hover_fg_optional,
    mcmeta_version_table_list_row_height_px,
    mcmeta_version_table_min_height_px,
)

OBJ_MCMETA_STANDARD_APPLY_BTN = "McmetaCellApplyBtn"
OBJ_MCMETA_STANDARD_OP_BTN = "McmetaCellOpBtn"


class McmetaStandardTableRowHoverController(QObject):
    """视口悬停与操作列单元格高亮；``parent`` 为表格以便随表格销毁。"""

    def __init__(
        self,
        table: QTableWidget,
        *,
        highlight_columns: tuple[int, ...] = (1, 2),
    ) -> None:
        super().__init__(table)
        self._table = table
        self._highlight_columns = highlight_columns
        self._installed = False

    def install_on_viewport(self) -> None:
        if not self._installed:
            self._table.viewport().installEventFilter(self)
            self._installed = True

    def remove_from_viewport(self) -> None:
        if self._installed:
            self._table.viewport().removeEventFilter(self)
            self._installed = False

    def set_hover_row(self, row: int | None) -> None:
        row = row if row is not None and row >= 0 else None
        if row == getattr(self._table, "_mcmeta_hover_row", None):
            return
        self._table._mcmeta_hover_row = row  # type: ignore[attr-defined]
        for r in range(self._table.rowCount()):
            for c in self._highlight_columns:
                host = self._table.cellWidget(r, c)
                if isinstance(host, McmetaStandardTableCellHost):
                    host.set_hover_highlight(row is not None and r == row)
        self._table.viewport().update()

    def sync_hover_from_cursor(self) -> None:
        w = QApplication.widgetAt(QCursor.pos())
        if w is None or not self._table.isAncestorOf(w):
            self.set_hover_row(None)
            return
        vp = self._table.viewport()
        local = vp.mapFromGlobal(QCursor.pos())
        r = self._table.rowAt(int(local.y()))
        self.set_hover_row(r if r >= 0 else None)

    def eventFilter(self, obj: QObject, event: QEvent) -> bool:  # type: ignore[override]
        if obj is self._table.viewport():
            if event.type() == QEvent.Type.MouseMove and isinstance(event, QMouseEvent):
                r = self._table.rowAt(int(event.position().y()))
                self.set_hover_row(r if r >= 0 else None)
            elif event.type() == QEvent.Type.Leave:
                QTimer.singleShot(0, self.sync_hover_from_cursor)
        return super().eventFilter(obj, event)


class _McmetaStandardButtonRowHoverForwarder(QObject):
    def __init__(self, controller: McmetaStandardTableRowHoverController, row: int) -> None:
        super().__init__()
        self._controller = controller
        self._row = row

    def eventFilter(self, obj: QObject, event: QEvent) -> bool:  # type: ignore[override]
        et = event.type()
        if et in (QEvent.Type.Enter, QEvent.Type.HoverMove, QEvent.Type.MouseMove):
            self._controller.set_hover_row(self._row)
        elif et == QEvent.Type.Leave:
            QTimer.singleShot(0, self._controller.sync_hover_from_cursor)
        return False


class McmetaStandardTableCellHost(QWidget):
    """操作列单元格背景：透明底纹 + 悬停行铺色。"""

    def __init__(
        self,
        table: QTableWidget,
        row: int,
        controller: McmetaStandardTableRowHoverController,
    ) -> None:
        super().__init__()
        self._table = table
        self._row = row
        self._controller = controller
        self._hover_highlight = False
        self.setAutoFillBackground(False)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setMouseTracking(True)

    def set_hover_highlight(self, on: bool) -> None:
        if self._hover_highlight == on:
            return
        self._hover_highlight = on
        self.update()

    def _hover_fill(self) -> QColor:
        bg = getattr(self._table, "_mcmeta_hover_bg", None)
        return bg if isinstance(bg, QColor) else QColor("#e6e6e6")

    def paintEvent(self, event) -> None:  # type: ignore[override]
        p = QPainter(self)
        if self._hover_highlight:
            p.fillRect(self.rect(), self._hover_fill())
        p.end()

    def enterEvent(self, event) -> None:  # type: ignore[override]
        self._controller.set_hover_row(self._row)
        super().enterEvent(event)

    def leaveEvent(self, event) -> None:  # type: ignore[override]
        QTimer.singleShot(0, self._controller.sync_hover_from_cursor)
        super().leaveEvent(event)

    def mouseMoveEvent(self, event) -> None:  # type: ignore[override]
        self._controller.set_hover_row(self._row)
        super().mouseMoveEvent(event)


def attach_mcmeta_row_hover_to_button(
    btn: QPushButton,
    controller: McmetaStandardTableRowHoverController,
    row: int,
) -> None:
    """单格多按钮时，每个按钮都需安装，避免悬停行错位。"""
    btn.setAttribute(Qt.WidgetAttribute.WA_Hover, True)
    fwd = _McmetaStandardButtonRowHoverForwarder(controller, row)
    fwd.setParent(btn)
    btn.installEventFilter(fwd)


def mcmeta_standard_wrap_action_button(
    btn: QPushButton,
    table: QTableWidget,
    row: int,
    controller: McmetaStandardTableRowHoverController,
) -> QWidget:
    host = McmetaStandardTableCellHost(table, row, controller)
    attach_mcmeta_row_hover_to_button(btn, controller, row)
    outer = QVBoxLayout(host)
    outer.setContentsMargins(0, 0, 0, 0)
    outer.setSpacing(0)
    outer.addStretch(1)
    mid = QHBoxLayout()
    mid.setContentsMargins(0, 0, 0, 0)
    mid.addStretch(1)
    mid.addWidget(btn, 0, Qt.AlignmentFlag.AlignCenter)
    mid.addStretch(1)
    outer.addLayout(mid)
    outer.addStretch(1)
    return host


class McmetaStandardTextColumnsDelegate(QStyledItemDelegate):
    """指定文本列：斑马纹 + 悬停行主题色；Metro10 下整表无斑马纹时各列白底自绘。"""

    def __init__(
        self,
        parent_table: QTableWidget,
        *,
        text_columns: tuple[int, ...],
        metro_no_zebra: bool,
    ) -> None:
        super().__init__(parent_table)
        self._text_columns = text_columns
        self._metro_no_zebra = metro_no_zebra

    def paint(self, painter: QPainter, option: QStyleOptionViewItem, index) -> None:  # type: ignore[override]
        if index.column() not in self._text_columns:
            super().paint(painter, option, index)
            return
        parent = self.parent()
        if not isinstance(parent, QTableWidget):
            super().paint(painter, option, index)
            return
        opt = QStyleOptionViewItem(option)
        self.initStyleOption(opt, index)
        hr = getattr(parent, "_mcmeta_hover_row", None)
        row = index.row()
        hover_bg = getattr(parent, "_mcmeta_hover_bg", None)
        if not isinstance(hover_bg, QColor):
            hover_bg = METRO_LIST_ROW_HOVER_COLOR
        hover_fg = getattr(parent, "_mcmeta_hover_fg", None)

        if self._metro_no_zebra:
            painter.save()
            painter.setClipRect(opt.rect)
            if hr is not None and row == hr:
                painter.fillRect(opt.rect, hover_bg)
            else:
                painter.fillRect(opt.rect, QColor("#ffffff"))
            painter.setPen(METRO_LIST_ROW_TEXT_COLOR)
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
            painter.fillRect(opt.rect, hover_bg)
            fg = hover_fg if isinstance(hover_fg, QColor) else opt.palette.color(QPalette.ColorRole.Text)
            painter.setPen(fg)
            tr = opt.rect.adjusted(4, 0, -4, 0)
            elided = QFontMetrics(opt.font).elidedText(
                opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
            )
            painter.drawText(tr, int(opt.displayAlignment), elided)
            painter.restore()
            return

        super().paint(painter, option, index)


class McmetaStandardFirstColumnDelegate(McmetaStandardTextColumnsDelegate):
    """首列专用（与三列操作表一致）。"""

    def __init__(self, parent_table: QTableWidget, *, metro_no_zebra: bool) -> None:
        super().__init__(parent_table, text_columns=(0,), metro_no_zebra=metro_no_zebra)


def apply_mcmeta_table_viewport_fill_below_items(table: QTableWidget, theme_id: str) -> None:
    """表体视口在末行之下的区域着色，避免透明 item 与 QSS 组合时透出异常底色。

    **颜色依据**：与当前主题下列表/ supplement 表体的主底色一致——Metro 等浅色表为 ``#ffffff``（同 ``table_list_item_bg_primary``）；Minecraft 为 ``#000000``（同 supplement 中表 ``background-color``，常量 ``MCMETA_VIEWPORT_FILL_BELOW_ITEMS_MINECRAFT_HEX``）。
    """
    tid = normalize_theme_id(theme_id)
    hx = MCMETA_VIEWPORT_FILL_BELOW_ITEMS_MINECRAFT_HEX if tid == "Minecraft" else "#ffffff"
    vp = table.viewport()
    vp.setAutoFillBackground(True)
    pal = vp.palette()
    pal.setColor(QPalette.ColorRole.Window, QColor(hx))
    vp.setPalette(pal)


def clear_mcmeta_table_current_cell(table: QTableWidget) -> None:
    """操作表为 NoSelection，但首列 ItemIsSelectable 仍会产生 current cell；清除以免 QSS :selected 状铺满留白。"""
    table.setCurrentItem(None)


def apply_mcmeta_standard_table_row_heights(table: QTableWidget, theme_id: str) -> None:
    tid = normalize_theme_id(theme_id)
    h = mcmeta_version_table_list_row_height_px(tid)
    if h is None:
        return
    vh = table.verticalHeader()
    vh.setDefaultSectionSize(h)
    for r in range(table.rowCount()):
        table.setRowHeight(r, h)


def configure_mcmeta_standard_action_table(
    table: QTableWidget,
    theme_id: str,
    *,
    column_labels: tuple[str, str, str] = ("说明", "", ""),
) -> McmetaStandardTableRowHoverController:
    """配置列宽、斑马纹、delegate、悬停色、最小高度，并安装视口悬停；返回控制器。"""
    tid = normalize_theme_id(theme_id)
    metro10 = tid == "Metro10"
    table.setObjectName("McmetaVersionTable")
    table.setColumnCount(3)
    table.setHorizontalHeaderLabels(list(column_labels))
    table.setShowGrid(False)
    table.verticalHeader().setVisible(False)
    table.setEditTriggers(table.EditTrigger.NoEditTriggers)
    table.setSelectionMode(QAbstractItemView.SelectionMode.NoSelection)
    table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectItems)
    table.setAutoScroll(False)
    hh = table.horizontalHeader()
    hh.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
    hh.setSectionResizeMode(1, QHeaderView.ResizeMode.Fixed)
    table.setColumnWidth(1, 72)
    hh.setSectionResizeMode(2, QHeaderView.ResizeMode.Fixed)
    table.setColumnWidth(2, 72)

    table._mcmeta_hover_row = None  # type: ignore[attr-defined]
    table._mcmeta_hover_bg = mcmeta_table_row_hover_bg(tid)  # type: ignore[attr-defined]
    table._mcmeta_hover_fg = mcmeta_table_row_hover_fg_optional(tid)  # type: ignore[attr-defined]
    table.setAlternatingRowColors(not metro10)
    table.setItemDelegate(McmetaStandardFirstColumnDelegate(table, metro_no_zebra=metro10))
    table.setMouseTracking(True)
    table.viewport().setMouseTracking(True)
    table.setMinimumHeight(mcmeta_version_table_min_height_px(tid))

    apply_mcmeta_table_viewport_fill_below_items(table, tid)
    apply_minecraft_list_table_header_chrome(table, tid)

    ctrl = McmetaStandardTableRowHoverController(table)
    ctrl.install_on_viewport()
    return ctrl


def apply_game_resource_language_table_chrome(
    table: QTableWidget,
    theme_id: str,
) -> McmetaStandardTableRowHoverController:
    """游戏资源语言管理：前 3 列为文本，第 4 列为操作区；与操作表共用 QSS / 悬停 token。"""
    tid = normalize_theme_id(theme_id)
    metro10 = tid == "Metro10"
    table.setObjectName("GameResourceLanguageTable")
    table.setShowGrid(False)
    table.verticalHeader().setVisible(False)
    table.setEditTriggers(table.EditTrigger.NoEditTriggers)
    table.setSelectionMode(QAbstractItemView.SelectionMode.NoSelection)
    table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectItems)
    table.setAutoScroll(False)
    table._mcmeta_hover_row = None  # type: ignore[attr-defined]
    table._mcmeta_hover_bg = mcmeta_table_row_hover_bg(tid)  # type: ignore[attr-defined]
    table._mcmeta_hover_fg = mcmeta_table_row_hover_fg_optional(tid)  # type: ignore[attr-defined]
    table.setAlternatingRowColors(not metro10)
    table.setItemDelegate(
        McmetaStandardTextColumnsDelegate(
            table, text_columns=(0, 1, 2), metro_no_zebra=metro10
        )
    )
    table.setMouseTracking(True)
    table.viewport().setMouseTracking(True)
    table.setMinimumHeight(mcmeta_version_table_min_height_px(tid))
    apply_mcmeta_table_viewport_fill_below_items(table, tid)
    apply_minecraft_list_table_header_chrome(table, tid)

    ctrl = McmetaStandardTableRowHoverController(table, highlight_columns=(3,))
    ctrl.install_on_viewport()
    return ctrl


def apply_block_icon_resource_table_chrome(
    table: QTableWidget,
    theme_id: str,
) -> McmetaStandardTableRowHoverController:
    """方块图标资源管理：前三列为文本，第四列为操作区；语义与语言管理表一致。"""
    tid = normalize_theme_id(theme_id)
    metro10 = tid == "Metro10"
    table.setObjectName("GameResourceBlockIconTable")
    table.setShowGrid(False)
    table.verticalHeader().setVisible(False)
    table.setEditTriggers(table.EditTrigger.NoEditTriggers)
    table.setSelectionMode(QAbstractItemView.SelectionMode.NoSelection)
    table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectItems)
    table.setAutoScroll(False)
    table._mcmeta_hover_row = None  # type: ignore[attr-defined]
    table._mcmeta_hover_bg = mcmeta_table_row_hover_bg(tid)  # type: ignore[attr-defined]
    table._mcmeta_hover_fg = mcmeta_table_row_hover_fg_optional(tid)  # type: ignore[attr-defined]
    table.setAlternatingRowColors(not metro10)
    table.setItemDelegate(
        McmetaStandardTextColumnsDelegate(
            table, text_columns=(0, 1, 2), metro_no_zebra=metro10
        )
    )
    table.setMouseTracking(True)
    table.viewport().setMouseTracking(True)
    table.setMinimumHeight(mcmeta_version_table_min_height_px(tid))
    apply_mcmeta_table_viewport_fill_below_items(table, tid)
    apply_minecraft_list_table_header_chrome(table, tid)

    ctrl = McmetaStandardTableRowHoverController(table, highlight_columns=(3,))
    ctrl.install_on_viewport()
    return ctrl


def reapply_mcmeta_standard_action_table_theme(
    table: QTableWidget,
    controller: McmetaStandardTableRowHoverController,
    theme_id: str,
) -> None:
    """主题切换后刷新悬停色、斑马纹、delegate 与行高（不重建表格行）。"""
    tid = normalize_theme_id(theme_id)
    metro10 = tid == "Metro10"
    table._mcmeta_hover_bg = mcmeta_table_row_hover_bg(tid)  # type: ignore[attr-defined]
    table._mcmeta_hover_fg = mcmeta_table_row_hover_fg_optional(tid)  # type: ignore[attr-defined]
    table.setAlternatingRowColors(not metro10)
    table.setItemDelegate(McmetaStandardFirstColumnDelegate(table, metro_no_zebra=metro10))
    table.setMinimumHeight(mcmeta_version_table_min_height_px(tid))
    apply_mcmeta_standard_table_row_heights(table, tid)
    apply_mcmeta_table_viewport_fill_below_items(table, tid)
    clear_mcmeta_table_current_cell(table)
    apply_minecraft_list_table_header_chrome(table, tid)
    controller.set_hover_row(None)
    table.viewport().update()
