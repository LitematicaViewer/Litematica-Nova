"""为列表型 QTableWidget 追加与各主题一致的 QSS（主主题文件未覆盖 QAbstractItemView）。"""

from __future__ import annotations

from PySide6.QtGui import QColor

from litematicaba.core.settings import DEFAULT_THEME, VALID_THEMES
from litematicaba.ui.table.metro10_list_profile import METRO10_LIST_TABLE_TOKENS
from litematicaba.ui.table.minecraft_list_profile import (
    MINECRAFT_LIST_TABLE_TOKENS,
    TABLE_LIST_ROW_HEIGHT_MINECRAFT_PX,
    minecraft_list_view_supplement_qss,
)

# 仅作用于带 objectName 的宿主，避免污染其它表格控件。
# 注意：QTableWidget 行高基本不受 QSS 的 ::item min-height 控制，须由代码 setDefaultSectionSize/setRowHeight 应用下列数值。
_SEL_DIALOG = "QDialog#McmetaVersionPickerDialog QTableWidget#OptionsMetaDownloadTable"
_SEL_SECTION = "QWidget#McmetaStandardTableSection QTableWidget#UITestActionTable"
# UI 测试页「内容物」列表：Minecraft 下与操作表共用同一套表格皮肤
_SEL_UI_TEST_CONTENT_LIST = "QWidget#UiTestContentListSection QTableWidget#UITestContentListTable"
_SEL_LANG = "QDialog#GameResourceLanguageDialog QTableWidget#OptionsLanguageFileTable"
_SEL_BLOCK_ICON = "QDialog#GameResourceBlockIconDialog QTableWidget#OptionsBlockIconResourceTable"
_SEL_ITEM_ICON = "QDialog#GameResourceItemIconDialog QTableWidget#OptionsItemIconResourceTable"
_SEL_MATERIAL = "QDialog#MaterialListDialog QTableWidget#MaterialListTable"
_SEL_PROPERTIES_REGION = "QTableWidget#PropertiesRegionTable"
# mcmeta 操作表语义（选中 #000000）；不含材料表（材料表走列表型 delegate / Metro 覆写）
_SEL_OP_DIALOGS = f"{_SEL_DIALOG}, {_SEL_SECTION}, {_SEL_LANG}, {_SEL_BLOCK_ICON}, {_SEL_ITEM_ICON}"
_SEL = f"{_SEL_OP_DIALOGS}, {_SEL_MATERIAL}, {_SEL_PROPERTIES_REGION}"
# Minecraft：操作表 + UI 测试「内容物」表共用底纹；内容列表选中色与 delegate 一致（#3c3c3c）
_SEL_MC_ALL_TABLES = f"{_SEL}, {_SEL_UI_TEST_CONTENT_LIST}"

# QTDefault / Metro8 / Metro10 / Minecraft：不强制「行高 > 按钮高」；其中 Metro10、Minecraft 仍用下列固定行高。
# 其余主题必须保证行高大于典型 QPushButton，避免操作列裁切。
MCMETA_TABLE_ROW_HEIGHT_LOOSE_THEMES = frozenset({"QTDefault", "Metro8", "Metro10", "Minecraft"})
MCMETA_TABLE_ROW_HEIGHT_FALLBACK_TIGHT_PX = 52

MCMETA_TABLE_ROW_HEIGHT_PX_BY_THEME: dict[str, int] = {
    "Metro10": METRO10_LIST_TABLE_TOKENS.row_height,
    "Minecraft": TABLE_LIST_ROW_HEIGHT_MINECRAFT_PX,
    "Glass7": 52,
    "Fluent11": 52,
    "LightMac": 50,
    "Bootstrap5": 52,
}


def mcmeta_version_table_list_row_height_px(theme_id: str) -> int | None:
    tid = theme_id if theme_id in VALID_THEMES else DEFAULT_THEME
    if tid in MCMETA_TABLE_ROW_HEIGHT_PX_BY_THEME:
        return MCMETA_TABLE_ROW_HEIGHT_PX_BY_THEME[tid]
    if tid in MCMETA_TABLE_ROW_HEIGHT_LOOSE_THEMES:
        return None
    return MCMETA_TABLE_ROW_HEIGHT_FALLBACK_TIGHT_PX

# 与各主题控件疏密一致：版本列表区域最小高度（像素），由对话框 setMinimumHeight 使用。
MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME: dict[str, int] = {
    "QTDefault": 300,
    "Glass7": 320,
    "Metro8": 288,
    "Metro10": METRO10_LIST_TABLE_TOKENS.min_height,
    "Fluent11": 336,
    "LightMac": 312,
    "Bootstrap5": 324,
    "Minecraft": MINECRAFT_LIST_TABLE_TOKENS.min_height_px,
}


def mcmeta_version_table_min_height_px(theme_id: str) -> int:
    tid = theme_id if theme_id in VALID_THEMES else DEFAULT_THEME
    return MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME.get(tid, MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME["QTDefault"])


