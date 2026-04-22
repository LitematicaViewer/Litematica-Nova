"""统计页（design §2.4）：按旧版逻辑解析当前激活的 ``.litematic`` 并展示指标。"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from PySide6.QtCore import QThread, Qt, Signal
from PySide6.QtGui import QShowEvent
from PySide6.QtWidgets import (
    QCheckBox,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.legacy_statistics import LegacyStatisticsResult, compute_legacy_statistics
from litematicaba.ui.material_list_dialog import MaterialListDialog
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.pages.properties_page import PropertiesPage


class _StatisticsComputeThread(QThread):
    result_ready = Signal(object)
    failed = Signal(str)

    def __init__(self, path: Path, include_entities: bool, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._path = path.resolve()
        self._include_entities = include_entities

    def run(self) -> None:  # type: ignore[override]
        try:
            r = compute_legacy_statistics(self._path, include_entities=self._include_entities)
            self.result_ready.emit(r)
        except Exception as exc:
            self.failed.emit(str(exc))


class StatisticsPage(QWidget):
    """依赖 ``PropertiesPage`` 的当前文件路径；是否在材料预扫后再统计由设置决定。"""

    def __init__(
        self,
        properties_page: PropertiesPage,
        parent: QWidget | None = None,
        *,
        material_scan_prewarmer: MaterialListScanPrewarmer | None = None,
        defer_stats_until_material_prewarm: Callable[[], bool] | None = None,
    ) -> None:
        super().__init__(parent)
        self._props = properties_page
        self._material_scan_prewarmer = material_scan_prewarmer
        self._defer_stats_until_material_prewarm = defer_stats_until_material_prewarm
        self._thread: _StatisticsComputeThread | None = None
        """若当前有线程在跑时再次请求统计，则记最近一次待执行请求（路径 + force）。"""
        self._queued_job: tuple[Path, bool] | None = None
        self._cached_path: Path | None = None
        self._cached_entities: bool | None = None
        self._last_result: LegacyStatisticsResult | None = None

        self._hint = QLabel(
            "密度分母为「最后一个子区域」的包围格数（与旧版 LitematicaViewer 一致）；"
            "非空气方块数 num 为全部子区域累计。"
        )
        self._hint.setWordWrap(True)
        self._hint.setStyleSheet("color: palette(mid); font-size: 11px;")

        mat_row = QHBoxLayout()
        self._btn_material_list = QPushButton("材料列表")
        self._btn_material_list.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self._btn_material_list.clicked.connect(self._on_material_list_clicked)
        mat_row.addWidget(self._btn_material_list, 1)

        self._lbl_path = QLabel("请在「属性」页加载 .litematic。")
        self._lbl_path.setWordWrap(True)

        self._cb_entities = QCheckBox("统计实体（排除掉落物/蝙蝠/经验球/潜影弹）")
        self._cb_entities.toggled.connect(self._on_entities_toggled)

        self._btn_go = QPushButton("重新统计")
        self._btn_go.clicked.connect(self._on_refresh_clicked)

        row = QHBoxLayout()
        row.addWidget(self._cb_entities)
        row.addStretch()
        row.addWidget(self._btn_go)

        box = QGroupBox("统计指标")
        form = QFormLayout(box)
        self._v_red = self._readonly_line_edit(align_right=True)
        self._v_mat = self._readonly_line_edit(align_right=False)
        self._v_liq = self._readonly_line_edit(align_right=True)
        self._v_den = self._readonly_line_edit(align_right=True)
        self._v_debug = QLabel("-")
        self._v_debug.setWordWrap(True)
        self._v_debug.setStyleSheet("color: palette(mid);")
        form.addRow("红石偏度：", self._v_red)
        form.addRow("统计分类：", self._v_mat)
        form.addRow("液体偏度：", self._v_liq)
        form.addRow("密度：", self._v_den)
        form.addRow("调试：", self._v_debug)

        root = QVBoxLayout(self)
        root.addLayout(mat_row)
        root.addWidget(self._hint)
        root.addWidget(self._lbl_path)
        root.addLayout(row)
        root.addWidget(box)
        root.addStretch()

        self._props.active_file_changed.connect(self._on_active_file_changed)
        if self._material_scan_prewarmer is not None:
            self._material_scan_prewarmer.finished_for_path.connect(self._on_material_prewarm_finished)

    def _should_wait_material_prewarm_for_stats(self) -> bool:
        if self._material_scan_prewarmer is None:
            return False
        if self._defer_stats_until_material_prewarm is None:
            return True
        return self._defer_stats_until_material_prewarm()

    @staticmethod
    def _readonly_line_edit(*, align_right: bool) -> QLineEdit:
        w = QLineEdit()
        w.setReadOnly(True)
        w.setMinimumWidth(0)
        w.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        align = Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter if align_right else Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter
        w.setAlignment(align)
        return w

    def _on_refresh_clicked(self) -> None:
        self._start_compute(force=True)

    def _on_material_list_clicked(self) -> None:
        if self._props.active_file_path() is None:
            QMessageBox.information(self, "材料列表", "请先在「属性」页打开一个投影文件。")
            return
        MaterialListDialog.open_for_properties(
            self._props,
            self,
            material_scan_prewarmer=self._material_scan_prewarmer,
        )

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        self._sync_path_label()
        p = self._props.active_file_path()
        if p is None:
            self._clear_metrics()
            return
        if self._thread is not None and self._thread.isRunning():
            self._set_loading_metrics()
            return
        if self._should_wait_material_prewarm_for_stats() and self._material_scan_prewarmer is not None:
            if self._material_scan_prewarmer.is_busy_for(p.resolve()):
                self._set_loading_metrics()
                return
        self._start_compute(force=False)

    def _on_active_file_changed(self, _path: str) -> None:
        self._invalidate_cache()
        self._sync_path_label()
        if self._should_wait_material_prewarm_for_stats():
            self._set_loading_metrics()
            return
        self._start_compute(force=False)

    def _on_material_prewarm_finished(self, p: Path) -> None:
        if not self._should_wait_material_prewarm_for_stats():
            return
        cur = self._props.active_file_path()
        if cur is None or cur.resolve() != p.resolve():
            return
        self._start_compute(force=False)

    def _on_entities_toggled(self, _checked: bool) -> None:
        self._invalidate_cache()
        if self._props.active_file_path() is not None:
            self._start_compute(force=False)

    def _invalidate_cache(self) -> None:
        self._cached_path = None
        self._cached_entities = None
        self._last_result = None

    def _sync_path_label(self) -> None:
        p = self._props.active_file_path()
        self._lbl_path.setText(str(p) if p is not None else "请在「属性」页加载 .litematic。")

    def _set_loading_metrics(self) -> None:
        self._v_red.setText("…")
        self._v_mat.setText("…")
        self._v_liq.setText("…")
        self._v_den.setText("…")
        self._v_debug.setText("计算中…")

    def _start_compute(self, *, force: bool) -> None:
        p = self._props.active_file_path()
        if p is None:
            self._queued_job = None
            self._clear_metrics()
            self._btn_go.setEnabled(True)
            return
        inc_ent = self._cb_entities.isChecked()
        resolved = p.resolve()
        if self._thread is not None and self._thread.isRunning():
            self._queued_job = (resolved, force)
            self._set_loading_metrics()
            return
        if (
            not force
            and self._last_result is not None
            and self._cached_path == resolved
            and self._cached_entities == inc_ent
        ):
            self._apply_result(self._last_result)
            self._btn_go.setEnabled(True)
            return

        self._queued_job = None
        self._btn_go.setEnabled(False)
        self._set_loading_metrics()

        th = _StatisticsComputeThread(resolved, inc_ent, self)
        self._thread = th
        th.result_ready.connect(self._on_thread_result)
        th.failed.connect(self._on_thread_failed)
        th.finished.connect(self._on_thread_finished)
        th.start()

    def _on_thread_finished(self) -> None:
        self._thread = None
        pending = self._queued_job
        self._queued_job = None
        cur = self._props.active_file_path()
        if pending is not None and cur is not None:
            path_p, force_p = pending
            if cur.resolve() == path_p:
                self._start_compute(force=force_p)
                return
        self._btn_go.setEnabled(True)

    def _on_thread_result(self, r: LegacyStatisticsResult) -> None:
        cur = self._props.active_file_path()
        if cur is None or cur.resolve() != r.source_path:
            return
        inc_ent = self._cb_entities.isChecked()
        self._cached_path = r.source_path
        self._cached_entities = inc_ent
        self._last_result = r
        self._apply_result(r)

    def _on_thread_failed(self, message: str) -> None:
        self._v_red.setText("-")
        self._v_mat.setText("-")
        self._v_liq.setText("-")
        self._v_den.setText("-")
        self._v_debug.setText(message)
        QMessageBox.warning(self, "统计失败", message)

    def _clear_metrics(self) -> None:
        self._v_red.setText("-")
        self._v_mat.setText("-")
        self._v_liq.setText("-")
        self._v_den.setText("-")
        self._v_debug.setText("-")

    @staticmethod
    def _pct1(ratio: float | None) -> str:
        if ratio is None:
            return "-"
        return f"{ratio * 100:.1f}%"

    def _apply_result(self, r: LegacyStatisticsResult) -> None:
        self._v_red.setText(self._pct1(r.redstone_skew_ratio))
        self._v_mat.setText(r.material_label_zh)
        liq_pct = self._pct1(r.fluid_ratio)
        if r.fluid_ratio is None:
            self._v_liq.setText("-")
        else:
            self._v_liq.setText(f"{liq_pct}  ({r.fluid_units}u)")
        self._v_den.setText(self._pct1(r.density_ratio))

        lx, ly, lz = r.last_region_size
        dbg = (
            f"非空气方块 num={r.num_non_air}，字典键数={r.distinct_block_keys}，"
            f"末区尺寸 {lx}×{ly}×{lz}"
        )
        if r.metadata_total_blocks is not None:
            if r.metadata_matches_computed_num:
                dbg += f"，Metadata.TotalBlocks={r.metadata_total_blocks}（与 num 一致）"
            else:
                dbg += f"，Metadata.TotalBlocks={r.metadata_total_blocks}（与 num 不一致）"
        else:
            dbg += "，Metadata.TotalBlocks 未读取"
        self._v_debug.setText(dbg)
