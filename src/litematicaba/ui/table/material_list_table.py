"""材料列表对话框内 ``QTableWidget``：列表型 ``table_list_hover_bg`` / ``table_list_selected_bg``（delegate + 图标列同步）。"""

from __future__ import annotations

from PySide6.QtCore import QEvent, QModelIndex, QObject, QPoint, Qt, QTimer
from PySide6.QtGui import (
    QColor,
    QCursor,
    QFontMetrics,
    QGuiApplication,
    QMouseEvent,
    QPainter,
    QPalette,
    QPixmap,
)
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLabel,
    QStyledItemDelegate,
    QStyleOptionViewItem,
    QTableWidget,
    QVBoxLayout,
    QWidget,
)

from litematicaba.ui.table.metro10_list_profile import (
    METRO10_LIST_TABLE_TOKENS,
    apply_metro_list_flat_chrome_flags,
    content_list_row_height_px,
    is_metro_list_table_theme,
)
from litematicaba.ui.table.minecraft_list_profile import (
    MINECRAFT_LIST_TABLE_TOKENS,
    apply_minecraft_content_list_chrome_flags,
    apply_minecraft_list_table_header_chrome,
    is_minecraft_list_table_theme,
)
from litematicaba.ui.theme import normalize_theme_id
from litematicaba.ui.themes.base import icon_dir
from litematicaba.ui.widgets.mcmeta_standard_table import apply_mcmeta_table_viewport_fill_below_items

_BLOCK_ICON_PM: QPixmap | None = None


def material_list_block_icon_pixmap_32() -> QPixmap:
    """材料列表缺省占位图：``resources/icon/block_example.png`` 缩放到 **32×32**。"""
    global _BLOCK_ICON_PM
    if _BLOCK_ICON_PM is not None and not _BLOCK_ICON_PM.isNull():
        return _BLOCK_ICON_PM
    path = icon_dir() / "block_example.png"
    pm = QPixmap(str(path))
    if pm.isNull():
        fb = QPixmap(32, 32)
        fb.fill(QColor(120, 120, 120))
        _BLOCK_ICON_PM = fb
    else:
        _BLOCK_ICON_PM = pm.scaled(
            32,
            32,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.FastTransformation,
        )
    return _BLOCK_ICON_PM


def material_list_block_icon_pixmap_32_for_block(block_id: str) -> QPixmap:
    """按「应用到材料列表」资源解析方块 PNG；图标列**固定 32×32**，不采用列表型 ``table_list_thumb_px`` 约定。"""
    from litematicaba.core.game_resource_block_icon import (
        normalize_material_list_block_local_id,
        resolve_material_list_icon_path,
    )
    from litematicaba.core.material_list_icon_pixmap_cache import prewarmed_scaled_pixmap_for_local_id

    lid = normalize_material_list_block_local_id(block_id)
    hit = prewarmed_scaled_pixmap_for_local_id(lid)
    if hit is not None:
        return hit

    p = resolve_material_list_icon_path(block_id)
    if p is not None:
        pm = QPixmap(str(p))
        if not pm.isNull():
            return pm.scaled(
                32,
                32,
                Qt.AspectRatioMode.KeepAspectRatio,
                Qt.TransformationMode.FastTransformation,
            )
    return material_list_block_icon_pixmap_32()


def sync_material_list_row_heights(table: QTableWidget, theme_id: str) -> None:
    tid = normalize_theme_id(theme_id)
    rh = content_list_row_height_px(tid)
    vh = table.verticalHeader()
    if rh is None:
        for r in range(table.rowCount()):
            table.resizeRowToContents(r)
    else:
        vh.setDefaultSectionSize(rh)
        for r in range(table.rowCount()):
            table.setRowHeight(r, rh)


def sync_material_list_icon_backgrounds(table: QTableWidget, theme_id: str) -> None:
    """图标列与内容列表一致：Metro / Minecraft 下行选、悬停铺 ``table_list_*`` 底色。"""
    tid = normalize_theme_id(theme_id)
    if not (is_metro_list_table_theme(tid) or tid == "Minecraft"):
        return
    m10 = METRO10_LIST_TABLE_TOKENS
    mc = MINECRAFT_LIST_TABLE_TOKENS
    selected = {idx.row() for idx in table.selectionModel().selectedRows()}
    hr = getattr(table, "_material_hover_row", None)
    for r in range(table.rowCount()):
        w = table.cellWidget(r, 0)
        if w is None:
            continue
        is_sel = r in selected
        is_hov = hr is not None and r == hr and not is_sel
        if is_metro_list_table_theme(tid):
            if is_sel:
                w.setStyleSheet(f"background-color: {m10.selected_bg_hex}; border: none;")
            elif is_hov:
                w.setStyleSheet(f"background-color: {m10.hover_bg_hex}; border: none;")
            else:
                w.setStyleSheet("background-color: transparent; border: none;")
        else:
            if is_sel:
                w.setStyleSheet(f"background-color: {mc.selected_bg_hex}; border: none;")
            elif is_hov:
                w.setStyleSheet(f"background-color: {mc.hover_bg_hex}; border: none;")
            else:
                w.setStyleSheet("background-color: transparent; border: none;")


