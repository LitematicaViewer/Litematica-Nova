"""Deepslate / WebView 渲染包更新说明与（将来）联网检查入口。

按设计：Deepslate 在构建阶段打入静态资源，不会在每次启动时从 GitHub 拉取源码。
若日后提供独立的渲染资源包更新通道，在此实现下载与校验逻辑。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from PySide6.QtWidgets import QWidget


def show_renderer_update_info(parent: QWidget | None = None) -> None:
    """向用户说明当前更新策略；手动「检查更新」按钮调用。"""
    from PySide6.QtWidgets import QMessageBox

    QMessageBox.information(
        parent,
        "渲染组件（Deepslate）",
        "Deepslate 相关脚本在发布时随本软件一并打包，启动时不会从 GitHub 下载代码。\n\n"
        "若需新版渲染能力，请安装本软件的完整更新。\n"
        "将来若开放「渲染资源包」独立更新，将在此提供下载与校验。",
    )
