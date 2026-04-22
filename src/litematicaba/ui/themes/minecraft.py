"""Minecraft 主题。"""

from __future__ import annotations

from litematicaba.ui.table.minecraft_list_profile import minecraft_theme_table_item_min_height_px
from litematicaba.ui.themes.base import ThemeDef, qss_url, resource_dir

# 10×10 源图：四边各 4px 为九宫格边界（四角 4×4 固定，边条与中心区按 stretch 拉伸，等同 Windows 边框图逻辑）
_BTN_SLICE = 4
# 控件总高度目标（px）；QSS 的 min/max-height 作用于 **内容区**，不含 border 与 padding
_BTN_OUTER_HEIGHT = 40
_BTN_PAD_V = 4
_BTN_PAD_H = 12
# 侧栏内按钮总高
_NAV_BTN_OUTER_HEIGHT = 48
# 单行输入类控件总高（QSS min/max-height 为内容区，不含 border 与 padding）
_INPUT_OUTER_HEIGHT = 44
_INPUT_BORDER_PX = 2
_INPUT_PAD_V = 4
_INPUT_PAD_H = 8
def build_qss() -> str:
    d = resource_dir("minecraft")
    bn = qss_url(d / "button_normal_min.png")
    bh = qss_url(d / "button_hover_min.png")
    bc = qss_url(d / "button_check_min.png")
    bd = qss_url(d / "button_disabled_min.png")
    radio_mc_default = qss_url(d / "radio_default.png")
    radio_mc_hover = qss_url(d / "radio_hover.png")
    radio_mc_checked = qss_url(d / "radio_checked.png")
    radio_mc_checked_hover = qss_url(d / "radio_checked_hover.png")
    checkbox_mc_default = qss_url(d / "checkbox_default.png")
    checkbox_mc_hover = qss_url(d / "checkbox_hover.png")
    checkbox_mc_checked = qss_url(d / "checkbox_checked.png")
    checkbox_mc_checked_hover = qss_url(d / "checkbox_checked_hover.png")
    s = _BTN_SLICE
    border_v = 2 * s
    pad_v = 2 * _BTN_PAD_V
    # 总高 ≈ 内容区 min-height + 上下 padding + 上下 border（与 Qt 盒模型一致）
    h_content = _BTN_OUTER_HEIGHT - border_v - pad_v
    h_nav_content = _NAV_BTN_OUTER_HEIGHT - border_v - pad_v
    pv = _BTN_PAD_V
    ph = _BTN_PAD_H
    ib = _INPUT_BORDER_PX
    ib_v = 2 * ib
    ip_v = 2 * _INPUT_PAD_V
    h_input_content = _INPUT_OUTER_HEIGHT - ib_v - ip_v
    ipv = _INPUT_PAD_V
    iph = _INPUT_PAD_H
    list_item_row_h = minecraft_theme_table_item_min_height_px()

    def bi(url: str) -> str:
        return (
            f'border-width: {s}px; border-style: solid; border-color: transparent; '
            f'border-image: url("{url}") {s} {s} {s} {s} stretch stretch;'
        )

    return f"""
    QWidget {{
        background-color: #3c3c3c;
        color: #c6c6c6;
        font-size: 12pt;
        font-family: "Unifont", "GNU Unifont", "Courier New", monospace;
    }}
    QMainWindow > QWidget {{ background-color: #3c3c3c; }}
    QScrollArea {{ border: 2px solid #555555; background-color: #2b2b2b; }}
    QGroupBox {{
        font-weight: bold;
        border: 2px solid #555555;
        border-radius: 0;
        margin-top: 10px;
        padding-top: 8px;
        background-color: #2b2b2b;
    }}
    QGroupBox::title {{ subcontrol-origin: margin; left: 8px; padding: 0 4px; }}

    QListWidget::item,
    QTreeView::item,
    QTableView::item {{
        min-height: {list_item_row_h}px;
    }}

    QPushButton, QToolButton {{
        background-color: transparent;
        border-radius: 0;
        padding: {pv}px {ph}px;
        min-height: {h_content}px;
        max-height: {h_content}px;
        color: #ffffff;
        {bi(bn)}
    }}
    QPushButton:default, QToolButton:default {{
        color: #ffff00;
        {bi(bn)}
    }}
    QPushButton:hover:enabled, QToolButton:hover:enabled {{
        color: #ffff00;
        {bi(bh)}
    }}
    QPushButton:pressed, QToolButton:pressed {{
        color: #ffff00;
        {bi(bn)}
    }}
    QPushButton:checked, QToolButton:checked {{
        color: #ffff00;
        {bi(bc)}
    }}
    QPushButton:checked:hover:enabled, QToolButton:checked:hover:enabled {{
        color: #ffff00;
        {bi(bc)}
    }}
    QPushButton:checked:pressed, QToolButton:checked:pressed {{
        color: #ffff00;
        {bi(bn)}
    }}
    QPushButton:disabled, QToolButton:disabled {{
        color: #808080;
        {bi(bd)}
    }}
    QPushButton:checked:disabled, QToolButton:checked:disabled {{
        color: #808080;
        {bi(bd)}
    }}

    #navSidebar QPushButton#navExpand,
    #navSidebar QPushButton#navItem {{
        min-height: {h_nav_content}px;
        max-height: {h_nav_content}px;
    }}

    QLineEdit, QComboBox {{
        border: {ib}px solid #a0a0a0;
        border-radius: 0;
        padding: {ipv}px {iph}px;
        background-color: #000000;
        color: #ffffff;
        min-height: {h_input_content}px;
        max-height: {h_input_content}px;
    }}
    QLineEdit:focus, QComboBox:focus {{
        border: {ib}px solid #ffffff;
    }}
    QLineEdit:focus:hover, QComboBox:focus:hover {{
        border: {ib}px solid #ffffff;
    }}
    QComboBox::drop-down {{
        border: none;
        width: 24px;
        background-color: #000000;
    }}

    QAbstractSpinBox {{
        border: {ib}px solid #a0a0a0;
        border-radius: 0;
        padding: {ipv}px {iph}px;
        padding-right: 28px;
        background-color: #000000;
        color: #ffffff;
        min-height: {h_input_content}px;
        max-height: {h_input_content}px;
    }}
    QAbstractSpinBox:focus {{
        border: {ib}px solid #ffffff;
    }}
    QAbstractSpinBox:focus:hover {{
        border: {ib}px solid #ffffff;
    }}
    QAbstractSpinBox::up-button, QAbstractSpinBox::down-button {{
        subcontrol-origin: border;
        background-color: #000000;
        border: none;
        width: 14px;
    }}
    QAbstractSpinBox::up-button:hover:enabled, QAbstractSpinBox::down-button:hover:enabled {{
        background-color: #2a2a2a;
    }}

    QRadioButton::indicator {{
        width: 20px; height: 20px;
        border: none;
        background: transparent;
        image: url("{radio_mc_default}");
    }}
    QRadioButton::indicator:hover:enabled {{ image: url("{radio_mc_hover}"); }}
    QRadioButton::indicator:checked {{ image: url("{radio_mc_checked}"); }}
    QRadioButton::indicator:checked:hover:enabled {{ image: url("{radio_mc_checked_hover}"); }}
    QCheckBox::indicator {{
        width: 20px; height: 20px;
        border: none;
        background-color: transparent;
        image: url("{checkbox_mc_default}");
    }}
    QCheckBox::indicator:hover:enabled {{ image: url("{checkbox_mc_hover}"); }}
    QCheckBox::indicator:checked {{ image: url("{checkbox_mc_checked}"); }}
    QCheckBox::indicator:checked:hover:enabled {{ image: url("{checkbox_mc_checked_hover}"); }}

    QSlider::groove:horizontal {{ height: 8px; background: #555555; border-radius: 0; }}
    QSlider::handle:horizontal {{
        width: 12px; height: 18px; margin: -5px 0;
        background: #737373; border: 2px solid #000000; border-radius: 0;
    }}
    QFrame[frameShape="4"] {{ color: #555555; max-height: 2px; }}
    """


THEME_MINECRAFT = ThemeDef(theme_id="Minecraft", build_qss=build_qss, widget_support={"tile"})
