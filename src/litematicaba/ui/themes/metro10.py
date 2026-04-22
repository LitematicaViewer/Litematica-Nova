from __future__ import annotations

from litematicaba.ui.themes.base import ThemeDef, qss_url, resource_dir
from litematicaba.ui.themes.sidebar_nav_win10 import NAV_SIDEBAR_WIN10_QSS


def build_qss() -> str:
    d = resource_dir("metro10")
    radio10_default = qss_url(d / "radio_default.svg")
    radio10_hover = qss_url(d / "radio_hover.svg")
    radio10_checked = qss_url(d / "radio_checked.svg")
    radio10_checked_hover = qss_url(d / "radio_checked_hover.svg")
    checkbox10_default = qss_url(d / "checkbox_default.svg")
    checkbox10_hover = qss_url(d / "checkbox_hover.svg")
    checkbox10_checked = qss_url(d / "checkbox_checked.svg")
    checkbox10_checked_hover = qss_url(d / "checkbox_checked_hover.svg")
    slider10_handle = qss_url(d / "slider_handle.svg")
    arrow10_up = qss_url(d / "arrow_up.svg")
    arrow10_down = qss_url(d / "arrow_down.svg")
    arrow10_left = qss_url(d / "arrow_left.svg")
    arrow10_right = qss_url(d / "arrow_right.svg")
    spin10_plus = qss_url(d / "spin_plus.svg")
    spin10_minus = qss_url(d / "spin_minus.svg")

    return (
        NAV_SIDEBAR_WIN10_QSS
        + """
    QWidget { background-color: #ffffff; color: #1a1a1a; font-size: 10pt; }
    QMainWindow > QWidget { background-color: #ffffff; }
    QScrollArea { border: none; background-color: transparent; }
    QGroupBox {
        font-weight: bold;
        border: 1px solid #c8c8c8;
        border-radius: 0px;
        margin-top: 10px;
        padding-top: 8px;
        background-color: #fafafa;
    }
    QGroupBox::title { subcontrol-origin: margin; left: 10px; padding: 0 4px; }

    /* 列表默认行高约定：60px；QComboBox 下拉项：32px */
    QListWidget::item,
    QTreeView::item,
    QTableView::item {
        min-height: 60px;
    }
    QComboBox QListView::item {
        min-height: 32px;
    }

    QPushButton, QToolButton {
        background-color: #cccccc;
        border: 2px solid transparent;
        border-radius: 0px;
        padding: 0px 12px;
        min-height: 27px;
        max-height: 27px;
    }
    QPushButton:hover:enabled, QToolButton:hover:enabled {
        border-color: #7a7a7a;
    }
    QPushButton:pressed, QToolButton:pressed { background-color: #999999; }
    QPushButton:checked, QToolButton:checked {
        background-color: #999999;
    }
    QPushButton:disabled, QToolButton:disabled {
        color: #7a7a7a;
    }

    QLineEdit, QComboBox {
        border: 2px solid #999999;
        border-radius: 0px;
        padding: 3px 8px;
        background-color: #ffffff;
        min-height: 22px;
    }
    QLineEdit:hover:enabled, QComboBox:hover:enabled { border-color: #666666; }
    QLineEdit:focus { border-color: #0078d7; }
    QLineEdit:focus:hover { border-color: #0078d7; }
    QComboBox::drop-down { border: none; width: 24px; }

    QAbstractSpinBox {
        border: 2px solid #999999;
        border-radius: 0px;
        padding: 0px 60px 0px 8px;
        background-color: #ffffff;
        min-height: 28px;
        max-height: 28px;
    }
    QAbstractSpinBox:hover:enabled { border-color: #666666; }
    QAbstractSpinBox:focus { border-color: #0078d7; }
    QAbstractSpinBox:focus:hover { border-color: #0078d7; }
    QAbstractSpinBox::up-button, QAbstractSpinBox::down-button {
        subcontrol-origin: border;
        background: transparent;
        border: none;
        width: 28px;
        height: 28px;
    }
    QAbstractSpinBox::up-button:hover:enabled, QAbstractSpinBox::down-button:hover:enabled { background: #e9e9e9; }
    QAbstractSpinBox::up-button:pressed, QAbstractSpinBox::down-button:pressed { background: #d9d9d9; }
    QAbstractSpinBox::up-button { subcontrol-position: right; right: 2px; }
    QAbstractSpinBox::down-button { subcontrol-position: right; left: -28px; right: 2px; }
    QAbstractSpinBox::up-arrow, QSpinBox::up-arrow, QDoubleSpinBox::up-arrow {
        image: url("%s");
        width: 16px;
        height: 16px;
    }
    QAbstractSpinBox::down-arrow, QSpinBox::down-arrow, QDoubleSpinBox::down-arrow {
        image: url("%s");
        width: 16px;
        height: 16px;
    }

    QRadioButton::indicator {
        width: 20px; height: 20px;
        border: none;
        background: transparent;
        image: url("%s");
    }
    QRadioButton::indicator:hover:enabled { image: url("%s"); }
    QRadioButton::indicator:checked { image: url("%s"); }
    QRadioButton::indicator:checked:hover:enabled { image: url("%s"); }
    QCheckBox::indicator {
        width: 20px; height: 20px;
        border: none;
        background-color: transparent;
        image: url("%s");
    }
    QCheckBox::indicator:hover:enabled { image: url("%s"); }
    QCheckBox::indicator:checked { image: url("%s"); }
    QCheckBox::indicator:checked:hover:enabled { image: url("%s"); }

    QSlider:horizontal { min-height: 24px; }
    QSlider::groove:horizontal { height: 2px; margin: 11px 0; background: transparent; border-radius: 1px; }
    QSlider::sub-page:horizontal { height: 2px; margin: 11px 0; background: #0078d7; border-radius: 1px; }
    QSlider::add-page:horizontal { height: 2px; margin: 11px 0; background: #999999; border-radius: 1px; }
    QSlider::handle:horizontal {
        width: 8px; height: 24px; margin: -10px 0;
        background: transparent;
        border: none;
        image: url("%s");
    }

    QScrollBar:vertical {
        background: #e9e9e9;
        width: 16px;
        margin: 16px 0 16px 0;
        border: none;
    }
    QScrollBar::handle:vertical {
        background: #bababa;
        min-height: 20px;
        border: none;
    }
    QScrollBar::handle:vertical:hover { background: #8c8c8c; }
    QScrollBar::handle:vertical:pressed { background: #5d5d5d; }
    QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical { background: transparent; }
    QScrollBar::sub-line:vertical {
        background: #e9e9e9;
        height: 16px;
        subcontrol-position: top;
        subcontrol-origin: margin;
        border: none;
    }
    QScrollBar::add-line:vertical {
        background: #e9e9e9;
        height: 16px;
        subcontrol-position: bottom;
        subcontrol-origin: margin;
        border: none;
    }
    QScrollBar::up-arrow:vertical { image: url("%s"); width: 16px; height: 16px; }
    QScrollBar::down-arrow:vertical { image: url("%s"); width: 16px; height: 16px; }

    QScrollBar:horizontal {
        background: #e9e9e9;
        height: 16px;
        margin: 0 16px 0 16px;
        border: none;
    }
    QScrollBar::handle:horizontal {
        background: #bababa;
        min-width: 20px;
        border: none;
    }
    QScrollBar::handle:horizontal:hover { background: #8c8c8c; }
    QScrollBar::handle:horizontal:pressed { background: #5d5d5d; }
    QScrollBar::add-page:horizontal, QScrollBar::sub-page:horizontal { background: transparent; }
    QScrollBar::sub-line:horizontal {
        background: #e9e9e9;
        width: 16px;
        subcontrol-position: left;
        subcontrol-origin: margin;
        border: none;
    }
    QScrollBar::add-line:horizontal {
        background: #e9e9e9;
        width: 16px;
        subcontrol-position: right;
        subcontrol-origin: margin;
        border: none;
    }
    QScrollBar::left-arrow:horizontal { image: url("%s"); width: 16px; height: 16px; }
    QScrollBar::right-arrow:horizontal { image: url("%s"); width: 16px; height: 16px; }
    """
        % (
            spin10_plus,
            spin10_minus,
            radio10_default,
            radio10_hover,
            radio10_checked,
            radio10_checked_hover,
            checkbox10_default,
            checkbox10_hover,
            checkbox10_checked,
            checkbox10_checked_hover,
            slider10_handle,
            arrow10_up,
            arrow10_down,
            arrow10_left,
            arrow10_right,
        )
    )


THEME_METRO10 = ThemeDef(theme_id="Metro10", build_qss=build_qss)
