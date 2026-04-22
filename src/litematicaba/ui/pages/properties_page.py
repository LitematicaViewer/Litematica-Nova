"""属性页（设计文档 §2.3）：Litematica 投影头部元数据的展示与编辑。

本模块实现「属性」标签页 UI，负责：
- 从 .litematic 文件加载 SNBT 中的 Metadata 相关字段（通过 ``SnbtProperties``）；
- 在表单中编辑可写字段（展示用 ``file_name``、Metadata ``Name``/作者/描述、预览图等），只读字段展示尺寸、版本、时间戳、**密度**等；
- 将修改写回原文件或「另存为」新路径。
- 根级 ``Regions``：表格展示各子区域名称（可编辑）、``Size``/``Position`` 的 x/y/z（只读）；保存时按行顺序重写 ``Regions`` 键名。

**布局：** 中间 ``QScrollArea`` 承载表单（stretch=1），底部 ``footer_bar`` 单独一行承载保存等按钮，按钮条不随内容滚动。

**密度（体积行）：** 非 SNBT 持久化字段，由 ``TotalBlocks / TotalVolume`` 派生，格式为一位小数的百分数，表示非空气方块占包围体素格数的比例；``TotalVolume<=0`` 时显示 ``-``。

**内部名称：** 对应 Metadata 的 ``Name`` 字符串，与「文件」分组中的显示用 ``file_name``（默认与磁盘文件名一致）语义不同。

状态约定：
- ``_loading``：为 True 时忽略 ``textChanged`` 触发的脏标记，避免程序化填充 UI 时误标为已修改；
- ``_dirty``：用户是否改动了相对磁盘上的当前内容；换文件前会据此弹出是否丢弃的确认框。
- ``_baseline_snapshot``：最近一次成功从磁盘加载后的元数据快照；「恢复默认值」将当前编辑还原为该快照（非清空表单）；**保存到原文件不会刷新快照**。
"""

from __future__ import annotations

import array
from datetime import datetime
from pathlib import Path

