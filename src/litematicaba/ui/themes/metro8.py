"""Metro8 主题。侧栏 Win10 导航 QSS 与 Metro10 共用 ``sidebar_nav_win10``（仅 Metro 系列启用）。"""

from __future__ import annotations

from litematicaba.ui.themes.base import ThemeDef, qss_url, resource_dir
from litematicaba.ui.themes.sidebar_nav_win10 import NAV_SIDEBAR_WIN10_QSS


def build_qss() -> str:
    d = resource_dir("metro8")
    radio8_default = qss_url(d / "radio_default.svg")
    radio8_checked = qss_url(d / "radio_checked.svg")
    checkbox8_default = qss_url(d / "checkbox_default.svg")
    checkbox8_checked = qss_url(d / "checkbox_checked.svg")

    return (
        NAV_SIDEBAR_WIN10_QSS
        + """
    QWidget { background-color: #ffffff; color: #000000; font-size: 10pt; }
    QMainWindow > QWidget { background-color: #ffffff; }
    QScrollArea { border: none; background-color: transparent; }
    QGroupBox {
        font-weight: bold;
        border: none;
        border-radius: 0;
        margin-top: 8px;
        padding-top: 6px;
        background-color: #e6e6e6;
    }
    QGroupBox::title { subcontrol-origin: margin; left: 8px; padding: 0 4px; }
    QPushButton {
        background-color: #00a300;
        color: #ffffff;
        border: none;
        border-radius: 0;
        padding: 8px 14px;
        min-height: 24px;
    }
    QPushButton:hover { background-color: #008a00; }
    QPushButton:pressed { background-color: #007000; }
    QPushButton:checked { background-color: #0078d7; color: #ffffff; }
    QLineEdit, QComboBox {
        border: 2px solid #000000;
        border-radius: 0;
        padding: 4px 8px;
        background-color: #ffffff;
        min-height: 22px;
    }
    QRadioButton::indicator {
        width: 20px; height: 20px;
        border: none;
        background: transparent;
        image: url("%s");
    }
    QRadioButton::indicator:checked { image: url("%s"); }
    QRadioButton::indicator:checked:hover:enabled { image: url("%s"); }
    QCheckBox::indicator {
        width: 20px; height: 20px;
        border: none;
        background-color: transparent;
        image: url("%s");
    }
    QCheckBox::indicator:checked { image: url("%s"); }
    QCheckBox::indicator:checked:hover:enabled { image: url("%s"); }
    QSlider::groove:horizontal { height: 6px; background: #cccccc; border-radius: 0; }
    QSlider::handle:horizontal {
        width: 14px; height: 22px; margin: -8px 0;
        background: #00a300; border-radius: 0;
    }
    QFrame[frameShape="4"] { color: #000000; max-height: 2px; }
    """
        % (
            radio8_default,
            radio8_checked,
            radio8_checked,
            checkbox8_default,
            checkbox8_checked,
            checkbox8_checked,
        )
    )


THEME_METRO8 = ThemeDef(theme_id="Metro8", build_qss=build_qss, widget_support={"tile"})

