"""材料列表独立窗口（design §2.8）：磁盘缓存优先；未过期时不重复扫描。

子区域列表优先复用属性页已解析的 ``Regions``，避免对大文件再次 ``Schematic.load`` 阻塞 UI；
仅在无法与属性页对齐时于后台线程读取键名。「重新加载」或缓存过期时后台重扫；有缓存时先以中灰色显示缓存行再刷新。
图标列固定 32×32；大行数时分批替换图标列以减轻主线程卡顿。
"""

from __future__ import annotations

import csv
from datetime import datetime
from itertools import chain
from pathlib import Path

from PySide6.QtCore import QThread, QTimer, Qt, Signal
from PySide6.QtGui import QBrush, QColor, QCloseEvent, QFontMetrics, QPalette, QPixmap, QShowEvent, QStandardItemModel
from PySide6.QtWidgets import (
    QAbstractItemView,
    QApplication,
    QCheckBox,
    QComboBox,
    QDialog,
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QPushButton,
    QHeaderView,
    QSizePolicy,
    QSpinBox,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from litematicaba.core.game_resource_language import load_runtime_language_map
from litematicaba.core.litematic_block_scan import scan_litematic_block_counts, sorted_block_counts
from litematicaba.core.snbt_properties import load_snbt_properties
from litematicaba.core.material_list_cache import (
    cache_is_stale,
    file_mtime_ns,
    load_material_cache,
    save_material_cache,
)
from litematicaba.ui.pages.properties_page import PropertiesPage
from litematicaba.ui.material_list_icon_prewarmer import request_icon_prewarm_from_material_or_flake_ui
from litematicaba.ui.material_list_scan_prewarmer import MaterialListScanPrewarmer
from litematicaba.ui.table.material_list_table import (
    configure_material_list_table,
    material_list_block_icon_pixmap_32,
    material_list_block_icon_pixmap_32_for_block,
    refresh_material_list_row_visuals,
    sync_material_list_row_heights,
)
from litematicaba.ui.theme import current_theme_id

# 材料列表导出：仅 Item + Total 两列（CSV / ASCII 文本表）
_MIN_ITEM_W = 7
_MIN_NUM_W = 7
# 超过此行数时图标列先占位再在事件循环中分批替换，减轻主线程尖峰。
_ICON_ROWS_INLINE = 400
_ICON_COLUMN_CHUNK = 80

_NAME_COL_MIN_W = 80
_NAME_COL_PAD_H = 16


def _material_list_icon_cell_widget(pm: QPixmap) -> QWidget:
    """图标列：单元格内水平垂直居中（表格对 ``cellWidget`` 默认左上对齐）。"""
    icon_label = QLabel()
    icon_label.setPixmap(pm)
    icon_label.setFixedSize(32, 32)
    icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
    wrap = QWidget()
    outer = QVBoxLayout(wrap)
    outer.setContentsMargins(0, 0, 0, 0)
    outer.setSpacing(0)
    outer.addStretch(1)
    inner = QHBoxLayout()
    inner.setContentsMargins(0, 0, 0, 0)
    inner.setSpacing(0)
    inner.addStretch(1)
    inner.addWidget(icon_label)
    inner.addStretch(1)
    outer.addLayout(inner)
    outer.addStretch(1)
    return wrap


def _sync_material_list_name_column_width(table: QTableWidget) -> None:
    """名称列宽度：表头与各行显示名中最长一行的像素宽度 + 边距。"""
    hi = table.horizontalHeaderItem(1)
    header_text = hi.text() if hi is not None else "名称"
    max_w = QFontMetrics(table.horizontalHeader().font()).horizontalAdvance(header_text)
    fm = QFontMetrics(table.font())
    for r in range(table.rowCount()):
        it = table.item(r, 1)
        if it is not None:
            max_w = max(max_w, fm.horizontalAdvance(it.text()))
    table.setColumnWidth(1, max(_NAME_COL_MIN_W, max_w + _NAME_COL_PAD_H))


def _load_block_cn_map() -> dict[str, str]:
    return load_runtime_language_map()


def _display_name(block_id: str, cn: dict[str, str]) -> str:
    if block_id.startswith("E/"):
        # 实体翻译：E/minecraft:pig -> entity.minecraft.pig
        raw_entity = block_id[2:]
        return (
            cn.get(f"entity.{raw_entity.replace(':', '.')}")
            or cn.get(f"entity.{raw_entity}")
            or block_id
        )
    raw = block_id.split("[")[0].strip()
    if ":" in raw:
        local = raw.split(":", 1)[1]
    else:
        local = raw
    return (
        cn.get(f"block.minecraft.{local}")
        or cn.get(f"block.{local}")
        or raw
    )


def _sorted_material_counts(counts: dict[str, int]) -> list[tuple[str, int]]:
    return sorted(((str(k), int(v)) for k, v in counts.items()), key=lambda row: (-row[1], row[0]))


_WIN_INVALID_FS = '<>:"/\\|?*\n\r\t'


def _filename_safe_segment(name: str) -> str:
    t = "".join(c if c not in _WIN_INVALID_FS else "_" for c in name)
    t = t.strip(" .")
    return t or "export"


def _litematic_internal_metadata_names(path: Path) -> tuple[str, str]:
    """读取 ``Metadata.Name``：返回 (用于标题等展示, 用于文件名的安全段)。"""
    try:
        raw = load_snbt_properties(path).internal_name.strip()
    except Exception:
        raw = ""
    display = raw if raw else "未命名"
    return display, _filename_safe_segment(display)


def _material_list_suggested_save_path(*, internal_segment: str, extension: str) -> str:
    """material_list_<内部名安全段>_<YYYY-MM-DD>_<HH.MM.SS>.<ext>，置于用户主目录下作为初始路径。"""
    ext = extension if extension.startswith(".") else f".{extension}"
    stamp = datetime.now().strftime("%Y-%m-%d_%H.%M.%S")
    return str(Path.home() / f"material_list_{internal_segment}_{stamp}{ext}")


def _material_list_export_title(
    *,
    litematic_path: Path | None,
    region_name: str | None,
    csv_source: Path | None,
    litematic_quoted_name: str | None = None,
) -> str:
    if litematic_path is not None:
        name = (
            litematic_quoted_name
            if litematic_quoted_name is not None
            else _litematic_internal_metadata_names(litematic_path)[0]
        )
        if region_name is None:
            return f"原理图的材料清单 '{name}' (1 of 1 区域)"
        return f"原理图的材料清单 '{name}' (选定区域：{region_name})"
    if csv_source is not None:
        return f"原理图的材料清单 '{csv_source.stem}' (自定义文件)"
    return "原理图的材料清单"


def _material_list_txt_table(title: str, rows_display: list[tuple[str, int]]) -> str:
    """ASCII 表格：Item | Total 两列（与 CSV 含义一致）。"""
    w_item = max(_MIN_ITEM_W, len("Item"), max(len(r[0]) for r in rows_display) if rows_display else 0)
    w_tot = max(_MIN_NUM_W, len("Total"), max(len(str(r[1])) for r in rows_display) if rows_display else 0)

    title_inner = w_item + 1 + w_tot
    sep = "+" + "-" * w_item + "+" + "-" * w_tot + "+"

    def txt_cell_text(s: str, w: int) -> str:
        return (" " + str(s).strip())[:w].ljust(w)

    def row_header(a: str, b: str) -> str:
        return "|" + txt_cell_text(a, w_item) + "|" + txt_cell_text(b, w_tot) + "|"

    def row_data(item: str, tot: int) -> str:
        c_item = (" " + str(item).strip())[:w_item].ljust(w_item)
        return "|" + c_item + "|" + str(tot).rjust(w_tot) + "|"

    title_pad = (" " + title.strip())[:title_inner].ljust(title_inner)
    lines = [
        sep,
        "|" + title_pad + "|",
        sep,
        row_header("Item", "Total"),
        sep,
    ]
    for item, total in rows_display:
        lines.append(row_data(item, total))
    lines.extend(
        [
            sep,
            row_header("Item", "Total"),
            sep,
        ]
    )
    return "\n".join(lines) + "\n"


class _RegionKeysThread(QThread):
    """大文件时避免在主线程 ``Schematic.load`` 仅取子区域键（回退路径）。"""

    done = Signal(object)  # list[tuple[str, str]] 显示名与 litemapy 键（此处相同）

    def __init__(self, path: Path, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._path = path.resolve()

    def run(self) -> None:  # type: ignore[override]
        try:
            from litemapy import Schematic

            sch = Schematic.load(str(self._path))
            keys = list(sch.regions.keys())
        except Exception:
            keys = []
        self.done.emit([(k, k) for k in keys])


class _MaterialScanThread(QThread):
    finished_ok = Signal(dict)
    failed = Signal(str)

    def __init__(
        self,
        path: Path,
        *,
        region_name: str | None,
        include_entities: bool,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._path = path.resolve()
        self._region = region_name
        self._inc = include_entities

    def run(self) -> None:  # type: ignore[override]
        try:
            settings = load_settings()
            if settings.material_list_scan_backend == MATERIAL_LIST_SCAN_BACKEND_NATIVE and self._region is None:
                d = material_counts_from_analysis(self._path, include_entities=self._inc)
            else:
                d = scan_litematic_block_counts(
                    self._path,
                    include_entities=self._inc,
                    region_name=self._region,
                )
            self.finished_ok.emit(d)
        except Exception as exc:
            self.failed.emit(str(exc))


class MaterialListDialog(QDialog):
    """非模态材料列表；依赖 ``PropertiesPage`` 获取当前激活路径。"""

    def __init__(
        self,
        properties_page: PropertiesPage,
        parent: QWidget | None = None,
        *,
        initial_region_name: str | None = None,
        material_scan_prewarmer: MaterialListScanPrewarmer | None = None,
    ) -> None:
        super().__init__(parent)
        self.setObjectName("MaterialListDialog")
        self.setWindowTitle("材料列表")
        self.setMinimumSize(560, 420)
        self.setModal(False)
        self._props = properties_page
        self._scan_prewarmer = material_scan_prewarmer
        self._waiting_prewarm_path: Path | None = None
        self._cn = _load_block_cn_map()
        self._thread: _MaterialScanThread | None = None
        self._base_rows: list[tuple[str, int]] = []
        self._litematic_path: Path | None = None
        self._region_name: str | None = None
        self._csv_mode = False
        self._csv_source: Path | None = None
        self._pending_queue: bool = False
        self._table_gray: bool = False
        self._pending_initial_region_name: str | None = initial_region_name
        self._region_thread: _RegionKeysThread | None = None
        self._region_thread_apply_force_refresh: bool = False
        self._table_icon_fill_gen: int = 0
        app = QApplication.instance()
        self._theme_id = current_theme_id(app) if app is not None else "QTDefault"

        top = QHBoxLayout()
        self._workbook = QComboBox()
        self._workbook.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self._workbook.currentIndexChanged.connect(self._on_workbook_changed)
        top.addWidget(QLabel("工作簿："))
        top.addWidget(self._workbook, 1)

        btn_row = QHBoxLayout()
        self._btn_reload = QPushButton("重新加载")
        self._btn_reload.clicked.connect(self._on_reload_clicked)
        self._btn_export = QPushButton("写入文件…")
        self._btn_export.clicked.connect(self._on_export_clicked)
        self._spin_mult = QSpinBox()
        self._spin_mult.setRange(1, 999_999)
        self._spin_mult.setValue(1)
        self._spin_mult.setPrefix("倍数 ")
        self._spin_mult.valueChanged.connect(self._refresh_multiplier_only)
        self._cb_entities = QCheckBox("统计实体")
        self._cb_entities.toggled.connect(self._on_entities_toggled)
        btn_row.addWidget(self._btn_reload)
        btn_row.addWidget(self._btn_export)
        btn_row.addWidget(self._spin_mult)
        btn_row.addStretch()
        btn_row.addWidget(self._cb_entities)

        self._status = QLabel("")
        self._status.setStyleSheet("color: palette(mid);")

        self._table = QTableWidget(0, 3)
        self._table.setHorizontalHeaderLabels(["图标", "名称", "总计"])
        self._table.verticalHeader().setVisible(False)
        self._table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self._table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        self._table.setColumnWidth(0, 40)
        self._table.horizontalHeader().setStretchLastSection(True)
        self._table.setColumnWidth(2, 100)
        self._table.horizontalHeader().setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        self._table.horizontalHeader().setSectionResizeMode(2, QHeaderView.ResizeMode.Fixed)
        configure_material_list_table(self._table, self._theme_id)

        root = QVBoxLayout(self)
        root.addLayout(top)
        root.addLayout(btn_row)
        root.addWidget(self._status)
        root.addWidget(self._table, 1)

        self._reload_from_context()

    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        request_icon_prewarm_from_material_or_flake_ui()

    def closeEvent(self, event: QCloseEvent) -> None:
        self._clear_prewarm_wait()
        if self._region_thread is not None and self._region_thread.isRunning():
            self._region_thread.requestInterruption()
        super().closeEvent(event)

    def _clear_prewarm_wait(self) -> None:
        if self._scan_prewarmer is not None and self._waiting_prewarm_path is not None:
            try:
                self._scan_prewarmer.finished_for_path.disconnect(self._on_wait_prewarm_done)
            except (TypeError, RuntimeError):
                pass
        self._waiting_prewarm_path = None

    def _on_wait_prewarm_done(self, p: Path) -> None:
        w = self._waiting_prewarm_path
        if w is None or p.resolve() != w.resolve():
            return
        self._clear_prewarm_wait()
        self._start_material_flow(force_refresh=False)

    def _select_workbook_region(self, name: str, *, force_refresh: bool = False) -> None:
        self._workbook.blockSignals(True)
        idx = 0
        for i in range(self._workbook.count()):
            if self._workbook.itemData(i) == name:
                idx = i
                break
        self._workbook.setCurrentIndex(idx)
        self._workbook.blockSignals(False)
        self._apply_workbook_selection(force_refresh=force_refresh)

    def _fill_region_items_from_entries(self, entries: list[tuple[str, str]]) -> None:
        """entries：``(显示名, litemapy 区域键)``。"""
        self._workbook.blockSignals(True)
        self._workbook.clear()
        self._workbook.addItem("整个投影", None)
        for disp, key in entries:
            self._workbook.addItem(f"选定区域：{disp}", key)
        self._workbook.addItem("选定层级（需分层模块）", "__DISABLED__")
        d_idx = self._workbook.count() - 1
        m = self._workbook.model()
        if isinstance(m, QStandardItemModel):
            it = m.item(d_idx)
            if it is not None:
                it.setEnabled(False)
        self._workbook.addItem("自定义文件…", "__CSV__")
        self._workbook.blockSignals(False)

    def _reload_from_context(self, *, force_refresh: bool = False) -> None:
        path = self._props.active_file_path()
        if path is None:
            self._status.setText("无激活投影文件。")
            self._table.setRowCount(0)
            self._litematic_path = None
            return
        self._litematic_path = path.resolve()
        self._csv_mode = False
        self._csv_source = None
        self._region_thread_apply_force_refresh = force_refresh
        ents = self._props.material_list_region_entries_for_active_file()
        if ents is not None:
            self._fill_region_items_from_entries(ents)
            if self._pending_initial_region_name is not None:
                n = self._pending_initial_region_name
                self._pending_initial_region_name = None
                self._select_workbook_region(n, force_refresh=force_refresh)
            else:
                self._apply_workbook_selection(force_refresh=force_refresh)
            return
        self._fill_region_items_from_entries([])
        self._status.setText("正在读取子区域列表…")
        self._start_region_keys_thread(path.resolve())

    def _start_region_keys_thread(self, resolved: Path) -> None:
        if self._region_thread is not None and self._region_thread.isRunning():
            return
        th = _RegionKeysThread(resolved, self)
        self._region_thread = th
        th.done.connect(self._on_region_keys_ready)
        th.finished.connect(self._on_region_thread_finished)
        th.start()

    def _on_region_thread_finished(self) -> None:
        self._region_thread = None

    def _on_region_keys_ready(self, entries_obj: object) -> None:
        entries: list[tuple[str, str]] = []
        if isinstance(entries_obj, list):
            for row in entries_obj:
                if isinstance(row, (list, tuple)) and len(row) >= 2:
                    entries.append((str(row[0]), str(row[1])))
        self._fill_region_items_from_entries(entries)
        fr = self._region_thread_apply_force_refresh
        if self._pending_initial_region_name is not None:
            n = self._pending_initial_region_name
            self._pending_initial_region_name = None
            self._select_workbook_region(n, force_refresh=fr)
        else:
            self._apply_workbook_selection(force_refresh=fr)

    def _current_region_param(self) -> str | None:
        data = self._workbook.currentData()
        if data is None:
            return None
        if isinstance(data, str) and data in ("__DISABLED__", "__CSV__"):
            return None
        if isinstance(data, str):
            return data
        return None

    def _on_workbook_changed(self, index: int) -> None:
        if index < 0:
            return
        data = self._workbook.itemData(index)
        if data == "__CSV__":
            self._workbook.blockSignals(True)
            path, _ = QFileDialog.getOpenFileName(
                self,
                "选择材料列表 CSV",
                "",
                "CSV (*.csv);;All (*.*)",
            )
            if path:
                self._load_csv_counts(Path(path))
            else:
                self._workbook.setCurrentIndex(0)
            self._workbook.blockSignals(False)
            return
        if data == "__DISABLED__":
            self._workbook.blockSignals(True)
            self._workbook.setCurrentIndex(0)
            self._workbook.blockSignals(False)
            return
        self._csv_mode = False
        self._csv_source = None
        self._region_name = self._current_region_param()
        self._start_material_flow()

    def _load_csv_counts(self, csv_path: Path) -> None:
        try:
            counts: dict[str, int] = {}
            with csv_path.open("r", encoding="utf-8-sig", newline="") as f:
                r = csv.reader(f)
                rows_iter = iter(r)
                first = next(rows_iter, None)
                total_col = 1
                if first and len(first) >= 2 and first[0].strip().lower() == "item":
                    try:
                        total_col = next(
                            i for i, c in enumerate(first) if c.strip().lower() == "total"
                        )
                    except StopIteration:
                        total_col = 1
                    data_rows = rows_iter
                else:
                    data_rows = chain([] if first is None else [first], rows_iter)
                for row in data_rows:
                    if len(row) <= total_col:
                        continue
                    key = row[0].strip()
                    if not key or key.lower() in ("id", "block", "方块", "名称", "item", "block_id"):
                        continue
                    try:
                        counts[key] = counts.get(key, 0) + int(float(row[total_col]))
                    except ValueError:
                        continue
        except OSError as exc:
            QMessageBox.warning(self, "CSV", f"读取失败：{exc}")
            self._workbook.setCurrentIndex(0)
            return
        self._csv_mode = True
        self._csv_source = csv_path
        self._litematic_path = None
        self._base_rows = sorted_block_counts(counts)
        self._status.setText(f"自定义文件：{csv_path.name}")
        self._fill_table(self._base_rows, gray=False)

    def _apply_workbook_selection(self, *, force_refresh: bool = False) -> None:
        self._region_name = self._current_region_param()
        self._start_material_flow(force_refresh=force_refresh)

    def _on_reload_clicked(self) -> None:
        if self._csv_mode and self._csv_source is not None:
            self._load_csv_counts(self._csv_source)
            return
        self._reload_from_context(force_refresh=True)

    def _on_entities_toggled(self, _on: bool) -> None:
        if not self._csv_mode:
            self._start_material_flow(force_refresh=True)

    def _start_material_flow(self, *, force_refresh: bool = False) -> None:
        if self._csv_mode:
            return
        path = self._props.active_file_path()
        if path is None:
            return
        resolved = path.resolve()
        inc = self._cb_entities.isChecked()
        region = self._region_name

        if (
            self._scan_prewarmer is not None
            and not self._csv_mode
            and region is None
            and not inc
            and self._scan_prewarmer.is_busy_for(resolved)
        ):
            self._clear_prewarm_wait()
            self._waiting_prewarm_path = resolved
            self._status.setText("后台正在扫描整个投影，与打开材料列表共用进度…")
            self._table.setRowCount(0)
            self._scan_prewarmer.finished_for_path.connect(self._on_wait_prewarm_done)
            return

        if self._thread is not None and self._thread.isRunning():
            self._pending_queue = True
            return

        cached = load_material_cache(resolved, region_name=region, include_entities=inc)
        need_scan = True
        if cached is not None:
            counts, mns = cached
            stale = cache_is_stale(resolved, mns)
            self._base_rows = sorted_block_counts(counts)
            if force_refresh:
                self._fill_table(self._base_rows, gray=True)
                self._status.setText("已从缓存显示（中灰色）；正在重新扫描…")
            elif stale:
                self._fill_table(self._base_rows, gray=True)
                self._status.setText("已从缓存显示（中灰色）；文件已变更，正在后台刷新…")
            else:
                self._fill_table(self._base_rows, gray=False)
                self._status.setText("已从缓存加载。")
                need_scan = False
        else:
            self._status.setText("正在扫描投影…")
            self._table.setRowCount(0)

        if not need_scan:
            return

        self._thread = _MaterialScanThread(
            resolved,
            region_name=region,
            include_entities=inc,
            parent=self,
        )
        self._thread.finished_ok.connect(self._on_scan_done)
        self._thread.failed.connect(self._on_scan_failed)
        self._thread.finished.connect(self._on_scan_thread_finished)
        self._thread.start()

    def _on_scan_thread_finished(self) -> None:
        self._thread = None
        if self._pending_queue:
            self._pending_queue = False
            self._start_material_flow(force_refresh=True)

    def _on_scan_done(self, counts: dict) -> None:
        th = self.sender()
        if not isinstance(th, _MaterialScanThread):
            return
        expected = th._path
        path = self._props.active_file_path()
        if path is None or path.resolve() != expected:
            return
        if th._region != self._region_name or th._inc != self._cb_entities.isChecked():
            return
        try:
            mns = file_mtime_ns(path)
            save_material_cache(
                path.resolve(),
                region_name=th._region,
                include_entities=th._inc,
                counts=dict(counts),
                mtime_ns=mns,
            )
        except OSError:
            pass
        self._base_rows = sorted_block_counts(dict(counts))
        self._fill_table(self._base_rows, gray=False)
        self._status.setText("已更新为最新扫描结果。")

    def _on_scan_failed(self, msg: str) -> None:
        self._status.setText(msg)
        QMessageBox.warning(self, "材料列表", msg)

    def _fill_table(self, rows: list[tuple[str, int]], *, gray: bool) -> None:
        self._table_icon_fill_gen += 1
        fill_gen = self._table_icon_fill_gen

        self._table_gray = gray
        mult = self._spin_mult.value()
        if gray:
            brush = QBrush(QColor(140, 140, 140))
        else:
            brush = self._table.palette().brush(QPalette.ColorRole.Text)
        n = len(rows)
        self._table.setRowCount(n)

        if n <= _ICON_ROWS_INLINE:
            for i, (bid, cnt) in enumerate(rows):
                pm = material_list_block_icon_pixmap_32_for_block(bid)
                self._table.setCellWidget(i, 0, _material_list_icon_cell_widget(pm))

                name_item = QTableWidgetItem(_display_name(bid, self._cn))
                name_item.setForeground(brush)
                name_item.setData(Qt.ItemDataRole.UserRole, bid)
                self._table.setItem(i, 1, name_item)

                total_item = QTableWidgetItem(str(cnt * mult))
                total_item.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
                total_item.setForeground(brush)
                self._table.setItem(i, 2, total_item)
            _sync_material_list_name_column_width(self._table)
            sync_material_list_row_heights(self._table, self._theme_id)
            refresh_material_list_row_visuals(self._table)
            return

        ph = material_list_block_icon_pixmap_32()
        for i, (bid, cnt) in enumerate(rows):
            self._table.setCellWidget(i, 0, _material_list_icon_cell_widget(QPixmap(ph)))
            name_item = QTableWidgetItem(_display_name(bid, self._cn))
            name_item.setForeground(brush)
            name_item.setData(Qt.ItemDataRole.UserRole, bid)
            self._table.setItem(i, 1, name_item)
            total_item = QTableWidgetItem(str(cnt * mult))
            total_item.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            total_item.setForeground(brush)
            self._table.setItem(i, 2, total_item)
        _sync_material_list_name_column_width(self._table)
        sync_material_list_row_heights(self._table, self._theme_id)
        refresh_material_list_row_visuals(self._table)
        self._icon_chunk_rows = rows
        self._icon_chunk_idx = 0
        QTimer.singleShot(0, lambda: self._pump_material_list_icon_column(fill_gen))

    def _pump_material_list_icon_column(self, gen: int) -> None:
        if gen != self._table_icon_fill_gen:
            return
        rows = self._icon_chunk_rows
        i0 = self._icon_chunk_idx
        i1 = min(i0 + _ICON_COLUMN_CHUNK, len(rows))
        for i in range(i0, i1):
            bid, _cnt = rows[i]
            pm = material_list_block_icon_pixmap_32_for_block(bid)
            self._table.setCellWidget(i, 0, _material_list_icon_cell_widget(pm))
        self._icon_chunk_idx = i1
        if i1 < len(rows):
            QTimer.singleShot(0, lambda: self._pump_material_list_icon_column(gen))
        else:
            refresh_material_list_row_visuals(self._table)

    def _refresh_multiplier_only(self) -> None:
        if not self._base_rows:
            return
        self._fill_table(self._base_rows, gray=self._table_gray)

    def _on_export_clicked(self) -> None:
        if not self._base_rows:
            QMessageBox.information(self, "写入文件", "没有可导出的数据。")
            return
        internal_seg = "export"
        lit_display: str | None = None
        if self._litematic_path is not None:
            lit_display, internal_seg = _litematic_internal_metadata_names(self._litematic_path)
        elif self._csv_source is not None:
            internal_seg = _filename_safe_segment(self._csv_source.stem)
        suggested = _material_list_suggested_save_path(internal_segment=internal_seg, extension=".csv")
        path, filt = QFileDialog.getSaveFileName(
            self,
            "导出材料列表",
            suggested,
            "逗号分隔 (*.csv);;文本 (*.txt)",
        )
        if not path:
            return
        mult = self._spin_mult.value()
        rows = [(b, c * mult) for b, c in self._base_rows]
        title = _material_list_export_title(
            litematic_path=self._litematic_path,
            region_name=self._region_name,
            csv_source=self._csv_source,
            litematic_quoted_name=lit_display,
        )
        display_rows = [(_display_name(bid, self._cn), cnt) for bid, cnt in rows]
        try:
            if filt.startswith("逗号") or path.lower().endswith(".csv"):
                if not path.lower().endswith(".csv"):
                    path += ".csv"
                with open(path, "w", newline="", encoding="utf-8-sig") as f:
                    w = csv.writer(f, quoting=csv.QUOTE_NONNUMERIC)
                    w.writerow(["Item", "Total"])
                    for item, total in display_rows:
                        w.writerow([item, total])
            else:
                if not path.lower().endswith(".txt"):
                    path += ".txt"
                Path(path).write_text(
                    _material_list_txt_table(title, display_rows),
                    encoding="utf-8",
                )
        except OSError as exc:
            QMessageBox.warning(self, "写入失败", str(exc))
            return
        QMessageBox.information(self, "写入文件", f"已保存到：\n{path}")

    @staticmethod
    def open_for_properties(
        properties_page: PropertiesPage,
        parent: QWidget | None = None,
        *,
        initial_region_name: str | None = None,
        material_scan_prewarmer: MaterialListScanPrewarmer | None = None,
    ) -> MaterialListDialog:
        dlg = MaterialListDialog(
            properties_page,
            parent,
            initial_region_name=initial_region_name,
            material_scan_prewarmer=material_scan_prewarmer,
        )
        dlg.show()
        return dlg