from PySide6.QtCore import Qt, QThread, QTimer, Signal
from PySide6.QtGui import QColor, QImage, QPainter, QPixmap, QResizeEvent, QShowEvent
from PySide6.QtWidgets import (
    QApplication,
    QComboBox,
    QDialog,
    QFileDialog,
    QFormLayout,
    QFrame,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QAbstractItemView,
    QInputDialog,
    QMessageBox,
    QPushButton,
    QHeaderView,
    QScrollArea,
    QSizePolicy,
    QStyle,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.projection_library import ProjectionLibraryError, ProjectionLibraryStore
from litematicaba.core.native_backend_bridge import (
    AnalyzeResult,
    LoadAnalyzeResult,
    load_and_analyze_litematic,
    open_embedded_viewer,
)
from litematicaba.core.settings import AppSettings, DEFAULT_LITEMATIC_ASYNC_LOAD_MIN_BYTES, RENDER_BUILD_MODE_NORMAL
from litematicaba.core.snbt_properties import (
    RegionInfo,
    SnbtProperties,
    copy_snbt_properties,
    regions_after_save_commit,
    save_snbt_properties,
    snbt_properties_from_dict,
)
from litematicaba.ui.theme import current_theme_id
from litematicaba.ui.widgets.themed_plain_table import ThemedPlainQTableWidget


# 超过该像素数则不构建界面预览图（仍保留 SNBT 中的原始列表供保存）
_MAX_PREVIEW_PIXELS_SHOW = 40_000


def _format_library_entry_path(path: str) -> str:
    raw = path.strip()
    if not raw:
        return "未设置原地址"
    try:
        return str(Path(raw))
    except OSError:
        return raw


class _PreviewCanvas(QFrame):
    """固定 140×140 的预览区域，将任意比例的 ``QPixmap`` 居中、等比缩放后绘制。

    无图时显示占位文案与半透明底，便于与正式预览区分。
    """

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("propertiesPreviewCanvas")
        self.setFrameShape(QFrame.Shape.Box)
        self.setFixedSize(140, 140)
        self._pixmap: QPixmap | None = None  # 当前要绘制的位图；None 表示占位状态

    def set_preview(self, pixmap: QPixmap | None) -> None:
        """更新预览内容并触发重绘。"""
        self._pixmap = pixmap
        self.update()

    def paintEvent(self, event) -> None:  # type: ignore[override]
        super().paintEvent(event)
        painter = QPainter(self)
        # 轻微暗底，使浅色预览边缘在浅色主题下仍可见
        painter.fillRect(self.rect(), QColor(20, 20, 20, 18))
        if self._pixmap is None or self._pixmap.isNull():
            painter.setPen(self.palette().mid().color())
            painter.drawText(self.rect(), Qt.AlignmentFlag.AlignCenter, "140 x 140")
            return
        # KeepAspectRatio：在 140×140 内完整显示，可能留边
        fitted = self._pixmap.scaled(
            self.size(),
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        x = (self.width() - fitted.width()) // 2
        y = (self.height() - fitted.height()) // 2
        painter.drawPixmap(x, y, fitted)


class _LitematicLoadWaitDialog(QDialog):
    """大文件加载提示：勿用 ``QProgressDialog`` 的 0–0 忙碌范围，默认 ``autoClose`` 会误判为已完成而瞬间关闭。"""

    def __init__(self, parent: QWidget | None, file_name: str, size_kb: float) -> None:
        super().__init__(parent)
        self.setWindowTitle("加载投影")
        self.setModal(True)
        self.setWindowModality(Qt.WindowModality.WindowModal)
        lay = QVBoxLayout(self)
        lbl = QLabel(f"正在加载…\n{file_name}\n约 {size_kb:.1f} KB")
        lbl.setWordWrap(True)
        lay.addWidget(lbl)
        btn = QPushButton("中断")
        lay.addWidget(btn)
        btn.clicked.connect(self.reject)


class _ProjectLoadWorker(QThread):
    """在 **子进程** 中执行 ``load_snbt_properties``，避免与 Qt 主进程争夺 CPython GIL 导致整窗未响应。

    ``QThread`` 仅负责 ``join`` 子进程与反序列化；主线程事件循环可继续处理重绘与加载对话框。
    """

    finished_ok = Signal(object)
    failed = Signal(str)

    def __init__(self, file_path: str | Path) -> None:
        super().__init__()
        self._file_path = Path(file_path)
        self._include_entities = True

    def abort_child_process(self) -> None:
        """从 UI 线程调用：终止仍在运行的子进程（例如用户点击「中断」）。"""
        self.requestInterruption()

    def run(self) -> None:  # type: ignore[override]
        ctx = multiprocessing.get_context("spawn")
        rq = ctx.Queue(maxsize=1)
        proc = ctx.Process(
            target=mp_load_snbt_properties_for_ui,
            args=(str(self._file_path.resolve()), rq),
        )
        with self._proc_lock:
            self._child_proc = proc
        proc.start()
        proc.join()
        exit_code = proc.exitcode
        with self._proc_lock:
            self._child_proc = None

        try:
            kind, payload = rq.get_nowait()
        except Empty:
            if exit_code == 0:
                self.failed.emit("加载失败（进程已结束但未返回数据）")
            else:
                self.failed.emit("加载已中断")
            return

        if kind == "err":
            self.failed.emit(str(payload))
            return

        try:
            data = pickle.loads(payload)
        except Exception as exc:
            self.failed.emit(f"反序列化失败：{exc}")
            return
        self.finished_ok.emit(data)


class _ProjectLoadAnalyzeWorker(QThread):
    finished_ok = Signal(object)
    failed = Signal(str)

    def __init__(self, file_path: str | Path, *, include_entities: bool = True) -> None:
        super().__init__()
        self._file_path = Path(file_path)
        self._include_entities = include_entities

    def abort_child_process(self) -> None:
        self.requestInterruption()

    def run(self) -> None:  # type: ignore[override]
        try:
            result = load_and_analyze_litematic(
                self._file_path,
                include_entities=self._include_entities,
            )
        except Exception as exc:
            self.failed.emit(str(exc))
            return
        if self.isInterruptionRequested():
            self.failed.emit("加载已中断")
            return
        self.finished_ok.emit(result)


class PropertiesPage(QWidget):
    """主属性页：中间内容为可滚动区域，底部操作按钮条固定在页面下沿（不参与滚动）。"""

    active_file_changed = Signal(str)
    analysis_ready = Signal(object)

    @staticmethod
    def _align_numeric_line_edit(w: QLineEdit) -> None:
        """尺寸、体积、版本号等纯数字只读框：文本右对齐便于纵列对比位数。"""
        w.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)

    def __init__(
        self,
        app_settings: AppSettings | None = None,
        projection_library: ProjectionLibraryStore | None = None,
    ) -> None:
        super().__init__()
        # 初次构建 UI 期间为 True，避免 setText 等触发 _mark_dirty
        self._projection_library = projection_library or ProjectionLibraryStore()
        self._app_settings = app_settings
        self._loading = True
        self._dirty = False
        self._current_data = SnbtProperties()
        # 内存中的预览图（ARGB）；与文件里 PreviewImageData 列表互转
        self._preview_image = QImage()
        # True：保存时用 ``_current_data.preview_image_data``（可能与界面缩略图分辨率不同）
        self._save_preview_from_model_list = False
        # 完整路径提示（含未保存星号）；显示用中间省略，完整内容在 ToolTip
        self._full_file_hint_text = ""
        self._display_file_path: Path | None = None
        self._display_file_name_override: str | None = None
        self._active_file_path: Path | None = None
        self._pending_file_path: Path | None = None
        # 成功 load 后的元数据副本，供「恢复默认值」撤销自打开以来的编辑（保存后亦不自动刷新此快照）
        self._baseline_snapshot: SnbtProperties | None = None
        # 异步打开 .litematic：递增序号丢弃过期线程结果；加载中禁用部分按钮并显示等待光标
        self._snbt_load_seq = 0
        self._snbt_load_busy = False
        self._snbt_load_wait_cursor_pushed = False
        self._load_thread: _ProjectLoadAnalyzeWorker | None = None
        self._snbt_progress_dialog: QDialog | None = None
        self._properties_load_state = "empty"
        # 异步加载大文件时，在路径标签上显示「加载中…」指向的目标（与 ``_current_data`` 可能尚未切换）
        self._async_load_target_path: Path | None = None
        self._litematic_async_load_min_bytes = (
            int(app_settings.litematic_async_load_min_bytes)
            if app_settings is not None
            else DEFAULT_LITEMATIC_ASYNC_LOAD_MIN_BYTES
        )
        self._projection_library_limit = int(
            app_settings.projection_library_limit if app_settings is not None else 20
        )
        self._render_cache_file: Path | None = None
        self._render_cache_source_file: Path | None = None
        self._properties_embedded_process = None
        self._properties_embedded_cache_file: Path | None = None

        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        scroll = QScrollArea(self)
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        scroll.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        # stretch=1：滚动区占满标题栏与页脚之间的剩余高度；页脚始终在可视区域底边
        root.addWidget(scroll, 1)

        body = QWidget()
        body.setMinimumWidth(0)
        scroll.setWidget(body)
        body_l = QVBoxLayout(body)
        body_l.setContentsMargins(16, 16, 16, 16)
        body_l.setSpacing(12)

        body_l.addLayout(self._build_file_select_row())
        body_l.addWidget(self._build_basic_meta_box())
        body_l.addLayout(self._build_meta_and_preview_row())
        body_l.addWidget(self._build_version_box())
        body_l.addWidget(self._build_regions_box())

        # 与 body 左右边距对齐；上 8px 与滚动内容留出缝隙，下 16px 贴窗口底
        footer_bar = QWidget()
        footer_lay = QVBoxLayout(footer_bar)
        footer_lay.setContentsMargins(16, 8, 16, 16)
        footer_lay.setSpacing(0)
        footer_lay.addLayout(self._build_footer_row())
        root.addWidget(footer_bar)

        self._wire_change_tracking()
        self._apply_model_to_ui(self._current_data)
        self._loading = False
        # 允许 QLineEdit 在窄布局下收缩，避免把整行撑得过宽
        self._apply_line_edit_horizontal_shrink()

    def apply_app_settings(self, s: AppSettings) -> None:
        """主窗口在选项变更时调用，更新投影加载阈值（字节）。"""
        self._app_settings = s
        self._litematic_async_load_min_bytes = int(s.normalized().litematic_async_load_min_bytes)
        self._projection_library_limit = int(s.normalized().projection_library_limit)

    def active_file_path(self) -> Path | None:
        """当前激活的投影文件路径；无文件时为 ``None``（供统计等模块读取）。"""
        return self._active_file_path

    def display_file_path(self) -> Path | None:
        return self._display_file_path or self._pending_file_path or self._active_file_path or self._current_data.file_path

    def set_render_cache(self, file_path: str | Path, cache_file: str | Path) -> None:
        active = self.active_file_path()
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
            self._render_cache_file = None
            self._sync_properties_render_preview()
            return
        self._render_cache_source_file = source
        self._render_cache_file = cache
        self._sync_properties_render_preview()

    def open_external_file(self) -> None:
        self._on_choose_external_file()

    def open_file_path(self, file_path: str | Path) -> None:
        if not self._confirm_discard_if_dirty():
            return
        self._clear_properties_render_preview()
        self._apply_projection_display_context(file_path)
        self._load_from_file(file_path)

    def _apply_projection_display_context(self, file_path: str | Path) -> None:
        path = Path(file_path)
        try:
            entry = self._projection_library.find_entry_by_backup_path(path)
        except ProjectionLibraryError:
            entry = None
        if entry is None:
            self._display_file_path = path
            self._display_file_name_override = path.name
            return
        original = entry.original_path
        self._display_file_path = original if original is not None else path
        self._display_file_name_override = entry.display_name or entry.original_file_name or path.name

    def material_list_region_entries_for_active_file(self) -> list[tuple[str, str]] | None:
        """材料列表子区域下拉用 ``(显示名, litemapy 区域键)``。

        与内存中当前模型、路径一致时返回，避免打开材料列表时再对大文件 ``Schematic.load``。
        若路径不一致或尚无模型则返回 ``None``（由对话框回退异步读取）。
        打开文件过程中返回 ``None``，避免材料列表沿用上一文件的区域列表。"""
        if self._snbt_load_busy:
            return None
        p = self.active_file_path()
        if p is None or self._current_data.file_path is None:
            return None
        try:
            if p.resolve() != Path(self._current_data.file_path).resolve():
                return None
        except OSError:
            return None
        if not self._current_data.regions:
            return []
        return [(r.name, r.source_key) for r in self._current_data.regions]

    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        # 路径标签宽度随窗口变化，需重新计算中间省略
        self._update_file_hint_elide()

    def closeEvent(self, event) -> None:  # type: ignore[override]
        self._stop_properties_embedded_viewer()
        super().closeEvent(event)

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        # Minecraft 主题下列较窄时 SNBT 表单易挤成一团，给左列设最小宽度
        self._apply_snbt_column_min_width()
        self.apply_regions_table_theme()
        # 首帧布局完成后宽度才稳定，延迟一次省略计算
        QTimer.singleShot(0, self._update_file_hint_elide)
        QTimer.singleShot(0, self._sync_properties_render_preview)

    def _clear_properties_render_preview(self) -> None:
        self._stop_properties_embedded_viewer()
        self._render_cache_file = None
        self._render_cache_source_file = None
        if hasattr(self, "_properties_render_container"):
            self._properties_render_container.hide()
        if hasattr(self, "_properties_render_placeholder"):
            self._properties_render_placeholder.setText(
                "渲染页还没有构建过 3D cache。构建完成后这里会显示固定摄像机的嵌入式 3D 预览。"
            )
            self._properties_render_placeholder.show()

    def _show_loading_state_for_file(self, path: Path) -> None:
        try:
            self._pending_file_path = path.resolve()
        except OSError:
            self._pending_file_path = path
        self._properties_load_state = "loading"
        self._loading = True
        self._file_name_edit.setText(self._display_file_name_override or path.name)
        self._internal_name_edit.setText("加载中…")
        self._author_edit.setText("")
        self._description_edit.setText("")
        for w in (
            self._created_time_readonly,
            self._modified_time_readonly,
            self._size_x_readonly,
            self._size_y_readonly,
            self._size_z_readonly,
            self._total_blocks_readonly,
            self._total_volume_readonly,
            self._density_readonly,
            self._litematica_ver_readonly,
            self._mc_data_ver_readonly,
        ):
            w.setText("…")
        self._on_clear_preview_no_dirty()
        self._regions_table.blockSignals(True)
        self._regions_table.setRowCount(0)
        self._regions_table.blockSignals(False)
        self._regions_empty_hint.setText("正在读取投影属性…")
        self._regions_empty_hint.show()
        self._full_file_hint_text = f"加载中… {path.resolve()}"
        self._active_file_hint.setToolTip(self._full_file_hint_text)
        self._update_file_hint_elide()
        self._clear_properties_render_preview()
        self._loading = False

    def _show_load_failed_state(self, path: Path | None, message: str) -> None:
        self._properties_load_state = "failed"
        self._pending_file_path = None
        self._loading = True
        target = path or self._async_load_target_path or self.display_file_path()
        self._file_name_edit.setText(target.name if isinstance(target, Path) else "")
        self._internal_name_edit.setText("加载失败")
        self._author_edit.setText("")
        self._description_edit.setText(str(message))
        for w in (
            self._created_time_readonly,
            self._modified_time_readonly,
            self._size_x_readonly,
            self._size_y_readonly,
            self._size_z_readonly,
            self._total_blocks_readonly,
            self._total_volume_readonly,
            self._density_readonly,
            self._litematica_ver_readonly,
            self._mc_data_ver_readonly,
        ):
            w.setText("-")
        self._on_clear_preview_no_dirty()
        self._regions_table.blockSignals(True)
        self._regions_table.setRowCount(0)
        self._regions_table.blockSignals(False)
        self._regions_empty_hint.setText(f"投影属性加载失败：{message}")
        self._regions_empty_hint.show()
        self._clear_properties_render_preview()
        self._loading = False

    def _sync_properties_render_preview(self) -> None:
        if not hasattr(self, "_properties_render_container"):
            return
        active = self.active_file_path()
        cache = self._render_cache_file
        if active is None or cache is None or not cache.is_file():
            self._stop_properties_embedded_viewer()
            self._properties_render_container.hide()
            self._properties_render_placeholder.show()
            return
        if not self.isVisible():
            self._stop_properties_embedded_viewer()
            self._properties_render_container.hide()
            self._properties_render_placeholder.setText("3D cache 已完成。打开属性页后会显示嵌入式 3D 预览。")
            self._properties_render_placeholder.show()
            return
        if (
            self._properties_embedded_process is not None
            and self._properties_embedded_process.poll() is None
            and self._properties_embedded_cache_file is not None
            and self._properties_embedded_cache_file.resolve() == cache.resolve()
        ):
            self._properties_render_placeholder.hide()
            self._properties_render_container.show()
            return
        self._stop_properties_embedded_viewer()
        try:
            self._properties_embedded_process = open_embedded_viewer(
                active,
                cache,
                parent_hwnd=int(self._properties_render_container.winId()),
                preview_mode=True,
                preview_spin=True,
                display_mode=getattr(self._app_settings, "render_build_mode", RENDER_BUILD_MODE_NORMAL),
            )
            self._properties_embedded_cache_file = cache.resolve()
        except Exception as exc:
            self._properties_render_container.hide()
            self._properties_render_placeholder.setText(f"嵌入式 3D 预览启动失败：{exc}")
            self._properties_render_placeholder.show()
            return
        self._properties_render_placeholder.hide()
        self._properties_render_container.show()

    def _stop_properties_embedded_viewer(self) -> None:
        process = self._properties_embedded_process
        self._properties_embedded_process = None
        self._properties_embedded_cache_file = None
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except Exception:
                try:
                    process.kill()
                except Exception:
                    pass

    def apply_regions_table_theme(self) -> None:
        """与 UI 测试页内容列表一致，随全局主题刷新区域表样式与行高。"""
        app = QApplication.instance()
        if app is None:
            return
        self._regions_table.apply_theme(current_theme_id(app))
        self._regions_table.sync_row_heights()

    # -------------------------------------------------------------------------
    # UI 构建：自上而下与主窗口 body 结构一致
    # -------------------------------------------------------------------------

    def _build_file_select_row(self) -> QHBoxLayout:
        row = QHBoxLayout()
        self._btn_choose_external = QPushButton("选择文件...")
        self._btn_choose_library = QPushButton("在库中选择...")
        self._active_file_hint = QLabel("当前未激活文件")
        self._active_file_hint.setStyleSheet("color: palette(mid);")
        self._active_file_hint.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Preferred)
        row.addWidget(self._btn_choose_external)
        row.addWidget(self._btn_choose_library)
        row.addWidget(self._active_file_hint, 1)

        self._btn_choose_external.clicked.connect(self._on_choose_external_file)
        self._btn_choose_library.clicked.connect(self._on_choose_from_library)
        return row

    def _build_basic_meta_box(self) -> QGroupBox:
        """「文件名称」：UI 展示用名，当前实现中与加载路径的 ``path.name`` 同步，**不是** Metadata ``Name``。

        真正的内部标识见 SNBT 分组中的「内部名称」（``internal_name`` ↔ ``Metadata.Name``）。
        """
        box = QGroupBox("文件")
        form = QFormLayout(box)
        self._file_name_edit = QLineEdit()
        self._file_name_edit.setPlaceholderText("文件名称")
        form.addRow("文件名称：", self._file_name_edit)
        return box

    def _build_meta_and_preview_row(self) -> QHBoxLayout:
        """左侧 SNBT 表单（stretch 2）+ 右侧预览（stretch 1），横向并排。"""
        row = QHBoxLayout()
        row.setSpacing(12)
        self._snbt_column = QWidget()
        self._snbt_column.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Preferred)
        self._snbt_column.setMinimumWidth(0)
        snbt_outer = QVBoxLayout(self._snbt_column)
        snbt_outer.setContentsMargins(0, 0, 0, 0)
        snbt_outer.addWidget(self._build_snbt_box())
        preview_box = self._build_embedded_render_box()
        preview_box.setSizePolicy(QSizePolicy.Policy.Preferred, QSizePolicy.Policy.Preferred)
        row.addWidget(self._snbt_column, 2)
        row.addWidget(preview_box, 1)
        return row

    def _build_embedded_render_box(self) -> QGroupBox:
        box = QGroupBox("3D 渲染预览")
        lay = QVBoxLayout(box)
        lay.setSpacing(8)
        self._properties_render_placeholder = QLabel("渲染页还没有构建过 3D cache。构建完成后这里会显示固定摄像机的嵌入式 3D 预览。")
        self._properties_render_placeholder.setWordWrap(True)
        self._properties_render_placeholder.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._properties_render_placeholder.setStyleSheet("color: palette(mid);")
        self._properties_render_container = QWidget()
        self._properties_render_container.setObjectName("propertiesEmbeddedRenderContainer")
        self._properties_render_container.setMinimumSize(220, 180)
        self._properties_render_container.hide()
        lay.addWidget(self._properties_render_placeholder, 1)
        lay.addWidget(self._properties_render_container, 1)
        return box

    def _build_snbt_box(self) -> QGroupBox:
        """Metadata 主表单。

        写回 ``save_snbt_properties`` 的字符串字段由本分组的可编辑行提供：``Name``/``Author``/``Description``（及预览像素数组，见预览区）。
        「文件」分组里的 ``file_name`` 属独立控件，**无对应 Metadata 键**，另存为默认名等使用 ``SnbtProperties.file_name``。
        只读：时间、包围尺寸、``TotalBlocks``/``TotalVolume``、**密度**（两统计量比值，不落盘）。
        """
        box = QGroupBox("SNBT 元数据")
        form = QFormLayout(box)

        # Litematica Metadata.Name：游戏/材料列表等使用的内部名，可与磁盘文件名不同
        self._internal_name_edit = QLineEdit()
        self._author_edit = QLineEdit()
        self._description_edit = QLineEdit()
        self._created_time_readonly = QLineEdit()
        self._modified_time_readonly = QLineEdit()
        self._size_x_readonly = QLineEdit()
        self._size_y_readonly = QLineEdit()
        self._size_z_readonly = QLineEdit()
        self._total_blocks_readonly = QLineEdit()
        self._total_volume_readonly = QLineEdit()
        # 占用率 = TotalBlocks/TotalVolume，仅展示；保存时不写入 NBT（无对应键）
        self._density_readonly = QLineEdit()

        # 时间、包围盒尺寸、方块/总计/密度等由文件解析结果派生，用户不可在此直接改 SNBT 统计字段
        self._created_time_readonly.setReadOnly(True)
        self._modified_time_readonly.setReadOnly(True)
        self._size_x_readonly.setReadOnly(True)
        self._size_y_readonly.setReadOnly(True)
        self._size_z_readonly.setReadOnly(True)
        self._total_blocks_readonly.setReadOnly(True)
        self._total_volume_readonly.setReadOnly(True)
        self._density_readonly.setReadOnly(True)
        for w in (
            self._size_x_readonly,
            self._size_y_readonly,
            self._size_z_readonly,
            self._total_blocks_readonly,
            self._total_volume_readonly,
            self._density_readonly,
        ):
            w.setMaximumWidth(120)
            self._align_numeric_line_edit(w)

        form.addRow("内部名称：", self._internal_name_edit)
        form.addRow("作者：", self._author_edit)
        form.addRow("描述：", self._description_edit)
        form.addRow("创建时间：", self._created_time_readonly)
        form.addRow("修改时间：", self._modified_time_readonly)
        form.addRow("尺寸：", self._build_size_row_widget())
        form.addRow("体积：", self._build_volume_row_widget())
        form.setHorizontalSpacing(8)
        return box

    def _build_size_row_widget(self) -> QWidget:
        w = QWidget()
        row = QHBoxLayout(w)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(6)
        row.addWidget(QLabel("x:"))
        row.addWidget(self._size_x_readonly)
        row.addWidget(QLabel("y:"))
        row.addWidget(self._size_y_readonly)
        row.addWidget(QLabel("z:"))
        row.addWidget(self._size_z_readonly)
        row.addStretch()
        return w

    def _build_volume_row_widget(self) -> QWidget:
        """单行展示 ``TotalBlocks``、``TotalVolume`` 及由二者计算的密度百分数。"""
        w = QWidget()
        row = QHBoxLayout(w)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(6)
        row.addWidget(QLabel("方块:"))
        row.addWidget(self._total_blocks_readonly)
        row.addWidget(QLabel("总计:"))
        row.addWidget(self._total_volume_readonly)
        row.addWidget(QLabel("密度:"))
        row.addWidget(self._density_readonly)
        row.addStretch()
        return w

    def _build_preview_box(self) -> QGroupBox:
        """PreviewImageData：导入外部图或从文件加载；缩放算法影响写入 140×140 时的采样方式。"""
        box = QGroupBox("预览图")
        lay = QVBoxLayout(box)
        lay.setSpacing(8)

        self._preview_canvas = _PreviewCanvas()
        self._preview_count_hint = QLabel("PreviewImageData: 0 项")
        self._preview_count_hint.setStyleSheet("color: palette(mid);")
        self._preview_sample_combo = QComboBox()
        self._preview_sample_combo.addItem("平滑（Smooth）", Qt.TransformationMode.SmoothTransformation)
        self._preview_sample_combo.addItem("邻近（Nearest）", Qt.TransformationMode.FastTransformation)
        self._btn_clear_preview = QPushButton("清空预览图")
        self._btn_import_preview = QPushButton("导入预览图")

        lay.addWidget(self._preview_canvas, 0, Qt.AlignmentFlag.AlignHCenter)
        lay.addWidget(self._preview_count_hint)
        lay.addWidget(QLabel("缩放采样方式："))
        lay.addWidget(self._preview_sample_combo)
        lay.addWidget(self._btn_clear_preview)
        lay.addWidget(self._btn_import_preview)
        lay.addStretch()

        self._btn_clear_preview.clicked.connect(self._on_clear_preview)
        self._btn_import_preview.clicked.connect(self._on_import_preview)
        return box

    def _build_version_box(self) -> QGroupBox:
        """Litematica 文件格式版本与 Minecraft 数据版本，仅展示。"""
        box = QGroupBox("版本信息")
        form = QFormLayout(box)
        self._litematica_ver_readonly = QLineEdit()
        self._mc_data_ver_readonly = QLineEdit()
        self._litematica_ver_readonly.setReadOnly(True)
        self._mc_data_ver_readonly.setReadOnly(True)
        self._align_numeric_line_edit(self._litematica_ver_readonly)
        self._align_numeric_line_edit(self._mc_data_ver_readonly)
        form.addRow("投影文件版本：", self._litematica_ver_readonly)
        form.addRow("Minecraft 数据版本：", self._mc_data_ver_readonly)
        version_hint = QLabel("只读：来自当前 .litematic 文件；这里不是版本转换入口。")
        version_hint.setWordWrap(True)
        version_hint.setStyleSheet("color: palette(mid);")
        form.addRow("", version_hint)
        form.setHorizontalSpacing(8)
        return box

    def _build_regions_box(self) -> QGroupBox:
        """根级 ``Regions``：名称可编辑，尺寸与相对位置只读（设计文档 §2.3.4）。"""
        box = QGroupBox("区域列表")
        lay = QVBoxLayout(box)
        app = QApplication.instance()
        _tid = current_theme_id(app) if app is not None else "QTDefault"
        self._regions_table = ThemedPlainQTableWidget(theme_id=_tid)
        self._regions_table.setObjectName("PropertiesRegionTable")
        self._regions_table.setColumnCount(7)
        self._regions_table.setHorizontalHeaderLabels(
            [
                "区域名称（双击修改）",
                "尺寸 x",
                "尺寸 y",
                "尺寸 z",
                "位置 x",
                "位置 y",
                "位置 z",
            ]
        )
        name_header = self._regions_table.horizontalHeaderItem(0)
        if name_header is not None:
            name_header.setToolTip("双击该列单元格可修改区域名称；也可选中行后按 F2 进入编辑。")
        self._regions_table.verticalHeader().setVisible(False)
        self._regions_table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self._regions_table.setEditTriggers(
            QAbstractItemView.EditTrigger.DoubleClicked
            | QAbstractItemView.EditTrigger.SelectedClicked
            | QAbstractItemView.EditTrigger.EditKeyPressed
        )
        rh = self._regions_table.horizontalHeader()
        rh.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        for col in range(1, 7):
            rh.setSectionResizeMode(col, QHeaderView.ResizeMode.ResizeToContents)
        self._regions_table.setMinimumHeight(180)
        self._regions_table.itemChanged.connect(self._on_regions_item_changed)
        lay.addWidget(self._regions_table)
        self._regions_empty_hint = QLabel("当前文件无子区域，或 Regions 无法解析。")
        self._regions_empty_hint.setStyleSheet("color: palette(mid);")
        self._regions_empty_hint.setWordWrap(True)
        lay.addWidget(self._regions_empty_hint)
        return box

    def _build_footer_row(self) -> QHBoxLayout:
        """保存 / 另存为 / 恢复为打开文件时的快照；转换格式尚未实现。

        由 ``__init__`` 放入 ``PropertiesPage`` 根布局底部，置于 ``QScrollArea`` 之外，故不随表单滚动。

        相邻按钮间距 = 当前样式 ``PM_LayoutHorizontalSpacing`` + 8px（该指标为 -1 时按 6px 计），避免 Metro10 / Minecraft 等大按钮主题下控件挤在一起。
        """
        row = QHBoxLayout()
        # 在主题默认水平间距上 +8px，避免高按钮样式（Metro10、Minecraft）下相邻按钮视觉粘连
        style = self.style()
        base = 6
        if style is not None:
            pm = style.pixelMetric(QStyle.PixelMetric.PM_LayoutHorizontalSpacing)
            if pm >= 0:
                base = pm
        row.setSpacing(base + 8)
        row.addStretch()
        self._btn_save = QPushButton("保存")
        self._btn_save_as = QPushButton("另存为")
        self._btn_restore = QPushButton("恢复默认值")
        self._btn_convert = QPushButton("转换格式")
        self._btn_convert.setEnabled(False)

        self._btn_save.clicked.connect(self._on_save)
        self._btn_save_as.clicked.connect(self._on_save_as)
        self._btn_restore.clicked.connect(self._on_restore_defaults)
        self._btn_convert.clicked.connect(lambda: self._show_not_implemented("转换格式"))

        row.addWidget(self._btn_save)
        row.addWidget(self._btn_save_as)
        row.addWidget(self._btn_restore)
        row.addWidget(self._btn_convert)
        return row

    def _wire_change_tracking(self) -> None:
        """仅对会写回 SNBT 的输入框挂接脏标记；只读控件不连接。"""
        self._file_name_edit.textChanged.connect(self._mark_dirty)
        self._internal_name_edit.textChanged.connect(self._mark_dirty)
        self._author_edit.textChanged.connect(self._mark_dirty)
        self._description_edit.textChanged.connect(self._mark_dirty)

    def _on_regions_item_changed(self, item: QTableWidgetItem) -> None:
        """仅「区域名称」列（第 0 列）的编辑触发脏标记。"""
        if item.column() != 0:
            return
        self._mark_dirty()

    def _apply_regions_table(self, data: SnbtProperties) -> None:
        """用模型填充区域表；在 ``_loading`` 为 True 时调用可避免触发 ``itemChanged`` 脏标记。"""
        self._regions_table.blockSignals(True)
        self._regions_table.setRowCount(0)
        ro = Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled
        editable = ro | Qt.ItemFlag.ItemIsEditable
        for ri, reg in enumerate(data.regions):
            self._regions_table.insertRow(ri)
            name_item = QTableWidgetItem(reg.name)
            name_item.setFlags(editable)
            self._regions_table.setItem(ri, 0, name_item)
            vals = (*reg.size, *reg.position)
            for ci, val in enumerate(vals, start=1):
                cell = QTableWidgetItem(str(val))
                cell.setFlags(ro)
                self._regions_table.setItem(ri, ci, cell)
        self._regions_table.blockSignals(False)
        self._regions_empty_hint.setText("当前文件无子区域，或 Regions 无法解析。")
        self._regions_empty_hint.setVisible(len(data.regions) == 0)
        self._regions_table.sync_row_heights()

    def _collect_regions_from_table(self) -> list[RegionInfo]:
        """行序与 ``_current_data.regions`` 对齐；只从第 0 列读取新名称，几何字段沿用模型。"""
        rows = self._regions_table.rowCount()
        base = self._current_data.regions
        if rows == 0:
            return []
        out: list[RegionInfo] = []
        for i in range(rows):
            name_item = self._regions_table.item(i, 0)
            name = name_item.text().strip() if name_item is not None else ""
            if i < len(base):
                b = base[i]
                out.append(RegionInfo(source_key=b.source_key, name=name, size=b.size, position=b.position))
            else:
                out.append(RegionInfo(source_key=name, name=name, size=(0, 0, 0), position=(0, 0, 0)))
        return out

    # -------------------------------------------------------------------------
    # 槽函数：文件选择、预览、保存
    # -------------------------------------------------------------------------

    def _on_choose_external_file(self) -> None:
        """通过系统对话框打开 .litematic；若有未保存修改先确认是否丢弃。"""
        if not self._confirm_discard_if_dirty():
            return
        path, _ = QFileDialog.getOpenFileName(
            self,
            "选择投影文件",
            "",
            "Litematic Files (*.litematic);;All Files (*.*)",
        )
        if not path:
            return
        try:
            entry = self._projection_library.import_projection(
                path,
                limit=self._projection_library_limit,
            )
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "导入失败", str(exc))
            return
        self._clear_properties_render_preview()
        self._apply_projection_display_context(entry.backup_file_path)
        self._load_from_file(entry.backup_file_path)

    def _on_choose_from_library(self) -> None:
        """项目内「库」选择器尚未接入。"""
        if not self._confirm_discard_if_dirty():
            return
        try:
            entries = self._projection_library.load_entries()
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        if not entries:
            QMessageBox.information(self, "投影库", "投影库为空，请先导入一个 .litematic 文件。")
            return
        labels = [
            f"{entry.display_name} | {_format_library_entry_path(entry.original_file_path)}"
            for entry in entries
        ]
        selected, ok = QInputDialog.getItem(
            self,
            "从投影库中选择",
            "投影",
            labels,
            0,
            False,
        )
        if not ok or not selected:
            return
        entry = entries[labels.index(selected)]
        try:
            backup_path = self._projection_library.open_entry_backup(entry.entry_id)
        except ProjectionLibraryError as exc:
            QMessageBox.critical(self, "投影库", str(exc))
            return
        self._clear_properties_render_preview()
        self._apply_projection_display_context(backup_path)
        self._load_from_file(backup_path)

    def _on_clear_preview(self) -> None:
        """清空内存预览与画布，并标记脏（将写入空的 PreviewImageData）。"""
        self._save_preview_from_model_list = False
        self._preview_image = QImage()
        if hasattr(self, "_preview_canvas"):
            self._preview_canvas.set_preview(None)
        if hasattr(self, "_preview_count_hint"):
            self._preview_count_hint.setText("PreviewImageData: 0 项")
        self._mark_dirty()

    def _on_import_preview(self) -> None:
        """居中裁成正方形后缩放到 140×140，与 Litematica 常见预览尺寸一致。"""
        path, _ = QFileDialog.getOpenFileName(
            self,
            "导入预览图",
            "",
            "Images (*.png *.jpg *.jpeg *.bmp *.webp);;All Files (*.*)",
        )
        if not path:
            return

        raw = QImage(path)
        if raw.isNull():
            QMessageBox.warning(self, "导入预览图", "图片读取失败，请更换文件后重试。")
            return

        # 取最短边为边长，从中心裁剪，避免非正方形原图被强行拉伸变形
        side = min(raw.width(), raw.height())
        crop_x = (raw.width() - side) // 2
        crop_y = (raw.height() - side) // 2
        square = raw.copy(crop_x, crop_y, side, side)
        mode = self._preview_sample_combo.currentData()
        if mode is None:
            mode = Qt.TransformationMode.SmoothTransformation
        self._preview_image = square.scaled(
            140,
            140,
            Qt.AspectRatioMode.IgnoreAspectRatio,
            mode,
        )
        pix = QPixmap.fromImage(self._preview_image)
        self._preview_canvas.set_preview(pix)
        # 固定 140×140 像素 → ARGB 列表长度恒为 19600
        self._preview_count_hint.setText(f"PreviewImageData: {140 * 140} 项")
        self._save_preview_from_model_list = False
        self._mark_dirty()

    def apply_render_as_preview(self, image: QImage) -> None:
        """由「渲染」页将当前正交图裁成正方形并缩放到 140×140，写入内存预览并标记脏。"""
        if image.isNull():
            return
        side = min(image.width(), image.height())
        crop_x = (image.width() - side) // 2
        crop_y = (image.height() - side) // 2
        square = image.copy(crop_x, crop_y, side, side)
        mode = self._preview_sample_combo.currentData()
        if mode is None:
            mode = Qt.TransformationMode.SmoothTransformation
        self._preview_image = square.scaled(
            140,
            140,
            Qt.AspectRatioMode.IgnoreAspectRatio,
            mode,
        )
        self._preview_canvas.set_preview(QPixmap.fromImage(self._preview_image))
        self._preview_count_hint.setText(f"PreviewImageData: {140 * 140} 项")
        self._save_preview_from_model_list = False
        self._mark_dirty()

    def _on_restore_defaults(self) -> None:
        """用 ``_baseline_snapshot`` 覆盖 ``_current_data`` 并刷新 UI。

        先置 ``_dirty=False`` 再 ``_apply_model_to_ui``，避免路径标签仍带未保存星号。
        快照仅在 ``_load_from_file`` 成功时更新；保存、编辑不会改写快照。
        """
        if self._current_data.file_path is None:
            QMessageBox.information(self, "恢复默认值", "请先加载一个投影文件。")
            return
        if self._baseline_snapshot is None:
            QMessageBox.information(self, "恢复默认值", "没有可用的加载快照，请重新打开该文件。")
            return
        self._loading = True
        self._dirty = False
        self._current_data = copy_snbt_properties(self._baseline_snapshot)
        self._apply_model_to_ui(self._current_data)
        self._loading = False

    def _on_save(self) -> None:
        """写回 ``_current_data.file_path``；成功后用新模型替换内存状态并清除脏标记。"""
        if self._current_data.file_path is None:
            QMessageBox.information(self, "保存", "请先选择一个 .litematic 文件。")
            return
        try:
            model = self._collect_ui_to_model()
            save_snbt_properties(model)
        except Exception as exc:
            QMessageBox.critical(self, "保存失败", f"写入 SNBT 失败：\n{exc}")
            return
        self._current_data = regions_after_save_commit(model)
        self._dirty = False
        self._sync_title_hint()
        QMessageBox.information(self, "保存", "已保存到原文件。")

    def _on_save_as(self) -> None:
        """写入用户选择的新路径，随后 ``_load_from_file`` 切换到该文件作为当前上下文。"""
        if self._current_data.file_path is None:
            QMessageBox.information(self, "另存为", "请先选择一个 .litematic 文件。")
            return
        default_name = self._file_name_edit.text().strip() or self._current_data.file_path.name
        out, _ = QFileDialog.getSaveFileName(
            self,
            "另存为",
            str(self._current_data.file_path.with_name(default_name)),
            "Litematic Files (*.litematic);;All Files (*.*)",
        )
        if not out:
            return
        try:
            model = self._collect_ui_to_model()
            written = save_snbt_properties(model, out)
        except Exception as exc:
            QMessageBox.critical(self, "另存为失败", f"写入 SNBT 失败：\n{exc}")
            return
        self._apply_projection_display_context(written)
        self._load_from_file(written)

    def _set_snbt_load_busy(self, busy: bool, *, wait_cursor: bool = True) -> None:
        """锁定部分按钮；``wait_cursor`` 为 True 时叠加等待光标（同步加载小文件用，异步大文件仅用对话框）。"""
        self._snbt_load_busy = busy
        if not busy:
            self._async_load_target_path = None
        for w in (
            self._btn_choose_external,
            self._btn_choose_library,
            self._btn_save,
            self._btn_save_as,
            self._btn_restore,
        ):
            w.setEnabled(not busy)
        app = QApplication.instance()
        if app is None:
            return
        if busy:
            if wait_cursor:
                app.setOverrideCursor(Qt.CursorShape.WaitCursor)
                self._snbt_load_wait_cursor_pushed = True
        else:
            if self._snbt_load_wait_cursor_pushed:
                app.restoreOverrideCursor()
                self._snbt_load_wait_cursor_pushed = False
        self._sync_title_hint()

    def _on_snbt_load_thread_finished(self) -> None:
        th = self.sender()
        if th is self._load_thread:
            self._load_thread = None

    def _close_snbt_progress_dialog(self) -> None:
        dlg = self._snbt_progress_dialog
        if dlg is None:
            return
        self._snbt_progress_dialog = None
        dlg.blockSignals(True)
        dlg.hide()
        dlg.deleteLater()

    def _commit_loaded_snbt_model(self, data: SnbtProperties) -> None:
        """将成功解析的模型写入内存并刷新 UI（同步/异步路径共用）。"""
        if data.file_path is None:
            raise ValueError("loaded projection is missing file_path")
        active_path = Path(data.file_path).resolve()
        data.file_path = active_path
        if self._display_file_name_override:
            data.file_name = self._display_file_name_override
        self._active_file_path = active_path
        self._pending_file_path = None
        self._current_data = data
        self._baseline_snapshot = copy_snbt_properties(data)
        self._dirty = False
        self._loading = True
        self._apply_model_to_ui(data)
        self._loading = False
        self._properties_load_state = "loaded"
        _p = str(active_path)

        def _emit_active() -> None:
            self.active_file_changed.emit(_p)

            # 下一事件循环再通知各页，避免与预览解码、重绘挤在同一主线程切片里
        QTimer.singleShot(0, _emit_active)

    def _commit_loaded_project(self, result: LoadAnalyzeResult) -> None:
        data = snbt_properties_from_dict(result.snbt)
        result_path = Path(result.file_path).resolve()
        if data.file_path is None:
            data.file_path = result_path
        else:
            try:
                if Path(data.file_path).resolve() != result_path:
                    data.file_path = result_path
            except OSError:
                data.file_path = result_path
        self._commit_loaded_snbt_model(data)
        analysis = AnalyzeResult(file_path=result.file_path, output=result.output)
        try:
            self._projection_library.update_analysis_metadata(result.file_path, result.output)
        except ProjectionLibraryError:
            pass
        QTimer.singleShot(0, lambda: self.analysis_ready.emit(analysis))

    def _on_snbt_loaded_ok(self, data: LoadAnalyzeResult, seq: int) -> None:
        if seq != self._snbt_load_seq:
            return
        try:
            pending = self._pending_file_path or self._async_load_target_path
            result_path = Path(data.file_path).resolve()
            if pending is not None and result_path != pending.resolve():
                raise ValueError(f"load result path mismatch: pending={pending.resolve()} result={result_path}")
            self._commit_loaded_project(data)
        except Exception as exc:
            self._show_load_failed_state(self._async_load_target_path, str(exc))
            QMessageBox.critical(self, "打开失败", f"回填属性失败：\n{exc}")
        finally:
            self._set_snbt_load_busy(False)
            self._close_snbt_progress_dialog()

    def _on_snbt_loaded_fail(self, message: str, seq: int) -> None:
        if seq != self._snbt_load_seq:
            return
        failed_path = self._async_load_target_path
        self._show_load_failed_state(failed_path, message)
        self._set_snbt_load_busy(False)
        self._close_snbt_progress_dialog()
        QMessageBox.critical(self, "打开失败", f"读取 SNBT 失败：\n{message}")

    def _on_async_snbt_load_canceled(self) -> None:
        """用户点击「中断」：作废当前序号并终止子进程（勿 ``QThread.terminate``）。"""
        self._snbt_load_seq += 1
        w = self._load_thread
        if w is not None:
            w.abort_child_process()
            if w.isRunning():
                w.wait(8000)
        self._load_thread = None
        self._show_load_failed_state(self._async_load_target_path, "加载已取消")
        self._set_snbt_load_busy(False)
        self._close_snbt_progress_dialog()

    def _load_snbt_sync(self, path: Path, seq: int) -> None:
        """主线程直接 ``load_snbt_properties``（小文件或阈值较高时）。"""
        try:
            data = load_snbt_properties(path)
        except Exception as exc:
            if seq == self._snbt_load_seq:
                QMessageBox.critical(self, "打开失败", f"读取 SNBT 失败：\n{exc}")
            return
        if seq != self._snbt_load_seq:
            return
        self._commit_loaded_snbt_model(data)

    def _load_from_file(self, file_path: str | Path) -> None:
        """按设置决定同步或后台加载；大于阈值时在后台 ``amulet_nbt.load`` 并显示可中断进度。

        成功后同步更新 ``_baseline_snapshot``，供「恢复默认值」使用。
        """
        self._snbt_load_seq += 1
        seq = self._snbt_load_seq
        if self._load_thread is not None:
            old = self._load_thread
            self._load_thread = None
            old.abort_child_process()
            old.finished.connect(old.deleteLater)
        self._close_snbt_progress_dialog()

        path = Path(file_path)
        try:
            size = path.stat().st_size
        except OSError as exc:
            QMessageBox.critical(self, "打开失败", f"无法访问文件：\n{exc}")
            return
        path = path.resolve()

        self._show_loading_state_for_file(path)
        show_dialog = size > self._litematic_async_load_min_bytes

        # 异步路径：顶部路径行同步显示「加载中…」，避免子进程很快完成时模态框一闪而过无反馈
        self._async_load_target_path = path
        self._set_snbt_load_busy(True, wait_cursor=not show_dialog)
        if show_dialog:
            dlg = _LitematicLoadWaitDialog(self, path.name, size / 1024.0)
            dlg.rejected.connect(self._on_async_snbt_load_canceled)
            self._snbt_progress_dialog = dlg
            dlg.show()
            dlg.raise_()
            dlg.activateWindow()
            app = QApplication.instance()
            if app is not None:
                app.processEvents()

        def _kick_off() -> None:
            if seq != self._snbt_load_seq:
                return
            worker = _ProjectLoadAnalyzeWorker(path)
            self._load_thread = worker
            worker.finished_ok.connect(
                lambda d, s=seq: self._on_snbt_loaded_ok(d, s),
                Qt.ConnectionType.QueuedConnection,
            )
            worker.failed.connect(
                lambda err, s=seq: self._on_snbt_loaded_fail(err, s),
                Qt.ConnectionType.QueuedConnection,
            )
            worker.finished.connect(self._on_snbt_load_thread_finished)
            worker.start()

        QTimer.singleShot(0, _kick_off)

    def _apply_model_to_ui(self, data: SnbtProperties) -> None:
        """单向：数据模型 → 控件文本与预览；不修改 ``_dirty``（由调用方控制）。"""
        self._file_name_edit.setText(data.file_name)
        self._internal_name_edit.setText(data.internal_name)
        self._author_edit.setText(data.author)
        self._description_edit.setText(data.description)
        self._created_time_readonly.setText(self._format_timestamp(data.created_unix))
        self._modified_time_readonly.setText(self._format_timestamp(data.modified_unix))
        ex, ey, ez = data.enclosing_size
        self._size_x_readonly.setText(str(ex))
        self._size_y_readonly.setText(str(ey))
        self._size_z_readonly.setText(str(ez))
        self._total_blocks_readonly.setText(str(data.total_blocks))
        self._total_volume_readonly.setText(str(data.total_volume))
        # 密度随方块数/总计刷新；不参与 _collect_ui_to_model
        self._density_readonly.setText(self._format_block_density_pct(data.total_blocks, data.total_volume))
        self._litematica_ver_readonly.setText(str(data.litematic_version))
        self._mc_data_ver_readonly.setText(str(data.minecraft_data_version))
        self._set_preview_from_argb_list(data.preview_image_data)
        self._apply_regions_table(data)
        self._sync_title_hint()

    def _apply_line_edit_horizontal_shrink(self) -> None:
        """构造完成后统一收紧所有 QLineEdit，利于窄窗口与表单对齐。"""
        for w in self.findChildren(QLineEdit):
            w.setMinimumWidth(0)
            w.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)

    def _apply_snbt_column_min_width(self) -> None:
        """Minecraft 主题字体/样式下表单更易折行，为 SNBT 列保留最小可读宽度。"""
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        if tid == "Minecraft":
            self._snbt_column.setMinimumWidth(400)
        else:
            self._snbt_column.setMinimumWidth(0)

    def _update_file_hint_elide(self) -> None:
        """根据标签可用宽度对路径做中间省略；过窄时用父行宽度估算（减去两侧按钮大致占位）。"""
        text = self._full_file_hint_text
        w = self._active_file_hint.width()
        if w < 48:
            row = self._active_file_hint.parentWidget()
            if row is not None:
                w = max(48, row.width() - 280)
        elided = self._active_file_hint.fontMetrics().elidedText(
            text,
            Qt.TextElideMode.ElideMiddle,
            max(48, w - 8),
        )
        self._active_file_hint.setText(elided)

    def _collect_ui_to_model(self) -> SnbtProperties:
        """从控件组装即将写入磁盘的模型。

        修改时间取当前毫秒；``TotalBlocks``/``TotalVolume``/包围尺寸/版本等沿用 ``_current_data``（用户在本页不能改统计）。
        密度为派生显示，不包含在 ``SnbtProperties`` 中。
        """
        model = SnbtProperties(
            file_path=self._current_data.file_path,
            file_name=self._file_name_edit.text().strip(),
            internal_name=self._internal_name_edit.text().strip(),
            author=self._author_edit.text().strip(),
            description=self._description_edit.text().strip(),
            created_unix=self._current_data.created_unix,
            modified_unix=int(datetime.now().timestamp() * 1000),
            enclosing_size=self._current_data.enclosing_size,
            total_blocks=self._current_data.total_blocks,
            total_volume=self._current_data.total_volume,
            litematic_version=self._current_data.litematic_version,
            minecraft_data_version=self._current_data.minecraft_data_version,
            preview_image_data=self._preview_to_argb_list(),
            regions=self._collect_regions_from_table(),
        )
        return model

    def _set_preview_from_argb_list(self, data: list[int]) -> None:
        if not hasattr(self, "_preview_canvas"):
            self._save_preview_from_model_list = True
            self._preview_image = QImage()
            return
        """将文件中的 PreviewImageData（每元素 32 位 ARGB）还原为 ``QImage`` 并显示。

        仅当列表长度为完全平方数时才认为合法；否则清空预览且不标脏（加载阶段）。
        像素数大于 ``_MAX_PREVIEW_PIXELS_SHOW`` 时不加载界面预览；保存仍用 ``_current_data.preview_image_data``。
        """
        if not data:
            self._save_preview_from_model_list = True
            self._on_clear_preview_no_dirty()
            return
        side = int(len(data) ** 0.5)
        if side * side != len(data):
            self._save_preview_from_model_list = True
            self._on_clear_preview_no_dirty()
            return
        if len(data) > _MAX_PREVIEW_PIXELS_SHOW:
            self._save_preview_from_model_list = True
            self._on_clear_preview_no_dirty()
            self._preview_count_hint.setText(
                f"PreviewImageData: {len(data)} 项（>{_MAX_PREVIEW_PIXELS_SHOW} 像素，已跳过界面加载）"
            )
            return
        try:
            raw = array.array("I", (int(data[i]) & 0xFFFFFFFF for i in range(side * side))).tobytes()
            image = QImage(raw, side, side, side * 4, QImage.Format.Format_ARGB32)
            if image.isNull():
                raise RuntimeError("QImage from buffer is null")
            self._preview_image = image.copy()
        except Exception:
            image = QImage(side, side, QImage.Format.Format_ARGB32)
            for y in range(side):
                base = y * side
                for x in range(side):
                    image.setPixel(x, y, int(data[base + x]) & 0xFFFFFFFF)
            self._preview_image = image
        self._preview_canvas.set_preview(QPixmap.fromImage(self._preview_image))
        self._preview_count_hint.setText(f"PreviewImageData: {len(data)} 项")
        self._save_preview_from_model_list = True

    def _preview_to_argb_list(self) -> list[int]:
        """将当前预览写回 SNBT 用的 ARGB32 列表。

        若自文件载入后仅做了界面缩略解码，则仍写回 ``_current_data.preview_image_data`` 原列表。
        """
        if self._save_preview_from_model_list:
            return list(self._current_data.preview_image_data)
        if self._preview_image.isNull():
            return []
        image = self._preview_image.convertToFormat(QImage.Format.Format_ARGB32)
        out: list[int] = []
        for y in range(image.height()):
            for x in range(image.width()):
                out.append(int(image.pixel(x, y)))
        return out

    def _on_clear_preview_no_dirty(self) -> None:
        """与 ``_on_clear_preview`` 相同视觉效果，但不调用 ``_mark_dirty``（用于加载失败或非法数据）。"""
        self._preview_image = QImage()
        if hasattr(self, "_preview_canvas"):
            self._preview_canvas.set_preview(None)
        if hasattr(self, "_preview_count_hint"):
            self._preview_count_hint.setText("PreviewImageData: 0 项")

    def _confirm_discard_if_dirty(self) -> bool:
        """返回 True 表示可继续（无脏数据或用户确认丢弃）。"""
        if not self._dirty:
            return True
        ans = QMessageBox.question(
            self,
            "未保存更改",
            "当前文件有未保存修改，是否丢弃并继续？",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        return ans == QMessageBox.StandardButton.Yes

    def _mark_dirty(self) -> None:
        """由可编辑控件信号触发；加载模型期间由 ``_loading`` 短路。"""
        if self._loading:
            return
        self._dirty = True
        self._sync_title_hint()

    def _sync_title_hint(self) -> None:
        """更新顶部路径文案、ToolTip 与省略显示；未保存时在文案末尾追加 `` *``。"""
        if self._snbt_load_busy and self._async_load_target_path is not None:
            base = f"加载中… {self._async_load_target_path}"
        else:
            base = str(self._current_data.file_path) if self._current_data.file_path else "当前未激活文件"
        self._full_file_hint_text = f"{base}{' *' if self._dirty else ''}"
        self._active_file_hint.setToolTip(self._full_file_hint_text)
        self._update_file_hint_elide()

    @staticmethod
    def _format_timestamp(ts: int) -> str:
        """将 Unix 时间格式化为本地可读字符串。

        Litematica 常用毫秒（>1e10 阈值），否则按秒处理；无效或异常时退回原始数字或 ``-``。
        """
        if ts <= 0:
            return "-"
        try:
            sec = ts / 1000 if ts > 10_000_000_000 else ts
            dt = datetime.fromtimestamp(sec)
            return dt.strftime("%Y-%m-%d %H:%M:%S")
        except Exception:
            return str(ts)

    @staticmethod
    def _format_block_density_pct(blocks: int, volume: int) -> str:
        """将 ``TotalBlocks``（非空气方块数）与 ``TotalVolume``（包围体素格数）转为占用率字符串。

        公式 ``100 * blocks / volume``，输出形如 ``12.3%``（固定一位小数）。
        ``volume <= 0`` 时无法定义比例，返回 ``-``（例如未加载或损坏元数据）。
        """
        if volume <= 0:
            return "-"
        pct = 100.0 * float(blocks) / float(volume)
        return f"{pct:.1f}%"

    def _sync_title_hint(self) -> None:
        if self._snbt_load_busy and self._async_load_target_path is not None:
            base = f"加载中... {self._async_load_target_path}"
        else:
            display_path = self.display_file_path()
            base = str(display_path) if display_path else "当前未激活文件"
        self._full_file_hint_text = f"{base}{' *' if self._dirty else ''}"
        self._active_file_hint.setToolTip(self._full_file_hint_text)
        self._update_file_hint_elide()

    def _show_not_implemented(self, action: str) -> None:
        """占位功能的统一提示，避免静默无响应。"""
        QMessageBox.information(self, "属性页", f"{action} 功能将在后续接入真实读写逻辑。")
