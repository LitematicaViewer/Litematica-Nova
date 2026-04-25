"""分层物品图标预载：后台遍历 PNG，并在主线程写入 32×32 pixmap 缓存。"""

from __future__ import annotations

from collections import deque
from collections.abc import Callable

from PySide6.QtCore import QObject, QThread, QTimer, Qt, Signal
from PySide6.QtGui import QImage, QPixmap

from litematicaba.core.game_resource_item_icon import (
    ensure_initial_item_seeded,
    layering_item_icon_resolution_tag,
    layering_item_icon_search_root,
)
from litematicaba.core.layering_item_icon_pixmap_cache import (
    store_prewarmed_layering_item_pixmap,
)
from litematicaba.core.settings import (
    DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT,
    DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS,
)

_ATTACHED: LayeringItemIconPrewarmer | None = None
_READ_BATCH_BYTES = 32


def attach_layering_item_icon_prewarmer(p: "LayeringItemIconPrewarmer") -> None:
    global _ATTACHED
    _ATTACHED = p


def restart_layering_item_icon_prewarm() -> None:
    if _ATTACHED is not None:
        _ATTACHED.restart()


def _scale_item_icon_qimage(img: QImage) -> QImage:
    return img.scaled(
        32,
        32,
        Qt.AspectRatioMode.IgnoreAspectRatio,
        Qt.TransformationMode.FastTransformation,
    )


class _CollectPngBytesThread(QThread):
    batch_ready = Signal(str, object)  # tag, list[tuple[str, bytes]]
    done_tag = Signal(str)

    def __init__(self, read_batch: int, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._read_batch = max(1, read_batch)

    def run(self) -> None:  # type: ignore[override]
        ensure_initial_item_seeded()
        root = layering_item_icon_search_root()
        if not root.is_dir():
            self.done_tag.emit("")
            return
        tag = layering_item_icon_resolution_tag()
        batch: list[tuple[str, bytes]] = []
        seen: set[str] = set()
        try:
            for p in root.rglob("*.png"):
                if self.isInterruptionRequested():
                    return
                stem = p.stem
                if not stem or stem in seen:
                    continue
                try:
                    data = p.read_bytes()
                except OSError:
                    continue
                seen.add(stem)
                batch.append((stem, data))
                if len(batch) >= self._read_batch:
                    self.batch_ready.emit(tag, batch)
                    batch = []
            if batch:
                self.batch_ready.emit(tag, batch)
        finally:
            self.done_tag.emit(tag)


class LayeringItemIconPrewarmer(QObject):
    def __init__(
        self,
        parent: QObject | None = None,
        config_provider: Callable[[], tuple[int, int, str]] | None = None,
    ) -> None:
        super().__init__(parent)
        self._config_provider = config_provider
        self._worker: _CollectPngBytesThread | None = None
        self._decode_queue: deque[tuple[str, object]] = deque()
        self._active_list: list[tuple[str, object]] | None = None
        self._active_i: int = 0
        self._gap_ms: int = DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS
        self._slice: int = DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT
        self._timer = QTimer(self)
        self._timer.setInterval(self._gap_ms)
        self._timer.timeout.connect(self._pump_decode_slice)

    def set_config_provider(self, fn: Callable[[], tuple[int, int, str]] | None) -> None:
        self._config_provider = fn

    def _read_config_tuple(self) -> tuple[int, int]:
        gap = DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS
        slc = DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT
        if self._config_provider is not None:
            try:
                gap, slc, _mode = self._config_provider()
            except Exception:
                pass
        return gap, slc

    def is_active(self) -> bool:
        if self._timer.isActive():
            return True
        if self._worker is not None and self._worker.isRunning():
            return True
        if self._decode_queue or self._active_list is not None:
            return True
        return False

    def apply_live_config(self) -> None:
        gap, slc = self._read_config_tuple()
        self._gap_ms = max(0, min(2000, int(gap)))
        self._slice = max(1, min(500, int(slc)))
        self._timer.setInterval(self._gap_ms)

    def stop(self) -> None:
        if self._worker is not None:
            self._worker.requestInterruption()
            self._worker = None
        self._decode_queue.clear()
        self._active_list = None
        self._active_i = 0
        self._timer.stop()

    def start(self) -> None:
        self.stop()
        gap, slc = self._read_config_tuple()
        self._gap_ms = max(0, min(2000, int(gap)))
        self._slice = max(1, min(500, int(slc)))
        self._timer.setInterval(self._gap_ms)

        w = _CollectPngBytesThread(_READ_BATCH_BYTES, self)
        self._worker = w
        w.batch_ready.connect(self._on_worker_batch)
        w.done_tag.connect(self._on_worker_done_tag)
        w.start()

    def restart(self) -> None:
        self.stop()
        self.start()

    def _on_worker_batch(self, tag: str, items: object) -> None:
        if not isinstance(items, list):
            return
        self._decode_queue.append((tag, items))
        if not self._timer.isActive():
            self._timer.start()

    def _on_worker_done_tag(self, _tag: str) -> None:
        self._worker = None

    def _pump_decode_slice(self) -> None:
        cur = layering_item_icon_resolution_tag()
        decoded = 0
        while decoded < self._slice:
            if self._active_list is None:
                if not self._decode_queue:
                    self._timer.stop()
                    return
                tag, batch = self._decode_queue.popleft()
                if tag != cur:
                    continue
                if not isinstance(batch, list):
                    continue
                self._active_list = batch
                self._active_i = 0

            if layering_item_icon_resolution_tag() != cur:
                self._active_list = None
                self._active_i = 0
                cur = layering_item_icon_resolution_tag()
                continue

            if self._active_i >= len(self._active_list):
                self._active_list = None
                self._active_i = 0
                continue

            row = self._active_list[self._active_i]
            self._active_i += 1
            if not isinstance(row, tuple) or len(row) != 2:
                decoded += 1
                continue
            stem, payload = row[0], row[1]
            if not isinstance(stem, str) or not isinstance(payload, (bytes, bytearray)):
                decoded += 1
                continue

            img = QImage.fromData(bytes(payload))
            if not img.isNull():
                pm = QPixmap.fromImage(_scale_item_icon_qimage(img))
                if not pm.isNull():
                    store_prewarmed_layering_item_pixmap(stem, pm)
            decoded += 1