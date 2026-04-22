"""主界面各业务页占位（后续替换为完整实现）。"""

from pathlib import Path

from PySide6.QtCore import QRectF, QSize, Qt
from PySide6.QtGui import QPaintEvent, QPainter, QPixmap
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import QLabel, QSizePolicy, QVBoxLayout, QWidget


def _aspect_fit_rect(total_w: int, total_h: int, content_w: float, content_h: float) -> QRectF | None:
    if content_w <= 0 or content_h <= 0 or total_w <= 0 or total_h <= 0:
        return None
    aspect = content_w / content_h
    w, h = total_w, total_h
    draw_w = min(w, int(h * aspect))
    draw_h = max(1, int(draw_w / aspect))
    if draw_h > h:
        draw_h = h
        draw_w = max(1, int(draw_h * aspect))
    x = (w - draw_w) / 2
    y = (h - draw_h) / 2
    return QRectF(x, y, float(draw_w), float(draw_h))


class _HomePixmapLogoWidget(QWidget):
    """主页 PNG Logo：在控件矩形内等比居中绘制，不拉伸。"""

    def __init__(self, image_path: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._pixmap = QPixmap(image_path)
        self.setMinimumSize(0, 0)
        pol = QSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        self.setSizePolicy(pol)

    def minimumSizeHint(self) -> QSize:
        return QSize(0, 0)

    def sizeHint(self) -> QSize:
        if self._pixmap.isNull():
            return QSize(200, 48)
        w, h = self._pixmap.width(), self._pixmap.height()
        cap_w = 520
        if w > cap_w and h > 0:
            s = cap_w / w
            return QSize(cap_w, max(1, int(h * s)))
        return QSize(max(1, w), max(1, h))

    def paintEvent(self, event: QPaintEvent) -> None:
        super().paintEvent(event)
        if self._pixmap.isNull():
            return
        pw = float(self._pixmap.width())
        ph = float(self._pixmap.height())
        rect = _aspect_fit_rect(self.width(), self.height(), pw, ph)
        if rect is None:
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        p.drawPixmap(rect.toRect(), self._pixmap, self._pixmap.rect())


class _HomeSvgLogoWidget(QWidget):
    """主页 SVG Logo：在控件矩形内等比居中渲染，不拉伸。"""

    def __init__(self, svg_path: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._renderer = QSvgRenderer(svg_path, self)
        self.setMinimumSize(0, 0)
        pol = QSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        self.setSizePolicy(pol)

    def minimumSizeHint(self) -> QSize:
        return QSize(0, 0)

    def sizeHint(self) -> QSize:
        ds = self._renderer.defaultSize()
        if ds.width() <= 0 or ds.height() <= 0:
            return QSize(240, 56)
        w, h = ds.width(), ds.height()
        cap_w = 520
        if w > cap_w:
            s = cap_w / w
            return QSize(cap_w, max(1, int(h * s)))
        return QSize(w, h)

    def paintEvent(self, event: QPaintEvent) -> None:
        super().paintEvent(event)
        if not self._renderer.isValid():
            return
        default_size = self._renderer.defaultSize()
        if default_size.height() <= 0 or default_size.width() <= 0:
            return
        rect = _aspect_fit_rect(
            self.width(),
            self.height(),
            float(default_size.width()),
            float(default_size.height()),
        )
        if rect is None:
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        self._renderer.render(p, rect)


def _empty_page(title: str, hint: str) -> QWidget:
    w = QWidget()
    lay = QVBoxLayout(w)
    lay.setAlignment(Qt.AlignmentFlag.AlignCenter)
    t = QLabel(title)
    t.setAlignment(Qt.AlignmentFlag.AlignCenter)
    f = t.font()
    f.setPointSize(16)
    f.setBold(True)
    t.setFont(f)
    h = QLabel(hint)
    h.setAlignment(Qt.AlignmentFlag.AlignCenter)
    h.setStyleSheet("color: palette(mid);")
    lay.addWidget(t)
    lay.addWidget(h)
    return w


class HomePage(QWidget):
    def __init__(self) -> None:
        super().__init__()
        root = QVBoxLayout(self)
        root.setContentsMargins(24, 24, 24, 24)
        root.addStretch(1)

        logo_col = QWidget()
        logo_col.setMinimumSize(0, 0)
        logo_col.setSizePolicy(
            QSizePolicy.Policy.Expanding,
            QSizePolicy.Policy.Expanding,
        )
        col_lay = QVBoxLayout(logo_col)
        col_lay.setContentsMargins(0, 0, 0, 0)
        col_lay.setSpacing(12)

        mc_path = self._resolve_logo("minecraft-style-logo.png")
        ba_path = self._resolve_logo("bluearchive-style-logo.svg")

        if mc_path is not None:
            mc_logo = _HomePixmapLogoWidget(str(mc_path))
            mc_logo.setObjectName("homeLogoMinecraft")
            col_lay.addWidget(mc_logo, 1)
        if ba_path is not None:
            ba_logo = _HomeSvgLogoWidget(str(ba_path))
            ba_logo.setObjectName("homeLogoBlueArchive")
            col_lay.addWidget(ba_logo, 1)

        if mc_path is None and ba_path is None:
            hint = QLabel("未找到 logo/minecraft-style-logo.png 与 logo/bluearchive-style-logo.svg")
            hint.setStyleSheet("color: palette(mid);")
            hint.setAlignment(Qt.AlignmentFlag.AlignCenter)
            col_lay.addWidget(hint)
        elif mc_path is None:
            hint = QLabel("未找到 logo/minecraft-style-logo.png")
            hint.setStyleSheet("color: palette(mid);")
            hint.setAlignment(Qt.AlignmentFlag.AlignCenter)
            col_lay.addWidget(hint)
        elif ba_path is None:
            hint = QLabel("未找到 logo/bluearchive-style-logo.svg")
            hint.setStyleSheet("color: palette(mid);")
            hint.setAlignment(Qt.AlignmentFlag.AlignCenter)
            col_lay.addWidget(hint)

        root.addWidget(logo_col, 1, Qt.AlignmentFlag.AlignHCenter)
        root.addStretch(1)

    @staticmethod
    def _resolve_logo(filename: str) -> Path | None:
        for parent in Path(__file__).resolve().parents:
            candidate = parent / "logo" / filename
            if candidate.is_file():
                return candidate
        return None


class LibraryPage(QWidget):
    def __init__(self) -> None:
        super().__init__()
        root = QVBoxLayout(self)
        root.addWidget(_empty_page("投影库（Schematics Library）", "占位：投影文件管理将在后续实现。"))


class PropertiesPage(QWidget):
    def __init__(self) -> None:
        super().__init__()
        root = QVBoxLayout(self)
        root.addWidget(_empty_page("属性（Properties）", "占位：SNBT 元数据读写将在后续实现。"))


class ReplacePage(QWidget):
    def __init__(self) -> None:
        super().__init__()
        root = QVBoxLayout(self)
        root.addWidget(_empty_page("替换（Replace）", "占位：方块替换功能将在后续实现。"))