# 版本管理表格「整行悬停」底色（与 QSS 列表/底纹搭配；Minecraft 按产品约定）。
MCMETA_TABLE_ROW_HOVER_BG_HEX_BY_THEME: dict[str, str] = {
    "QTDefault": "#e8e8e8",
    "Glass7": "#c8dcff",
    "Metro8": "#d0d0d0",
    "Metro10": METRO10_LIST_TABLE_TOKENS.hover_bg_hex,
    "Fluent11": "#f0f6fc",
    "LightMac": "#e5e5e5",
    "Bootstrap5": "#e9ecef",
    "Minecraft": MINECRAFT_LIST_TABLE_TOKENS.hover_bg_hex,
}

# 悬停时首列自绘文字色；未列出则使用调色板默认文字色。
MCMETA_TABLE_ROW_HOVER_FG_HEX_BY_THEME: dict[str, str] = {
    "Minecraft": MINECRAFT_LIST_TABLE_TOKENS.text_fg_hex,
}


def mcmeta_table_row_hover_bg(theme_id: str) -> QColor:
    tid = theme_id if theme_id in VALID_THEMES else DEFAULT_THEME
    hx = MCMETA_TABLE_ROW_HOVER_BG_HEX_BY_THEME.get(tid, "#e8e8e8")
    return QColor(hx)


def mcmeta_table_row_hover_fg_optional(theme_id: str) -> QColor | None:
    tid = theme_id if theme_id in VALID_THEMES else DEFAULT_THEME
    hx = MCMETA_TABLE_ROW_HOVER_FG_HEX_BY_THEME.get(tid)
    return QColor(hx) if hx else None


LIST_VIEW_QSS_BY_THEME: dict[str, str] = {
    "Fluent11": f"""
    {_SEL} {{
      border: 1px solid #d1d1d6;
      border-radius: 6px;
      background-color: #ffffff;
      alternate-background-color: #f5f5f7;
      padding: 4px;
      outline: none;
    }}
    {_SEL}::item:selected {{
      background-color: #0067c0;
      color: #ffffff;
    }}
    """,
    "Glass7": f"""
    {_SEL} {{
      border: 1px solid rgba(80,120,180,0.55);
      border-radius: 4px;
      background-color: rgba(255,255,255,0.75);
      alternate-background-color: rgba(255,255,255,0.55);
      padding: 4px;
      outline: none;
    }}
    {_SEL}::item:selected {{
      background-color: rgba(0,114,198,0.9);
      color: #ffffff;
    }}
    """,
    "Metro8": f"""
    {_SEL} {{
      border: 2px solid #000000;
      border-radius: 0;
      background-color: #ffffff;
      alternate-background-color: #f0f0f0;
      padding: 2px;
      outline: none;
    }}
    {_SEL}::item:selected {{
      background-color: #0078d7;
      color: #ffffff;
    }}
    QDialog#MaterialListDialog QTableWidget#MaterialListTable::item:selected {{
      background-color: {METRO10_LIST_TABLE_TOKENS.selected_bg_hex};
      color: {METRO10_LIST_TABLE_TOKENS.text_fg_hex};
    }}
    """,
    "Metro10": f"""
    {_SEL} {{
      border: none;
      border-radius: 0;
      background-color: #ffffff;
      alternate-background-color: #ffffff;
      padding: 0px;
      outline: none;
    }}
    {_SEL}::item {{
      background-color: transparent;
      border: none;
    }}
    {_SEL}::item:selected {{
      background-color: #ffffff;
      color: #1a1a1a;
    }}
    {_SEL} QTableCornerButton::section {{
      background-color: #ffffff;
      border: none;
    }}
    QDialog#MaterialListDialog QTableWidget#MaterialListTable::item:selected {{
      background-color: {METRO10_LIST_TABLE_TOKENS.selected_bg_hex};
      color: {METRO10_LIST_TABLE_TOKENS.text_fg_hex};
    }}
    """,
    "Bootstrap5": f"""
    {_SEL} {{
      border: 1px solid #ced4da;
      border-radius: 6px;
      background-color: #ffffff;
      alternate-background-color: #f8f9fa;
      padding: 4px;
      outline: none;
    }}
    {_SEL}::item:selected {{
      background-color: #0d6efd;
      color: #ffffff;
    }}
    """,
    "LightMac": f"""
    {_SEL} {{
      border: 1px solid #b4b4b4;
      border-radius: 4px;
      background-color: #ffffff;
      alternate-background-color: #f6f6f6;
      padding: 4px;
      outline: none;
    }}
    {_SEL}::item:selected {{
      background-color: #3d8ce0;
      color: #ffffff;
    }}
    """,
    "Minecraft": minecraft_list_view_supplement_qss(
        sel_mc_all_tables=_SEL_MC_ALL_TABLES,
        sel_op_tables=_SEL_OP_DIALOGS,
        sel_ui_content_list=_SEL_UI_TEST_CONTENT_LIST,
        sel_material_list=_SEL_MATERIAL,
    ),
}


def list_view_supplement_qss(theme_id: str) -> str:
    return LIST_VIEW_QSS_BY_THEME.get(theme_id, "").strip()