def refresh_material_list_row_visuals(table: QTableWidget) -> None:
    tid = str(getattr(table, "_material_list_theme_id", "QTDefault"))
    sync_material_list_icon_backgrounds(table, tid)
    table.viewport().update()


def _material_list_row_payload(table: QTableWidget, row: int) -> tuple[str, str, int] | None:
    """返回 (方块 ID, 名称列展示文, 总计数量)；无效行返回 None。"""
    name_it = table.item(row, 1)
    total_it = table.item(row, 2)
    if name_it is None:
        return None
    raw = name_it.data(Qt.ItemDataRole.UserRole)
    bid = raw if isinstance(raw, str) else name_it.text()
    display = name_it.text()
    if total_it is None:
        total = 0
    else:
        try:
            total = int(str(total_it.text()).replace(",", "").strip())
        except ValueError:
            total = 0
    return bid, display, total


def _material_list_total_detail_html(total: int, *, fg_hex: str | None = None) -> str:
    """设计稿：总数 = 组数 × 64 + 余数 = 盒数 潜影盒（数字加粗）。"""
    g = total // 64
    r = total % 64
    box = f"{total / 1728:.2f}"
    inner = f"总计：<b>{total}</b> = <b>{g}</b> × 64 + <b>{r}</b> = <b>{box}</b> 潜影盒"
    if fg_hex:
        return f'<span style="color:{fg_hex}">{inner}</span>'
    return inner


def _material_list_hover_popup_sheet_and_fg(theme_id: str) -> tuple[str, str | None]:
    """悬浮窗 QSS 与富文本前景色；非 Metro 列表 / Minecraft 时返回 (\"\", None) 用调色板。"""
    tid = normalize_theme_id(theme_id)
    if is_metro_list_table_theme(tid):
        return (
            "QWidget#MaterialListRowHoverPopup { background-color: #f0f0f0; border: 2px solid #0078d7; }"
            "QLabel { color: #000000; background: transparent; border: none; }",
            "#000000",
        )
    if tid == "Minecraft":
        return (
            "QWidget#MaterialListRowHoverPopup { background-color: #000000; border: 2px solid #999999; }"
            "QLabel { color: #ffffff; background: transparent; border: none; }",
            "#ffffff",
        )
    return ("", None)


def _material_list_hover_popup_sync_from_global(
    table: QTableWidget, popup: "_MaterialListRowHoverPopup", global_pos: QPoint
) -> None:
    """按全局坐标下指针在视口内的行，显示/更新/隐藏悬浮窗（用于 MouseMove 与滚轮）。"""
    vp = table.viewport()
    local = vp.mapFromGlobal(global_pos)
    if not vp.rect().contains(local):
        popup.hide()
        popup.popup_reset_row()
        return
    r = table.rowAt(int(local.y()))
    if r >= 0 and popup.prepare_for_row(r):
        popup.move_near(global_pos)
        popup.show()
    else:
        popup.hide()
        popup.popup_reset_row()


