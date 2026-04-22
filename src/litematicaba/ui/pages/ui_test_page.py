"""UI 测试页（design §2.0.11）：主题与控件样本。"""

from __future__ import annotations

from PySide6.QtGui import QFontMetrics, QResizeEvent
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QRadioButton,
    QScrollArea,
    QSlider,
    QSpinBox,
    QStackedWidget,
    QScrollBar,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
    QSizePolicy,
)

from litematicaba.core.settings import AppSettings
from litematicaba.core.content_tile_layout_db import ContentTileLayoutDB, TileLayoutRecord
from litematicaba.ui.content_display.list_table.view import ContentListTableWidget, ContentRow
from litematicaba.ui.content_display.public.factory import generate_content_rows
from litematicaba.ui.content_display.tiles.inventory_grid import InventoryGridWidget, GridPos
from litematicaba.ui.table.metro10_list_profile import bind_ui_test_tables_to_metro10_list_profile
from litematicaba.ui.table.minecraft_list_profile import bind_ui_test_tables_to_minecraft_list_profile
from litematicaba.ui.theme import normalize_theme_id
from litematicaba.ui.widgets.mcmeta_standard_table import (
    OBJ_MCMETA_STANDARD_APPLY_BTN,
    OBJ_MCMETA_STANDARD_OP_BTN,
    McmetaStandardTableCellHost,
    apply_mcmeta_standard_table_row_heights,
    clear_mcmeta_table_current_cell,
    configure_mcmeta_standard_action_table,
    mcmeta_standard_wrap_action_button,
    reapply_mcmeta_standard_action_table_theme,
)

# 内容物：显示方式与排序联动（磁贴 ↔ 自由排序；杂志/瀑布 ↔ 非自由排序）
_VIEW_MODE_LIST = "列表"
_VIEW_MODES_WHEN_FREE_SORT = (
    "磁贴",
    "网格图标（大）",
    "网格图标（中）",
    "网格图标（小）",
    _VIEW_MODE_LIST,
)
_VIEW_MODES_WHEN_FREE_SORT_LOCKED = (
    "磁贴",
    "网格图标（大）",
    "网格图标（中）",
    "网格图标（小）",
    _VIEW_MODE_LIST,
)
_VIEW_MODES_WHEN_NOT_FREE_SORT = (
    "网格图标（大）",
    "网格图标（中）",
    "网格图标（小）",
    _VIEW_MODE_LIST,
    "杂志",
    "瀑布",
)

# 「排序」首项固定；其余项由内容物列表的表头动态生成（见 design §2.0.11b）
_SORT_FREE_LABEL = "自由排序"
_SORT_FREE_LOCKED_LABEL = "自由排序（锁定）"
_DEFAULT_SORT_BY_HEADER = "名称"
_CONTENT_TILE_LAYOUT_SCOPE = "ui_test_content_tiles"


def sort_combo_labels_from_column_headers(headers: tuple[str, ...]) -> list[str]:
    """由列表表头生成「排序」下拉框条目：自由排序 + 按<表头>（内容物数据库约定，见 design §2.0.11b）。"""
    return [_SORT_FREE_LABEL, _SORT_FREE_LOCKED_LABEL] + [f"按{h}" for h in headers]


