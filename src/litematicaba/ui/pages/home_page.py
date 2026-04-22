from __future__ import annotations

from collections.abc import Callable

from PySide6.QtCore import Qt
from PySide6.QtWidgets import QLabel, QPushButton, QVBoxLayout, QWidget


class HomePage(QWidget):
    def __init__(
        self,
        *,
        on_open_file: Callable[[], None] | None = None,
        on_open_statistics: Callable[[], None] | None = None,
        on_open_render: Callable[[], None] | None = None,
    ) -> None:
        super().__init__()
        root = QVBoxLayout(self)
        root.setContentsMargins(32, 32, 32, 32)
        root.setSpacing(16)
        root.addStretch(1)

        title = QLabel("Litematica Blueprint Assistant")
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title_font = title.font()
        title_font.setPointSize(max(18, title_font.pointSize() + 4))
        title_font.setBold(True)
        title.setFont(title_font)

        hint = QLabel(
            "第一阶段主链：打开 .litematic、做结构分析、构建单一 3D cache，"
            "再用同一份 cache 驱动嵌入式预览和弹窗 viewer。"
        )
        hint.setAlignment(Qt.AlignmentFlag.AlignCenter)
        hint.setWordWrap(True)

        btn_open = QPushButton("打开 .litematic")
        btn_stats = QPushButton("查看分析统计")
        btn_render = QPushButton("进入 3D 渲染页")
        for btn in (btn_open, btn_stats, btn_render):
            btn.setMinimumWidth(220)
            btn.setMinimumHeight(40)

        if on_open_file is not None:
            btn_open.clicked.connect(on_open_file)
        else:
            btn_open.setEnabled(False)
        if on_open_statistics is not None:
            btn_stats.clicked.connect(on_open_statistics)
        else:
            btn_stats.setEnabled(False)
        if on_open_render is not None:
            btn_render.clicked.connect(on_open_render)
        else:
            btn_render.setEnabled(False)

        root.addWidget(title)
        root.addWidget(hint)
        root.addSpacing(8)
        root.addWidget(btn_open, 0, Qt.AlignmentFlag.AlignHCenter)
        root.addWidget(btn_stats, 0, Qt.AlignmentFlag.AlignHCenter)
        root.addWidget(btn_render, 0, Qt.AlignmentFlag.AlignHCenter)
        root.addStretch(2)
