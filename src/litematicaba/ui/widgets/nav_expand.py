"""侧栏展开/收起：Metro 下为三横线 + 展开时粗体应用名；其它主题走标准按钮。"""

from __future__ import annotations

from PySide6.QtCore import QEvent, QRect, Qt
from PySide6.QtGui import QColor, QEnterEvent, QPainter, QPen
from PySide6.QtWidgets import QApplication, QPushButton

from litematicaba.ui.theme import current_theme_id

_BG = QColor("#f7f7f7")
_BG_HOVER = QColor("#e2e2e2")
_BORDER_HOVER = QColor("#999999")
_FG = QColor("#1a1a1a")

_WIN10_SIDEBAR_THEMES = frozenset({"Metro10", "Metro8"})
_APP_TITLE = "Litematica BA"
_ICON_SLOT = 48
_HAMBURGER_PX = 16


class NavExpandButton(QPushButton):
    """Metro：三横线；展开时右侧粗体应用名。收起时仅居中显示三横线。"""

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.setObjectName("navExpand")
        self.setCheckable(False)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setAttribute(Qt.WidgetAttribute.WA_Hover, True)
        self.setText("")  # 文案自绘
        self._sidebar_expanded = True
        self._apply_nav_theme_attrs()

    def set_sidebar_expanded(self, expanded: bool) -> None:
        self._sidebar_expanded = expanded
        self.update()

    def _use_win10_sidebar(self) -> bool:
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        return tid in _WIN10_SIDEBAR_THEMES

    def _apply_nav_theme_attrs(self) -> None:
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        win10 = tid in _WIN10_SIDEBAR_THEMES
        mc = tid == "Minecraft"
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, not (win10 or mc))
        self.setMinimumHeight(48)
        self.setMaximumHeight(48)

    def changeEvent(self, event: QEvent) -> None:
        if event.type() == QEvent.Type.StyleChange:
            self._apply_nav_theme_attrs()
            self.update()
        super().changeEvent(event)

    def enterEvent(self, event: QEnterEvent) -> None:
        super().enterEvent(event)
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        if tid in _WIN10_SIDEBAR_THEMES or tid == "Minecraft":
            self.update()

    def leaveEvent(self, event) -> None:
        super().leaveEvent(event)
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        if tid in _WIN10_SIDEBAR_THEMES or tid == "Minecraft":
            self.update()

    @staticmethod
    def _draw_hamburger(p: QPainter, rect: QRect, color: QColor | None = None) -> None:
        p.setPen(QPen(color if color else _FG, 2, Qt.PenStyle.SolidLine, Qt.PenCapStyle.FlatCap))
        x1, x2 = rect.left() + 1, rect.right() - 1
        h = rect.height()
        y1 = rect.top() + h // 4
        y2 = rect.top() + h // 2
        y3 = rect.top() + 3 * h // 4
        p.drawLine(x1, y1, x2, y1)
        p.drawLine(x1, y2, x2, y2)
        p.drawLine(x1, y3, x2, y3)

    def paintEvent(self, event) -> None:
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        is_win10 = tid in _WIN10_SIDEBAR_THEMES
        is_mc = tid == "Minecraft"

        if not is_win10 and not is_mc:
            super().paintEvent(event)
            return

        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        r = self.rect()

        if is_win10:
            hover = self.underMouse() and self.isEnabled()
            bg = _BG_HOVER if hover else _BG
            p.fillRect(r, bg)
            if hover:
                p.setPen(QPen(_BORDER_HOVER, 1))
                p.drawRect(r.adjusted(0, 0, -1, -1))
        else:
            # Minecraft 主题背景
            from PySide6.QtWidgets import QStyleOptionButton, QStyle
            opt = QStyleOptionButton()
            self.initStyleOption(opt)
            opt.text = ""
            self.style().drawControl(QStyle.ControlElement.CE_PushButton, opt, p, self)

        # 三横线颜色：Win10 为黑色，MC 为白色
        fg_color = QColor("white") if is_mc else _FG
        
        # 三横线位置：收起时居中；展开时 Win10 在左侧槽位，MC 居中（因为没有文字）
        if self._sidebar_expanded and is_win10:
            hx = (_ICON_SLOT - _HAMBURGER_PX) // 2
            hy = (r.height() - _HAMBURGER_PX) // 2
            ham = QRect(hx, hy, _HAMBURGER_PX, _HAMBURGER_PX)
        else:
            hx = (r.width() - _HAMBURGER_PX) // 2
            hy = (r.height() - _HAMBURGER_PX) // 2
            ham = QRect(hx, hy, _HAMBURGER_PX, _HAMBURGER_PX)

        self._draw_hamburger(p, ham, fg_color)

        if self._sidebar_expanded and is_win10:
            f = self.font()
            f.setBold(True)
            p.setFont(f)
            p.setPen(_FG)
            text_rect = QRect(_ICON_SLOT, 0, max(0, r.width() - _ICON_SLOT), r.height())
            p.drawText(text_rect, Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, _APP_TITLE)