class UiTestPage(QWidget):
    def __init__(
        self,
        *,
        show_tile_grid: bool = False,
        theme_id: str = "QTDefault",
        tile_auto_place_preferred_cols: int = 9,
        tile_view_right_padding_px: int = 64,
    ) -> None:
        super().__init__()
        self._show_tile_grid_enabled = show_tile_grid
        self._tile_auto_place_preferred_cols = tile_auto_place_preferred_cols
        self._tile_view_right_padding_px = tile_view_right_padding_px
        self._theme_id = normalize_theme_id(theme_id)
        self._content_tile_layout_db = ContentTileLayoutDB()
        self._content_tile_cols = 64
        self._content_tile_rows = 64
        self._content_tile_view_rows = 12
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)

        inner = QWidget()
        v = QVBoxLayout(inner)
        v.setSpacing(16)
        v.setContentsMargins(16, 16, 16, 24)

        title = QLabel("UI 测试页（title）")
        # 主题 QSS 可能通过 QWidget 统一设置 font-size，需在控件级别显式覆盖
        title.setStyleSheet("font-size: 22pt; background: transparent;")
        v.addWidget(title)

        subtitle = QLabel("主题与排版自检（subtitle）")
        subtitle.setStyleSheet("font-size: 18pt; background: transparent;")
        v.addWidget(subtitle)

        tag = QLabel("切换主题后对照本页（tag）")
        tag.setStyleSheet("font-size: 9pt; background: transparent; color: #999999;")
        v.addWidget(tag)

        body = QLabel(
            "本页集中展示与全应用共用的排版与控件样式，便于切换主题后回归检视。"
            "正文区用于观察<strong>行距</strong>与次要文字色；各模块应复用同一套 token，而非单独硬编码。（body）"
        )
        body.setWordWrap(True)
        body.setStyleSheet("background: transparent;")
        v.addWidget(body)

        tiles_subtitle = QLabel("磁贴")
        tiles_subtitle.setStyleSheet("font-size: 18pt; background: transparent;")
        v.addWidget(tiles_subtitle)

        self._inventory_grid_host = QWidget()
        self._inventory_grid_host_lay = QVBoxLayout(self._inventory_grid_host)
        self._inventory_grid_host_lay.setContentsMargins(0, 0, 0, 0)
        self._inventory_grid_host_lay.setSpacing(0)
        self._inventory_grid = self._build_demo_inventory_grid(self._theme_id)
        self._inventory_grid_host_lay.addWidget(self._inventory_grid, alignment=Qt.AlignmentFlag.AlignLeft)
        v.addWidget(self._inventory_grid_host, alignment=Qt.AlignmentFlag.AlignLeft)

        form = QWidget()
        fg = QGridLayout(form)
        fg.setHorizontalSpacing(12)
        fg.setVerticalSpacing(10)

        r1 = QRadioButton("单选项 A")
        r2 = QRadioButton("单选项 B")
        r1.setChecked(True)
        fg.addWidget(QLabel("单选："), 0, 0, Qt.AlignmentFlag.AlignTop)
        rb = QVBoxLayout()
        rb.addWidget(r1)
        rb.addWidget(r2)
        rbw = QWidget()
        rbw.setLayout(rb)
        fg.addWidget(rbw, 0, 1)

        fg.addWidget(QLabel("复选："), 1, 0, Qt.AlignmentFlag.AlignTop)
        fg.addWidget(QCheckBox("示例复选框"), 1, 1)

        fg.addWidget(QLabel("单行输入："), 2, 0)
        fg.addWidget(QLineEdit("占位文本"), 2, 1)

        fg.addWidget(QLabel("下拉框："), 3, 0)
        cb = QComboBox()
        cb.addItems(["选项一", "选项二", "选项三"])
        fg.addWidget(cb, 3, 1)

        slider_row = QHBoxLayout()
        sl = QSlider(Qt.Orientation.Horizontal)
        sl.setRange(0, 100)
        sl.setValue(40)
        val = QLabel("40")
        val.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        # 固定宽度，避免数值位数变化导致布局抖动（0~100 => 3 位）
        fm = QFontMetrics(val.font())
        val.setFixedWidth(fm.horizontalAdvance("000") + 8)
        sl.valueChanged.connect(lambda x: val.setText(str(x)))
        slider_row.addWidget(sl, 1)
        slider_row.addWidget(val)
        sw = QWidget()
        sw.setLayout(slider_row)
        fg.addWidget(QLabel("滑动条："), 4, 0)
        fg.addWidget(sw, 4, 1)

        fg.addWidget(QLabel("按钮："), 5, 0, Qt.AlignmentFlag.AlignTop)
        btn_row = QHBoxLayout()
        btn_row.setSpacing(8)
        btn_row.setContentsMargins(0, 0, 0, 0)

        b1 = QPushButton("普通按钮")
        b2 = QPushButton("可勾选")
        b2.setCheckable(True)
        b2.setChecked(True)
        b3 = QPushButton("禁用")
        b3.setEnabled(False)
        b4 = QPushButton("强调/默认")
        b4.setDefault(True)

        btn_row.addWidget(b1)
        btn_row.addWidget(b2)
        btn_row.addWidget(b3)
        btn_row.addWidget(b4)
        btn_row.addStretch()

        bw = QWidget()
        bw.setLayout(btn_row)
        fg.addWidget(bw, 5, 1)

        sp = QSpinBox()
        sp.setRange(-999, 999)
        sp.setValue(12)
        fg.addWidget(QLabel("数字调节："), 6, 0)
        fg.addWidget(sp, 6, 1)

        form_subtitle = QLabel("表单")
        form_subtitle.setStyleSheet("font-size: 18pt; background: transparent;")
        v.addWidget(form_subtitle)

        v.addWidget(form)

        content_subtitle = QLabel("内容物")
        content_subtitle.setStyleSheet("font-size: 18pt; background: transparent;")
        v.addWidget(content_subtitle)

        content_ctrl = QWidget()
        content_row = QHBoxLayout(content_ctrl)
        content_row.setContentsMargins(0, 0, 0, 0)
        content_row.setSpacing(16)

        content_row.addWidget(QLabel("显示方式："), 0, Qt.AlignmentFlag.AlignVCenter)
        self._view_mode_combo = QComboBox()
        content_row.addWidget(self._view_mode_combo, 1)

        content_row.addWidget(QLabel("排序："), 0, Qt.AlignmentFlag.AlignVCenter)
        self._sort_combo = QComboBox()
        content_row.addWidget(self._sort_combo, 1)

        # 参与「排序」下拉的表头（不含半隐藏列「缩略图」；含「大小」）
        self._content_column_headers: tuple[str, ...] = (
            "名称",
            "时间",
            "大小",
            "额外表头1",
            "额外表头2",
        )
        self._populate_sort_options_from_headers(self._content_column_headers)

        self._sort_combo.currentIndexChanged.connect(self._apply_view_modes_for_current_sort)
        self._apply_view_modes_for_current_sort()

        v.addWidget(content_ctrl)

        self._content_rows: list[ContentRow] = generate_content_rows(20)
        self._content_list = ContentListTableWidget(self._content_rows, theme_id=theme_id)
        self._content_list.setObjectName("UITestContentListTable")
        self._content_list_host = QWidget()
        self._content_list_host.setObjectName("UiTestContentListSection")
        _cl_host_lay = QVBoxLayout(self._content_list_host)
        _cl_host_lay.setContentsMargins(0, 0, 0, 0)
        _cl_host_lay.setSpacing(0)
        _cl_host_lay.addWidget(self._content_list)
        self._sort_combo.currentTextChanged.connect(self._on_content_sort_changed)
        self._on_content_sort_changed(self._sort_combo.currentText())

        self._content_view_stack = QStackedWidget()
        self._content_view_stack.addWidget(self._content_list_host)
        self._content_tile_grid = self._create_content_tile_grid(
            self._ordered_rows_for_sort(self._sort_combo.currentText())
        )
        self._content_tile_view = self._create_fixed_tile_view(self._content_tile_grid)
        self._content_view_stack.addWidget(self._content_tile_view)
        self._content_view_other = QLabel("当前显示方式尚未实现样式样本。")
        self._content_view_other.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._content_view_other.setStyleSheet("color: palette(mid);")
        self._content_view_stack.addWidget(self._content_view_other)
        v.addWidget(self._content_view_stack, 1)

        op_table_subtitle = QLabel("操作表")
        op_table_subtitle.setStyleSheet("font-size: 18pt; background: transparent;")
        v.addWidget(op_table_subtitle)

        self._mcmeta_section = QWidget()
        self._mcmeta_section.setObjectName("McmetaStandardTableSection")
        mcmeta_sec_lay = QVBoxLayout(self._mcmeta_section)
        mcmeta_sec_lay.setContentsMargins(0, 0, 0, 0)
        mcmeta_sec_lay.setSpacing(8)
        mcmeta_hint = QLabel(
            "与「选项 → 游戏资源 → 管理游戏资源」弹窗相同。"
            "Minecraft 下上方「内容物」列表与之共用边框与底纹，行悬停 #2b2b2b。"
        )
        mcmeta_hint.setWordWrap(True)
        mcmeta_hint.setStyleSheet("background: transparent; color: palette(mid);")
        mcmeta_sec_lay.addWidget(mcmeta_hint)
        self._mcmeta_demo_table = QTableWidget()
        self._mcmeta_hover = configure_mcmeta_standard_action_table(
            self._mcmeta_demo_table,
            self._theme_id,
            column_labels=("说明", "", ""),
        )
        self._mcmeta_demo_table.setObjectName("UITestActionTable")
        _demo_rows: list[tuple[str, bool]] = [
            ("已安装样本（应用 + 清除）", True),
            ("未安装样本（仅下载）", False),
            ("已安装样本 2", True),
            ("未安装样本 2", False),
        ]
        self._mcmeta_demo_table.setRowCount(len(_demo_rows))
        for i, (label, installed) in enumerate(_demo_rows):
            col0 = QTableWidgetItem(label)
            col0.setFlags(Qt.ItemFlag.ItemIsSelectable | Qt.ItemFlag.ItemIsEnabled)
            self._mcmeta_demo_table.setItem(i, 0, col0)
            if installed:
                apply_btn = QPushButton("应用")
                apply_btn.setObjectName(OBJ_MCMETA_STANDARD_APPLY_BTN)
                apply_btn.setCheckable(True)
                apply_btn.setAutoExclusive(False)
                apply_btn.setChecked(i == 0)
                self._mcmeta_demo_table.setCellWidget(
                    i,
                    1,
                    mcmeta_standard_wrap_action_button(
                        apply_btn, self._mcmeta_demo_table, i, self._mcmeta_hover
                    ),
                )
                op_btn = QPushButton("清除")
            else:
                self._mcmeta_demo_table.setCellWidget(
                    i,
                    1,
                    McmetaStandardTableCellHost(self._mcmeta_demo_table, i, self._mcmeta_hover),
                )
                op_btn = QPushButton("下载")
            op_btn.setObjectName(OBJ_MCMETA_STANDARD_OP_BTN)
            self._mcmeta_demo_table.setCellWidget(
                i,
                2,
                mcmeta_standard_wrap_action_button(
                    op_btn, self._mcmeta_demo_table, i, self._mcmeta_hover
                ),
            )
        apply_mcmeta_standard_table_row_heights(self._mcmeta_demo_table, self._theme_id)
        self._mcmeta_hover.set_hover_row(None)
        clear_mcmeta_table_current_cell(self._mcmeta_demo_table)
        mcmeta_sec_lay.addWidget(self._mcmeta_demo_table)
        bind_ui_test_tables_to_metro10_list_profile(
            content_list=self._content_list,
            action_table=self._mcmeta_demo_table,
            theme_id=self._theme_id,
        )
        bind_ui_test_tables_to_minecraft_list_profile(
            content_list=self._content_list,
            action_table=self._mcmeta_demo_table,
            theme_id=self._theme_id,
        )
        v.addWidget(self._mcmeta_section)

        self._view_mode_combo.currentTextChanged.connect(self._on_content_view_mode_changed)
        self._on_content_view_mode_changed(self._view_mode_combo.currentText())

        scroll.setWidget(inner)
        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.addWidget(scroll)

    def _populate_sort_options_from_headers(
        self,
        headers: tuple[str, ...],
        *,
        default_sort_label: str | None = None,
    ) -> None:
        """根据列表表头填充「排序」下拉框；默认选中「按名称」（若存在）。"""
        self._content_column_headers = headers
        labels = sort_combo_labels_from_column_headers(headers)
        prev = self._sort_combo.currentText()
        self._sort_combo.blockSignals(True)
        self._sort_combo.clear()
        self._sort_combo.addItems(labels)
        default_label = default_sort_label or f"按{_DEFAULT_SORT_BY_HEADER}"
        if default_label in labels:
            self._sort_combo.setCurrentText(default_label)
        elif prev in labels:
            self._sort_combo.setCurrentText(prev)
        else:
            self._sort_combo.setCurrentIndex(0)
        self._sort_combo.blockSignals(False)

    def set_content_column_headers(
        self,
        headers: tuple[str, ...],
        *,
        default_sort_label: str | None = None,
    ) -> None:
        """由内容物组的列表表头更新「排序」下拉框（业务层应保证含「名称」「时间」等约定列）。"""
        self._populate_sort_options_from_headers(headers, default_sort_label=default_sort_label)
        self._apply_view_modes_for_current_sort()

    def _apply_view_modes_for_current_sort(self) -> None:
        """按排序方式约束「显示方式」可选项；非法时回退为「列表」（默认按名称，见 design §2.0.11b）。"""
        sort_text = self._sort_combo.currentText()
        if sort_text == _SORT_FREE_LABEL:
            modes = _VIEW_MODES_WHEN_FREE_SORT
        elif sort_text == _SORT_FREE_LOCKED_LABEL:
            modes = _VIEW_MODES_WHEN_FREE_SORT_LOCKED
        else:
            modes = _VIEW_MODES_WHEN_NOT_FREE_SORT
        prev = self._view_mode_combo.currentText()
        self._view_mode_combo.blockSignals(True)
        self._view_mode_combo.clear()
        self._view_mode_combo.addItems(modes)
        self._view_mode_combo.blockSignals(False)
        if prev in modes:
            self._view_mode_combo.setCurrentText(prev)
        else:
            fallback = _VIEW_MODE_LIST if _VIEW_MODE_LIST in modes else modes[0]
            self._view_mode_combo.setCurrentText(fallback)
        if hasattr(self, "_content_view_stack"):
            self._on_content_view_mode_changed(self._view_mode_combo.currentText())

    def _on_content_view_mode_changed(self, text: str) -> None:
        if text == "磁贴":
            self._content_view_stack.setCurrentWidget(self._content_tile_view)
        elif text == _VIEW_MODE_LIST:
            self._content_view_stack.setCurrentWidget(self._content_list_host)
        else:
            self._content_view_stack.setCurrentWidget(self._content_view_other)

    def _on_content_sort_changed(self, text: str) -> None:
        self._content_list.apply_sort(text)
        if hasattr(self, "_content_tile_grid"):
            self._refresh_content_tile_grid(self._ordered_rows_for_sort(text))
            is_free_drag = text == _SORT_FREE_LABEL
            self._content_tile_grid.set_tiles_draggable(is_free_drag)
            self._content_tile_grid.set_draw_grid(self._show_tile_grid_enabled and is_free_drag)

    def _ordered_rows_for_sort(self, sort_label: str) -> list[ContentRow]:
        if sort_label == _SORT_FREE_LABEL:
            return list(self._content_rows)
        if not sort_label.startswith("按"):
            return list(self._content_rows)
        key_name = sort_label[1:]
        key_map = {
            "名称": lambda r: r.name.lower(),
            "时间": lambda r: r.sort_ts,
            "大小": lambda r: r.size_sort_key,
            "额外表头1": lambda r: r.extra1.lower(),
            "额外表头2": lambda r: r.extra2.lower(),
        }
        key_fn = key_map.get(key_name)
        if key_fn is None:
            return list(self._content_rows)
        return sorted(self._content_rows, key=key_fn)

    def _create_content_tile_grid(self, rows: list[ContentRow]) -> InventoryGridWidget:
        cols = self._content_tile_cols
        grid_rows = self._content_tile_rows
        is_free_drag = self._sort_combo.currentText() == _SORT_FREE_LABEL
        cell_px, gap_px = self._tile_metrics_for_theme(self._theme_id)
        grid = InventoryGridWidget(
            cols=cols,
            rows=grid_rows,
            cell_px=cell_px,
            gap_px=gap_px,
            draw_grid=self._show_tile_grid_enabled and is_free_drag,
        )
        grid.set_auto_place_preferred_cols(self._tile_auto_place_preferred_cols)
        grid.set_scroll_extent_padding(cols=4, rows=4)
        grid.set_tiles_draggable(is_free_drag)
        grid.set_layout_changed_callback(self._save_content_tile_layout)
        saved_layout = self._content_tile_layout_db.load_scope(_CONTENT_TILE_LAYOUT_SCOPE)
        for row in rows:
            saved = saved_layout.get(row.name)
            saved_pos = GridPos(saved.col, saved.row) if saved is not None else None
            span_cols = saved.span_cols if saved is not None else 4
            span_rows = saved.span_rows if saved is not None else 4
            try:
                grid.add_thumbnail_tile(
                    span_cols=span_cols,
                    span_rows=span_rows,
                    pos=saved_pos,
                    label=row.name,
                    thumb_path=row.thumb_path,
                    layout_key=row.name,
                )
            except ValueError:
                # 已存布局可能因规则变化而失效：回退到自动落位，避免整页失败。
                grid.add_thumbnail_tile(
                    span_cols=4,
                    span_rows=4,
                    pos=None,
                    label=row.name,
                    thumb_path=row.thumb_path,
                    layout_key=row.name,
                )
        return grid

    def _create_fixed_tile_view(self, grid: InventoryGridWidget) -> QWidget:
        holder = QWidget()
        holder.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Preferred)
        outer = QVBoxLayout(holder)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)

        body = QWidget()
        body.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        body_row = QHBoxLayout(body)
        body_row.setContentsMargins(0, 0, 0, 0)
        body_row.setSpacing(0)

        area = QScrollArea()
        area.setWidgetResizable(False)
        area.setFrameShape(QScrollArea.Shape.NoFrame)
        view_h = self._cells_to_px(self._content_tile_view_rows, grid.cell_px, grid.gap_px)
        area.setFixedHeight(view_h)
        area.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        area.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        area.setWidget(grid)
        self._content_tile_scroll_area = area

        vbar = QScrollBar(Qt.Orientation.Vertical)
        vbar.hide()
        self._content_tile_outer_vbar = vbar
        self._sync_external_vbar()

        hbar = QScrollBar(Qt.Orientation.Horizontal)
        hbar.hide()
        self._content_tile_outer_hbar = hbar
        self._sync_external_hbar()

        area.verticalScrollBar().rangeChanged.connect(lambda _a, _b: self._sync_external_vbar())
        area.verticalScrollBar().valueChanged.connect(vbar.setValue)
        vbar.valueChanged.connect(area.verticalScrollBar().setValue)
        area.horizontalScrollBar().rangeChanged.connect(lambda _a, _b: self._sync_external_hbar())
        area.horizontalScrollBar().valueChanged.connect(hbar.setValue)
        hbar.valueChanged.connect(area.horizontalScrollBar().setValue)

        body_row.addWidget(area)
        body_row.addWidget(vbar)
        body_row.addSpacing(50)
        outer.addWidget(body, 0, Qt.AlignmentFlag.AlignLeft)
        outer.addWidget(hbar, 0, Qt.AlignmentFlag.AlignLeft)
        regen_row = QHBoxLayout()
        regen_row.setContentsMargins(0, 8, 0, 0)
        regen_row.setSpacing(0)
        regen_btn = QPushButton("重新生成")
        regen_btn.clicked.connect(self._regenerate_content_tiles_for_test)
        regen_row.addWidget(regen_btn, 0, Qt.AlignmentFlag.AlignLeft)
        regen_row.addStretch(1)
        outer.addLayout(regen_row)
        self._update_content_tile_viewport_size()
        return holder

    def _cells_to_px(self, cells: int, cell_px: int, gap_px: int) -> int:
        if cells <= 0:
            return 0
        return cells * cell_px + (cells - 1) * gap_px

    def _sync_external_hbar(self) -> None:
        if not hasattr(self, "_content_tile_scroll_area"):
            return
        src = self._content_tile_scroll_area.horizontalScrollBar()
        dst = self._content_tile_outer_hbar
        dst.blockSignals(True)
        dst.setRange(src.minimum(), src.maximum())
        dst.setPageStep(src.pageStep())
        dst.setSingleStep(src.singleStep())
        dst.setValue(src.value())
        dst.blockSignals(False)
        dst.setVisible(src.maximum() > src.minimum())

    def _sync_external_vbar(self) -> None:
        if not hasattr(self, "_content_tile_scroll_area"):
            return
        src = self._content_tile_scroll_area.verticalScrollBar()
        dst = self._content_tile_outer_vbar
        dst.blockSignals(True)
        dst.setRange(src.minimum(), src.maximum())
        dst.setPageStep(src.pageStep())
        dst.setSingleStep(src.singleStep())
        dst.setValue(src.value())
        dst.blockSignals(False)
        dst.setVisible(src.maximum() > src.minimum())

    def _update_content_tile_viewport_size(self) -> None:
        if not hasattr(self, "_content_tile_scroll_area"):
            return
        vbar_w = self._content_tile_outer_vbar.sizeHint().width() if hasattr(self, "_content_tile_outer_vbar") else 16
        # 视图宽度只由窗口可用空间决定（右侧保留固定空白），不受当前内容宽度影响。
        available = max(240, self.width() - 50 - vbar_w - self._tile_view_right_padding_px)
        self._content_tile_scroll_area.setFixedWidth(available)
        self._content_tile_outer_hbar.setFixedWidth(available)
        self._sync_external_hbar()

    def _refresh_content_tile_grid(self, rows: list[ContentRow]) -> None:
        new_grid = self._create_content_tile_grid(rows)
        old_grid = self._content_tile_grid
        self._content_tile_grid = new_grid
        self._content_tile_scroll_area.takeWidget()
        self._content_tile_scroll_area.setWidget(new_grid)
        view_h = self._cells_to_px(self._content_tile_view_rows, new_grid.cell_px, new_grid.gap_px)
        self._content_tile_scroll_area.setFixedHeight(view_h)
        self._update_content_tile_viewport_size()
        self._sync_external_vbar()
        self._sync_external_hbar()
        old_grid.deleteLater()
        self._save_content_tile_layout()

    def _save_content_tile_layout(self) -> None:
        if not hasattr(self, "_content_tile_grid"):
            return
        records = [
            TileLayoutRecord(
                tile_key=tile_key,
                col=col,
                row=row,
                span_cols=span_cols,
                span_rows=span_rows,
            )
            for tile_key, col, row, span_cols, span_rows in self._content_tile_grid.export_layout_records()
        ]
        self._content_tile_layout_db.save_scope(_CONTENT_TILE_LAYOUT_SCOPE, records)
        # 内容变化后再次应用视口宽度，防止被内容 sizeHint 牵引。
        self._update_content_tile_viewport_size()

    def _regenerate_content_tiles_for_test(self) -> None:
        self._content_tile_layout_db.clear_scope(_CONTENT_TILE_LAYOUT_SCOPE)
        self._content_rows = generate_content_rows(len(self._content_rows))
        self._content_list.set_rows(self._content_rows)
        self._refresh_content_tile_grid(self._ordered_rows_for_sort(self._sort_combo.currentText()))

    def _tile_metrics_for_theme(self, theme_id: str) -> tuple[int, int]:
        tid = normalize_theme_id(theme_id)
        if tid == "Fluent11":
            # Fluent11：保持中型磁贴 100x100（2*44 + 12）
            return 44, 12
        return 48, 4

    def _build_demo_inventory_grid(self, theme_id: str) -> InventoryGridWidget:
        cell_px, gap_px = self._tile_metrics_for_theme(theme_id)
        grid = InventoryGridWidget(
            cols=10,
            rows=6,
            cell_px=cell_px,
            gap_px=gap_px,
            draw_grid=self._show_tile_grid_enabled,
        )
        grid.set_auto_place_preferred_cols(self._tile_auto_place_preferred_cols)
        grid.add_tile(span_cols=4, span_rows=4, pos=GridPos(0, 0), bg_color="#5c9fd6", label="大 4×4")
        grid.add_tile(span_cols=2, span_rows=2, pos=GridPos(4, 0), bg_color="#7eb87e", label="中 2×2")
        grid.add_tile(span_cols=1, span_rows=1, pos=GridPos(6, 1), bg_color="#d4a84b", label="小 1×1")
        grid.add_tile(span_cols=4, span_rows=2, pos=GridPos(4, 2), bg_color="#7d8fd6", label="宽 4×2")
        return grid

    def apply_settings(self, s: AppSettings) -> None:
        """同步选项中的「显示磁贴网格」到可拖拽磁贴区域。"""
        old_cell_px, old_gap_px = self._tile_metrics_for_theme(self._theme_id)
        self._theme_id = normalize_theme_id(s.theme_id)
        new_cell_px, new_gap_px = self._tile_metrics_for_theme(self._theme_id)
        self._show_tile_grid_enabled = s.show_tile_grid
        self._tile_auto_place_preferred_cols = s.tile_auto_place_preferred_cols
        self._tile_view_right_padding_px = s.tile_view_right_padding_px
        if (old_cell_px, old_gap_px) != (new_cell_px, new_gap_px):
            new_demo_grid = self._build_demo_inventory_grid(self._theme_id)
            self._inventory_grid_host_lay.replaceWidget(self._inventory_grid, new_demo_grid)
            self._inventory_grid.deleteLater()
            self._inventory_grid = new_demo_grid
            self._refresh_content_tile_grid(self._ordered_rows_for_sort(self._sort_combo.currentText()))
        self._inventory_grid.set_auto_place_preferred_cols(self._tile_auto_place_preferred_cols)
        self._inventory_grid.set_draw_grid(s.show_tile_grid)
        content_free_drag = self._sort_combo.currentText() == _SORT_FREE_LABEL
        self._content_tile_grid.set_auto_place_preferred_cols(self._tile_auto_place_preferred_cols)
        self._content_tile_grid.set_draw_grid(self._show_tile_grid_enabled and content_free_drag)
        self._content_list.apply_list_theme(s.theme_id)
        reapply_mcmeta_standard_action_table_theme(
            self._mcmeta_demo_table, self._mcmeta_hover, self._theme_id
        )
        bind_ui_test_tables_to_metro10_list_profile(
            content_list=self._content_list,
            action_table=self._mcmeta_demo_table,
            theme_id=self._theme_id,
        )
        bind_ui_test_tables_to_minecraft_list_profile(
            content_list=self._content_list,
            action_table=self._mcmeta_demo_table,
            theme_id=self._theme_id,
        )
        self._update_content_tile_viewport_size()

    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        self._update_content_tile_viewport_size()
