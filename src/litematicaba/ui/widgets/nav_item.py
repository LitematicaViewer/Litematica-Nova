"""侧栏导航项：Metro10 / Metro8 下为 Win10 风格自绘；其它主题走标准 QPushButton + 主题 QSS。"""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import QEvent, QRect, QSize, Qt
from PySide6.QtGui import QColor, QEnterEvent, QIcon, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QApplication, QPushButton

from litematicaba.ui.theme import current_theme_id
from litematicaba.ui.themes.base import icon_dir

# 与 ``themes/sidebar_nav_win10.py`` 中侧栏色值一致
_BG = QColor("#f7f7f7")
_BG_HOVER = QColor("#e2e2e2")
_BORDER_HOVER = QColor("#999999")
_INDICATOR = QColor("#0078d7")
_TEXT = QColor("#1a1a1a")
_PLACEHOLDER_FILL = QColor("#e0e0e0")
_PLACEHOLDER_BORDER = QColor("#999999")

_WIN10_SIDEBAR_THEMES = frozenset({"Metro10", "Metro8"})

_ICON_SLOT = 48
_ICON_PX = 16
_INDICATOR_W = 4


def _resolve_nav_icon_path(icon_stem: str) -> Path:
    """``icon/<stem>.svg`` 存在则用，否则 ``icon/undefined.svg``。"""
    root = icon_dir()
    candidate = root / f"{icon_stem}.svg"
    if candidate.is_file():
        return candidate
    return root / "undefined.svg"


def _load_nav_pixmap_16(path: Path) -> QPixmap:
    pm = QIcon(str(path)).pixmap(_ICON_PX, _ICON_PX)
    if pm.isNull():
        fallback = icon_dir() / "undefined.svg"
        if fallback.is_file():
            pm = QIcon(str(fallback)).pixmap(_ICON_PX, _ICON_PX)
    return pm


