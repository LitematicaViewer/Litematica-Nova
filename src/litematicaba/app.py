from __future__ import annotations

import atexit
import sys

from PySide6.QtWidgets import QApplication, QMessageBox

from litematicaba.core.config import (
    set_user_accepted_unwritable_data_dir,
    user_data_dir_is_writable,
)
from litematicaba.core.native_backend_bridge import cleanup_backend_processes
from litematicaba.core.settings import load_settings
from litematicaba.ui.theme import apply_theme


_pyi_splash = None
try:
    import pyi_splash as _pyi_splash
except ImportError:
    _pyi_splash = None

def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("Litematica-BA")
    atexit.register(cleanup_backend_processes)
    app.aboutToQuit.connect(cleanup_backend_processes)
    if getattr(sys, "frozen", False) and not user_data_dir_is_writable():
        r = QMessageBox.question(
            None,
            "数据目录不可用",
            "当前目录无法写入数据，你的修改可能会在关闭软件后全部丢失。建议更换该软件的存放位置。你确定仍要继续在这个环境下使用软件吗？",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if r != QMessageBox.StandardButton.Yes:
            return 0
        set_user_accepted_unwritable_data_dir()
    settings = load_settings()
    apply_theme(app, settings.theme_id)

    from litematicaba.ui.main_window import MainWindow

    window = MainWindow()
    window.show()
    app.processEvents()
    if _pyi_splash is not None:
        try:
            _pyi_splash.close()
        except Exception:
            pass
    return app.exec()
