from __future__ import annotations

from litematicaba.ui.themes.base import ThemeDef


def build_qss() -> str:
    return """
    QWidget { background-color: #f7f7f8; color: #202020; font-size: 10pt; }
    QMainWindow > QWidget { background-color: #f7f7f8; }
    QScrollArea { border: none; background-color: transparent; }
    QGroupBox {
        font-weight: bold;
        border: 1px solid #e0e0e4;
        border-radius: 8px;
        margin-top: 12px;
        padding-top: 10px;
        background-color: #ffffff;
    }
    QGroupBox::title { subcontrol-origin: margin; left: 12px; padding: 0 6px; }
    QPushButton {
        background-color: #ffffff;
        border: 1px solid #d1d1d6;
        border-radius: 6px;
        padding: 6px 14px;
        min-height: 22px;
    }
    QPushButton:hover { background-color: #f0f6fc; border-color: #0067c0; }
    QPushButton:pressed { background-color: #e5eff8; }
    QPushButton:checked {
        background-color: #0067c0;
        color: #ffffff;
        border-color: #005499;
    }
    QLineEdit, QComboBox {
        border: 1px solid #d1d1d6;
        border-radius: 6px;
        padding: 4px 10px;
        background-color: #ffffff;
        min-height: 22px;
    }
    QComboBox::drop-down { border: none; width: 28px; }
    """


THEME_FLUENT11 = ThemeDef(theme_id="Fluent11", build_qss=build_qss)
