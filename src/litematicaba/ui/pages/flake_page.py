"""分层页：Y 轴俯视切片的最小真实闭环。"""

from __future__ import annotations

import math
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from PySide6.QtCore import QPoint, QPointF, QRectF, Qt, QThread, QTimer, Signal
from PySide6.QtGui import QColor, QMouseEvent, QPainter, QPixmap, QShowEvent, QWheelEvent
from PySide6.QtWidgets import (
    QComboBox,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QPushButton,
    QSizePolicy,
    QSlider,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.game_resource_language import load_runtime_language_map
from litematicaba.core.native_backend_bridge import get_cache_layer, get_cache_layer_meta
from litematicaba.core.settings import (
    RENDER_BUILD_MODE_FAST_EXPERIMENTAL,
    RENDER_BUILD_MODE_FULL,
    RENDER_BUILD_MODE_NORMAL,
)
from litematicaba.ui.material_list_dialog import MaterialListDialog
from litematicaba.ui.material_list_icon_prewarmer import request_icon_prewarm_from_material_or_flake_ui
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.minecraft_top_sprite import minecraft_top_view_pixmap_for_block
from litematicaba.ui.pages.properties_page import PropertiesPage
from litematicaba.ui.table.material_list_table import material_list_block_icon_pixmap_32_for_block


@dataclass(slots=True)
class _PaletteVisual:
    block_id: str
    state_id: str
    display_name: str
    properties: dict[str, str]
    icon: QPixmap


@dataclass(slots=True)
class _LayerBlockVisual:
    x: int
    y: int
    z: int
    palette_id: int
    palette: _PaletteVisual


def _state_id(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return block_id
    pairs = ",".join(f"{key}={value}" for key, value in sorted(properties.items()))
    return f"{block_id}[{pairs}]"


def _display_name(block_id: str, names: dict[str, str]) -> str:
    local = block_id.split(":", 1)[1] if ":" in block_id else block_id
    return names.get(f"block.minecraft.{local}") or names.get(f"block.{local}") or block_id


_STATE_KEY_LABELS = {
    "axis": "轴向",
    "facing": "朝向",
    "half": "半部",
    "shape": "形状",
    "type": "类型",
    "waterlogged": "含水",
    "open": "打开",
    "powered": "充能",
    "lit": "点亮",
    "snowy": "覆雪",
    "attached": "已连接",
    "distance": "距离",
    "level": "等级",
    "age": "生长阶段",
}


_STATE_VALUE_LABELS = {
    "north": "朝北",
    "south": "朝南",
    "east": "朝东",
    "west": "朝西",
    "up": "朝上",
    "down": "朝下",
    "x": "X 轴",
    "y": "Y 轴",
    "z": "Z 轴",
    "top": "上半",
    "bottom": "下半",
    "upper": "上半",
    "lower": "下半",
    "left": "左",
    "right": "右",
    "single": "单块",
    "double": "双层",
    "inner_left": "内角左",
    "inner_right": "内角右",
    "outer_left": "外角左",
    "outer_right": "外角右",
    "straight": "直线",
    "true": "是",
    "false": "否",
}


_STATE_SHORT_VALUE_LABELS = {
    "north": "北",
    "south": "南",
    "east": "东",
    "west": "西",
    "up": "上",
    "down": "下",
    "x": "X",
    "y": "Y",
    "z": "Z",
    "top": "上",
    "bottom": "下",
    "upper": "上",
    "lower": "下",
    "left": "左",
    "right": "右",
    "true": "是",
    "false": "否",
}


def _friendly_state_value(value: str) -> str:
    key = value.strip().lower()
    if key in _STATE_VALUE_LABELS:
        return _STATE_VALUE_LABELS[key]
    return value.replace("_", " ")


def _friendly_state_short_value(value: str) -> str:
    key = value.strip().lower()
    if key in _STATE_SHORT_VALUE_LABELS:
        return _STATE_SHORT_VALUE_LABELS[key]
    return _friendly_state_value(value)[:2]


def _friendly_state_text(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return f"方块 ID：{block_id}"
    parts = []
    for key, value in sorted(properties.items()):
        label = _STATE_KEY_LABELS.get(key, key.replace("_", " "))
        parts.append(f"{label}：{_friendly_state_value(value)}")
    return f"方块 ID：{block_id}\n状态：" + "，".join(parts)


_STATE_KEY_LABELS = {
    "axis": "\u8f74\u5411",
    "facing": "\u671d\u5411",
    "half": "\u534a\u90e8",
    "shape": "\u5f62\u72b6",
    "type": "\u7c7b\u578b",
    "waterlogged": "\u542b\u6c34",
    "open": "\u6253\u5f00",
    "powered": "\u5145\u80fd",
    "lit": "\u70b9\u4eae",
    "snowy": "\u8986\u96ea",
    "attached": "\u5df2\u8fde\u63a5",
    "distance": "\u8ddd\u79bb",
    "level": "\u7b49\u7ea7",
    "age": "\u751f\u957f\u9636\u6bb5",
}

_STATE_VALUE_LABELS = {
    "north": "\u671d\u5317",
    "south": "\u671d\u5357",
    "east": "\u671d\u4e1c",
    "west": "\u671d\u897f",
    "up": "\u671d\u4e0a",
    "down": "\u671d\u4e0b",
    "x": "X \u8f74",
    "y": "Y \u8f74",
    "z": "Z \u8f74",
    "top": "\u4e0a\u534a",
    "bottom": "\u4e0b\u534a",
    "upper": "\u4e0a\u534a",
    "lower": "\u4e0b\u534a",
    "left": "\u5de6",
    "right": "\u53f3",
    "single": "\u5355\u5757",
    "double": "\u53cc\u5c42",
    "inner_left": "\u5185\u89d2\u5de6",
    "inner_right": "\u5185\u89d2\u53f3",
    "outer_left": "\u5916\u89d2\u5de6",
    "outer_right": "\u5916\u89d2\u53f3",
    "straight": "\u76f4\u7ebf",
    "true": "\u662f",
    "false": "\u5426",
}

_STATE_SHORT_VALUE_LABELS = {
    "north": "\u5317",
    "south": "\u5357",
    "east": "\u4e1c",
    "west": "\u897f",
    "up": "\u4e0a",
    "down": "\u4e0b",
    "x": "X",
    "y": "Y",
    "z": "Z",
    "top": "\u4e0a",
    "bottom": "\u4e0b",
    "upper": "\u4e0a",
    "lower": "\u4e0b",
    "left": "\u5de6",
    "right": "\u53f3",
    "true": "\u662f",
    "false": "\u5426",
}


def _friendly_state_value(value: str) -> str:
    key = value.strip().lower()
    if key in _STATE_VALUE_LABELS:
        return _STATE_VALUE_LABELS[key]
    return value.replace("_", " ")


def _friendly_state_short_value(value: str) -> str:
    key = value.strip().lower()
    if key in _STATE_SHORT_VALUE_LABELS:
        return _STATE_SHORT_VALUE_LABELS[key]
    return _friendly_state_value(value)[:2]


def _friendly_state_text(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return f"\u65b9\u5757 ID\uff1a{block_id}"
    parts = []
    for key, value in sorted(properties.items()):
        label = _STATE_KEY_LABELS.get(key, key.replace("_", " "))
        parts.append(f"{label}\uff1a{_friendly_state_value(value)}")
    return f"\u65b9\u5757 ID\uff1a{block_id}\n\u72b6\u6001\uff1a" + "\uff0c".join(parts)


_DIRECTION_LABELS = {
    "north": "\u5317",
    "south": "\u5357",
    "east": "\u4e1c",
    "west": "\u897f",
    "up": "\u4e0a",
    "down": "\u4e0b",
}

_BOOLEAN_STATE_LABELS = {
    "open": ("\u5f00\u542f", "\u5173\u95ed"),
    "waterlogged": ("\u542b\u6c34", "\u4e0d\u542b\u6c34"),
    "powered": ("\u901a\u7535", "\u672a\u901a\u7535"),
    "lit": ("\u70b9\u4eae", "\u672a\u70b9\u4eae"),
    "attached": ("\u5df2\u8fde\u63a5", "\u672a\u8fde\u63a5"),
    "snowy": ("\u8986\u96ea", "\u65e0\u8986\u96ea"),
}

_STATE_KEY_LABELS.update(
    {
        "power": "\u5f3a\u5ea6",
        "east": "\u4e1c",
        "west": "\u897f",
        "north": "\u5317",
        "south": "\u5357",
    }
)
_STATE_VALUE_LABELS.update(
    {
        "none": "\u65e0",
        "side": "\u4fa7\u8fde",
        "low": "\u77ee",
        "tall": "\u9ad8",
    }
)


def _friendly_state_lines(properties: dict[str, str]) -> list[str]:
    lines: list[str] = []
    connection_dirs: list[str] = []
    wall_connections: list[str] = []
    for key in ("north", "east", "south", "west", "up", "down"):
        value = properties.get(key)
        if value is None:
            continue
        label = _DIRECTION_LABELS.get(key, key)
        if value == "true":
            connection_dirs.append(label)
        elif value == "side":
            connection_dirs.append(label)
        elif value == "up":
            connection_dirs.append(f"{label}\u4e0a")
        elif value in ("low", "tall"):
            height_label = "\u9ad8" if value == "tall" else "\u77ee"
            wall_connections.append(f"{label}{height_label}")
    if connection_dirs:
        lines.append("\u8fde\u63a5\uff1a" + "\u3001".join(connection_dirs))
    if wall_connections:
        lines.append("\u8fde\u63a5\uff1a" + "\u3001".join(wall_connections))

    for key in ("facing", "rotation"):
        value = properties.get(key)
        if value:
            lines.append(f"\u671d\u5411\uff1a{_friendly_state_short_value(value)}")
    for key in ("half", "axis", "type", "shape"):
        value = properties.get(key)
        if value:
            label = _STATE_KEY_LABELS.get(key, key)
            lines.append(f"{label}\uff1a{_friendly_state_value(value)}")
    for key, (true_label, false_label) in _BOOLEAN_STATE_LABELS.items():
        value = properties.get(key)
        if value == "true":
            lines.append(true_label)
        elif value == "false":
            lines.append(false_label)
    for key, value in sorted(properties.items()):
        if key in {"north", "east", "south", "west", "up", "down", "facing", "rotation", "half", "axis", "type", "shape"}:
            continue
        if key in _BOOLEAN_STATE_LABELS:
            continue
        label = _STATE_KEY_LABELS.get(key, key.replace("_", " "))
        lines.append(f"{label}\uff1a{_friendly_state_value(value)}")
    return lines


def _friendly_state_text(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return f"\u65b9\u5757 ID\uff1a{block_id}"
    lines = _friendly_state_lines(properties)
    if not lines:
        return f"\u65b9\u5757 ID\uff1a{block_id}"
    return f"\u65b9\u5757 ID\uff1a{block_id}\n\u72b6\u6001\uff1a" + "\uff1b".join(lines)


class _VisualMetaWorker(QThread):
    finished_ok = Signal(int, str, object)
    failed = Signal(int, str)

    def __init__(self, seq: int, cache_file: Path) -> None:
        super().__init__()
        self._seq = seq
        self._cache_file = cache_file

    def run(self) -> None:  # type: ignore[override]
        try:
            payload = get_cache_layer_meta(self._cache_file)
        except Exception as exc:
            self.failed.emit(self._seq, str(exc))
            return
        self.finished_ok.emit(self._seq, str(self._cache_file), payload)


class _VisualLayerWorker(QThread):
    finished_ok = Signal(int, int, str, int, object)
    failed = Signal(int, int, int, str)

    def __init__(self, seq: int, session_id: int, cache_file: Path, y: int) -> None:
        super().__init__()
        self._seq = seq
        self._session_id = session_id
        self._cache_file = cache_file
        self._y = int(y)

    def run(self) -> None:  # type: ignore[override]
        try:
            payload = get_cache_layer(self._cache_file, self._y)
        except Exception as exc:
            self.failed.emit(self._seq, self._session_id, self._y, str(exc))
            return
        self.finished_ok.emit(self._seq, self._session_id, str(self._cache_file), self._y, payload)


class _LayerHoverPopup(QWidget):
    def __init__(self, parent: QWidget) -> None:
        super().__init__(parent.window())
        self.setWindowFlags(
            Qt.WindowType.ToolTip
            | Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowDoesNotAcceptFocus
        )
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating, True)
        self.setObjectName("LayerBlockHoverPopup")

        self._icon = QLabel()
        self._icon.setFixedSize(32, 32)
        self._icon.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._name = QLabel()
        self._name.setWordWrap(False)
        self._coord = QLabel()
        self._coord.setWordWrap(False)
        self._id = QLabel()
        self._id.setWordWrap(True)

        top = QHBoxLayout()
        top.setContentsMargins(0, 0, 0, 0)
        top.setSpacing(8)
        top.addWidget(self._icon)
        top.addWidget(self._name, 1)

        root = QVBoxLayout(self)
        root.setContentsMargins(8, 8, 8, 8)
        root.setSpacing(4)
        root.addLayout(top)
        root.addWidget(self._coord)
        root.addWidget(self._id)
        self.setStyleSheet(
            "QWidget#LayerBlockHoverPopup { background: #111; border: 2px solid #888; color: #fff; }"
            "QLabel { color: #fff; background: transparent; }"
        )

    def show_block(self, block: _LayerBlockVisual, global_pos: QPoint) -> None:
        self._icon.setPixmap(block.palette.icon)
        self._name.setText(block.palette.display_name)
        self._coord.setText(f"x={block.x}  y={block.y}  z={block.z}")
        self._id.setText(_friendly_state_text(block.palette.block_id, block.palette.properties))
        self.adjustSize()
        self.move(global_pos + QPoint(14, 14))
        self.show()


class LayerSliceCanvas(QWidget):
    """轻量 2D 俯视切片画布：滚轮缩放、拖动平移、悬停查块。"""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setMinimumHeight(320)
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self._size_x = 0
        self._size_z = 0
        self._scale = 16.0
        self._min_scale = 1.0
        self._pan = QPointF(16.0, 16.0)
        self._drag_start: QPoint | None = None
        self._drag_pan = QPointF()
        self._blocks: dict[tuple[int, int], _LayerBlockVisual] = {}
        self._hover: _LayerBlockVisual | None = None
        self._popup = _LayerHoverPopup(self)
        self._status = "请先在“属性”页加载 .litematic。"
        self._show_state_badges = True

        self._layer_pixmap: QPixmap | None = None
        self._layer_pixmap_tile_px = 0

    def set_status(self, text: str) -> None:
        self._status = text
        if text:
            self._blocks.clear()
            self._invalidate_layer_pixmap()
        self.update()

    def set_meta(self, size_x: int, size_z: int) -> None:
        self._size_x = max(0, int(size_x))
        self._size_z = max(0, int(size_z))
        self._blocks.clear()
        self._invalidate_layer_pixmap()
        self.fit_to_view()

    def set_layer(self, blocks: list[_LayerBlockVisual]) -> None:
        self._status = ""
        self._blocks = {(block.x, block.z): block for block in blocks}
        self._invalidate_layer_pixmap()
        self._hover = None
        self._popup.hide()
        self.update()

    def set_state_badges_enabled(self, enabled: bool) -> None:
        self._show_state_badges = bool(enabled)
        self.update()

    def fit_to_view(self) -> None:
        if self._size_x <= 0 or self._size_z <= 0 or self.width() <= 0 or self.height() <= 0:
            return
        self._scale = self._compute_fit_scale()
        self._min_scale = self._scale
        self._pan = QPointF(
            (self.width() - self._size_x * self._scale) / 2.0,
            (self.height() - self._size_z * self._scale) / 2.0,
        )
        self._clamp_pan()
        self.update()

    def _compute_fit_scale(self) -> float:
        if self._size_x <= 0 or self._size_z <= 0 or self.width() <= 0 or self.height() <= 0:
            return 1.0
        margin = 24.0
        sx = max(0.001, (self.width() - margin * 2.0) / float(self._size_x))
        sz = max(0.001, (self.height() - margin * 2.0) / float(self._size_z))
        return max(0.001, min(96.0, sx, sz))

    def _refresh_min_scale(self) -> None:
        self._min_scale = self._compute_fit_scale()
        if self._scale < self._min_scale:
            self._scale = self._min_scale

    @staticmethod
    def _clamp_axis(pan: float, content: float, view: float) -> float:
        if content <= 0 or view <= 0:
            return pan
        if content <= view:
            low = 0.0
            high = view - content
        else:
            min_visible = max(48.0, min(view * 0.25, content * 0.5))
            low = min_visible - content
            high = view - min_visible
        if low > high:
            return (view - content) / 2.0
        return max(low, min(high, pan))

    def _clamp_pan(self) -> None:
        content_w = self._size_x * self._scale
        content_h = self._size_z * self._scale
        self._pan = QPointF(
            self._clamp_axis(self._pan.x(), content_w, float(self.width())),
            self._clamp_axis(self._pan.y(), content_h, float(self.height())),
        )

    def resizeEvent(self, event) -> None:  # type: ignore[override]
        super().resizeEvent(event)
        if not self._blocks and self._size_x > 0:
            self.fit_to_view()
        elif self._size_x > 0 and self._size_z > 0:
            self._refresh_min_scale()
            self._clamp_pan()
            self.update()

    def wheelEvent(self, event: QWheelEvent) -> None:  # type: ignore[override]
        if self._size_x <= 0 or self._size_z <= 0:
            return
        angle = event.angleDelta().y()
        if angle == 0:
            return
        cursor = event.position()
        before_x = (cursor.x() - self._pan.x()) / self._scale
        before_z = (cursor.y() - self._pan.y()) / self._scale
        factor = 1.18 if angle > 0 else 1.0 / 1.18
        self._scale = max(self._min_scale, min(96.0, self._scale * factor))
        self._pan = QPointF(cursor.x() - before_x * self._scale, cursor.y() - before_z * self._scale)
        self._clamp_pan()
        self._sync_hover(event.position().toPoint(), event.globalPosition().toPoint())
        self.update()
        event.accept()

    def mousePressEvent(self, event: QMouseEvent) -> None:  # type: ignore[override]
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_start = event.position().toPoint()
            self._drag_pan = QPointF(self._pan)
            self.setCursor(Qt.CursorShape.ClosedHandCursor)
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event: QMouseEvent) -> None:  # type: ignore[override]
        pos = event.position().toPoint()
        if self._drag_start is not None:
            delta = pos - self._drag_start
            self._pan = QPointF(self._drag_pan.x() + delta.x(), self._drag_pan.y() + delta.y())
            self._clamp_pan()
            self._popup.hide()
            self.update()
            return
        self._sync_hover(pos, event.globalPosition().toPoint())

    def mouseReleaseEvent(self, event: QMouseEvent) -> None:  # type: ignore[override]
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_start = None
            self.setCursor(Qt.CursorShape.ArrowCursor)
        super().mouseReleaseEvent(event)

    def leaveEvent(self, event) -> None:  # type: ignore[override]
        self._hover = None
        self._popup.hide()
        self.update()
        super().leaveEvent(event)

    def _sync_hover(self, pos: QPoint, global_pos: QPoint) -> None:
        if self._scale <= 0:
            return
        x = math.floor((pos.x() - self._pan.x()) / self._scale)
        z = math.floor((pos.y() - self._pan.y()) / self._scale)
        block = self._blocks.get((x, z))
        if block is self._hover:
            if block is not None and self._popup.isVisible():
                self._popup.move(global_pos + QPoint(14, 14))
            return
        self._hover = block
        if block is None:
            self._popup.hide()
        else:
            self._popup.show_block(block, global_pos)
        self.update()

    def _invalidate_layer_pixmap(self) -> None:
        self._layer_pixmap = None
        self._layer_pixmap_tile_px = 0

    def _layer_cache_tile_px(self) -> int:
        largest_axis = max(self._size_x, self._size_z, 1)
        return max(1, min(8, 8192 // largest_axis))

    def _ensure_layer_pixmap(self) -> QPixmap | None:
        if self._size_x <= 0 or self._size_z <= 0 or not self._blocks:
            return None
        tile_px = self._layer_cache_tile_px()
        if self._layer_pixmap is not None and self._layer_pixmap_tile_px == tile_px:
            return self._layer_pixmap

        pixmap = QPixmap(self._size_x * tile_px, self._size_z * tile_px)
        pixmap.fill(Qt.GlobalColor.transparent)
        painter = QPainter(pixmap)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
        source = QRectF(0, 0, 32, 32)
        for block in self._blocks.values():
            painter.drawPixmap(
                QRectF(block.x * tile_px, block.z * tile_px, tile_px, tile_px),
                block.palette.icon,
                source,
            )
        painter.end()
        self._layer_pixmap = pixmap
        self._layer_pixmap_tile_px = tile_px
        return pixmap

    def paintEvent(self, event) -> None:  # type: ignore[override]
        super().paintEvent(event)
        painter = QPainter(self)
        painter.fillRect(self.rect(), self.palette().base())
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, self._scale >= 10.0)
        painter.setPen(QColor(110, 110, 110))
        bounds = QRectF(self._pan.x(), self._pan.y(), self._size_x * self._scale, self._size_z * self._scale)
        if self._size_x > 0 and self._size_z > 0:
            painter.drawRect(bounds)

        if self._status:
            painter.setPen(self.palette().mid().color())
            painter.drawText(self.rect(), Qt.AlignmentFlag.AlignCenter, self._status)
            return

        first_x = max(0, math.floor((-self._pan.x()) / self._scale) - 1)
        last_x = min(self._size_x - 1, math.ceil((self.width() - self._pan.x()) / self._scale) + 1)
        first_z = max(0, math.floor((-self._pan.y()) / self._scale) - 1)
        last_z = min(self._size_z - 1, math.ceil((self.height() - self._pan.y()) / self._scale) + 1)
        source = QRectF(0, 0, 32, 32)
        if self._scale < 10.0:
            layer_pixmap = self._ensure_layer_pixmap()
            if layer_pixmap is not None:
                tile_px = max(1, self._layer_pixmap_tile_px)
                width = max(0, last_x - first_x + 1)
                height = max(0, last_z - first_z + 1)
                painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
                painter.drawPixmap(
                    QRectF(
                        self._pan.x() + first_x * self._scale,
                        self._pan.y() + first_z * self._scale,
                        width * self._scale,
                        height * self._scale,
                    ),
                    layer_pixmap,
                    QRectF(first_x * tile_px, first_z * tile_px, width * tile_px, height * tile_px),
                )
        else:
            for z in range(first_z, last_z + 1):
                for x in range(first_x, last_x + 1):
                    block = self._blocks.get((x, z))
                    if block is None:
                        continue
                    target = QRectF(
                        self._pan.x() + x * self._scale,
                        self._pan.y() + z * self._scale,
                        max(1.0, self._scale),
                        max(1.0, self._scale),
                    )
                    painter.drawPixmap(target, block.palette.icon, source)
                    if self._show_state_badges and block.palette.properties and self._scale >= 15.0:
                        painter.fillRect(target.adjusted(1, 1, -1, -self._scale * 0.62), QColor(0, 0, 0, 92))
                        painter.setPen(QColor(255, 255, 255))
                        badge = self._state_badge(block.palette.properties)
                        painter.drawText(target.adjusted(2, 0, -2, 0), Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignLeft, badge)
        if self._scale >= 18.0:
            painter.setPen(QColor(0, 0, 0, 55))
            for x in range(first_x, last_x + 2):
                xx = self._pan.x() + x * self._scale
                painter.drawLine(QPointF(xx, self._pan.y()), QPointF(xx, self._pan.y() + self._size_z * self._scale))
            for z in range(first_z, last_z + 2):
                zz = self._pan.y() + z * self._scale
                painter.drawLine(QPointF(self._pan.x(), zz), QPointF(self._pan.x() + self._size_x * self._scale, zz))

        if self._hover is not None:
            painter.setPen(QColor(255, 230, 120))
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawRect(
                QRectF(
                    self._pan.x() + self._hover.x * self._scale,
                    self._pan.y() + self._hover.z * self._scale,
                    self._scale,
                    self._scale,
                )
            )

    @staticmethod
    def _state_badge(properties: dict[str, str]) -> str:
        connected = [
            _DIRECTION_LABELS[key]
            for key in ("north", "east", "south", "west")
            if properties.get(key) in ("true", "low", "tall", "side", "up")
        ]
        if connected:
            return "".join(connected[:2])
        for key in ("facing", "axis", "half", "shape", "type", "waterlogged"):
            value = properties.get(key)
            if value:
                if key in _BOOLEAN_STATE_LABELS:
                    labels = _BOOLEAN_STATE_LABELS[key]
                    return labels[0][:1] if value == "true" else labels[1][:1]
                return _friendly_state_short_value(value)
        return "*"


class FlakePage(QWidget):
    """现有分层页上的 Y 轴俯视切片。"""

    def __init__(
        self,
        properties_page: PropertiesPage,
        parent: QWidget | None = None,
        *,
        app_settings=None,
        material_scan_prewarmer: MaterialListScanPrewarmer | None = None,
    ) -> None:
        super().__init__(parent)
        self._props = properties_page
        self._material_scan_prewarmer = material_scan_prewarmer
        self._render_mode = getattr(app_settings, "render_build_mode", RENDER_BUILD_MODE_NORMAL)
        self._meta_seq = 0
        self._layer_seq = 0
        self._layer_session_id = 0
        self._meta_worker: _VisualMetaWorker | None = None
        self._layer_worker: _VisualLayerWorker | None = None
        self._layer_workers: set[_VisualLayerWorker] = set()
        self._current_file: Path | None = None
        self._cache_file: Path | None = None
        self._visual_meta: dict[str, Any] | None = None
        self._palette: list[_PaletteVisual] = []
        self._layer_cache: dict[int, list[_LayerBlockVisual]] = {}
        self._high_priority_layers: deque[int] = deque()
        self._low_priority_layers: deque[int] = deque()
        self._queued_layers: set[int] = set()
        self._wanted_layer = 0
        self._lang: dict[str, str] | None = None

        self._layer_debounce = QTimer(self)
        self._layer_debounce.setSingleShot(True)
        self._layer_debounce.setInterval(120)
        self._layer_debounce.timeout.connect(self._load_current_layer)

        self._lbl_path = QLabel("请在“属性”页加载 .litematic。")
        self._lbl_path.setWordWrap(True)
        self._status_label = QLabel("分层数据尚未加载。")
        self._status_label.setWordWrap(True)
        self._status_label.setStyleSheet("color: palette(mid);")

        reg_row = QHBoxLayout()
        reg_row.addWidget(QLabel("子区域："))
        self._region_combo = QComboBox()
        self._region_combo.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self._region_combo.currentIndexChanged.connect(self._on_region_changed)
        reg_row.addWidget(self._region_combo, 1)

        layer_box = QGroupBox("层级控制（Y 轴俯视切片）")
        layer_grid = QGridLayout(layer_box)
        self._layer_slider = QSlider(Qt.Orientation.Horizontal)
        self._layer_slider.setRange(0, 0)
        self._layer_slider.setValue(0)
        self._layer_slider.setToolTip("切换当前 Y 层；数据来自已构建的 3D cache。")
        self._layer_slider.valueChanged.connect(self._on_layer_changed)
        self._layer_value_lbl = QLabel("层 Y = 0")
        self._fit_button = QPushButton("重置视角")
        self._fit_button.clicked.connect(self._fit_canvas)
        layer_step_row = QHBoxLayout()
        layer_step_row.setContentsMargins(0, 0, 0, 0)
        layer_step_row.setSpacing(6)
        self._btn_layer_minus_10 = QPushButton("--")
        self._btn_layer_minus_1 = QPushButton("-")
        self._btn_layer_plus_1 = QPushButton("+")
        self._btn_layer_plus_10 = QPushButton("++")
        self._btn_layer_minus_10.clicked.connect(lambda: self._step_layer(-10))
        self._btn_layer_minus_1.clicked.connect(lambda: self._step_layer(-1))
        self._btn_layer_plus_1.clicked.connect(lambda: self._step_layer(1))
        self._btn_layer_plus_10.clicked.connect(lambda: self._step_layer(10))
        for btn in (
            self._btn_layer_minus_10,
            self._btn_layer_minus_1,
            self._btn_layer_plus_1,
            self._btn_layer_plus_10,
        ):
            btn.setFixedWidth(42)
            layer_step_row.addWidget(btn)
        layer_grid.addWidget(QLabel("层索引："), 0, 0)
        layer_grid.addWidget(self._layer_slider, 0, 1)
        layer_grid.addWidget(self._layer_value_lbl, 1, 1)
        layer_grid.addWidget(self._fit_button, 0, 2)
        layer_grid.addLayout(layer_step_row, 1, 2)

        self._canvas = LayerSliceCanvas(self)
        self._canvas.set_state_badges_enabled(self._render_mode != RENDER_BUILD_MODE_FULL)
        self._btn_material = QPushButton("材料列表（当前区域）")
        self._btn_material.clicked.connect(self._on_material_list)

        root = QVBoxLayout(self)
        root.addWidget(self._lbl_path)
        root.addLayout(reg_row)
        root.addWidget(layer_box)
        root.addWidget(self._canvas, 1)
        root.addWidget(self._status_label)
        root.addWidget(self._btn_material)

        self._props.active_file_changed.connect(self._on_active_file_changed)

    def _on_layer_changed(self, v: int) -> None:
        self._layer_value_lbl.setText(f"层 Y = {v}")
        self._status_label.setText("正在准备当前层...")
        self._wanted_layer = int(v)
        self._layer_debounce.start()

    def _step_layer(self, delta: int) -> None:
        low = self._layer_slider.minimum()
        high = self._layer_slider.maximum()
        value = max(low, min(high, self._layer_slider.value() + int(delta)))
        self._layer_slider.setValue(value)

    def _sync_region_combo(self) -> None:
        self._region_combo.blockSignals(True)
        self._region_combo.clear()
        path = self._props.active_file_path()
        if path is None:
            self._region_combo.blockSignals(False)
            return
        try:
            ents = self._props.material_list_region_entries_for_active_file()
            if ents is not None:
                for display_name, source_key in ents:
                    self._region_combo.addItem(display_name, source_key)
            else:
                self._region_combo.addItem("整个投影", "")
        except Exception:
            self._region_combo.addItem("整个投影", "")
        self._region_combo.blockSignals(False)

    def _selected_region_name(self) -> str | None:
        if self._region_combo.count() <= 0:
            return None
        data = self._region_combo.currentData()
        text = str(data) if data is not None else ""
        return text or None

    def _on_active_file_changed(self, _path: str) -> None:
        self._sync_path_label()
        self._sync_region_combo()
        self._cache_file = None
        self._reset_visual_state()

    def _on_region_changed(self, _index: int) -> None:
        # 本轮只做整投影 Y 轴分层；区域下拉保留给材料列表和后续区域裁剪。
        return

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        request_icon_prewarm_from_material_or_flake_ui()
        self._sync_path_label()
        if self._region_combo.count() == 0:
            self._sync_region_combo()
        self._ensure_visual_meta_loaded()

    def set_render_cache(self, file_path: str | Path, cache_file: str | Path) -> None:
        active = self._active_file()
        source = Path(file_path)
        if active is None:
            return
        try:
            if active.resolve() != source.resolve():
                return
        except OSError:
            if str(active) != str(source):
                return
        cache = Path(cache_file)
        if not cache.is_file():
            self._cache_file = None
            self._reset_visual_state()
            return
        self._cache_file = cache.resolve()
        self._reset_visual_state()
        self._ensure_visual_meta_loaded()

    def set_render_mode(self, mode: str) -> None:
        mode = mode if mode in (RENDER_BUILD_MODE_NORMAL, RENDER_BUILD_MODE_FAST_EXPERIMENTAL, RENDER_BUILD_MODE_FULL) else RENDER_BUILD_MODE_NORMAL
        if mode == self._render_mode:
            return
        self._render_mode = mode
        self._canvas.set_state_badges_enabled(mode != RENDER_BUILD_MODE_FULL)
        self._rebuild_palette_icons()
        self._canvas._invalidate_layer_pixmap()
        self._canvas.update()

    def _sync_path_label(self) -> None:
        path = self._props.display_file_path()
        self._lbl_path.setText(str(path) if path is not None else "请在“属性”页加载 .litematic。")

    def _reset_visual_state(self) -> None:
        self._meta_seq += 1
        self._layer_seq += 1
        self._layer_session_id += 1
        self._layer_worker = None
        self._high_priority_layers.clear()
        self._low_priority_layers.clear()
        self._queued_layers.clear()
        self._visual_meta = None
        self._palette = []
        self._layer_cache.clear()
        self._current_file = self._active_file()
        self._canvas.set_status("正在等待分层数据...")
        self._status_label.setText("分层数据尚未加载。")
        self._layer_slider.blockSignals(True)
        self._layer_slider.setRange(0, 0)
        self._layer_slider.setValue(0)
        self._layer_slider.blockSignals(False)
        self._layer_value_lbl.setText("层 Y = 0")

    def _active_file(self) -> Path | None:
        path = self._props.active_file_path()
        return Path(path).resolve() if path is not None else None

    def _ready_cache_file(self) -> Path | None:
        if self._cache_file is None:
            return None
        if not self._cache_file.is_file():
            self._cache_file = None
            return None
        return self._cache_file

    def _ensure_visual_meta_loaded(self) -> None:
        path = self._active_file()
        if path is None:
            self._canvas.set_status("请先在“属性”页加载 .litematic。")
            self._status_label.setText("未选择投影文件。")
            return
        if self._current_file is None or self._current_file != path:
            self._reset_visual_state()
        cache_file = self._ready_cache_file()
        if cache_file is None:
            self._canvas.set_status("Layer view needs a finished 3D cache.\nBuild 3D cache on the Render page first.")
            self._status_label.setText("分层不可用：请先在“渲染”页构建 3D cache。")
            return
        if self._visual_meta is not None:
            return
        if self._meta_worker is not None and self._meta_worker.isRunning():
            return
        self._meta_seq += 1
        seq = self._meta_seq
        self._status_label.setText("正在从 3D cache 读取分层元数据...")
        self._canvas.set_status("Loading layer metadata from 3D cache...")
        worker = _VisualMetaWorker(seq, cache_file)
        self._meta_worker = worker
        worker.finished_ok.connect(self._on_visual_meta_ready)
        worker.failed.connect(self._on_visual_meta_failed)
        worker.finished.connect(worker.deleteLater)
        worker.start()

    def _on_visual_meta_ready(self, seq: int, file_path: str, payload: object) -> None:
        cache_file = self._ready_cache_file()
        if seq != self._meta_seq or cache_file is None or Path(file_path).resolve() != cache_file:
            return
        self._meta_worker = None
        if not isinstance(payload, dict):
            self._on_visual_meta_failed(seq, "cache-layer-meta 返回格式无效")
            return
        self._visual_meta = payload
        visual = payload.get("visual") if isinstance(payload.get("visual"), dict) else {}
        self._palette = self._build_palette(visual)
        size_x = int(visual.get("size_x", 0) or 0)
        size_y = int(visual.get("size_y", 0) or 0)
        size_z = int(visual.get("size_z", 0) or 0)
        self._canvas.set_meta(size_x, size_z)
        self._layer_slider.blockSignals(True)
        self._layer_slider.setRange(0, max(0, size_y - 1))
        self._layer_slider.setValue(0)
        self._layer_slider.blockSignals(False)
        self._layer_value_lbl.setText("层 Y = 0")
        self._status_label.setText(f"3D cache 分层已就绪：{size_x} x {size_y} x {size_z}，调色板 {len(self._palette)} 项。")
        self._load_current_layer()

    def _on_visual_meta_failed(self, seq: int, message: str) -> None:
        if seq != self._meta_seq:
            return
        self._meta_worker = None
        self._status_label.setText(f"分层元数据加载失败：{message}")
        self._canvas.set_status(f"分层元数据加载失败：\n{message}")

    def _build_palette(self, visual: dict[str, Any]) -> list[_PaletteVisual]:
        if self._lang is None:
            self._lang = load_runtime_language_map()
        palette_raw = visual.get("palette") if isinstance(visual, dict) else []
        property_pool = visual.get("property_pool") if isinstance(visual, dict) else []
        palette: list[_PaletteVisual] = []
        for row in list(palette_raw or []):
            if not isinstance(row, dict):
                continue
            block_id = str(row.get("block_id", "") or "")
            property_id = int(row.get("property_id", 0) or 0)
            props_raw = property_pool[property_id] if 0 <= property_id < len(property_pool) else {}
            properties = {str(k): str(v) for k, v in dict(props_raw or {}).items()}
            state_id = _state_id(block_id, properties)
            palette.append(
                _PaletteVisual(
                    block_id=block_id,
                    state_id=state_id,
                    display_name=_display_name(block_id, self._lang),
                    properties=properties,
                    icon=self._icon_for_palette_entry(block_id, state_id, properties),
                )
            )
        return palette

    def _icon_for_palette_entry(self, block_id: str, state_id: str, properties: dict[str, str]) -> QPixmap:
        if self._render_mode == RENDER_BUILD_MODE_FULL:
            sprite = minecraft_top_view_pixmap_for_block(block_id, properties)
            if sprite is not None and not sprite.isNull():
                return sprite
        return material_list_block_icon_pixmap_32_for_block(state_id)

    def _rebuild_palette_icons(self) -> None:
        for entry in self._palette:
            entry.icon = self._icon_for_palette_entry(entry.block_id, entry.state_id, entry.properties)

    def _layer_range(self) -> range:
        if not isinstance(self._visual_meta, dict):
            return range(0)
        visual = self._visual_meta.get("visual")
        if not isinstance(visual, dict):
            return range(0)
        return range(max(0, int(visual.get("size_y", 0) or 0)))

    def _enqueue_layer_front(self, y: int) -> None:
        if y in self._layer_cache or y not in self._layer_range():
            return
        if y in self._queued_layers:
            for queue in (self._high_priority_layers, self._low_priority_layers):
                try:
                    queue.remove(y)
                except ValueError:
                    pass
        self._high_priority_layers.appendleft(y)
        self._queued_layers.add(y)

    def _enqueue_layer_back(self, y: int) -> None:
        if y in self._layer_cache or y not in self._layer_range():
            return
        if y in self._queued_layers:
            for queue in (self._high_priority_layers, self._low_priority_layers):
                try:
                    queue.remove(y)
                except ValueError:
                    pass
        self._high_priority_layers.append(y)
        self._queued_layers.add(y)

    def _schedule_layers_around(self, center_y: int) -> None:
        if self._visual_meta is None:
            return
        self._enqueue_layer_front(center_y)
        for delta in (1, -1, 2, -2):
            self._enqueue_layer_back(center_y + delta)
        for y in self._layer_range():
            if y in self._layer_cache or y in self._queued_layers:
                continue
            self._low_priority_layers.append(y)
            self._queued_layers.add(y)
        self._pump_layer_scheduler()

    def _next_scheduled_layer(self) -> int | None:
        for queue in (self._high_priority_layers, self._low_priority_layers):
            while queue:
                y = queue.popleft()
                self._queued_layers.discard(y)
                if y not in self._layer_cache and y in self._layer_range():
                    return y
        return None

    def _on_layer_worker_finished(self, worker: _VisualLayerWorker) -> None:
        self._layer_workers.discard(worker)
        if self._layer_worker is worker:
            self._layer_worker = None
        self._pump_layer_scheduler()

    def _pump_layer_scheduler(self) -> None:
        if self._layer_worker is not None and self._layer_worker.isRunning():
            return
        cache_file = self._ready_cache_file()
        if cache_file is None or self._visual_meta is None:
            return
        y = self._next_scheduled_layer()
        if y is None:
            return
        self._layer_seq += 1
        worker = _VisualLayerWorker(self._layer_seq, self._layer_session_id, cache_file, y)
        self._layer_worker = worker
        self._layer_workers.add(worker)
        worker.finished_ok.connect(self._on_visual_layer_ready)
        worker.failed.connect(self._on_visual_layer_failed)
        worker.finished.connect(lambda w=worker: self._on_layer_worker_finished(w))
        worker.finished.connect(worker.deleteLater)
        worker.start()

    def _load_current_layer(self) -> None:
        path = self._active_file()
        cache_file = self._ready_cache_file()
        if path is None or cache_file is None or self._visual_meta is None:
            return
        y = int(self._layer_slider.value())
        self._wanted_layer = y
        cached = self._layer_cache.get(y)
        if cached is not None:
            self._canvas.set_layer(cached)
            self._schedule_layers_around(y)
            self._status_label.setText(f"层 Y={y}：{len(cached)} 个非空气方块。")
            return
        self._status_label.setText(f"Loading layer Y={y} from cache...")
        self._schedule_layers_around(y)
        return

    def _on_visual_layer_ready(self, seq: int, session_id: int, file_path: str, y: int, payload: object) -> None:
        cache_file = self._ready_cache_file()
        if session_id != self._layer_session_id or cache_file is None or Path(file_path).resolve() != cache_file:
            return
        if not isinstance(payload, dict):
            self._on_visual_layer_failed(seq, session_id, y, "cache-layer 返回格式无效")
            return
        blocks: list[_LayerBlockVisual] = []
        for row in list(payload.get("blocks", []) or []):
            if not isinstance(row, dict):
                continue
            palette_id = int(row.get("palette_id", -1))
            if palette_id < 0 or palette_id >= len(self._palette):
                continue
            blocks.append(
                _LayerBlockVisual(
                    x=int(row.get("x", 0) or 0),
                    y=int(y),
                    z=int(row.get("z", 0) or 0),
                    palette_id=palette_id,
                    palette=self._palette[palette_id],
                )
            )
        self._layer_cache[y] = blocks
        if y == self._wanted_layer:
            self._canvas.set_layer(blocks)
        if y != self._wanted_layer:
            return
        self._status_label.setText(f"层 Y={y}：{len(blocks)} 个非空气方块。滚轮缩放，左键拖动画布。")

    def _on_visual_layer_failed(self, seq: int, session_id: int, y: int, message: str) -> None:
        if session_id != self._layer_session_id:
            return
        if y != self._wanted_layer:
            return
        self._status_label.setText(f"层 Y={y} 加载失败：{message}")
        self._canvas.set_status(f"层 Y={y} 加载失败：\n{message}")

    def _fit_canvas(self) -> None:
        self._canvas.fit_to_view()

    def _on_material_list(self) -> None:
        if self._props.active_file_path() is None:
            QMessageBox.information(self, "材料列表", "请先在“属性”页打开一个投影文件。")
            return
        region = self._selected_region_name()
        MaterialListDialog.open_for_properties(
            self._props,
            self,
            initial_region_name=region,
            material_scan_prewarmer=self._material_scan_prewarmer,
        )
