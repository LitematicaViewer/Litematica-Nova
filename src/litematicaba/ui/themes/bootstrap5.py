from __future__ import annotations

from litematicaba.ui.themes.base import ThemeDef


def build_qss() -> str:
    return """
    QWidget { background-color: #ffffff; color: #212529; font-size: 10pt; }
    QMainWindow > QWidget { background-color: #ffffff; }
    QScrollArea { border: none; background-color: transparent; }
    QGroupBox {
        font-weight: bold;
        border: 1px solid #dee2e6;
        border-radius: 6px;
        margin-top: 10px;
        padding-top: 8px;
        background-color: #f8f9fa;
    }
    QGroupBox::title { subcontrol-origin: margin; left: 10px; padding: 0 4px; }
    QPushButton {
        background-color: #0d6efd;
        color: #ffffff;
        border: none;
        border-radius: 6px;
        padding: 6px 14px;
        min-height: 22px;
    }
    QPushButton:hover { background-color: #0b5ed7; }
    QPushButton:checked { background-color: #198754; color: #ffffff; }
    QLineEdit, QComboBox {
        border: 1px solid #ced4da;
        border-radius: 6px;
        padding: 4px 10px;
        background-color: #ffffff;
        min-height: 22px;
    }
    """


THEME_BOOTSTRAP5 = ThemeDef(theme_id="Bootstrap5", build_qss=build_qss)
