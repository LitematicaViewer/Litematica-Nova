from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from PySide6.QtCore import QThread, Qt, Signal
from PySide6.QtWidgets import (
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QPlainTextEdit,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.game_resource_language import load_runtime_language_map
from litematicaba.core.native_backend_bridge import AnalyzeResult, analyze_litematic, ensure_backend_ready
from litematicaba.ui.material_list_dialog import MaterialListDialog
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.pages.properties_page import PropertiesPage


_BUILDING_TYPE_LABELS = {
    "too_small": "结构过小",
    "redstone_machine": "红石机器",
    "redstone_power": "生电红石",
    "mechanical_build": "机械装置",
    "structural_mechanism": "结构机关",
    "building": "建筑",
}


class _AnalyzeThread(QThread):
    result_ready = Signal(object)
    failed = Signal(str)

    def __init__(self, path: Path, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._path = path.resolve()

    def run(self) -> None:  # type: ignore[override]
        try:
            ensure_backend_ready()
            result = analyze_litematic(self._path, include_entities=True)
            self.result_ready.emit(result)
        except Exception as exc:
            self.failed.emit(str(exc))


class StatisticsPage(QWidget):
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
        self._thread: _AnalyzeThread | None = None
        self._cached_path: Path | None = None
        self._last_result: AnalyzeResult | None = None
        self._lang_map = load_runtime_language_map()

        top_row = QHBoxLayout()
        self._btn_material_list = QPushButton("材料列表")
        self._btn_material_list.clicked.connect(self._on_material_list_clicked)
        self._btn_analyze = QPushButton("重新分析")
        self._btn_analyze.clicked.connect(self._on_refresh_clicked)
        top_row.addWidget(self._btn_material_list)
        top_row.addStretch(1)
        top_row.addWidget(self._btn_analyze)

        self._path_label = QLabel("请先在“属性”页打开 .litematic。")
        self._path_label.setWordWrap(True)
        self._status_label = QLabel("")
        self._status_label.setWordWrap(True)
        self._status_label.setStyleSheet("color: palette(mid);")

        summary_box = QGroupBox("结构分析")
        summary_form = QFormLayout(summary_box)
        self._field_total = self._readonly()
        self._field_region = self._readonly()
        self._field_size = self._readonly()
        self._field_density = self._readonly()
        self._field_building = self._readonly()
        self._field_redstone = self._readonly()
        self._field_fluid = self._readonly()
        self._field_entities = self._readonly()
        summary_form.addRow("非空气方块：", self._field_total)
        summary_form.addRow("区域数量：", self._field_region)
        summary_form.addRow("包围尺寸：", self._field_size)
        summary_form.addRow("密度：", self._field_density)
        summary_form.addRow("结构类型：", self._field_building)
        summary_form.addRow("红石偏度：", self._field_redstone)
        summary_form.addRow("流体偏度：", self._field_fluid)
        summary_form.addRow("实体种类：", self._field_entities)

        top_material_box = QGroupBox("主要材料")
        material_layout = QVBoxLayout(top_material_box)
        self._top_materials = QPlainTextEdit()
        self._top_materials.setReadOnly(True)
        self._top_materials.setPlaceholderText("分析后会显示主要材料。")
        self._top_materials.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        material_layout.addWidget(self._top_materials)

        root = QVBoxLayout(self)
        root.addLayout(top_row)
        root.addWidget(self._path_label)
        root.addWidget(self._status_label)
        root.addWidget(summary_box)
        root.addWidget(top_material_box, 1)

        self._props.active_file_changed.connect(self._on_active_file_changed)
        self._props.analysis_ready.connect(self.apply_external_analysis)
        self._sync_path_label()

    @staticmethod
    def _readonly() -> QLineEdit:
        field = QLineEdit()
        field.setReadOnly(True)
        return field

    def _on_material_list_clicked(self) -> None:
        if self._props.active_file_path() is None:
            QMessageBox.information(self, "材料列表", "请先打开一个 .litematic 文件。")
            return
        MaterialListDialog.open_for_properties(
            self._props,
            self,
            material_scan_prewarmer=self._material_scan_prewarmer,
        )

    def _on_refresh_clicked(self) -> None:
        self._start_analyze(force=True)

    def _on_active_file_changed(self, _path: str) -> None:
        self._cached_path = None
        self._last_result = None
        self._sync_path_label()
        self._clear_summary()
        self._status_label.setText("正在加载并分析当前投影...")

    def showEvent(self, event) -> None:  # type: ignore[override]
        super().showEvent(event)
        self._sync_path_label()
        if (
            self._props.active_file_path() is not None
            and self._last_result is None
            and not getattr(self._props, "_snbt_load_busy", False)
        ):
            self._start_analyze(force=False)

    def _sync_path_label(self) -> None:
        current = self._props.display_file_path()
        self._path_label.setText(str(current) if current is not None else "请先在“属性”页打开 .litematic。")

    def apply_external_analysis(self, result: AnalyzeResult) -> None:
        current = self._props.active_file_path()
        if current is None or current.resolve() != Path(result.file_path).resolve():
            return
        self._cached_path = Path(result.file_path).resolve()
        self._last_result = result
        self._apply_result(result)

    def _start_analyze(self, *, force: bool) -> None:
        path = self._props.active_file_path()
        if path is None:
            self._status_label.setText("没有可分析的文件。")
            return
        resolved = path.resolve()
        if not force and self._cached_path == resolved and self._last_result is not None:
            self._apply_result(self._last_result)
            return
        if self._thread is not None and self._thread.isRunning():
            return

        self._status_label.setText("正在调用现有 Rust 分析能力…")
        self._btn_analyze.setEnabled(False)
        thread = _AnalyzeThread(resolved, self)
        self._thread = thread
        thread.result_ready.connect(self._on_thread_result)
        thread.failed.connect(self._on_thread_failed)
        thread.finished.connect(self._on_thread_finished)
        thread.start()

    def _on_thread_finished(self) -> None:
        self._thread = None
        self._btn_analyze.setEnabled(True)

    def _on_thread_result(self, result: AnalyzeResult) -> None:
        current = self._props.active_file_path()
        if current is None or current.resolve() != Path(result.file_path).resolve():
            return
        self._cached_path = Path(result.file_path).resolve()
        self._last_result = result
        self._apply_result(result)

    def _on_thread_failed(self, message: str) -> None:
        self._status_label.setText(message)
        QMessageBox.warning(self, "分析失败", message)

    def _apply_result(self, result: AnalyzeResult) -> None:
        payload = result.output
        metadata = payload.get("metadata", {})
        analysis = payload.get("analysis", {})
        derived = payload.get("derived", {})
        building = derived.get("building", {})

        self._field_total.setText(str(int(analysis.get("total_non_air_blocks", 0) or 0)))
        self._field_region.setText(str(int(metadata.get("region_count", 0) or 0)))
        size = metadata.get("enclosing_size", {}) or {}
        self._field_size.setText(
            f"{int(size.get('x', 0) or 0)} x {int(size.get('y', 0) or 0)} x {int(size.get('z', 0) or 0)}"
        )
        self._field_density.setText(self._format_percent(float(building.get("density", 0.0) or 0.0)))
        building_type = str(building.get("building_type", "") or "")
        self._field_building.setText(_BUILDING_TYPE_LABELS.get(building_type, building_type or "-"))
        self._field_redstone.setText(self._format_percent(float(building.get("redstone_ratio", 0.0) or 0.0)))
        self._field_fluid.setText(self._format_percent(float(building.get("fluid_ratio", 0.0) or 0.0)))
        entity_count = len((analysis.get("entity_counts", {}) or {}).keys())
        self._field_entities.setText(str(entity_count))

        material_rows = list(derived.get("material_counts", []) or [])[:12]
        display_rows = []
        for row in material_rows:
            key = str(row.get("key", "") or "")
            count = int(row.get("count", 0) or 0)
            display_rows.append(f"{self._display_name(key)} x {count}")
        self._top_materials.setPlainText("\n".join(display_rows) if display_rows else "没有材料统计结果。")
        self._status_label.setText("分析完成，当前统计结果来自现有 Rust 后端。")

    def _clear_summary(self) -> None:
        for field in (
            self._field_total,
            self._field_region,
            self._field_size,
            self._field_density,
            self._field_building,
            self._field_redstone,
            self._field_fluid,
            self._field_entities,
        ):
            field.clear()
        self._top_materials.clear()

    @staticmethod
    def _format_percent(value: float) -> str:
        return f"{value * 100:.1f}%"

    def _display_name(self, base_id: str) -> str:
        raw = base_id.strip()
        if not raw:
            return "-"
        key = raw.split("[", 1)[0].split(":", 1)[-1]
        return self._lang_map.get(f"block.minecraft.{key}") or self._lang_map.get(f"item.minecraft.{key}") or raw