class NavItemButton(QPushButton):
    """
    - **Metro10 / Metro8**：48px 高、左侧选中条、48px 图标槽与占位图；展开时左文右图标区；收起时仅居中 16×16 图标。
    - **其它主题**：普通可勾选按钮，由当前主题 QSS 绘制。
    """

    def __init__(self, full: str, short: str, *, icon_stem: str = "undefined", parent=None) -> None:
        super().__init__(full, parent)
        self.setObjectName("navItem")
        self.setCheckable(True)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setProperty("labelFull", full)
        self.setProperty("labelShort", short)
        self.setProperty("iconStem", icon_stem)
        self.setAttribute(Qt.WidgetAttribute.WA_Hover, True)
        self._nav_expanded = True
        self._nav_icon_path = _resolve_nav_icon_path(icon_stem)
        self._icon_pixmap = _load_nav_pixmap_16(self._nav_icon_path)
        self.setIcon(QIcon(str(self._nav_icon_path)))
        self.setIconSize(QSize(_ICON_PX, _ICON_PX))
        self._apply_nav_theme_attrs()

    def set_nav_expanded(self, expanded: bool) -> None:
        self._nav_expanded = expanded
        self.update()

    def _use_custom_paint(self) -> bool:
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        return tid in _WIN10_SIDEBAR_THEMES or tid == "Minecraft"

    def _apply_nav_theme_attrs(self) -> None:
        custom = self._use_custom_paint()
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, not custom)
        if custom:
            self.setFixedHeight(48)
        else:
            self.setMinimumHeight(32)
            self.setMaximumHeight(16777215)

    def changeEvent(self, event: QEvent) -> None:
        if event.type() == QEvent.Type.StyleChange:
            self._apply_nav_theme_attrs()
            self.update()
        super().changeEvent(event)

    def enterEvent(self, event: QEnterEvent) -> None:
        super().enterEvent(event)
        if self._use_custom_paint():
            self.update()

    def leaveEvent(self, event) -> None:
        super().leaveEvent(event)
        if self._use_custom_paint():
            self.update()

    @staticmethod
    def _draw_placeholder_icon(p: QPainter, rect: QRect, color: QColor | None = None) -> None:
        p.setPen(QPen(color if color else _PLACEHOLDER_BORDER, 1))
        p.setBrush(_PLACEHOLDER_FILL if not color else Qt.GlobalColor.transparent)
        p.drawRoundedRect(rect.adjusted(0, 0, -1, -1), 2, 2)

    def _draw_nav_icon(self, p: QPainter, rect: QRect, color: QColor | None = None) -> None:
        if not self._icon_pixmap.isNull():
            pm = self._icon_pixmap
            if color:
                tmp = QPixmap(pm.size())
                tmp.fill(Qt.GlobalColor.transparent)
                tp = QPainter(tmp)
                tp.setCompositionMode(QPainter.CompositionMode.CompositionMode_Source)
                tp.drawPixmap(0, 0, pm)
                tp.setCompositionMode(QPainter.CompositionMode.CompositionMode_SourceIn)
                tp.fillRect(tmp.rect(), color)
                tp.end()
                pm = tmp
            p.drawPixmap(rect, pm)
        else:
            self._draw_placeholder_icon(p, rect, color)

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
        hover = self.underMouse() and self.isEnabled()
        checked = self.isChecked()

        if is_win10:
            bg = _BG_HOVER if hover else _BG
            p.fillRect(r, bg)
            if hover:
                p.setPen(QPen(_BORDER_HOVER, 1))
                p.drawRect(r.adjusted(0, 0, -1, -1))
        else:
            # Minecraft 主题：使用 QStyle 绘制九宫格背景，但排除文字和图标
            from PySide6.QtWidgets import QStyleOptionButton, QStyle
            opt = QStyleOptionButton()
            self.initStyleOption(opt)
            opt.text = ""
            opt.icon = QIcon()
            self.style().drawControl(QStyle.ControlElement.CE_PushButton, opt, p, self)

        expanded = self._nav_expanded
        icon_color = QColor("white") if is_mc else None

        if expanded:
            # 1. 图标在 Minecraft 下左对齐，Win10 下在 48px 槽位居中
            if is_mc:
                icon_margin = 16  # 从 12 改为 16，向右平移 4px
                icon_rect = QRect(icon_margin, (r.height() - _ICON_PX) // 2, _ICON_PX, _ICON_PX)
            else:
                slot = QRect(0, 0, _ICON_SLOT, r.height())
                icon_rect = QRect(0, 0, _ICON_PX, _ICON_PX)
                icon_rect.moveCenter(slot.center())
            
            self._draw_nav_icon(p, icon_rect, icon_color)

            # 2. 文字在 Minecraft 下居中对齐，Win10 下左对齐
            label = str(self.property("labelFull") or self.text())
            p.setFont(self.font())
            
            if is_mc:
                # 选中态文字变为黄色 #ffff00
                text_color = QColor("#ffff00") if checked else QColor("white")
                p.setPen(text_color)
                p.drawText(r, Qt.AlignmentFlag.AlignCenter, label)
            else:
                p.setPen(_TEXT)
                text_left = _ICON_SLOT
                text_rect = QRect(text_left, 0, max(0, r.width() - text_left), r.height())
                p.drawText(text_rect, Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, label)
        else:
            # 4. 收起时只显示图标，不显示文字
            slot = QRect(0, 0, r.width(), r.height())
            icon_rect = QRect(0, 0, _ICON_PX, _ICON_PX)
            icon_rect.moveCenter(slot.center())
            self._draw_nav_icon(p, icon_rect, icon_color)

        # 3. 选中指示条（仅 Win10）
        if is_win10 and checked:
            h_ind = 24
            y0 = (r.height() - h_ind) // 2
            p.fillRect(QRect(0, y0, _INDICATOR_W, h_ind), _INDICATOR)
