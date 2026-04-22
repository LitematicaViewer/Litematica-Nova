"""主窗口：左侧导航 + 右侧堆叠页面（design §2.0）。"""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import Qt, QThread, QTimer, Signal
from PySide6.QtGui import QGuiApplication, QResizeEvent, QShowEvent
from PySide6.QtWidgets import (
    QApplication,
    QButtonGroup,
    QFrame,
    QHBoxLayout,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.projection_library import ProjectionLibraryError, ProjectionLibraryStore
from litematicaba.core.projection_preview import (
    ProjectionPreviewError,
    ProjectionPreviewRequest,
    generate_projection_preview,
)
from litematicaba.core.settings import (
    BLOCK_ICON_PRELOAD_NEVER,
    BLOCK_ICON_PRELOAD_ON_LITEMATIC,
    BLOCK_ICON_PRELOAD_ON_MATERIAL_OR_FLAKE,
    BLOCK_ICON_PRELOAD_STARTUP,
    MATERIAL_LIST_PREWARM_ON_LITEMATIC,
    AppSettings,
    load_settings,
)
from litematicaba.ui.material_list_icon_prewarmer import (
    MaterialListIconPrewarmer,
    attach_material_list_icon_prewarmer,
    register_material_ui_icon_prewarm_hook,
)
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.pages import (
    FlakePage,
    HomePage,
    LibraryPage,
    OptionsPage,
    PropertiesPage,
    RenderPage,
    ReplacePage,
    StatisticsPage,
    UiTestPage,
)
from litematicaba.ui.theme import apply_theme, current_theme_id
from litematicaba.ui.perf_test_overlay import PerfTestController
from litematicaba.ui.widget_inspector import WidgetInspectorController
from litematicaba.ui.widgets.nav_expand import NavExpandButton
from litematicaba.ui.widgets.nav_item import NavItemButton


class AutoProjectionPreviewWorker(QThread):
    finished_ok = Signal(str)
    failed = Signal(str)

    def __init__(
        self,
        store: ProjectionLibraryStore,
        entry_id: str,
        cache_file: str,
    ) -> None:
        super().__init__()
        self._store = store
        self._entry_id = entry_id
        self._cache_file = cache_file

    def run(self) -> None:  # type: ignore[override]
        try:
            entry = self._store.get_entry(self._entry_id)
            path = generate_projection_preview(
                self._store,
                ProjectionPreviewRequest(entry=entry, cache_file=Path(self._cache_file)),
            )
        except Exception as exc:
            self.failed.emit(str(exc))
            return
        self.finished_ok.emit(str(path))

PAGE_HOME = 0
PAGE_LIBRARY = 1
PAGE_PROPERTIES = 2
PAGE_STATISTICS = 3
PAGE_FLAKE = 4
PAGE_RENDER = 5
PAGE_REPLACE = 6
PAGE_UI_TEST = 7
PAGE_OPTIONS = 8


class MainWindow(QWidget):
    """使用 QWidget 作顶层容器，便于与 QSS 背景一致；标题栏由系统装饰。"""

    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("LitematicaBA")
        self.resize(960, 600)

        self._settings = load_settings()
        apply_theme(QApplication.instance(), self._settings.theme_id)
        self._projection_library = ProjectionLibraryStore(
            default_limit=self._settings.projection_library_limit
        )
        self._auto_preview_workers: list[AutoProjectionPreviewWorker] = []

        self._sidebar_expanded = True
        self._sidebar_width_expanded = 220
        self._sidebar_width_collapsed = 48

        self._stack = QStackedWidget()
        self._stack.setObjectName("mainContentStack")
        self._properties_page = PropertiesPage(
            app_settings=self._settings,
            projection_library=self._projection_library,
        )
        self._library_page = LibraryPage(
            self._projection_library,
            app_settings=self._settings,
        )
        self._home_page = HomePage(
            on_open_file=self._open_file_from_home,
            on_open_statistics=lambda: self._navigate("statistics"),
            on_open_render=lambda: self._navigate("render"),
        )
        self._stack.addWidget(self._home_page)
        self._stack.addWidget(self._library_page)
        self._stack.addWidget(self._properties_page)
        self._material_list_scan_prewarmer = MaterialListScanPrewarmer(self)
        self._icon_prewarm_session_litematic = False
        self._icon_prewarm_session_material_ui = False
        self._initial_icon_prewarm_scheduled = False
        self._properties_page.active_file_changed.connect(self._on_properties_active_file_changed)
        self._library_page.open_projection.connect(self._open_projection_from_library)
        self._statistics_page = StatisticsPage(
            self._properties_page,
            material_scan_prewarmer=self._material_list_scan_prewarmer,
            defer_stats_until_material_prewarm=self._stats_wait_material_prewarm,
        )
        self._stack.addWidget(self._statistics_page)
        self._flake_page = FlakePage(
            self._properties_page,
            app_settings=self._settings,
            material_scan_prewarmer=self._material_list_scan_prewarmer,
        )
        self._stack.addWidget(self._flake_page)
        self._render_page = RenderPage(
            self._properties_page,
            app_settings=self._settings,
            material_scan_prewarmer=self._material_list_scan_prewarmer,
        )
        self._stack.addWidget(self._render_page)
        self._render_page.cache_ready.connect(self._properties_page.set_render_cache)
        self._render_page.cache_ready.connect(self._flake_page.set_render_cache)
        self._render_page.cache_ready.connect(self._on_render_cache_ready)
        self._render_page.build_mode_changed.connect(self._flake_page.set_render_mode)
        self._replace_page = ReplacePage()
        self._stack.addWidget(self._replace_page)
        self._ui_test_page = UiTestPage(
            show_tile_grid=self._settings.show_tile_grid,
            theme_id=self._settings.theme_id,
            tile_auto_place_preferred_cols=self._settings.tile_auto_place_preferred_cols,
            tile_view_right_padding_px=self._settings.tile_view_right_padding_px,
        )
        self._stack.addWidget(self._ui_test_page)
        self._options_page = OptionsPage()
        self._stack.addWidget(self._options_page)
        self._options_page.load(self._settings)
        self._options_page.settings_changed.connect(self._on_settings_changed)
        self._ui_test_page.apply_settings(self._settings)
        self._pages: dict[str, QWidget] = {
            "home": self._home_page,
            "library": self._library_page,
            "properties": self._properties_page,
            "statistics": self._statistics_page,
            "flake": self._flake_page,
            "render": self._render_page,
            "replace": self._replace_page,
            "ui_test": self._ui_test_page,
            "options": self._options_page,
        }
        for key, page in self._pages.items():
            page.setObjectName(f"page_{key}")

        self._expand_btn = NavExpandButton()
        self._expand_btn.setToolTip("收起侧栏")
        self._expand_btn.clicked.connect(self._toggle_sidebar)

        # icon_stem 对应 ``ui/resources/icon/<stem>.svg``，缺省则用 undefined.svg
        self._btn_home = NavItemButton("主页", "主", icon_stem="home")
        self._btn_library = NavItemButton("投影库", "库", icon_stem="gallery")
        self._btn_properties = NavItemButton("属性", "属", icon_stem="properties")
        self._btn_statistics = NavItemButton("统计", "统", icon_stem="statistics")
        self._btn_flake = NavItemButton("分层", "层", icon_stem="flake")
        self._btn_render = NavItemButton("渲染", "染", icon_stem="render")
        self._btn_replace = NavItemButton("替换", "替", icon_stem="replace")
        self._btn_ui_test = NavItemButton("UI 测试", "测", icon_stem="ui_debug")
        self._btn_options = NavItemButton("选项", "项", icon_stem="options")

        self._btn_group = QButtonGroup(self)
        self._btn_group.setExclusive(True)
        self._nav_buttons: dict[str, NavItemButton] = {
            "home": self._btn_home,
            "library": self._btn_library,
            "properties": self._btn_properties,
            "statistics": self._btn_statistics,
            "flake": self._btn_flake,
            "render": self._btn_render,
            "replace": self._btn_replace,
            "ui_test": self._btn_ui_test,
            "options": self._btn_options,
        }
        for b in (
            self._btn_home,
            self._btn_library,
            self._btn_properties,
            self._btn_statistics,
            self._btn_flake,
            self._btn_render,
            self._btn_replace,
            self._btn_ui_test,
            self._btn_options,
        ):
            self._btn_group.addButton(b)

        self._btn_home.clicked.connect(lambda: self._navigate("home"))
        self._btn_library.clicked.connect(lambda: self._navigate("library"))
        self._btn_properties.clicked.connect(lambda: self._navigate("properties"))
        self._btn_statistics.clicked.connect(lambda: self._navigate("statistics"))
        self._btn_flake.clicked.connect(lambda: self._navigate("flake"))
        self._btn_render.clicked.connect(lambda: self._navigate("render"))
        self._btn_replace.clicked.connect(lambda: self._navigate("replace"))
        self._btn_ui_test.clicked.connect(lambda: self._navigate("ui_test"))
        self._btn_options.clicked.connect(lambda: self._navigate("options"))

        self._stack.currentChanged.connect(self._sync_nav_checks)

        sep1 = QFrame()
        sep1.setFrameShape(QFrame.Shape.HLine)
        sep1.setFrameShadow(QFrame.Shadow.Plain)
        sep1.setFixedHeight(1)
        sep2 = QFrame()
        sep2.setFrameShape(QFrame.Shape.HLine)
        sep2.setFrameShadow(QFrame.Shadow.Plain)
        sep2.setFixedHeight(1)

        side = QWidget()
        side.setObjectName("navSidebar")
        side_l = QVBoxLayout(side)
        # Win10：相邻导航项无竖向间隙（分隔线仍占位 1px）
        side_l.setSpacing(0)
        side_l.setContentsMargins(0, 0, 0, 0)
        side_l.addWidget(self._expand_btn)
        side_l.addWidget(self._btn_home)
        side_l.addWidget(sep1)
        side_l.addWidget(self._btn_library)
        side_l.addWidget(self._btn_properties)
        side_l.addWidget(self._btn_statistics)
        side_l.addWidget(self._btn_flake)
        side_l.addWidget(self._btn_render)
        side_l.addWidget(self._btn_replace)
        side_l.addWidget(sep2)
        side_l.addStretch()
        side_l.addWidget(self._btn_ui_test)
        side_l.addWidget(self._btn_options)

        root = QHBoxLayout(self)
        root.setSpacing(0)
        root.setContentsMargins(0, 0, 0, 0)
        root.addWidget(side)
        root.addWidget(self._stack, 1)

        self._nav_sidebar = side
        self._btn_ui_test.setVisible(self._settings.show_ui_test_nav)
        self._apply_sidebar_geometry()
        self._widget_inspector = WidgetInspectorController(self)
        self._widget_inspector.set_enabled(self._settings.show_widget_inspector)
        self._perf_test = PerfTestController(self)
        self._perf_test.set_enabled(self._settings.perf_test_overlay)
        self._material_list_icon_prewarmer = MaterialListIconPrewarmer(
            self, config_provider=self._block_icon_prewarm_config
        )
        attach_material_list_icon_prewarmer(self._material_list_icon_prewarmer)
        register_material_ui_icon_prewarm_hook(self._maybe_start_icon_prewarm_from_material_ui)
        self._btn_home.setChecked(True)
        self._stack.setCurrentIndex(PAGE_HOME)

        # 避免子控件（如主页 Logo）曾出现过大 minimumSize 后把整窗最小宽度锁死
        self.setMinimumSize(0, 0)

    def _block_icon_prewarm_config(self) -> tuple[int, int, str]:
        s = self._settings.normalized()
        return (
            s.block_icon_prewarm_batch_interval_ms,
            s.block_icon_prewarm_batch_count,
            s.block_icon_prewarm_decode_thread,
        )

    def schedule_material_list_icon_prewarm(self) -> None:
        """窗口先完成首帧绘制后再预载图标，减轻与冷启动争用。"""
        QTimer.singleShot(1200, self._material_list_icon_prewarmer.start)

    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        self._widget_inspector.sync_geometry()
        self._perf_test.sync_geometry()

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        self._widget_inspector.sync_geometry()
        self._perf_test.sync_geometry()
        self._clamp_window_to_available_geometry()
        if not self._initial_icon_prewarm_scheduled:
            self._initial_icon_prewarm_scheduled = True
            if self._settings.block_icon_preload_mode == BLOCK_ICON_PRELOAD_STARTUP:
                self.schedule_material_list_icon_prewarm()

    def _clamp_window_to_available_geometry(self) -> None:
        screen = QGuiApplication.primaryScreen()
        if screen is None:
            return
        ag = screen.availableGeometry()
        nw = min(self.width(), max(320, ag.width()))
        nh = min(self.height(), max(240, ag.height()))
        if nw < self.width() or nh < self.height():
            self.resize(nw, nh)
        fg = self.frameGeometry()
        if not ag.contains(fg):
            x = max(ag.left(), min(fg.left(), ag.right() - fg.width()))
            y = max(ag.top(), min(fg.top(), ag.bottom() - fg.height()))
            self.move(x, y)

    def _toggle_sidebar(self) -> None:
        self._sidebar_expanded = not self._sidebar_expanded
        self._apply_sidebar_geometry()

    def _apply_sidebar_geometry(self) -> None:
        w = self._sidebar_width_expanded if self._sidebar_expanded else self._sidebar_width_collapsed
        self._nav_sidebar.setFixedWidth(w)
        app = QApplication.instance()
        tid = current_theme_id(app) if app is not None else "QTDefault"
        metro_nav = tid in ("Metro10", "Metro8")

        if self._sidebar_expanded:
            self._expand_btn.setToolTip("收起侧栏")
        else:
            self._expand_btn.setToolTip("展开侧栏")

        if metro_nav:
            self._expand_btn.setText("")
            self._expand_btn.set_sidebar_expanded(self._sidebar_expanded)
        else:
            self._expand_btn.setText("«" if self._sidebar_expanded else "»")

        for b in self._btn_group.buttons():
            full = b.property("labelFull") or ""
            short = b.property("labelShort") or ""
            b.setText(full if self._sidebar_expanded else short)
            b.set_nav_expanded(self._sidebar_expanded)
            if not self._sidebar_expanded:
                b.setToolTip(str(full))
            else:
                b.setToolTip("")

    def _sync_nav_checks(self, index: int) -> None:
        btn = None
        page = self._stack.widget(index)
        key = self._page_key(page)
        if key is not None:
            btn = self._nav_buttons.get(key)
        if btn is None:
            return
        self._btn_group.setExclusive(False)
        btn.setChecked(True)
        for other in self._btn_group.buttons():
            if other is not btn:
                other.setChecked(False)
        self._btn_group.setExclusive(True)

    def _navigate(self, key: str) -> None:
        target = self._pages.get(key)
        if target is None:
            return
        target_index = self._stack.indexOf(target)
        if target_index < 0:
            return
        if key == "properties":
            self._properties_page.apply_regions_table_theme()
        self._stack.setCurrentWidget(target)
        self._stack.raise_()
        target.raise_()
        if key == "render":
            if hasattr(target, "deactivate_native_surfaces_for_empty"):
                target.deactivate_native_surfaces_for_empty()
            self._hide_inactive_native_layers(target)
        target.updateGeometry()
        target.update()
        target.repaint()
        QApplication.processEvents()
        after_widget = self._stack.currentWidget()
        if after_widget is not target:
            self._stack.setCurrentWidget(target)

    def _open_file_from_home(self) -> None:
        self._navigate("properties")
        self._properties_page.open_external_file()

    def _open_projection_from_library(self, backup_path: str) -> None:
        self._navigate("properties")
        self._properties_page.open_file_path(backup_path)

    def _hide_inactive_native_layers(self, current_page: QWidget) -> None:
        for widget in self._native_like_widgets():
            owner = self._owning_page(widget)
            if owner is not None and owner is not current_page:
                widget.hide()
                widget.setVisible(False)

    def _dump_native_layers(self, current_page: QWidget) -> list[str]:
        entries: list[str] = []
        for widget in self._native_like_widgets():
            owner = self._owning_page(widget)
            owner_key = self._page_key(owner)
            g = widget.geometry()
            parent = widget.parentWidget()
            non_current_visible = owner is not current_page and widget.isVisible()
            entries.append(
                f"{widget.__class__.__name__}/{widget.objectName()!r} "
                f"owner={owner_key!r} visible={widget.isVisible()} "
                f"non_current_visible={non_current_visible} "
                f"geometry=({g.x()},{g.y()},{g.width()}x{g.height()}) "
                f"parent={parent.__class__.__name__ if parent is not None else None}/"
                f"{parent.objectName() if parent is not None else ''!r}"
            )
        if not entries:
            entries.append("none")
        return entries

    def _native_like_widgets(self) -> list[QWidget]:
        widgets: list[QWidget] = []
        for page in self._pages.values():
            for widget in page.findChildren(QWidget):
                if self._is_native_like_widget(widget):
                    widgets.append(widget)
        return widgets

    def _is_native_like_widget(self, widget: QWidget) -> bool:
        cls_name = widget.__class__.__name__
        return (
            "WebEngine" in cls_name
            or "OpenGL" in cls_name
            or "WindowContainer" in cls_name
            or widget.testAttribute(Qt.WidgetAttribute.WA_NativeWindow)
        )

    def _owning_page(self, widget: QWidget) -> QWidget | None:
        current: QWidget | None = widget
        while current is not None:
            if current in self._pages.values():
                return current
            current = current.parentWidget()
        return None

    def _page_key(self, page: QWidget | None) -> str | None:
        if page is None:
            return None
        for key, candidate in self._pages.items():
            if candidate is page:
                return key
        return None

    def _describe_page(self, page: QWidget | None) -> str:
        if page is None:
            return "<None>"
        g = page.geometry()
        parent = page.parentWidget()
        parent_name = parent.objectName() if parent is not None else ""
        return (
            f"key={self._page_key(page)!r} class={page.__class__.__name__} "
            f"objectName={page.objectName()!r} id={id(page)} "
            f"stack_index={self._stack.indexOf(page)} visible={page.isVisible()} "
            f"geometry=({g.x()},{g.y()},{g.width()}x{g.height()}) "
            f"parent={parent.__class__.__name__ if parent is not None else None}"
            f"/{parent_name!r}"
        )

    def _is_descendant_of(self, child: QWidget | None, ancestor: QWidget | None) -> bool:
        if child is None or ancestor is None:
            return False
        current: QWidget | None = child
        while current is not None:
            if current is ancestor:
                return True
            current = current.parentWidget()
        return False

    def _stats_wait_material_prewarm(self) -> bool:
        return self._settings.material_list_prewarm_mode == MATERIAL_LIST_PREWARM_ON_LITEMATIC

    def _on_render_cache_ready(self, file_path: str, cache_file: str) -> None:
        if not getattr(self._settings, "auto_generate_projection_preview", True):
            return
        try:
            entry = self._projection_library.find_entry_by_backup_path(file_path)
            if entry is None:
                return
        except (ProjectionPreviewError, ProjectionLibraryError):
            return
        worker = AutoProjectionPreviewWorker(self._projection_library, entry.entry_id, cache_file)
        self._auto_preview_workers.append(worker)
        worker.finished_ok.connect(lambda _path, w=worker: self._on_auto_preview_finished(w))
        worker.failed.connect(lambda message, w=worker: self._on_auto_preview_failed(w, message))
        worker.finished.connect(worker.deleteLater)
        worker.start()

    def _on_auto_preview_finished(self, worker: AutoProjectionPreviewWorker) -> None:
        if worker in self._auto_preview_workers:
            self._auto_preview_workers.remove(worker)
        self._library_page.refresh_library()

    def _on_auto_preview_failed(self, worker: AutoProjectionPreviewWorker, message: str) -> None:
        if worker in self._auto_preview_workers:
            self._auto_preview_workers.remove(worker)
        print(f"[LBA_PREVIEW] auto preview failed: {message}", flush=True)

    def _on_properties_active_file_changed(self, path_str: str) -> None:
        p = Path(path_str)
        if self._settings.material_list_prewarm_mode == MATERIAL_LIST_PREWARM_ON_LITEMATIC:
            self._material_list_scan_prewarmer.schedule(p)
        self._maybe_start_icon_prewarm_on_litematic()
        self._library_page.refresh_library()

    def _maybe_start_icon_prewarm_on_litematic(self) -> None:
        if self._settings.block_icon_preload_mode != BLOCK_ICON_PRELOAD_ON_LITEMATIC:
            return
        if self._icon_prewarm_session_litematic:
            return
        self._icon_prewarm_session_litematic = True
        self._material_list_icon_prewarmer.start()

    def _maybe_start_icon_prewarm_from_material_ui(self) -> None:
        if self._settings.block_icon_preload_mode != BLOCK_ICON_PRELOAD_ON_MATERIAL_OR_FLAKE:
            return
        if self._icon_prewarm_session_material_ui:
            return
        self._icon_prewarm_session_material_ui = True
        self._material_list_icon_prewarmer.start()

    def _on_settings_changed(self, s: AppSettings) -> None:
        old_icon = self._settings.block_icon_preload_mode
        old_decode_thread = self._settings.block_icon_prewarm_decode_thread
        self._settings = s
        apply_theme(QApplication.instance(), s.theme_id)
        self._btn_ui_test.setVisible(s.show_ui_test_nav)
        self._widget_inspector.set_enabled(s.show_widget_inspector)
        self._perf_test.set_enabled(s.perf_test_overlay)
        self._ui_test_page.apply_settings(s)
        self._properties_page.apply_regions_table_theme()
        self._properties_page.apply_app_settings(s)
        self._library_page.apply_app_settings(s)
        self._render_page.apply_deepslate_settings(s)
        self._apply_sidebar_geometry()
        if s.block_icon_preload_mode != old_icon:
            self._icon_prewarm_session_litematic = False
            self._icon_prewarm_session_material_ui = False
        if s.block_icon_preload_mode == BLOCK_ICON_PRELOAD_NEVER:
            self._material_list_icon_prewarmer.stop()
        elif s.block_icon_preload_mode == BLOCK_ICON_PRELOAD_STARTUP and old_icon != BLOCK_ICON_PRELOAD_STARTUP:
            self.schedule_material_list_icon_prewarm()
        elif (
            s.block_icon_preload_mode != BLOCK_ICON_PRELOAD_NEVER
            and self._material_list_icon_prewarmer.is_active()
        ):
            if s.block_icon_prewarm_decode_thread != old_decode_thread:
                self._material_list_icon_prewarmer.restart()
            else:
                self._material_list_icon_prewarmer.apply_live_config()
        if not s.show_ui_test_nav and self._stack.currentIndex() == PAGE_UI_TEST:
            self._stack.setCurrentIndex(PAGE_HOME)
