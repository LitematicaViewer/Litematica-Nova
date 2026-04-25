"""通用的进度展示对话框，支持进度条、文本更新和取消操作。"""

from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QDialog,
    QLabel,
    QProgressBar,
    QPushButton,
    QVBoxLayout,
    QWidget,
)


class GenericProgressDialog(QDialog):
    """包含进度条、状态文本和取消按钮的对话框。"""

    canceled = Signal()

    def __init__(self, title: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setMinimumWidth(400)
        self.setModal(True)

        layout = QVBoxLayout(self)

        self._label = QLabel("正在准备...")
        layout.addWidget(self._label)

        self._progress = QProgressBar()
        self._progress.setRange(0, 100)
        self._progress.setValue(0)
        layout.addWidget(self._progress)

        self._btn_cancel = QPushButton("取消")
        self._btn_cancel.clicked.connect(self._on_cancel_clicked)
        layout.addWidget(self._btn_cancel, 0, Qt.AlignmentFlag.AlignRight)

    def set_status(self, text: str) -> None:
        self._label.setText(text)

    def set_progress(self, value: int, total: int = 100) -> None:
        self._progress.setMaximum(total)
        self._progress.setValue(value)

    def _on_cancel_clicked(self) -> None:
        self.canceled.emit()
        self.reject()

    def closeEvent(self, event) -> None:  # type: ignore[no-untyped-def]
        self.canceled.emit()
        super().closeEvent(event)