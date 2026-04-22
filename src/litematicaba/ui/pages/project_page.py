from __future__ import annotations

from PySide6.QtWidgets import QLabel, QListWidget, QPushButton, QVBoxLayout, QWidget


class ProjectPage(QWidget):
    def __init__(self) -> None:
        super().__init__()
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("投影项目"))
        layout.addWidget(QPushButton("导入 .litematic 文件（占位）"))
        layout.addWidget(QListWidget())
