"""启动后或切换「应用到材料列表」图标包后，在后台遍历 PNG 并解码进 pixmap 缓存。

读盘在工作线程；解码可在主线程（默认）或工作线程（实验性），经定时器小批量在主线程转为 QPixmap 并入缓存。
"""

from __future__ import annotations

from collections import deque
from collections.abc import Callable

from PySide6.QtCore import QObject, QThread, QTimer, Qt, Signal
from PySide6.QtGui import QImage, QPixmap

from litematicaba.core.game_resource_block_icon import (
    ensure_initial_block_2d_seeded,
    material_list_icon_resolution_tag,
    material_list_icon_search_root,
)
from litematicaba.core.material_list_icon_pixmap_cache import store_prewarmed_scaled_pixmap
from litematicaba.core.settings import (
    BLOCK_ICON_PREWARM_DECODE_WORKER,
    DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT,
    DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS,
    DEFAULT_BLOCK_ICON_PREWARM_DECODE_THREAD,
)

_ATTACHED: MaterialListIconPrewarmer | None = None
_MATERIAL_UI_ICON_PREWARM_HOOK: Callable[[], None] | None = None

# 仅「主线程解码」路径：工作线程按批投递原始字节，减少跨线程信号次数。
_READ_BATCH_BYTES = 32


def register_material_ui_icon_prewarm_hook(fn: Callable[[], None] | None) -> None:
    """主窗口登记：在首次打开材料列表或进入分层页时触发（由设置决定是否真启动预载）。"""
    global _MATERIAL_UI_ICON_PREWARM_HOOK
    _MATERIAL_UI_ICON_PREWARM_HOOK = fn


def request_icon_prewarm_from_material_or_flake_ui() -> None:
    if _MATERIAL_UI_ICON_PREWARM_HOOK is not None:
        _MATERIAL_UI_ICON_PREWARM_HOOK()


def attach_material_list_icon_prewarmer(p: MaterialListIconPrewarmer) -> None:
    global _ATTACHED
    _ATTACHED = p


def restart_material_list_icon_prewarm() -> None:
    if _ATTACHED is not None:
        _ATTACHED.restart()


def _scale_icon_qimage(img: QImage) -> QImage:
    return img.scaled(
        32,
        32,
        Qt.AspectRatioMode.KeepAspectRatio,
        Qt.TransformationMode.FastTransformation,
    )


