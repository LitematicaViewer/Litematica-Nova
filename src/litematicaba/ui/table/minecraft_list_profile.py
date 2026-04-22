"""Minecraft「列表型表格」Profile：与 ``docs/style/table.md`` 中 Minecraft 列及 ``table_list_*`` 约定对齐的单一数据源。

几何上与 Metro 列表共用 ``METRO10_LIST_TABLE_TOKENS`` 的缩略图/拖拽列等；行高、配色、supplement QSS、视口填充等由本模块集中维护。"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Final

from PySide6.QtWidgets import QTableWidget

from litematicaba.ui.table.metro10_list_profile import METRO10_LIST_TABLE_TOKENS


@dataclass(frozen=True, slots=True)
class MinecraftListTableTokens:
    """``table_list_*`` 在 Minecraft 上的取值（文档为权威说明；此处为代码真源）。"""

    row_height_px: int = 44
    min_height_px: int = METRO10_LIST_TABLE_TOKENS.min_height
    item_bg_primary_hex: str = "#000000"
    item_bg_secondary_hex: str = "#1a1a1a"
    hover_bg_hex: str = "#2b2b2b"
    selected_bg_hex: str = "#3c3c3c"
    text_fg_hex: str = "#ffffff"
    # ``table_list_header_bg`` / ``table_list_header_text_fg``（与表体主底色一致）
    header_bg_hex: str = "#000000"
    header_text_fg_hex: str = "#c6c6c6"
    supplement_border: str = "2px solid #555555"
    supplement_padding_px: int = 4
    supplement_op_item_selected_bg_hex: str = "#000000"
    viewport_fill_below_items_hex: str = "#000000"


MINECRAFT_LIST_TABLE_TOKENS: Final = MinecraftListTableTokens()

# 与历史导出名兼容（含 ``list_view_supplement`` / ``view``）
TABLE_LIST_ROW_HEIGHT_MINECRAFT_PX: Final[int] = MINECRAFT_LIST_TABLE_TOKENS.row_height_px
TABLE_LIST_SELECTED_BG_MINECRAFT_HEX: Final[str] = MINECRAFT_LIST_TABLE_TOKENS.selected_bg_hex
MCMETA_VIEWPORT_FILL_BELOW_ITEMS_MINECRAFT_HEX: Final[str] = (
    MINECRAFT_LIST_TABLE_TOKENS.viewport_fill_below_items_hex
)


def is_minecraft_list_table_theme(theme_id: str) -> bool:
    from litematicaba.ui.theme import normalize_theme_id

    return normalize_theme_id(theme_id) == "Minecraft"


def _comma_selectors_with_suffix(selectors_csv: str, suffix: str) -> str:
    """``a, b`` → ``a{suffix}, b{suffix}``，供 ``::item`` 等子选择器。"""
    return ", ".join(f"{p.strip()}{suffix}" for p in selectors_csv.split(",") if p.strip())


def apply_minecraft_list_table_header_chrome(table: QTableWidget, theme_id: str) -> None:
    """Minecraft 列表表头：写在 ``horizontalHeader()`` 上，落实 ``table_list_header_*``（应用级 QSS 对 section 常无效）。"""
    from litematicaba.ui.theme import normalize_theme_id

    tid = normalize_theme_id(theme_id)
    hh = table.horizontalHeader()
    if not is_minecraft_list_table_theme(tid):
        hh.setStyleSheet("")
        return
    t = MINECRAFT_LIST_TABLE_TOKENS
    hh.setStyleSheet(
        f"QHeaderView::section {{ background-color: {t.header_bg_hex}; color: {t.header_text_fg_hex}; "
        f"border: none; padding: 4px 6px; }}"
    )


def apply_minecraft_content_list_chrome_flags(table: QTableWidget) -> None:
    """Minecraft 内容列表：无网格、斑马纹、空表级 QSS、鼠标追踪（见 ``ContentListTableWidget._apply_metro_table_chrome``）。"""
    table.setShowGrid(False)
    table.setAlternatingRowColors(True)
    table.setMouseTracking(True)
    table.viewport().setMouseTracking(True)
    table.setStyleSheet("")


def minecraft_list_view_supplement_qss(
    *,
    sel_mc_all_tables: str,
    sel_op_tables: str,
    sel_ui_content_list: str,
    sel_material_list: str,
) -> str:
    """``LIST_VIEW_QSS_BY_THEME[\"Minecraft\"]`` 片段；选择器由 ``list_view_supplement`` 注入。"""
    t = MINECRAFT_LIST_TABLE_TOKENS
    pad = t.supplement_padding_px
    sel_mc_items = _comma_selectors_with_suffix(sel_mc_all_tables, "::item")
    return f"""
    {sel_mc_all_tables} {{
      border: {t.supplement_border};
      border-radius: 0;
      background-color: {t.item_bg_primary_hex};
      alternate-background-color: {t.item_bg_secondary_hex};
      padding: {pad}px;
      outline: none;
    }}
    {sel_mc_items} {{
      color: {t.text_fg_hex};
    }}
    {sel_mc_all_tables} QHeaderView::section {{
      background-color: {t.header_bg_hex};
      color: {t.header_text_fg_hex};
      border: none;
      padding: 4px 6px;
    }}
    {sel_op_tables}::item:selected {{
      background-color: {t.supplement_op_item_selected_bg_hex};
      color: {t.text_fg_hex};
    }}
    {sel_ui_content_list}::item:selected {{
      background-color: {t.selected_bg_hex};
      color: {t.text_fg_hex};
    }}
    {sel_material_list}::item:selected {{
      background-color: {t.selected_bg_hex};
      color: {t.text_fg_hex};
    }}
    """


def minecraft_theme_table_item_min_height_px() -> int:
    """``themes/minecraft.py`` 中 ``QTableView::item`` min-height，与 ``table_list_row_height`` 一致。"""
    return MINECRAFT_LIST_TABLE_TOKENS.row_height_px


def bind_ui_test_tables_to_minecraft_list_profile(
    *,
    content_list: "ContentListTableWidget",
    action_table: QTableWidget,
    theme_id: str,
) -> None:
    """UI 测试页：在 Minecraft 下将「内容物列表 + 操作表」显式绑到本 Profile 度量与视口约定。"""
    if not is_minecraft_list_table_theme(theme_id):
        return

    from litematicaba.ui.widgets.mcmeta_standard_table import (
        apply_mcmeta_table_viewport_fill_below_items,
        apply_mcmeta_standard_table_row_heights,
        clear_mcmeta_table_current_cell,
    )

    t = MINECRAFT_LIST_TABLE_TOKENS
    content_list.setMinimumHeight(t.min_height_px)
    action_table.setMinimumHeight(t.min_height_px)
    apply_mcmeta_standard_table_row_heights(action_table, theme_id)
    apply_mcmeta_table_viewport_fill_below_items(action_table, theme_id)
    clear_mcmeta_table_current_cell(action_table)
