"""公共工厂方法：对外暴露统一的内容物显示构造入口。"""

from __future__ import annotations

from PySide6.QtWidgets import QWidget

from litematicaba.ui.content_display.list_table.view import (
    ContentListTableWidget,
    ContentRow,
    generate_sample_content_rows,
)


def generate_content_rows(n: int = 20) -> list[ContentRow]:
    """生成内容物示例数据（供 UI 样例页与调试使用）。"""
    return generate_sample_content_rows(n)


def create_content_list_widget(
    rows: list[ContentRow],
    *,
    theme_id: str = "QTDefault",
    parent: QWidget | None = None,
) -> ContentListTableWidget:
    """创建列表视图（列表模式专用）。"""
    return ContentListTableWidget(rows, parent=parent, theme_id=theme_id)

