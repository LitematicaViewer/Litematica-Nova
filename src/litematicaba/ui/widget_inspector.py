"""鼠标悬停时高亮控件并显示 objectName / 类型 / 尺寸（调试用，不拦截点击）。"""

from __future__ import annotations

from PySide6.QtCore import QPoint, QRect, Qt, QTimer
from PySide6.QtGui import QColor, QCursor, QPainter, QPen
from PySide6.QtWidgets import QApplication, QWidget


def _host_contains_global(host: QWidget, global_pos: QPoint) -> bool:
    tl = host.mapToGlobal(QPoint(0, 0))
    return QRect(tl, host.size()).contains(global_pos)


def _is_descendant_of(widget: QWidget, ancestor: QWidget) -> bool:
    w: QWidget | None = widget
    while w is not None:
        if w is ancestor:
            return True
        w = w.parentWidget()
    return False


def _describe_widget(w: QWidget) -> list[str]:
    cls = w.metaObject().className()
    oid = w.objectName()
    if oid:
        title = f"{cls}  ({oid})"
        id_line = f"objectName: {oid}"
    else:
        title = cls
        id_line = "objectName: (未设置)"
    size_line = f"尺寸: {w.width()} × {w.height()} px"
    return [title, id_line, size_line]


class WidgetInspectorOverlay(QWidget):
    """覆盖整个主窗口客户端区域；仅绘制高亮与标签，鼠标事件穿透。"""

    def __init__(self, host: QWidget) -> None:
        super().__init__(host)
        self._rect = QRect()
        self._lines: list[str] = []
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)

    def set_highlight(self, rect_in_host: QRect, lines: list[str]) -> None:
        self._rect = rect_in_host
        self._lines = lines
        self.update()

    def clear_highlight(self) -> None:
        self._rect = QRect()
        self._lines = []
        self.update()

    def paintEvent(self, event) -> None:  # noqa: ARG002
        if not self._rect.isValid() or self._rect.isEmpty():
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        fill = QColor("#ff00ff")
        fill.setAlpha(110)
        p.fillRect(self._rect, fill)
        pen = QPen(QColor("#ff00ff"))
        pen.setWidth(1)
        p.setPen(pen)
        p.drawRect(self._rect.adjusted(0, 0, -1, -1))

        if not self._lines:
            return
        text = "\n".join(self._lines)
        p.setFont(self.font())
        fm = p.fontMetrics()
        margin = 6
        max_w = max(220, self._rect.width() - margin * 2)
        text_rect = fm.boundingRect(
            QRect(0, 0, max_w, 5000),
            int(Qt.AlignmentFlag.AlignLeft | Qt.TextFlag.TextWordWrap),
            text,
        )
        pad = 4
        label_w = min(text_rect.width() + pad * 2, self.width() - margin * 2)
        label_h = text_rect.height() + pad * 2
        lx = self._rect.left() + margin
        ly = self._rect.top() + margin
        if ly + label_h > self.height() - margin:
            ly = max(margin, self._rect.top() - label_h - margin)
        if lx + label_w > self.width() - margin:
            lx = max(margin, self.width() - margin - label_w)
        label = QRect(lx, ly, label_w, label_h)
        bg = QColor(0, 0, 0, 200)
        p.fillRect(label, bg)
        p.setPen(QColor(255, 255, 255))
        p.drawText(
            label.adjusted(pad, pad, -pad, -pad),
            int(Qt.AlignmentFlag.AlignLeft | Qt.TextFlag.TextWordWrap),
            text,
        )


class WidgetInspectorController:
    """由主窗口持有：按设置启停定时器并刷新覆盖层。"""

    def __init__(self, host: QWidget) -> None:
        self._host = host
        self._overlay = WidgetInspectorOverlay(host)
        self._overlay.hide()
        self._timer = QTimer(host)
        self._timer.setInterval(40)
        self._timer.timeout.connect(self._tick)
        self._enabled = False

    def set_enabled(self, on: bool) -> None:
        if on == self._enabled:
            return
        self._enabled = on
        if on:
            self._sync_geometry()
            self._overlay.show()
            self._overlay.raise_()
            self._timer.start()
        else:
            self._timer.stop()
            self._overlay.clear_highlight()
            self._overlay.hide()

    def sync_geometry(self) -> None:
        if self._enabled:
            self._sync_geometry()

    def raise_overlay(self) -> None:
        if self._enabled:
            self._overlay.raise_()

    def _sync_geometry(self) -> None:
        self._overlay.setGeometry(self._host.rect())

    def _tick(self) -> None:
        if not self._enabled:
            return
        self._overlay.raise_()
        app = QApplication.instance()
        if app is None:
            return
        gp = QCursor.pos()
        if not _host_contains_global(self._host, gp):
            self._overlay.clear_highlight()
            return
        w = app.widgetAt(gp)
        if w is None or w is self._overlay:
            self._overlay.clear_highlight()
            return
        if not _is_descendant_of(w, self._host):
            self._overlay.clear_highlight()
            return
        if not w.isVisible():
            self._overlay.clear_highlight()
            return
        lines = _describe_widget(w)
        tl_g = w.mapToGlobal(QPoint(0, 0))
        tl_h = self._host.mapFromGlobal(tl_g)
        rect = QRect(tl_h, w.size())
        self._overlay.set_highlight(rect, lines)