class _MaterialListRowHoverPopup(QWidget):
    """材料列表行悬浮信息窗：跟随指针，展示图标、译名、ID、总计分解。"""

    _OFFSET = 14
    _LINE_H = 32

    def __init__(self, table: QTableWidget) -> None:
        super().__init__(table.window())
        self.setObjectName("MaterialListRowHoverPopup")
        self._table = table
        self._popup_fg_for_rich: str | None = None
        self.setWindowFlags(
            Qt.WindowType.ToolTip
            | Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowDoesNotAcceptFocus
        )
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating, True)
        self.setAutoFillBackground(True)

        self._row_cached = -1
        lh = self._LINE_H
        self._lbl_id = QLabel()
        self._lbl_id.setTextInteractionFlags(Qt.TextInteractionFlag.NoTextInteraction)
        self._lbl_id.setFixedHeight(lh)
        self._lbl_id.setWordWrap(False)
        self._lbl_id.setAlignment(Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft)
        self._lbl_total = QLabel()
        self._lbl_total.setTextFormat(Qt.TextFormat.RichText)
        self._lbl_total.setTextInteractionFlags(Qt.TextInteractionFlag.NoTextInteraction)
        self._lbl_total.setFixedHeight(lh)
        self._lbl_total.setWordWrap(False)
        self._lbl_total.setAlignment(Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft)

        self._lbl_item_prefix = QLabel("项目：")
        self._lbl_item_prefix.setFixedHeight(lh)
        self._lbl_item_prefix.setWordWrap(False)
        self._lbl_item_prefix.setAlignment(Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft)
        icon_row = QHBoxLayout()
        icon_row.setContentsMargins(0, 0, 0, 0)
        icon_row.setSpacing(6)
        icon_row.addWidget(self._lbl_item_prefix)
        self._icon_label = QLabel()
        self._icon_label.setFixedSize(32, 32)
        self._icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._name_plain = QLabel()
        self._name_plain.setFixedHeight(lh)
        self._name_plain.setWordWrap(False)
        self._name_plain.setAlignment(Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft)
        icon_row.addWidget(self._icon_label)
        icon_row.addWidget(self._name_plain, 1)

        root = QVBoxLayout(self)
        root.setContentsMargins(8, 8, 8, 8)
        root.setSpacing(0)
        root.addLayout(icon_row)
        root.addWidget(self._lbl_id)
        root.addWidget(self._lbl_total)

    def sync_palette_from_table(self) -> None:
        self.setPalette(self._table.palette())
        f = self._table.font()
        self.setFont(f)
        for w in (self._lbl_item_prefix, self._lbl_id, self._lbl_total, self._icon_label, self._name_plain):
            w.setFont(f)

    def apply_popup_chrome(self) -> None:
        tid = str(getattr(self._table, "_material_list_theme_id", "QTDefault"))
        qss, fg = _material_list_hover_popup_sheet_and_fg(tid)
        self._popup_fg_for_rich = fg
        if qss:
            self.setStyleSheet(qss)
            f = self._table.font()
            self.setFont(f)
            for w in (self._lbl_item_prefix, self._lbl_id, self._lbl_total, self._icon_label, self._name_plain):
                w.setFont(f)
        else:
            self.setStyleSheet("")
            self.sync_palette_from_table()

    def prepare_for_row(self, row: int) -> bool:
        pl = _material_list_row_payload(self._table, row)
        if pl is None:
            return False
        bid, display, total = pl
        if row != self._row_cached:
            self._row_cached = row
            pm = material_list_block_icon_pixmap_32_for_block(bid)
            self._icon_label.setPixmap(pm)
            self._name_plain.setText(display)
            self._lbl_id.setText(f"ID：{bid}")
        self._lbl_total.setText(
            _material_list_total_detail_html(total, fg_hex=self._popup_fg_for_rich)
        )
        return True

    def move_near(self, global_pos: QPoint) -> None:
        self.adjustSize()
        m = self._OFFSET
        fw, fh = self.width(), self.height()
        x = global_pos.x() + m
        y = global_pos.y() + m
        screen = QGuiApplication.screenAt(global_pos)
        if screen is None:
            screen = QGuiApplication.primaryScreen()
        geo = screen.availableGeometry()
        if x + fw > geo.right():
            x = global_pos.x() - fw - m
        if y + fh > geo.bottom():
            y = global_pos.y() - fh - m
        x = max(geo.left(), min(x, geo.right() - fw))
        y = max(geo.top(), min(y, geo.bottom() - fh))
        self.move(x, y)

    def popup_reset_row(self) -> None:
        self._row_cached = -1

    def showEvent(self, event) -> None:  # type: ignore[override]
        self.apply_popup_chrome()
        super().showEvent(event)


class _MaterialListTextColumnsDelegate(QStyledItemDelegate):
    """名称/总计列：Metro8·10 与 Minecraft 与 ``table_list_*`` 一致。"""

    def __init__(self, table: QTableWidget) -> None:
        super().__init__(table)
        self._table = table

    def paint(self, painter: QPainter, option: QStyleOptionViewItem, index) -> None:  # type: ignore[override]
        if index.column() not in (1, 2):
            super().paint(painter, option, index)
            return
        tid = normalize_theme_id(str(getattr(self._table, "_material_list_theme_id", "QTDefault")))
        opt = QStyleOptionViewItem(option)
        self.initStyleOption(opt, index)
        row = index.row()
        sel = self._table.selectionModel().isRowSelected(row, QModelIndex())
        hr = getattr(self._table, "_material_hover_row", None)
        text_pen = opt.palette.color(QPalette.ColorRole.Text)

        if is_metro_list_table_theme(tid):
            m10 = METRO10_LIST_TABLE_TOKENS
            painter.save()
            painter.setClipRect(opt.rect)
            if sel:
                painter.fillRect(opt.rect, QColor(m10.selected_bg_hex))
            elif hr is not None and row == hr:
                painter.fillRect(opt.rect, QColor(m10.hover_bg_hex))
            else:
                painter.fillRect(opt.rect, QColor(m10.item_bg_primary_hex))
            painter.setPen(text_pen)
            tr = opt.rect.adjusted(4, 0, -4, 0)
            elided = QFontMetrics(opt.font).elidedText(
                opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
            )
            painter.drawText(tr, int(opt.displayAlignment), elided)
            painter.restore()
            return

        if tid == "Minecraft":
            mc = MINECRAFT_LIST_TABLE_TOKENS
            if sel:
                painter.save()
                painter.setClipRect(opt.rect)
                painter.fillRect(opt.rect, QColor(mc.selected_bg_hex))
                painter.setPen(QColor(mc.text_fg_hex))
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
                painter.fillRect(opt.rect, QColor(mc.hover_bg_hex))
                painter.setPen(QColor(mc.text_fg_hex))
                tr = opt.rect.adjusted(4, 0, -4, 0)
                elided = QFontMetrics(opt.font).elidedText(
                    opt.text, Qt.TextElideMode.ElideRight, max(1, tr.width())
                )
                painter.drawText(tr, int(opt.displayAlignment), elided)
                painter.restore()
                return
            super().paint(painter, option, index)
            return

        super().paint(painter, option, index)


