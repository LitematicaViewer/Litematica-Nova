"""渲染页：优先使用内置 vscode-nbt 同源 3D（StructureRenderer + litematicToStructure）；否则回退 Deepslate VoxelRenderer + 九向预设。"""

from __future__ import annotations

import base64
import json
from collections.abc import Callable
from pathlib import Path

from PySide6.QtCore import QThread, QTimer, QUrl, Qt, QSize, Signal
from PySide6.QtGui import QImage, QPixmap, QShowEvent
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QFileDialog,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSlider,
    QStyle,
    QStyleOptionButton,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.litematic_voxel_export import build_region_voxels_payload
from litematicaba.core.nbt_viewer_bundle import (
    external_nbt_viewer_dir,
    packaged_nbt_viewer_dir,
    resolve_nbt_viewer_html_path,
)
from litematicaba.core.settings import AppSettings
from litematicaba.ui.material_list_dialog import MaterialListDialog
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.pages.properties_page import PropertiesPage

try:
    from PySide6.QtWebEngineWidgets import QWebEngineView

    _HAS_WEBENGINE = True
except ImportError:
    _HAS_WEBENGINE = False
    QWebEngineView = None  # type: ignore[misc, assignment]


if _HAS_WEBENGINE and QWebEngineView is not None:

    class _DeepslateWebEngineView(QWebEngineView):
        """避免 QWebEngineView 的 sizeHint/minimumSizeHint 抬高主窗口最小高度。"""

        def __init__(self, parent: QWidget | None = None) -> None:
            super().__init__(parent)
            self.setContextMenuPolicy(Qt.ContextMenuPolicy.NoContextMenu)

        def minimumSizeHint(self) -> QSize:  # type: ignore[override]
            return QSize(0, 0)

        def sizeHint(self) -> QSize:  # type: ignore[override]
            return QSize(0, 0)

else:
    _DeepslateWebEngineView = None  # type: ignore[misc, assignment]

# 与 design §2.6.3.5 / FR-R.6 及前端 ``lbaSetViewPreset(0..8)`` 一致
# 3×3：西北 北 东北 / 西 顶视 东 / 西南 南 东南
_VIEW_GRID: list[tuple[int, int, int, str, str]] = [
    (0, 0, 8, "西北", "西北（俯视）"),
    (0, 1, 1, "北", "北"),
    (0, 2, 5, "东北", "东北（俯视）"),
    (1, 0, 3, "西", "西"),
    (1, 1, 0, "顶视", "顶视图"),
    (1, 2, 4, "东", "东"),
    (2, 0, 7, "西南", "西南（俯视）"),
    (2, 1, 2, "南", "南"),
    (2, 2, 6, "东南", "东南（俯视）"),
]


def _view_dir_button_square_side(widget: QWidget) -> int:
    """由当前样式与字体推导方形边长，避免写死像素。"""
    st = widget.style()
    fm = widget.fontMetrics()
    fallback = max(fm.height() * 2 + 8, fm.horizontalAdvance("顶视") + 16)
    if st is None:
        return fallback
    opt = QStyleOptionButton()
    opt.initFrom(widget)
    opt.text = "顶视"
    opt.state = QStyle.StateFlag.State_Enabled
    sh = st.sizeFromContents(QStyle.ContentsType.CT_PushButton, opt, QSize(), widget)
    side = max(sh.width(), sh.height(), fm.height() + 8)
    return max(side, fallback)


def _web_resources_dir() -> Path:
    return Path(__file__).resolve().parents[2] / "resources" / "web"


def _viewer_html_path() -> Path:
    return _web_resources_dir() / "deepslate_viewer.html"


_EMPTY_VOXELS_B64 = base64.b64encode(
    json.dumps({"voxels": [], "camDist": 64}, separators=(",", ":")).encode("utf-8")
).decode("ascii")


