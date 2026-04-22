"""磁贴控件（多主题下几何一致；QTDefault 使用标准样式 + 向内 1px 边框）。"""

from __future__ import annotations

from PySide6.QtCore import QEvent, Qt
from PySide6.QtCore import QRect
from PySide6.QtCore import QTimer
from PySide6.QtGui import QCursor, QEnterEvent, QShowEvent
from PySide6.QtGui import QColor, QPainter, QPalette
from PySide6.QtWidgets import QApplication, QLabel, QVBoxLayout, QWidget

from litematicaba.core.settings import load_settings
from litematicaba.ui.theme import current_theme_id, theme_supports_widget

class TileWidget(QWidget):
    """固定尺寸磁贴。QTDefault 下无边框 QSS，背景与边框在 ``paintEvent`` 中绘制（边框向内 1px）。"""

    def __init__(
        self,
        w: int,
        h: int,
        bg_color: str,
        label: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.setFixedSize(w, h)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self._bg_color = bg_color
        self._label = QLabel(label)
        tf = self._label.font()
        tf.setPointSize(9)
        self._label.setFont(tf)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(8, 8, 8, 8)
        lay.setAlignment(Qt.AlignmentFlag.AlignBottom | Qt.AlignmentFlag.AlignLeft)
        lay.addWidget(self._label)

        self._use_qt_default_paint = False
        self._is_applying_style = False
        self._last_style_sig = ""
        self._effective_tid = "QTDefault"
        self._hovered = False
        self._error_highlight = False

        self.installEventFilter(self)
        self._label.installEventFilter(self)
        self._apply_style()

    def _apply_style(self) -> None:
        if self._is_applying_style:
            return
        self._is_applying_style = True
        try:
            app = QApplication.instance()
            tid = current_theme_id(app) if app is not None else load_settings().normalized().theme_id
            has_tile_style = theme_supports_widget(tid, "tile")
            effective_tid = tid if has_tile_style else "QTDefault"
            self._effective_tid = effective_tid
            self._use_qt_default_paint = effective_tid == "QTDefault"
            self._label.setStyleSheet(
                "background: transparent; border: none; color: palette(window-text);"
            )
            style_sig = (
                f"{effective_tid}|{self._bg_color}|hover={self._hovered}"
                if effective_tid in ("Metro10", "Fluent11")
                else f"{effective_tid}|{self._bg_color}"
            )
            if style_sig == self._last_style_sig:
                self.update()
                return
            self._last_style_sig = style_sig
            if self._use_qt_default_paint:
                self.setStyleSheet("")
            else:
                if effective_tid == "Metro10":
                    self.setStyleSheet(
                        "background-color: #efefef;"
                        " border: none; border-radius: 0px;"
                    )
                elif effective_tid == "Fluent11":
                    self.setStyleSheet(
                        "background: transparent;"
                        " border: none; border-radius: 10px;"
                    )
                else:
                    self.setStyleSheet(
                        f"background-color: {self._bg_color}; border: none; border-radius: 2px;"
                    )
            self.update()
        finally:
            self._is_applying_style = False

    def _paint_inner_border(
        self,
        painter: QPainter,
        rect: QRect,
        *,
        color: QColor,
        thickness: int,
    ) -> None:
        t = max(1, int(thickness))
        r = rect
        painter.fillRect(r.x(), r.y(), r.width(), t, color)
        painter.fillRect(r.x(), r.y() + r.height() - t, r.width(), t, color)
        painter.fillRect(r.x(), r.y() + t, t, max(0, r.height() - 2 * t), color)
        painter.fillRect(r.x() + r.width() - t, r.y() + t, t, max(0, r.height() - 2 * t), color)

    def _paint_tile_border(self, painter: QPainter, rect: QRect) -> None:
        if self._error_highlight:
            color = QColor("#cc3333")
            thickness = 2
        elif self._effective_tid == "Metro10" and not self._hovered:
            return
        elif self._effective_tid == "Fluent11":
            color = QColor("#999999") if self._hovered else QColor("#eaeaea")
            thickness = 1
        else:
            color = QColor("#999999") if self._hovered else self.palette().color(QPalette.ColorRole.Mid)
            thickness = 2 if self._hovered else 1
        if self._effective_tid == "Fluent11":
            painter.save()
            painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
            pen = painter.pen()
            pen.setColor(color)
            pen.setWidth(thickness)
            painter.setPen(pen)
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawRoundedRect(rect.adjusted(0, 0, -1, -1), 10.0, 10.0)
            painter.restore()
            return
        self._paint_inner_border(painter, rect, color=color, thickness=thickness)

    def _paint_fluent11_background(self, painter: QPainter, rect: QRect) -> None:
        fill = QColor(self._bg_color or "#fdfdfd")
        if self._hovered and not self._error_highlight:
            fill = fill.darker(106)
        painter.save()
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(fill)
        painter.drawRoundedRect(rect.adjusted(0, 0, -1, -1), 10.0, 10.0)
        painter.restore()

    def trigger_error_highlight(self, duration_ms: int = 1000) -> None:
        self._error_highlight = True
        self.update()
        QTimer.singleShot(duration_ms, self._clear_error_highlight)

    def _clear_error_highlight(self) -> None:
        self._error_highlight = False
        self.update()

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        self._apply_style()

    def changeEvent(self, event: QEvent) -> None:
        if event.type() in (QEvent.Type.StyleChange, QEvent.Type.PaletteChange):
            if not self._is_applying_style:
                self._apply_style()
        super().changeEvent(event)

    def enterEvent(self, event: QEnterEvent) -> None:
        self._hovered = True
        self._apply_style()
        super().enterEvent(event)

    def leaveEvent(self, event: QEvent) -> None:
        self._hovered = False
        self._apply_style()
        super().leaveEvent(event)

    def eventFilter(self, watched: object, event: QEvent) -> bool:
        if event.type() in (QEvent.Type.Enter, QEvent.Type.HoverEnter):
            if not self._hovered:
                self._hovered = True
                self._apply_style()
        elif event.type() in (QEvent.Type.Leave, QEvent.Type.HoverLeave):
            pos = self.mapFromGlobal(QCursor.pos())
            inside = self.rect().contains(pos)
            if self._hovered and not inside:
                self._hovered = False
                self._apply_style()
        return super().eventFilter(watched, event)

    def paintEvent(self, event) -> None:
        if self._effective_tid == "Fluent11":
            p = QPainter(self)
            self._paint_fluent11_background(p, self.rect())
            self._paint_tile_border(p, self.rect())
            return
        if not self._use_qt_default_paint:
            super().paintEvent(event)
        p_border = QPainter(self)
        if self._use_qt_default_paint:
            p_border.fillRect(self.rect(), QColor(self._bg_color))
        self._paint_tile_border(p_border, self.rect())