class _CollectPngBytesThread(QThread):
    batch_ready = Signal(str, object)  # tag, list[tuple[str, bytes]]
    done_tag = Signal(str)

    def __init__(self, read_batch: int, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._read_batch = max(1, read_batch)

    def run(self) -> None:  # type: ignore[override]
        ensure_initial_block_2d_seeded()
        root = material_list_icon_search_root()
        if not root.is_dir():
            self.done_tag.emit("")
            return
        tag = material_list_icon_resolution_tag()
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


class _DecodeIconsWorkerThread(QThread):
    """在工作线程完成 PNG 解码与缩放（实验性）；主线程仅 QPixmap 转换与写入缓存。"""

    decoded_batch = Signal(str, object)  # tag, list[tuple[str, QImage]]
    done_tag = Signal(str)

    def __init__(self, emit_batch_size: int, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._emit_batch_size = max(1, emit_batch_size)

    def run(self) -> None:  # type: ignore[override]
        ensure_initial_block_2d_seeded()
        root = material_list_icon_search_root()
        if not root.is_dir():
            self.done_tag.emit("")
            return
        tag = material_list_icon_resolution_tag()
        batch: list[tuple[str, QImage]] = []
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
                img = QImage.fromData(data)
                if img.isNull():
                    continue
                small = _scale_icon_qimage(img)
                if small.isNull():
                    continue
                batch.append((stem, small))
                if len(batch) >= self._emit_batch_size:
                    self.decoded_batch.emit(tag, batch)
                    batch = []
            if batch:
                self.decoded_batch.emit(tag, batch)
        finally:
            self.done_tag.emit(tag)


class MaterialListIconPrewarmer(QObject):
    def __init__(
        self,
        parent: QObject | None = None,
        config_provider: Callable[[], tuple[int, int, str]] | None = None,
    ) -> None:
        super().__init__(parent)
        self._config_provider = config_provider
        self._worker: QThread | None = None
        self._decode_queue: deque[tuple[str, object]] = deque()
        self._active_list: list[tuple[str, object]] | None = None
        self._active_i: int = 0
        self._worker_decode: bool = False
        self._gap_ms: int = DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS
        self._slice: int = DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT
        self._timer = QTimer(self)
        self._timer.setInterval(self._gap_ms)
        self._timer.timeout.connect(self._pump_decode_slice)

    def set_config_provider(
        self, fn: Callable[[], tuple[int, int, str]] | None
    ) -> None:
        self._config_provider = fn

    def _read_config_tuple(self) -> tuple[int, int, str]:
        gap = DEFAULT_BLOCK_ICON_PREWARM_BATCH_INTERVAL_MS
        slc = DEFAULT_BLOCK_ICON_PREWARM_BATCH_COUNT
        mode = DEFAULT_BLOCK_ICON_PREWARM_DECODE_THREAD
        if self._config_provider is not None:
            try:
                gap, slc, mode = self._config_provider()
            except Exception:
                pass
        return gap, slc, mode

    def is_active(self) -> bool:
        if self._timer.isActive():
            return True
        if self._worker is not None and self._worker.isRunning():
            return True
        if self._decode_queue or self._active_list is not None:
            return True
        return False

    def apply_live_config(self) -> None:
        """更新批次间隔与每批数量（不改变当前会话的解码线程模式；模式变更须 restart）。"""
        gap, slc, _mode = self._read_config_tuple()
        self._gap_ms = max(0, min(2000, int(gap)))
        self._slice = max(1, min(500, int(slc)))
        self._timer.setInterval(self._gap_ms)

    def stop(self) -> None:
        """停止后台读盘与主线程解码队列。"""
        if self._worker is not None:
            self._worker.requestInterruption()
            self._worker = None
        self._decode_queue.clear()
        self._active_list = None
        self._active_i = 0
        self._timer.stop()

    def start(self) -> None:
        self.stop()
        gap, slc, mode = self._read_config_tuple()
        self._gap_ms = max(0, min(2000, int(gap)))
        self._slice = max(1, min(500, int(slc)))
        self._worker_decode = mode == BLOCK_ICON_PREWARM_DECODE_WORKER
        self._timer.setInterval(self._gap_ms)
        if self._worker_decode:
            w = _DecodeIconsWorkerThread(slc, self)
            self._worker = w
            w.decoded_batch.connect(self._on_worker_decoded_batch)
            w.done_tag.connect(self._on_worker_done_tag)
            w.start()
        else:
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

    def _on_worker_decoded_batch(self, tag: str, items: object) -> None:
        if not isinstance(items, list):
            return
        self._decode_queue.append((tag, items))
        if not self._timer.isActive():
            self._timer.start()

    def _on_worker_done_tag(self, _tag: str) -> None:
        self._worker = None

    def _pump_decode_slice(self) -> None:
        cur = material_list_icon_resolution_tag()
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

            if material_list_icon_resolution_tag() != cur:
                self._active_list = None
                self._active_i = 0
                cur = material_list_icon_resolution_tag()
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
            if not isinstance(stem, str):
                decoded += 1
                continue

            if self._worker_decode:
                if isinstance(payload, QImage) and not payload.isNull():
                    pm = QPixmap.fromImage(payload)
                    if not pm.isNull():
                        store_prewarmed_scaled_pixmap(stem, pm)
            else:
                if isinstance(payload, (bytes, bytearray)):
                    img = QImage.fromData(bytes(payload))
                    if not img.isNull():
                        pm = QPixmap.fromImage(_scale_icon_qimage(img))
                        if not pm.isNull():
                            store_prewarmed_scaled_pixmap(stem, pm)
            decoded += 1
