from __future__ import annotations

from PySide6.QtWidgets import QLabel, QPushButton, QTextEdit, QVBoxLayout, QWidget


class AnalyzePage(QWidget):
    def __init__(self) -> None:
        super().__init__()
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("分析模块"))
        layout.addWidget(QPushButton("执行分析（占位）"))
        output = QTextEdit()
        output.setReadOnly(True)
        output.setPlaceholderText("后续接入 src 里的分析逻辑输出...")
        layout.addWidget(output, 1)
