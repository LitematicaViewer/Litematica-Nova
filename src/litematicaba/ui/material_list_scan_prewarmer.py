"""投影打开后后台预扫「整个投影 / 不含实体」材料计数，供材料列表秒开并让统计延后启动。"""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import QObject, QThread, QTimer, Signal

from litematicaba.core.material_list_cache import (
    cache_is_stale,
    file_mtime_ns,
    load_material_cache,
    save_material_cache,
)
from litematicaba.core.native_backend_bridge import material_counts_from_analysis


class _WholeProjectScanThread(QThread):
    ok = Signal(int, object, object)  # token, Path, dict[str, int]
    failed = Signal(int, object, str)  # token, Path, err

    def __init__(self, path: Path, token: int, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._path = path.resolve()
        self._token = token

    def run(self) -> None:  # type: ignore[override]
        try:
            d = material_counts_from_analysis(self._path, include_entities=False)
            self.ok.emit(self._token, self._path, d)
        except Exception as exc:
            self.failed.emit(self._token, self._path, str(exc))


class MaterialListScanPrewarmer(QObject):
    """预扫与材料列表默认工作簿一致：``region_name is None``、``include_entities False``。"""

    finished_for_path = Signal(object)  # Path

    def __init__(self, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._thread: _WholeProjectScanThread | None = None
        self._gen = 0
        self._focus_path: Path | None = None

    def is_busy_for(self, path: Path) -> bool:
        if self._thread is None or not self._thread.isRunning():
            return False
        if self._focus_path is None:
            return False
        return self._focus_path.resolve() == path.resolve()

    def schedule(self, path: Path) -> None:
        resolved = path.resolve()
        self._gen += 1
        token = self._gen
        self._focus_path = resolved

        cached = load_material_cache(resolved, region_name=None, include_entities=False)
        if cached is not None:
            _counts, mns = cached
            if not cache_is_stale(resolved, mns):
                QTimer.singleShot(0, lambda: self._emit_done_if_current(token, resolved))
                return

        th = _WholeProjectScanThread(resolved, token, self)
        self._thread = th
        th.ok.connect(self._on_thread_ok)
        th.failed.connect(self._on_thread_failed)
        th.finished.connect(self._on_thread_finished)
        th.start()

    def _emit_done_if_current(self, token: int, path: Path) -> None:
        if token != self._gen:
            return
        self.finished_for_path.emit(path)

    def _on_thread_finished(self) -> None:
        self._thread = None

    def _on_thread_ok(self, token: int, path: Path, counts: dict) -> None:
        if token != self._gen:
            return
        try:
            mns = file_mtime_ns(path)
            save_material_cache(
                path,
                region_name=None,
                include_entities=False,
                counts=dict(counts),
                mtime_ns=mns,
            )
        except OSError:
            pass
        self.finished_for_path.emit(path)

    def _on_thread_failed(self, token: int, path: Path, _err: str) -> None:
        if token != self._gen:
            return
        # 仍发完成，避免统计页永久等待；材料列表打开时会再尝试扫描。
        self.finished_for_path.emit(path)
