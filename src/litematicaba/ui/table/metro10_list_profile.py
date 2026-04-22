"""Metro10「列表型表格」Profile：与 ``docs/style/table.md`` 中 ``table_list_*`` Token 对齐的单一数据源。

``Metro8`` 与 ``Metro10`` 在内容物列表上共用同一套扁平 Chrome 与几何度量时，从本模块取数（与当前实现一致）。"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Final

from PySide6.QtWidgets import QTableWidget

_METRO_LIST_THEME_IDS: Final[frozenset[str]] = frozenset({"Metro8", "Metro10"})


@dataclass(frozen=True, slots=True)
class Metro10ListTableTokens:
    """``table_list_*`` 在 Metro10 上的取值（文档为权威说明；此处为代码真源）。"""

    row_height: int = 60
    min_height: int = 400
    thumb_px: int = 40
    thumb_col_w: int = 56
    drag_col_w: int = 24
    drag_hint_draw_w: int = 10
    drag_hint_draw_h: int = 24
    header_bg_hex: str = "#ffffff"
    item_bg_primary_hex: str = "#ffffff"
    item_bg_secondary_hex: str = "#ffffff"
    hover_bg_hex: str = "#e6e6e6"
    selected_bg_hex: str = "#cccccc"
    text_fg_hex: str = "#000000"
    hover_text_hex: str = "#000000"
    selected_text_hex: str = "#000000"


METRO10_LIST_TABLE_TOKENS: Final = Metro10ListTableTokens()

# Metro8 / Metro10 内容列表共用：透明壳、无单元格边框、表头区透明（见 ``ContentListTableWidget._apply_metro_table_chrome``）
METRO_LIST_FLAT_TABLE_QSS: Final[str] = """
QTableWidget { background-color: transparent; border: none; outline: none; }
QTableWidget::item { background-color: transparent; border: none; outline: none; }
QTableWidget::item:focus { outline: none; border: none; }
QTableWidget:focus { outline: none; }
QHeaderView::section { background-color: transparent; }
"""


def is_metro_list_table_theme(theme_id: str) -> bool:
    """是否与 ``ContentListTableWidget`` 的 Metro 扁平列表分支一致（Metro8 / Metro10）。"""
    from litematicaba.ui.theme import normalize_theme_id

    return normalize_theme_id(theme_id) in _METRO_LIST_THEME_IDS


def is_metro10_list_table_theme(theme_id: str) -> bool:
    from litematicaba.ui.theme import normalize_theme_id

    return normalize_theme_id(theme_id) == "Metro10"


def content_list_row_height_px(theme_id: str) -> int | None:
    """内容物列表等 ``setRowHeight``：``QTDefault`` 为 ``None``（由 ``resizeRowToContents``）；``Minecraft`` 为 **44**；其余非默认主题为 Metro 样本 **60**。"""
    from litematicaba.ui.theme import normalize_theme_id

    tid = normalize_theme_id(theme_id)
    if tid == "QTDefault":
        return None
    if tid == "Minecraft":
        from litematicaba.ui.table.minecraft_list_profile import TABLE_LIST_ROW_HEIGHT_MINECRAFT_PX

        return TABLE_LIST_ROW_HEIGHT_MINECRAFT_PX
    return METRO10_LIST_TABLE_TOKENS.row_height


def apply_metro_list_flat_chrome_flags(table: QTableWidget) -> None:
    """Metro 列表扁平行为：无网格、无斑马纹、鼠标追踪、套 ``METRO_LIST_FLAT_TABLE_QSS``。

    不安装 delegate；由调用方在设置本 QSS 后安装 ``_MetroContentListDelegate`` 等。
    """
    table.setShowGrid(False)
    table.setAlternatingRowColors(False)
    table.setMouseTracking(True)
    table.viewport().setMouseTracking(True)
    table.setStyleSheet(METRO_LIST_FLAT_TABLE_QSS)


def apply_metro10_supplement_table_metrics(table: QTableWidget) -> None:
    """与 ``list_view_supplement`` / 操作表共用：最小高度与行高对齐 ``METRO10_LIST_TABLE_TOKENS``。"""
    t = METRO10_LIST_TABLE_TOKENS
    table.setMinimumHeight(t.min_height)
    vh = table.verticalHeader()
    vh.setDefaultSectionSize(t.row_height)
    for r in range(table.rowCount()):
        table.setRowHeight(r, t.row_height)


def bind_ui_test_tables_to_metro10_list_profile(
    *,
    content_list: "ContentListTableWidget",
    action_table: QTableWidget,
    theme_id: str,
) -> None:
    """UI 测试页：在 Metro10 下将「内容物列表 + 操作表」显式绑到本 Profile 度量与视口约定。

    内容列表在 ``ContentListTableWidget`` 构造时已按主题应用 Chrome；此处仅做 Metro10 下的二次对齐与操作表同步。
    """
    if not is_metro10_list_table_theme(theme_id):
        return

    from litematicaba.ui.widgets.mcmeta_standard_table import (
        apply_mcmeta_table_viewport_fill_below_items,
        apply_mcmeta_standard_table_row_heights,
        clear_mcmeta_table_current_cell,
    )

    t = METRO10_LIST_TABLE_TOKENS
    content_list.setMinimumHeight(t.min_height)
    apply_metro10_supplement_table_metrics(action_table)
    apply_mcmeta_table_viewport_fill_below_items(action_table, theme_id)
    clear_mcmeta_table_current_cell(action_table)
    apply_mcmeta_standard_table_row_heights(action_table, theme_id)