class _MaterialListHoverFilter(QObject):
    def __init__(self, table: QTableWidget) -> None:
        super().__init__(table)
        self._table = table

    def eventFilter(self, obj: QObject, event: QEvent) -> bool:  # type: ignore[override]
        if obj is self._table.viewport():
            tid = normalize_theme_id(str(getattr(self._table, "_material_list_theme_id", "QTDefault")))
            popup = getattr(self._table, "_material_list_row_hover_popup", None)
            if event.type() == QEvent.Type.MouseMove and isinstance(event, QMouseEvent):
                r = self._table.rowAt(int(event.position().y()))
                if is_metro_list_table_theme(tid) or tid == "Minecraft":
                    self._set_hover(r if r >= 0 else None)
                if isinstance(popup, _MaterialListRowHoverPopup):
                    _material_list_hover_popup_sync_from_global(
                        self._table, popup, event.globalPosition().toPoint()
                    )
            elif event.type() == QEvent.Type.Wheel:
                if isinstance(popup, _MaterialListRowHoverPopup) and popup.isVisible():
                    tbl = self._table

                    def _after_scroll() -> None:
                        p = getattr(tbl, "_material_list_row_hover_popup", None)
                        if isinstance(p, _MaterialListRowHoverPopup) and p.isVisible():
                            _material_list_hover_popup_sync_from_global(tbl, p, QCursor.pos())

                    QTimer.singleShot(0, _after_scroll)
            elif event.type() == QEvent.Type.Leave:
                if is_metro_list_table_theme(tid) or tid == "Minecraft":
                    self._set_hover(None)
                if isinstance(popup, _MaterialListRowHoverPopup):
                    popup.hide()
                    popup.popup_reset_row()
        return super().eventFilter(obj, event)

    def _set_hover(self, row: int | None) -> None:
        if row == getattr(self._table, "_material_hover_row", None):
            return
        self._table._material_hover_row = row  # type: ignore[attr-defined]
        refresh_material_list_row_visuals(self._table)


def _install_material_list_interaction_once(table: QTableWidget) -> None:
    if getattr(table, "_material_list_interaction_installed", False):
        return
    table._material_list_interaction_installed = True  # type: ignore[attr-defined]
    table.setMouseTracking(True)
    table.viewport().setMouseTracking(True)
    table._material_hover_row = None  # type: ignore[attr-defined]
    d = _MaterialListTextColumnsDelegate(table)
    table.setItemDelegateForColumn(1, d)
    table.setItemDelegateForColumn(2, d)
    popup = _MaterialListRowHoverPopup(table)
    table._material_list_row_hover_popup = popup  # type: ignore[attr-defined]
    filt = _MaterialListHoverFilter(table)
    table._material_list_hover_filter = filt  # type: ignore[attr-defined]
    table.viewport().installEventFilter(filt)
    table.selectionModel().selectionChanged.connect(lambda *_: refresh_material_list_row_visuals(table))


def configure_material_list_table(table: QTableWidget, theme_id: str) -> None:
    tid = normalize_theme_id(theme_id)
    table._material_list_theme_id = tid  # type: ignore[attr-defined]
    table.setObjectName("MaterialListTable")
    if is_metro_list_table_theme(tid):
        apply_metro_list_flat_chrome_flags(table)
    elif is_minecraft_list_table_theme(tid):
        apply_minecraft_content_list_chrome_flags(table)
    else:
        table.setShowGrid(False)
        table.setAlternatingRowColors(tid != "QTDefault")
        table.setStyleSheet("")
    apply_mcmeta_table_viewport_fill_below_items(table, tid)
    apply_minecraft_list_table_header_chrome(table, tid)
    _install_material_list_interaction_once(table)
    sync_material_list_row_heights(table, tid)
    refresh_material_list_row_visuals(table)
