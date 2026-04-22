from __future__ import annotations

from litematicaba.ui.themes.base import ThemeDef


def build_qss() -> str:
    return """
    QWidget { background-color: #ececec; color: #222222; font-size: 10pt; }
    QMainWindow > QWidget { background-color: #ececec; }
    QScrollArea { border: none; background-color: transparent; }
    QPushButton {
        background: qlineargradient(x1:0, y1:0, x2:0, y2:1, stop:0 #ffffff, stop:1 #e4e4e4);
        border: 1px solid #a8a8a8;
        border-radius: 5px;
        padding: 5px 12px;
        min-height: 22px;
    }
    QPushButton:checked {
        background: qlineargradient(x1:0, y1:0, x2:0, y2:1, stop:0 #6aabf5, stop:1 #3d8ce0);
        color: #ffffff;
        border-color: #2a6fc4;
    }
    QLineEdit, QComboBox {
        border: 1px solid #b4b4b4;
        border-radius: 4px;
        padding: 4px 8px;
        background-color: #ffffff;
        min-height: 22px;
    }
    """


THEME_LIGHTMAC = ThemeDef(theme_id="LightMac", build_qss=build_qss)
