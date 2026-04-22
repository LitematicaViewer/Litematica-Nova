from __future__ import annotations

from litematicaba.ui.themes.base import ThemeDef


def build_qss() -> str:
    return """
    QWidget { background-color: #d8e4f0; color: #1a2a3a; font-size: 10pt; }
    QMainWindow > QWidget { background-color: #d8e4f0; }
    QScrollArea { border: none; background-color: transparent; }
    QGroupBox {
        font-weight: bold;
        border: 1px solid rgba(255,255,255,0.6);
        border-radius: 6px;
        margin-top: 10px;
        padding-top: 8px;
        background-color: rgba(255,255,255,0.35);
    }
    QGroupBox::title { subcontrol-origin: margin; left: 10px; padding: 0 4px; }
    QPushButton {
        background-color: rgba(255,255,255,0.45);
        border: 1px solid rgba(80,120,180,0.5);
        border-radius: 4px;
        padding: 6px 12px;
        min-height: 22px;
    }
    QPushButton:hover { background-color: rgba(255,255,255,0.65); }
    QPushButton:checked {
        background-color: rgba(0,114,198,0.85);
        color: #ffffff;
        border-color: #005a9e;
    }
    QLineEdit, QComboBox {
        border: 1px solid rgba(80,120,180,0.55);
        border-radius: 4px;
        padding: 4px 8px;
        background-color: rgba(255,255,255,0.7);
        min-height: 22px;
    }
    QSlider::groove:horizontal { height: 4px; background: rgba(0,0,0,0.12); border-radius: 2px; }
    QSlider::handle:horizontal {
        width: 16px; height: 16px; margin: -6px 0;
        background: #2b7bcd; border-radius: 8px;
    }
    """


THEME_GLASS7 = ThemeDef(theme_id="Glass7", build_qss=build_qss)
