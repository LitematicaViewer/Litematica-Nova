from __future__ import annotations

import time
from pathlib import Path

from PySide6.QtCore import QTimer, Qt, Signal
from PySide6.QtGui import QShowEvent
from PySide6.QtWidgets import (
    QComboBox,
    QCheckBox,
    QFrame,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QPushButton,
    QProgressBar,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.native_backend_bridge import (
    RENDER_BUILD_MODE_FAST_EXPERIMENTAL,
    RENDER_BUILD_MODE_FULL,
    RENDER_BUILD_MODE_NORMAL,
    VALID_RENDER_BUILD_MODES,
    ViewerLaunch,
    cancel_cache_build,
    cancel_layer_precache,
    ensure_backend_ready,
    mark_layer_precache_activity,
    open_embedded_viewer,
    open_popup_viewer,
    poll_cache_build,
    poll_layer_precache,
    start_cache_build,
    start_layer_precache,
)
from litematicaba.core.settings import AppSettings, save_settings
from litematicaba.ui.material_list_dialog import MaterialListDialog
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.pages.properties_page import PropertiesPage

LAYER_PRECACHE_TIMEOUT_SECONDS = 600


class RenderPage(QWidget):
    cache_ready = Signal(str, str)
    build_mode_changed = Signal(str)

    def __init__(
        self,
        properties_page: PropertiesPage,
        *,
        app_settings=None,
        material_scan_prewarmer: MaterialListScanPrewarmer | None = None,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.setObjectName("renderBridgeRoot")
        self.setMinimumSize(0, 0)
        self._props = properties_page
        self._material_scan_prewarmer = material_scan_prewarmer
        self._app_settings = app_settings or AppSettings()
        self._current_launch: ViewerLaunch | None = None
        self._current_cache_file: Path | None = None
        self._layer_precache_launch = None
        self._layer_precache_session = 0
        self._layer_precache_started_at = 0.0
        self._embedded_process = None
        self._render_state = "no_file"
        self._state_detail = ""
        self._build_mode = getattr(self._app_settings, "render_build_mode", RENDER_BUILD_MODE_NORMAL)

        self._poll_timer = QTimer(self)
        self._poll_timer.setInterval(350)
        self._poll_timer.timeout.connect(self._poll_build_progress)
        self._layer_precache_timer = QTimer(self)
        self._layer_precache_timer.setInterval(500)
        self._layer_precache_timer.timeout.connect(self._poll_layer_precache)

        self._path_label = QLabel("请先在“属性”页打开 .litematic。")
        self._path_label.setObjectName("renderBridgePathLabel")
        self._path_label.setWordWrap(True)

        self._mode_label = QLabel("模式")
        self._mode_label.setObjectName("renderBridgeModeLabel")
        self._mode_combo = QComboBox()
        self._mode_combo.setObjectName("renderBridgeModeCombo")
        self._mode_combo.addItem("普通模式", RENDER_BUILD_MODE_NORMAL)
        self._mode_combo.addItem("快速模式（实验）", RENDER_BUILD_MODE_FAST_EXPERIMENTAL)
        self._mode_combo.addItem("完整模式", RENDER_BUILD_MODE_FULL)
        self._mode_combo.currentIndexChanged.connect(self._on_build_mode_changed)
        self._apply_build_mode_selection()
        self._precache_layers_check = QCheckBox("同时预生成分层")
        self._precache_layers_check.setToolTip("默认关闭；勾选后会在 3D cache 完成后追加一个分层预生成阶段。")

        actions = QHBoxLayout()
        actions.addWidget(self._mode_label)
        actions.addWidget(self._mode_combo)
        actions.addWidget(self._precache_layers_check)
        self._btn_build = QPushButton("构建 3D cache")
        self._btn_build.clicked.connect(self._on_build_clicked)
        self._btn_open_popup = QPushButton("打开弹窗 Viewer")
        self._btn_open_popup.clicked.connect(self._on_open_popup_clicked)
        self._btn_open_popup.setEnabled(False)
        self._btn_reset_view = QPushButton("重置视角")
        self._btn_reset_view.clicked.connect(self._on_reset_view_clicked)
        self._btn_materials = QPushButton("材料列表")
        self._btn_materials.clicked.connect(self._on_material_list_clicked)
        actions.addWidget(self._btn_build)
        actions.addWidget(self._btn_open_popup)
        actions.addWidget(self._btn_reset_view)
        actions.addStretch(1)
        actions.addWidget(self._btn_materials)

        self._progress = QProgressBar()
        self._progress.setRange(0, 1000)
        self._progress.setValue(0)
        self._progress_label = QLabel("尚未开始构建 3D cache。")
        self._progress_label.setWordWrap(True)
        self._status_label = QLabel("")
        self._status_label.setWordWrap(True)
        self._status_label.setStyleSheet("color: palette(mid);")
        self._empty_card = self._create_empty_card()
        self._native_container = QWidget(self)
        self._native_container.setObjectName("renderBridgeNativeContainer")
        self._native_container.setMinimumSize(0, 0)

        root = QVBoxLayout(self)
        root.setContentsMargins(9, 9, 9, 9)
        header = QLabel("Render Bridge Page / 渲染页")
        header.setObjectName("renderBridgePageHeader")
        header.setAlignment(Qt.AlignmentFlag.AlignCenter)
        font = header.font()
        font.setPointSize(max(16, font.pointSize() + 4))
        font.setBold(True)
        header.setFont(font)
        root.addWidget(header)
        root.addWidget(self._path_label)
        root.addLayout(actions)
        root.addWidget(self._progress)
        root.addWidget(self._progress_label)
        root.addWidget(self._status_label)
        self._preview_stack = QStackedWidget()
        self._preview_stack.setObjectName("renderBridgePreviewStateStack")
        self._preview_stack.addWidget(self._empty_card)
        self._preview_stack.addWidget(self._native_container)

        root.addWidget(self._preview_stack, 1)

        self._props.active_file_changed.connect(self._on_active_file_changed)
        self._sync_path_label()
        self._refresh_render_state()

    def _create_empty_card(self) -> QWidget:
        card = QGroupBox("渲染页", self)
        card.setObjectName("renderBridgeEmptyCard")
        layout = QVBoxLayout(card)
        layout.setContentsMargins(24, 24, 24, 24)
        layout.setSpacing(12)
        layout.addStretch(1)

        self._empty_badge = QLabel("RENDER MODE ACTIVE")
        self._empty_badge.setObjectName("renderBridgeEmptyBadge")
        self._empty_badge.setAlignment(Qt.AlignmentFlag.AlignCenter)

        self._empty_title = QLabel("RENDER BRIDGE PAGE\n渲染页")
        self._empty_title.setObjectName("renderBridgeEmptyTitle")
        self._empty_title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title_font = self._empty_title.font()
        title_font.setPointSize(max(18, title_font.pointSize() + 6))
        title_font.setBold(True)
        self._empty_title.setFont(title_font)

        self._empty_file = QLabel("当前文件：未选择文件")
        self._empty_file.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty_file.setWordWrap(True)
        file_font = self._empty_file.font()
        file_font.setPointSize(max(12, file_font.pointSize() + 2))
        self._empty_file.setFont(file_font)

        self._empty_status = QLabel("cache 状态：未开始")
        self._empty_status.setObjectName("renderBridgeEmptyStatus")
        self._empty_status.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty_status.setWordWrap(True)
        status_font = self._empty_status.font()
        status_font.setPointSize(max(12, status_font.pointSize() + 2))
        status_font.setBold(True)
        self._empty_status.setFont(status_font)

        self._empty_reason = QLabel("请先在“属性”页打开 .litematic 文件。")
        self._empty_reason.setObjectName("renderBridgeEmptyReason")
        self._empty_reason.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty_reason.setWordWrap(True)
        self._empty_reason.setStyleSheet("color: palette(mid);")
        reason_font = self._empty_reason.font()
        reason_font.setPointSize(max(12, reason_font.pointSize() + 2))
        self._empty_reason.setFont(reason_font)

        self._empty_action = QPushButton("构建 3D cache")
        self._empty_action.setObjectName("renderBridgeEmptyAction")
        self._empty_action.setMinimumHeight(40)
        self._empty_action.clicked.connect(self._on_build_clicked)

        layout.addWidget(self._empty_badge, 0, Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self._empty_title)
        layout.addWidget(self._empty_file)
        layout.addWidget(self._empty_status)
        layout.addWidget(self._empty_reason)
        layout.addWidget(self._empty_action, 0, Qt.AlignmentFlag.AlignCenter)
        layout.addStretch(1)
        return card

    def _set_render_state(self, state: str, detail: str = "") -> None:
        self._render_state = state
        self._state_detail = detail
        self._refresh_render_state()

    def showEvent(self, event: QShowEvent) -> None:  # type: ignore[override]
        super().showEvent(event)
        self._refresh_render_state()
        self._force_render_repaint()
        QTimer.singleShot(0, self._force_render_repaint)

    def resizeEvent(self, event) -> None:  # type: ignore[override]
        super().resizeEvent(event)

    def closeEvent(self, event) -> None:  # type: ignore[override]
        self._cancel_layer_precache("窗口关闭")
        super().closeEvent(event)

    def _force_render_repaint(self) -> None:
        for widget in (self, self._preview_stack, self._empty_card, self._native_container):
            layout = widget.layout()
            if layout is not None:
                layout.activate()
            widget.updateGeometry()
            widget.update()
            widget.repaint()

    def _refresh_render_state(self) -> None:
        path = self._props.active_file_path()
        file_text = str(path) if path is not None else "未选择文件"
        display_path = self._props.display_file_path()
        if path is not None:
            file_text = str(display_path or path)
        state = self._render_state
        if path is None and state not in ("building", "error"):
            state = "no_file"
        elif path is not None and state == "no_file":
            state = "no_cache"

        status_map = {
            "no_file": "未开始",
            "no_cache": "未构建 cache",
            "building": "构建中",
            "ready": "已完成",
            "error": "加载失败",
        }
        reason_map = {
            "no_file": "请先在“属性”页打开 .litematic 文件，然后回到这里构建 3D cache。",
            "no_cache": "已选择文件，但还没有构建 3D cache。点击下方按钮开始构建。",
            "building": "正在构建 3D cache，请看上方进度条和状态文案。",
            "ready": "cache 已完成，嵌入式预览和弹窗 viewer 读取同一份 cache。",
            "error": self._state_detail or "预览加载失败，请查看上方状态文案。",
        }
        self._empty_file.setText(f"当前文件：{file_text}")
        self._empty_status.setText(f"cache 状态：{status_map.get(state, state)}")
        self._empty_reason.setText(reason_map.get(state, self._state_detail))
        self._empty_action.setEnabled(path is not None and state not in ("building", "ready"))
        self._empty_action.setVisible(state in ("no_file", "no_cache", "error"))
        self._btn_build.setEnabled(path is not None and state not in ("building", "ready"))
        self._btn_open_popup.setEnabled(state == "ready" and self._current_cache_file is not None)
        if state == "ready" and self._embedded_process is not None:
            self._preview_stack.setCurrentWidget(self._native_container)
        else:
            self._preview_stack.setCurrentWidget(self._empty_card)

    def debug_nav_state(self) -> str:
        rows = []
        for child in self.findChildren(QWidget):
            if child.parentWidget() is not self:
                continue
            g = child.geometry()
            rows.append(
                f"{child.__class__.__name__}/{child.objectName()!r} "
                f"visible={child.isVisible()} geometry=({g.x()},{g.y()},{g.width()}x{g.height()})"
            )
        return "; ".join(rows)

    def _apply_build_mode_selection(self) -> None:
        index = self._mode_combo.findData(self._build_mode)
        if index < 0:
            self._build_mode = RENDER_BUILD_MODE_NORMAL
            index = self._mode_combo.findData(self._build_mode)
        previous = self._mode_combo.blockSignals(True)
        self._mode_combo.setCurrentIndex(index)
        self._mode_combo.blockSignals(previous)

    def _on_build_mode_changed(self, index: int) -> None:
        mode = self._mode_combo.itemData(index)
        if mode not in VALID_RENDER_BUILD_MODES:
            mode = RENDER_BUILD_MODE_NORMAL
        self._build_mode = mode
        self._app_settings.render_build_mode = mode
        save_settings(self._app_settings)
        self._status_label.setText(f"当前构建模式：{self._build_mode_label(mode)}")
        self.build_mode_changed.emit(mode)
        self._refresh_render_state()
        if self._render_state == "ready" and self._current_cache_file is not None:
            self._stop_embedded_native_viewer()
            self._load_embedded_preview(emit_ready=False, start_precache=False)

    def apply_deepslate_settings(self, settings) -> None:
        if settings is None:
            return
        self._app_settings = settings
        self._build_mode = getattr(settings, "render_build_mode", RENDER_BUILD_MODE_NORMAL)
        self._apply_build_mode_selection()
        self.build_mode_changed.emit(self._build_mode)
        self._refresh_render_state()

    @staticmethod
    def _build_mode_label(mode: str) -> str:
        if mode == RENDER_BUILD_MODE_FAST_EXPERIMENTAL:
            return "快速模式（实验）"
        if mode == RENDER_BUILD_MODE_FULL:
            return "完整模式"
        return "普通模式"

    def _sync_path_label(self) -> None:
        current = self._props.display_file_path()
        self._path_label.setText(str(current) if current is not None else "请先在“属性”页打开 .litematic。")
        self._refresh_render_state()

    def _on_active_file_changed(self, _path: str) -> None:
        self._sync_path_label()
        self._reset_pipeline_state(cancel_running=True)

    def _reset_pipeline_state(self, *, cancel_running: bool) -> None:
        if cancel_running and self._current_launch is not None:
            cancel_cache_build(self._current_launch.launch_id)
        self._cancel_layer_precache("任务已取消")
        self._stop_embedded_native_viewer()
        self._poll_timer.stop()
        self._current_launch = None
        self._current_cache_file = None
        self._progress.setValue(0)
        self._progress_label.setText("尚未开始构建 3D cache。")
        self._status_label.setText("")
        self._btn_open_popup.setEnabled(False)
        self._btn_build.setEnabled(True)
        self._set_render_state("no_file" if self._props.active_file_path() is None else "no_cache")

    def _cancel_layer_precache(self, message: str = "") -> None:
        self._layer_precache_session += 1
        self._layer_precache_timer.stop()
        self._layer_precache_started_at = 0.0
        launch = self._layer_precache_launch
        self._layer_precache_launch = None
        if launch is not None:
            cancel_layer_precache(launch.launch_id)
            if message:
                self._status_label.setText(f"分层预生成已终止：{message}")

    def _on_build_clicked(self) -> None:
        path = self._props.active_file_path()
        if path is None:
            self._set_render_state("no_file")
            QMessageBox.information(self, "3D cache", "请先打开一个 .litematic 文件。")
            return
        if self._current_launch is not None:
            return
        self._cancel_layer_precache()
        try:
            ensure_backend_ready()
            launch = start_cache_build(path, build_mode=self._build_mode)
        except Exception as exc:
            QMessageBox.warning(self, "3D cache", str(exc))
            self._status_label.setText(str(exc))
            self._set_render_state("error", str(exc))
            return

        self._current_launch = launch
        self._current_cache_file = Path(launch.cache_file)
        self._progress.setValue(20)
        self._progress_label.setText("阶段 1/2：已启动 3D cache 构建，正在读取真实进度...")
        self._progress_label.setText("已启动 cache 构建，正在读取真实进度…")
        self._status_label.setText(
            f"当前模式：{self._build_mode_label(self._build_mode)}。"
            "底层使用现有 litematica_native_viewer，嵌入式和弹窗会共用同一份 cache。"
        )
        self._set_render_state("building", "正在构建 3D cache，请看上方进度。")
        self._poll_timer.start()

    def _poll_build_progress(self) -> None:
        launch = self._current_launch
        if launch is None:
            self._poll_timer.stop()
            return
        try:
            snapshot = poll_cache_build(launch.launch_id)
        except Exception as exc:
            self._poll_timer.stop()
            self._status_label.setText(str(exc))
            QMessageBox.warning(self, "3D cache", str(exc))
            self._current_launch = None
            self._set_render_state("error", str(exc))
            return

        progress = snapshot.progress
        if progress is not None:
            value = max(0, min(1000, int(round(progress.percent * 10))))
            self._progress.setValue(value)
            built = progress.built_chunks
            total = max(1, progress.total_chunks)
            phase = progress.phase or "building"
            self._progress_label.setText(
                f"{phase}  {progress.percent:.1f}%  ({built}/{total})"
            )
        if progress is not None and progress.ready:
            self._poll_timer.stop()
            self._status_label.setText("3D cache 已完成，正在把同一份 cache 送入嵌入式预览。")
            self._current_launch = None
            self._load_embedded_preview()
            return
        if not snapshot.running and (progress is None or not progress.ready):
            self._poll_timer.stop()
            self._current_launch = None
            message = "构建进程已结束，但没有拿到 ready cache。"
            self._status_label.setText(message)
            self._set_render_state("error", message)
            QMessageBox.warning(self, "3D cache", "构建进程结束了，但没有生成可用 cache。")

    def _load_embedded_preview(self, *, emit_ready: bool = True, start_precache: bool = True) -> None:
        path = self._props.active_file_path()
        cache = self._current_cache_file
        if path is None or cache is None:
            self._set_render_state("error", "缺少文件路径或 cache 路径，无法加载嵌入式预览。")
            return
        if not cache.exists():
            self._set_render_state("error", f"cache 文件不存在：{cache}")
            return
        try:
            self._embedded_process = open_embedded_viewer(
                path,
                cache,
                parent_hwnd=int(self._native_container.winId()),
                display_mode=self._build_mode,
            )
        except Exception as exc:
            self._set_render_state("error", str(exc))
            self._status_label.setText(str(exc))
            return
        self._progress.setValue(1000)
        self._progress_label.setText("cache 已完成：嵌入式入口已切到 native renderer。")
        self._status_label.setText(
            "嵌入式预览不再使用 HTML/独立解析链；当前和弹窗 Viewer 使用同一个 native renderer、同一份 cache。"
        )
        self._set_render_state("ready")
        if emit_ready:
            self.cache_ready.emit(str(path), str(cache))
        if start_precache and self._precache_layers_check.isChecked():
            self._start_layer_precache(cache)

    def _start_layer_precache(self, cache: Path) -> None:
        self._cancel_layer_precache()
        self._layer_precache_session += 1
        try:
            self._layer_precache_launch = start_layer_precache(cache, timeout_seconds=LAYER_PRECACHE_TIMEOUT_SECONDS)
        except Exception as exc:
            self._status_label.setText(f"3D cache 已完成；分层预生成启动失败：{exc}")
            return
        self._layer_precache_started_at = time.monotonic()
        self._progress_label.setText("阶段 2/2：正在预生成分层缓存...")
        self._status_label.setText("3D cache 已完成；正在追加分层预生成，不影响已生成的 cache 使用。")
        self._layer_precache_timer.start()

    def _poll_layer_precache(self) -> None:
        launch = self._layer_precache_launch
        if launch is None:
            self._layer_precache_timer.stop()
            return
        if self._embedded_process is not None and self._embedded_process.poll() is None:
            mark_layer_precache_activity(launch.cache_file)
        session = self._layer_precache_session
        if self._layer_precache_started_at and time.monotonic() - self._layer_precache_started_at > LAYER_PRECACHE_TIMEOUT_SECONDS:
            self._cancel_layer_precache("超时")
            self._progress_label.setText("cache 已完成；分层预生成超时，已强制终止。")
            return
        payload = poll_layer_precache(launch.launch_id)
        if session != self._layer_precache_session:
            return
        total = int(payload.get("total", 0) or 0)
        done = int(payload.get("done", 0) or 0)
        phase = str(payload.get("phase", "") or "")
        phase_text = "让路中" if phase == "yielding" else "预生成中"
        suffix = "；预算模式，剩余层进入分层页时懒加载" if bool(payload.get("degraded", False)) else ""
        if total > 0:
            self._progress_label.setText(f"阶段 2/2：分层{phase_text} {done}/{total}{suffix}")
        if payload.get("ready"):
            self._layer_precache_timer.stop()
            self._layer_precache_launch = None
            self._status_label.setText("分层预生成完成；分层页可直接复用预缓存结果。")
            self._progress_label.setText("cache 已完成；分层预生成完成。")
            return
        if not bool(payload.get("running", False)):
            self._layer_precache_timer.stop()
            self._layer_precache_launch = None
            error = payload.get("error") or phase or "已结束"
            self._status_label.setText(f"3D cache 已完成；分层预生成未完成：{error}")

    def _stop_embedded_native_viewer(self) -> None:
        process = self._embedded_process
        self._embedded_process = None
        if process is not None and process.poll() is None:
            process.terminate()

    def deactivate_native_surfaces_for_empty(self) -> None:
        return

    def _on_open_popup_clicked(self) -> None:
        path = self._props.active_file_path()
        cache = self._current_cache_file
        if path is None or cache is None:
            return
        try:
            open_popup_viewer(path, cache, display_mode=self._build_mode)
        except Exception as exc:
            QMessageBox.warning(self, "弹窗 Viewer", str(exc))
            self._status_label.setText(str(exc))

    def _on_reset_view_clicked(self) -> None:
        self._status_label.setText("当前渲染由 native viewer 负责，重置视角请在 viewer 窗口内按 R。")

    def _on_material_list_clicked(self) -> None:
        if self._props.active_file_path() is None:
            QMessageBox.information(self, "材料列表", "请先打开一个 .litematic 文件。")
            return
        MaterialListDialog.open_for_properties(
            self._props,
            self,
            material_scan_prewarmer=self._material_scan_prewarmer,
        )