class RenderCapturePreviewDialog(QDialog):
    """截屏 / 导出后的简易预览：可保存 PNG 或写入属性页预览图。"""

    _PREVIEW_MAX_EDGE_PX = 720

    def __init__(
        self,
        parent: QWidget | None,
        title: str,
        image: QImage,
        *,
        apply_preview: Callable[[QImage], None],
    ) -> None:
        super().__init__(parent)
        self.setWindowTitle(title)
        self._full_image = image
        self._apply_preview = apply_preview
        self._label = QLabel()
        self._label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setMinimumSize(420, 300)
        scroll.setWidget(self._label)
        btn_save = QPushButton("保存为 PNG…")
        btn_preview = QPushButton("写入预览图")
        btn_close = QPushButton("关闭")
        btn_row = QHBoxLayout()
        btn_row.addStretch(1)
        btn_row.addWidget(btn_save)
        btn_row.addWidget(btn_preview)
        btn_row.addWidget(btn_close)
        btn_row.addStretch(1)
        root = QVBoxLayout(self)
        root.addWidget(scroll, 1)
        root.addLayout(btn_row)
        btn_save.clicked.connect(self._on_save_png)
        btn_preview.clicked.connect(self._on_write_preview)
        btn_close.clicked.connect(self.reject)
        self._refresh_thumbnail()

    def _refresh_thumbnail(self) -> None:
        img = self._full_image
        if img.isNull():
            self._label.setText("（无图像）")
            return
        max_e = self._PREVIEW_MAX_EDGE_PX
        w, h = img.width(), img.height()
        if max(w, h) <= max_e:
            self._label.setPixmap(QPixmap.fromImage(img))
            return
        if w >= h:
            nw = max_e
            nh = max(1, round(h * max_e / w))
        else:
            nh = max_e
            nw = max(1, round(w * max_e / h))
        thumb = img.scaled(
            nw,
            nh,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        self._label.setPixmap(QPixmap.fromImage(thumb))

    def _on_save_png(self) -> None:
        if self._full_image.isNull():
            QMessageBox.warning(self, "保存", "没有可保存的图像。")
            return
        path, _ = QFileDialog.getSaveFileName(
            self,
            "保存 PNG",
            str(Path.home() / "render_capture.png"),
            "PNG (*.png)",
        )
        if not path:
            return
        if not path.lower().endswith(".png"):
            path += ".png"
        if not self._full_image.save(path, "PNG"):
            QMessageBox.warning(self, "保存", "保存失败。")
            return
        QMessageBox.information(self, "保存", f"已保存：\n{path}")

    def _on_write_preview(self) -> None:
        if self._full_image.isNull():
            QMessageBox.warning(self, "预览图", "没有可写入的图像。")
            return
        self._apply_preview(self._full_image)
        QMessageBox.information(
            self,
            "预览图",
            "已写入属性页预览（内存）。请到「属性」页确认并保存文件以写入 PreviewImageData。",
        )


def _effective_camera_fov_for_slider(raw: int) -> int:
    """磁吸：刻度 1–14 视为 0°，15–29 视为 30°；其它为实际值。"""
    r = int(raw)
    if 1 <= r <= 14:
        return 0
    if 15 <= r <= 29:
        return 30
    return r


class _VoxelPayloadThread(QThread):
    result_ready = Signal(object, str)  # dict | None, err

    def __init__(self, path: Path, region_name: str | None, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._path = path.resolve()
        self._region_name = region_name

    def run(self) -> None:  # type: ignore[override]
        try:
            payload, err = build_region_voxels_payload(self._path, self._region_name)
        except Exception as exc:
            self.result_ready.emit(None, str(exc))
            return
        if err:
            self.result_ready.emit(None, err)
            return
        self.result_ready.emit(payload, "")


class _NbtRawFileThread(QThread):
    """读取整份 .litematic 原始字节，供前端 Deepslate ``NbtFile.read``（与 vscode-nbt 一致）。"""

    result_ready = Signal(str, str)  # b64, err

    def __init__(self, path: Path, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._path = path.resolve()

    def run(self) -> None:  # type: ignore[override]
        try:
            raw = self._path.read_bytes()
        except OSError as exc:
            self.result_ready.emit("", str(exc))
            return
        self.result_ready.emit(base64.b64encode(raw).decode("ascii"), "")


class RenderPage(QWidget):
    """3D 预览：vscode-nbt 同源（若资源齐全）或 Deepslate 体素回退。"""

    def __init__(
        self,
        properties_page: PropertiesPage,
        *,
        app_settings: AppSettings | None = None,
        material_scan_prewarmer: MaterialListScanPrewarmer | None = None,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._props = properties_page
        self._material_scan_prewarmer = material_scan_prewarmer
        self._deepslate_invert_y: bool = (
            bool(app_settings.deepslate_invert_y) if app_settings is not None else False
        )
        self._nbt_camera_debug: bool = (
            bool(app_settings.nbt_viewer_camera_debug) if app_settings is not None else False
        )
        self._nbt_large_structure_threshold: int = (
            int(app_settings.nbt_viewer_large_structure_threshold)
            if app_settings is not None
            else 48 * 48 * 48
        )
        self._nbt_applied_mcmeta_version = ""
        if app_settings is not None:
            self._nbt_applied_mcmeta_version = (app_settings.nbt_mcmeta_target_version or "").strip()
        self._cache_nbt_export_params_from_settings(
            app_settings if app_settings is not None else AppSettings()
        )
        self._nbt_html_path, self._nbt_viewer_mode = resolve_nbt_viewer_html_path(
            self._nbt_applied_mcmeta_version
        )
        self._use_nbt_viewer: bool = bool(_HAS_WEBENGINE and self._nbt_html_path is not None)
        self._thread: _VoxelPayloadThread | None = None
        self._nbt_thread: _NbtRawFileThread | None = None
        self._load_superseded: bool = False
        self._pending_b64: str | None = None
        self._pending_nbt_b64: str | None = None
        self._viewer_ready: bool = False

        self._lbl_path = QLabel("请在「属性」页加载 .litematic。")
        self._lbl_path.setWordWrap(True)

        self._scheme_strip = QWidget()
        scheme_row = QHBoxLayout(self._scheme_strip)
        scheme_row.addWidget(QLabel("渲染方案："))
        self._scheme_combo = QComboBox()
        self._scheme_combo.addItem("deepslate", "deepslate")
        self._scheme_combo.addItem("nbt-viewer", "nbt-viewer")
        self._scheme_combo.currentIndexChanged.connect(self._on_scheme_changed)
        scheme_row.addWidget(self._scheme_combo)
        scheme_row.addStretch(1)

        self._region_strip = QWidget()
        reg_row = QHBoxLayout(self._region_strip)
        reg_row.addWidget(QLabel("选择区域："))
        self._region_combo = QComboBox()
        self._region_combo.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self._region_combo.currentIndexChanged.connect(self._on_region_changed)
        reg_row.addWidget(self._region_combo, 1)

        self._dir_box = QGroupBox("渲染设置")
        dir_outer = QVBoxLayout(self._dir_box)
        self._btn_refresh = QPushButton("重新加载3D")
        self._btn_refresh.clicked.connect(self._on_refresh_clicked)
        dir_outer.addWidget(self._btn_refresh)

        dir_center_row = QHBoxLayout()
        dir_center_row.addStretch(1)
        grid_host = QWidget()
        grid = QGridLayout(grid_host)
        grid.setSpacing(-1)
        st = self.style()
        if st is not None:
            sp = st.pixelMetric(QStyle.PixelMetric.PM_LayoutHorizontalSpacing, None, self)
            if sp > 0:
                grid.setHorizontalSpacing(sp)
                grid.setVerticalSpacing(sp)
        side = _view_dir_button_square_side(self)
        self._view_dir_buttons: list[QPushButton] = []
        self._last_view_preset = 0
        for row, col, preset_id, text, tip in _VIEW_GRID:
            btn = QPushButton(text)
            btn.setToolTip(tip)
            btn.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
            btn.setFixedSize(side, side)
            pid = int(preset_id)
            btn.clicked.connect(lambda _checked=False, p=pid: self._on_view_preset_button(p))
            self._view_dir_buttons.append(btn)
            grid.addWidget(btn, row, col)
        dir_center_row.addWidget(grid_host, 0, Qt.AlignmentFlag.AlignHCenter)
        dir_center_row.addStretch(1)
        dir_outer.addWidget(QLabel("观察方向"))
        dir_outer.addLayout(dir_center_row)

        fov_label = QLabel("视角 FOV（0°=正交）")
        dir_outer.addWidget(fov_label)

        fov_row = QHBoxLayout()
        self._fov_slider = QSlider(Qt.Orientation.Horizontal)
        self._fov_slider.setRange(0, 110)
        self._fov_slider.setValue(70)
        self._fov_slider.setTracking(True)
        self._fov_slider.setTickPosition(QSlider.TickPosition.TicksBelow)
        self._fov_slider.setTickInterval(10)
        self._fov_slider.valueChanged.connect(self._on_fov_slider_changed)
        self._fov_slider.sliderReleased.connect(self._on_fov_slider_released)
        self._fov_value_label = QLabel("70")
        self._fov_value_label.setMinimumWidth(32)
        fov_row.addWidget(self._fov_slider, 1)
        fov_row.addWidget(self._fov_value_label)
        dir_outer.addLayout(fov_row)

        self._btn_reset_camera = QPushButton("恢复默认视角")
        self._btn_reset_camera.setToolTip(
            "将 NBT 3D 相机恢复为加载后的默认 cRot、cPos、cDist（与 StructureEditor 初始轨道一致）"
        )
        self._btn_reset_camera.clicked.connect(self._on_reset_nbt_default_camera)
        self._btn_reset_camera.setVisible(self._use_nbt_viewer)
        dir_outer.addWidget(self._btn_reset_camera)

        btn_row = QHBoxLayout()
        self._btn_material = QPushButton("材料列表（当前区域）")
        self._btn_material.clicked.connect(self._on_material_list)
        self._btn_screenshot = QPushButton("截屏…")
        self._btn_screenshot.setToolTip("截取当前 3D 视图画布，在预览窗口中可保存或写入属性预览图")
        self._btn_screenshot.clicked.connect(self._on_screenshot_capture)
        self._btn_export_full = QPushButton("导出…")
        self._btn_export_full.setToolTip(
            "完整入镜导出：保持当前水平/仰角（cRot），将观察目标对准结构中心并拉远距离，使整体尽量落在画面内；"
            "分辨率与当前视口一致，需要更清晰时请拉大渲染区窗口。"
            " 透视与正交（FOV=0°）下的距离启发式可在「选项」—「NBT 3D — 完整入镜导出」中调整。"
        )
        self._btn_export_full.clicked.connect(self._on_full_export_capture)
        btn_row.addWidget(self._btn_material)
        btn_row.addWidget(self._btn_screenshot)
        btn_row.addWidget(self._btn_export_full)
        btn_row.addStretch()

        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setStyleSheet("color: palette(mid);")

        root = QVBoxLayout(self)
        root.addWidget(self._lbl_path)
        root.addWidget(self._scheme_strip)

        content_row = QHBoxLayout()
        left_col = QVBoxLayout()
        left_col.addWidget(self._region_strip)
        left_col.addLayout(btn_row)
        left_col.addWidget(QLabel("渲染界面"))
        left_col.addWidget(self._status)
        content_row.addLayout(left_col, 3)
        right_col = QVBoxLayout()
        right_col.addWidget(self._dir_box, 0, Qt.AlignmentFlag.AlignTop)
        right_col.addStretch(1)
        content_row.addLayout(right_col, 2)
        root.addLayout(content_row, 1)

        if not _HAS_WEBENGINE or QWebEngineView is None:
            tip = QLabel(
                "未安装 Qt WebEngine 组件，无法显示 3D 预览。\n"
                "请安装与当前 PySide6 版本匹配的 PySide6-WebEngine（或完整 Qt WebEngine 包）。"
            )
            tip.setWordWrap(True)
            tip.setAlignment(Qt.AlignmentFlag.AlignTop)
            root.addWidget(tip, 1)
            self._view = None
        else:
            assert _DeepslateWebEngineView is not None
            self._view = _DeepslateWebEngineView()
            self._view.setMinimumSize(0, 0)
            self._view.setSizePolicy(
                QSizePolicy.Policy.Expanding,
                QSizePolicy.Policy.Expanding,
            )
            self._view.loadFinished.connect(self._on_view_load_finished)
            if self._use_nbt_viewer:
                html = self._nbt_html_path
            else:
                html = _viewer_html_path()
            if html is not None and html.is_file():
                self._view.load(QUrl.fromLocalFile(str(html.resolve())))
            else:
                self._status.setText(f"缺少内置页面：{html}")
            left_col.addWidget(self._view, 1)

        self._sync_scheme_combo()
        self._props.active_file_changed.connect(self._on_active_file_changed)

    def _sync_scheme_combo(self) -> None:
        nbt_ok = bool(_HAS_WEBENGINE and self._nbt_html_path is not None)
        deepslate_ok = bool(_HAS_WEBENGINE and _viewer_html_path().is_file())
        m = self._scheme_combo.model()
        if m is not None:
            nbt_item = m.item(1)
            if nbt_item is not None:
                nbt_item.setEnabled(nbt_ok)
            deepslate_item = m.item(0)
            if deepslate_item is not None:
                deepslate_item.setEnabled(deepslate_ok)
        want = "nbt-viewer" if self._use_nbt_viewer else "deepslate"
        idx = self._scheme_combo.findData(want)
        if idx >= 0:
            self._scheme_combo.blockSignals(True)
            self._scheme_combo.setCurrentIndex(idx)
            self._scheme_combo.blockSignals(False)

    def _selected_scheme(self) -> str:
        d = self._scheme_combo.currentData()
        if isinstance(d, str) and d:
            return d
        return "deepslate"

    def _on_scheme_changed(self, _index: int) -> None:
        self._on_refresh_clicked()

    def _cache_nbt_export_params_from_settings(self, s: AppSettings) -> None:
        n = s.normalized()
        self._nbt_export_margin = n.nbt_export_full_margin
        self._nbt_export_perspective_min_distance = n.nbt_export_full_perspective_min_distance
        self._nbt_export_perspective_diag_extra = n.nbt_export_full_perspective_diag_extra
        self._nbt_export_orthographic_need_half_padding = n.nbt_export_full_orthographic_need_half_padding
        self._nbt_export_orthographic_height_scale = n.nbt_export_full_orthographic_height_scale
        self._nbt_export_orthographic_diag_extra = n.nbt_export_full_orthographic_diag_extra
        self._nbt_export_orthographic_min_distance = n.nbt_export_full_orthographic_min_distance
        self._nbt_export_orthographic_half_height_min = n.nbt_export_full_orthographic_half_height_min

    def _sync_export_full_params_js(self) -> None:
        if self._view is None or not self._viewer_ready or not self._use_nbt_viewer:
            return
        payload = {
            "margin": self._nbt_export_margin,
            "perspectiveMinDist": self._nbt_export_perspective_min_distance,
            "perspectiveDiagExtra": self._nbt_export_perspective_diag_extra,
            "orthographicNeedHalfPadding": self._nbt_export_orthographic_need_half_padding,
            "orthographicHeightScale": self._nbt_export_orthographic_height_scale,
            "orthographicDiagExtra": self._nbt_export_orthographic_diag_extra,
            "orthographicMinDist": self._nbt_export_orthographic_min_distance,
            "orthographicHalfHeightMin": self._nbt_export_orthographic_half_height_min,
        }
        self._view.page().runJavaScript(f"window.__lbaExportFullParams={json.dumps(payload)};")

    def _on_refresh_clicked(self) -> None:
        """重新解析 NBT 查看器入口（外部 data 或合并页），再加载当前投影。"""
        p, m = resolve_nbt_viewer_html_path(self._nbt_applied_mcmeta_version)
        nbt_ok = bool(_HAS_WEBENGINE and p is not None)
        use_nbt = self._selected_scheme() == "nbt-viewer"
        now_nbt = bool(use_nbt and nbt_ok)
        if use_nbt and not nbt_ok:
            self._status.setText("nbt-viewer 不可用，已回退到 deepslate。")
        if now_nbt != self._use_nbt_viewer:
            self._use_nbt_viewer = now_nbt
            self._btn_reset_camera.setVisible(now_nbt)
        self._nbt_html_path, self._nbt_viewer_mode = p, m
        if self._view is not None and _HAS_WEBENGINE:
            if now_nbt and p is not None and p.is_file():
                self._viewer_ready = False
                self._view.load(QUrl.fromLocalFile(str(p.resolve())))
            elif not now_nbt:
                html = _viewer_html_path()
                if html.is_file():
                    self._viewer_ready = False
                    self._view.load(QUrl.fromLocalFile(str(html.resolve())))
        self._sync_scheme_combo()
        self._schedule_load()

    def apply_deepslate_settings(self, s: AppSettings) -> None:
        """由主窗口在选项变更时调用：同步 mcmeta 应用版本与 Deepslate 纵向拖拽。"""
        self._deepslate_invert_y = bool(s.deepslate_invert_y)
        nv = (s.nbt_mcmeta_target_version or "").strip()
        if nv != self._nbt_applied_mcmeta_version:
            self._nbt_applied_mcmeta_version = nv
            self._on_refresh_clicked()
            return
        self._nbt_camera_debug = bool(s.nbt_viewer_camera_debug)
        self._nbt_large_structure_threshold = int(s.nbt_viewer_large_structure_threshold)
        self._cache_nbt_export_params_from_settings(s)
        if self._use_nbt_viewer:
            self._push_nbt_camera_debug()
            self._sync_nbt_large_structure_threshold_js()
            self._sync_export_full_params_js()
        else:
            self._push_invert_y_to_webview()

    def _push_invert_y_to_webview(self) -> None:
        if self._view is None or not self._viewer_ready:
            return
        lit = "true" if self._deepslate_invert_y else "false"
        self._view.page().runJavaScript(f"window.lbaSetInvertY({lit});")

    def _prime_nbt_camera_debug_global(self) -> None:
        """在注入 NBT 之前写入全局标志，供 StructureEditor.reveal 读取（避免仅依赖 JS 队列顺序）。"""
        if self._view is None or not self._viewer_ready or not self._use_nbt_viewer:
            return
        on = "true" if self._nbt_camera_debug else "false"
        self._view.page().runJavaScript(f"window.__lbaNbtCameraDebug = {on};")

    def _push_nbt_camera_debug(self) -> None:
        if self._view is None or not self._viewer_ready or not self._use_nbt_viewer:
            return
        on = "true" if self._nbt_camera_debug else "false"
        self._view.page().runJavaScript(f"window.lbaSetNbtCameraDebug({on});")

    def _sync_nbt_large_structure_threshold_js(self) -> None:
        if self._view is None or not self._viewer_ready or not self._use_nbt_viewer:
            return
        value = max(1_000, min(1_000_000_000, int(self._nbt_large_structure_threshold)))
        self._view.page().runJavaScript(f"window.__lbaLargeStructureThreshold={value};")

    def _current_view_preset(self) -> int:
        return max(0, min(8, int(self._last_view_preset)))

    def _on_view_preset_button(self, preset_id: int) -> None:
        self._last_view_preset = max(0, min(8, int(preset_id)))
        self._sync_view_preset_js()

    def _sync_view_preset_js(self) -> None:
        if self._view is None or not self._viewer_ready:
            return
        p = self._current_view_preset()
        self._view.page().runJavaScript(f"window.lbaSetViewPreset({p});")

    def _sync_nbt_default_camera_js(self) -> None:
        if self._view is None or not self._viewer_ready or not self._use_nbt_viewer:
            return
        self._view.page().runJavaScript(
            "(function(){try{if(typeof window.lbaResetNbtDefaultCamera==='function')"
            "window.lbaResetNbtDefaultCamera();}catch(e){console.error(e);}})();"
        )

    def _on_reset_nbt_default_camera(self) -> None:
        self._sync_nbt_default_camera_js()

    def _on_fov_slider_changed(self, value: int) -> None:
        eff = _effective_camera_fov_for_slider(value)
        self._fov_value_label.setText(str(eff))
        self._sync_camera_fov_js()

    def _on_fov_slider_released(self) -> None:
        raw = int(self._fov_slider.value())
        if 1 <= raw <= 14:
            self._fov_slider.blockSignals(True)
            self._fov_slider.setValue(0)
            self._fov_slider.blockSignals(False)
            self._on_fov_slider_changed(0)
        elif 15 <= raw <= 29:
            self._fov_slider.blockSignals(True)
            self._fov_slider.setValue(30)
            self._fov_slider.blockSignals(False)
            self._on_fov_slider_changed(30)

    def _sync_camera_fov_js(self) -> None:
        if self._view is None or not self._viewer_ready:
            return
        raw = int(self._fov_slider.value())
        v = _effective_camera_fov_for_slider(raw)
        # NBT：StructureEditor 在页面内 rAF 读取该全局并刷新；Deepslate：每帧 tick 读取
        self._view.page().runJavaScript(f"window.__lbaCameraFov={v};")

    def _deferred_sync_view_after_nbt_inject(self) -> None:
        """NBT 注入后：轨道相机恢复为 StructureEditor 初始 cRot/cPos/cDist（与「恢复默认视角」一致）。"""
        self._sync_nbt_default_camera_js()
        self._sync_camera_fov_js()
        self._sync_export_full_params_js()

    def _inject_b64(self, b64: str) -> None:
        if self._view is None or not self._viewer_ready:
            return
        self._view.page().runJavaScript(f"window.lbaLoadVoxelsB64({json.dumps(b64)});")

    def _inject_nbt_default_tree(self) -> None:
        if self._view is None or not self._viewer_ready:
            return
        empty = json.dumps(
            {
                "name": "",
                "root": {},
                "compression": "none",
                "littleEndian": False,
                "bedrockHeader": None,
            },
            separators=(",", ":"),
        )
        js = (
            "(function(){try{if(window.lbaDispatchNbtViewerInit){"
            f"window.lbaDispatchNbtViewerInit({{type:'default',readOnly:true,content:{empty}}});"
            "}}catch(e){console.error(e);}})();"
        )
        self._view.page().runJavaScript(js)

    def _inject_nbt_structure_b64(self, b64: str) -> None:
        if self._view is None or not self._viewer_ready:
            return
        b64_lit = json.dumps(b64)
        js = (
            "(function(){try{\n"
            f"var b64={b64_lit};"
            "var bin=atob(b64);"
            "var u8=new Uint8Array(bin.length);"
            "for(var i=0;i<bin.length;i++)u8[i]=bin.charCodeAt(i);"
            "var c=window.lbaReadNbtFileToJson(u8);"
            "window.lbaDispatchNbtViewerInit({type:'structure',readOnly:true,content:c});"
            "}catch(e){console.error(e);}"
            "})();"
        )
        self._view.page().runJavaScript(js)

    def _try_inject_pending(self) -> bool:
        if self._view is None or not self._viewer_ready:
            return False
        if self._use_nbt_viewer:
            if self._pending_nbt_b64 is None:
                return False
            pending = self._pending_nbt_b64
            self._pending_nbt_b64 = None
            self._last_view_preset = 0
            if pending == "":
                self._inject_nbt_default_tree()
            else:
                self._inject_nbt_structure_b64(pending)
            QTimer.singleShot(250, self._deferred_sync_view_after_nbt_inject)
            return True
        if self._pending_b64 is None:
            return False
        b64 = self._pending_b64
        self._pending_b64 = None
        self._last_view_preset = 0
        self._inject_b64(b64)
        self._sync_view_preset_js()
        self._sync_camera_fov_js()
        return True

    def _on_view_load_finished(self, ok: bool) -> None:
        self._viewer_ready = bool(ok)
        if ok:
            if self._use_nbt_viewer:
                self._prime_nbt_camera_debug_global()
                self._sync_nbt_large_structure_threshold_js()
            injected = self._try_inject_pending()
            if not self._use_nbt_viewer:
                self._last_view_preset = 0
                self._sync_view_preset_js()
                self._sync_camera_fov_js()
                self._push_invert_y_to_webview()
            else:
                self._sync_camera_fov_js()
                self._sync_export_full_params_js()
                if not injected and self._props.active_file_path() is None:
                    self._pending_nbt_b64 = ""
                    self._try_inject_pending()
        elif self._view is not None:
            self._status.setText("本地 Web 视图加载失败（请确认 resources/web 下 HTML 与 vendor 脚本齐全）。")

    def _sync_region_combo(self) -> None:
        self._region_combo.blockSignals(True)
        self._region_combo.clear()
        path = self._props.active_file_path()
        if path is None:
            self._region_combo.blockSignals(False)
            return
        self._region_combo.addItem("全部区域", None)
        try:
            ents = self._props.material_list_region_entries_for_active_file()
            if ents is not None:
                for display_name, source_key in ents:
                    self._region_combo.addItem(display_name, source_key)
            else:
                from litemapy import Schematic

                sch = Schematic.load(str(path))
                for k in sch.regions.keys():
                    self._region_combo.addItem(k, k)
        except Exception:
            pass
        self._region_combo.blockSignals(False)

    def _payload_region_name(self) -> str | None:
        """传给体素线程：``None`` 表示「全部区域合并」；非 ``None`` 为单区域名。"""
        d = self._region_combo.currentData()
        if d is None:
            return None
        return str(d)

    def _on_active_file_changed(self, _path: str) -> None:
        self._sync_path_label()
        self._sync_region_combo()
        self._schedule_load()

    def _on_region_changed(self, _index: int) -> None:
        self._schedule_load()

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        self._sync_path_label()
        if self._region_combo.count() == 0:
            self._sync_region_combo()
        if self._props.active_file_path() is not None:
            self._schedule_load()

    def _sync_path_label(self) -> None:
        p = self._props.active_file_path()
        self._lbl_path.setText(str(p) if p is not None else "请在「属性」页加载 .litematic。")

    def _schedule_load(self) -> None:
        path = self._props.active_file_path()
        if self._view is None:
            if path is None or self._region_combo.count() <= 0:
                self._status.setText("无可用文件或子区域。" if path is None else "该文件没有可加载的子区域。")
            else:
                self._status.setText("需要 Qt WebEngine 才能加载 3D 体素。")
            return

        if self._use_nbt_viewer:
            if path is None:
                self._pending_nbt_b64 = ""
                self._try_inject_pending()
                self._status.setText("无可用文件。")
                return
            resolved = path.resolve()
            if self._nbt_thread is not None and self._nbt_thread.isRunning():
                self._load_superseded = True
                self._status.setText("读取投影文件中…（将应用最新一次操作）")
                return
            self._load_superseded = False
            self._status.setText("读取投影文件中…")
            nth = _NbtRawFileThread(resolved, self)
            self._nbt_thread = nth
            nth.result_ready.connect(self._on_nbt_thread_result)
            nth.finished.connect(self._on_nbt_thread_finished)
            nth.start()
            return

        if path is None or self._region_combo.count() <= 0:
            self._pending_b64 = _EMPTY_VOXELS_B64
            self._try_inject_pending()
            self._status.setText("无可用文件或子区域。" if path is None else "该文件没有可加载的子区域。")
            return

        resolved = path.resolve()
        if self._thread is not None and self._thread.isRunning():
            self._load_superseded = True
            self._status.setText("准备体素数据中…（将应用最新一次选择）")
            return

        self._load_superseded = False
        self._status.setText("准备体素数据中…")
        th = _VoxelPayloadThread(resolved, self._payload_region_name(), self)
        self._thread = th
        th.result_ready.connect(self._on_thread_result)
        th.finished.connect(self._on_thread_finished)
        th.start()

    def _on_nbt_thread_finished(self) -> None:
        self._nbt_thread = None
        if self._load_superseded:
            self._load_superseded = False
            self._schedule_load()

    def _on_nbt_thread_result(self, b64: str, err: str) -> None:
        if self._props.active_file_path() is None:
            return
        if self._view is None:
            return
        if err:
            self._pending_nbt_b64 = ""
            self._try_inject_pending()
            self._status.setText(err)
            QMessageBox.warning(self, "NBT 预览", err)
            return
        self._pending_nbt_b64 = b64
        base = external_nbt_viewer_dir()
        v = self._nbt_applied_mcmeta_version.strip()
        if v:
            ver_path = base / v / "mcmeta" / "version.txt"
        else:
            ver_path = base / "mcmeta" / "version.txt"
        if not ver_path.is_file():
            ver_path = packaged_nbt_viewer_dir() / "mcmeta" / "version.txt"
        extra = ""
        if ver_path.is_file():
            try:
                extra = f"（mcmeta {ver_path.read_text(encoding='utf-8').strip()}）"
            except OSError:
                pass
        mode_hint = (
            f" [{self._nbt_viewer_mode}]"
            if self._nbt_viewer_mode in ("external", "merged")
            else ""
        )
        self._status.setText(f"已加载 vscode-nbt 同源 3D 预览{mode_hint}。{extra}".strip())
        self._try_inject_pending()

    def _on_thread_finished(self) -> None:
        self._thread = None
        if self._load_superseded:
            self._load_superseded = False
            self._schedule_load()

    def _on_thread_result(self, payload: dict | None, err: str) -> None:
        if self._props.active_file_path() is None:
            return
        if self._view is None:
            return
        if err:
            self._pending_b64 = _EMPTY_VOXELS_B64
            self._try_inject_pending()
            self._status.setText(err)
            QMessageBox.warning(self, "3D 渲染", err)
            return
        if payload is None:
            self._pending_b64 = _EMPTY_VOXELS_B64
            self._try_inject_pending()
            self._status.setText("无体素数据。")
            return

        note = str(payload.get("note", "") or "")
        wire: dict[str, object] = {
            "voxels": payload.get("voxels", []),
            "camDist": payload.get("camDist", 64),
        }
        raw = json.dumps(wire, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
        self._pending_b64 = base64.b64encode(raw).decode("ascii")
        extra = f" {note}" if note else ""
        self._status.setText(f"已加载 Deepslate 体素。{extra}".strip())
        self._try_inject_pending()

    @staticmethod
    def _qimage_from_png_data_url(result: object) -> QImage | None:
        if not result:
            return None
        s = str(result)
        prefix = "data:image/png;base64,"
        if not s.startswith(prefix):
            return None
        try:
            raw = base64.b64decode(s[len(prefix) :], validate=True)
        except Exception:
            return None
        img = QImage.fromData(raw, "PNG")
        if img.isNull():
            return None
        return img

    def _run_js_capture(self, mode: str, dialog_title: str) -> None:
        if self._view is None or not self._viewer_ready:
            QMessageBox.information(self, "渲染", "Web 视图未就绪，无法截取画布。")
            return
        # NBT：screen 为画布 toDataURL；full 为 StructureEditor._lbaExportPngDataUrlFull（同步字符串）。
        mode_lit = json.dumps(mode)
        if self._use_nbt_viewer:
            if mode == "full":
                js = (
                    "(function(){try{"
                    "var ed=window.__lbaNbtEditor;if(!ed||!ed.panels)return '';"
                    "var key=ed.type==='chunk'?'chunk':(ed.type==='structure'?'structure':null);"
                    "if(!key||!ed.panels[key])return '';"
                    "var se=ed.panels[key].editor();if(!se||!se.canvas)return '';"
                    "var isChunk=ed.type==='chunk';"
                    "if(typeof se._lbaExportPngDataUrlFull==='function')"
                    "return se._lbaExportPngDataUrlFull(isChunk);"
                    "return '';}catch(e){return '';}})()"
                )
            else:
                js = (
                    "(function(){var c=document.querySelector("
                    "'canvas.structure-3d:not(.click-detection)');"
                    "if(!c)return '';try{return c.toDataURL('image/png');}"
                    "catch(e){return '';}})()"
                )
        elif mode == "full":
            js = (
                "(function(){try{return window.lbaDeepslateCaptureRenderPng("
                f"{mode_lit}"
                ");}catch(e){return Promise.resolve('');}})()"
            )
        else:
            js = (
                "(function(){var c=document.getElementById('c');"
                "if(!c)return '';try{return c.toDataURL('image/png');}"
                "catch(e){return '';}})()"
            )

        def _done(res: object) -> None:
            img = self._qimage_from_png_data_url(res)
            if img is None:
                QMessageBox.warning(self, "渲染", "无法从画布读取图像。")
                return
            dlg = RenderCapturePreviewDialog(
                self,
                dialog_title,
                img,
                apply_preview=self._props.apply_render_as_preview,
            )
            dlg.exec()

        self._view.page().runJavaScript(js, _done)

    def _on_screenshot_capture(self) -> None:
        self._run_js_capture("screen", "截屏预览")

    def _on_full_export_capture(self) -> None:
        self._run_js_capture("full", "导出预览")

    def _on_material_list(self) -> None:
        if self._props.active_file_path() is None:
            QMessageBox.information(self, "材料列表", "请先在「属性」页打开一个投影文件。")
            return
        region = self._payload_region_name() if self._region_combo.count() > 0 else None
        MaterialListDialog.open_for_properties(
            self._props,
            self,
            initial_region_name=region,
            material_scan_prewarmer=self._material_scan_prewarmer,
        )
