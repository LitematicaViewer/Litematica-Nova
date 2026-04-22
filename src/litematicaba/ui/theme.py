"""全局主题：QSS 或 Qt 标准样式绘制（见 design §2.0.5）。"""

from __future__ import annotations

from pathlib import Path

from PySide6.QtGui import QFont, QFontDatabase
from PySide6.QtWidgets import QApplication, QStyle, QStyleFactory

from litematicaba.core.settings import DEFAULT_THEME, VALID_THEMES
from litematicaba.ui.themes.base import ui_dir
from litematicaba.ui.themes import THEMES
from litematicaba.ui.themes.list_view_supplement import list_view_supplement_qss

THEME_PROP = "_rdm_theme_id"

# 启动时缓存：在首次 setStyleSheet 之前记录的平台 QStyle，供 QTDefault 恢复标准绘制。
_cached_base_style: QStyle | None = None
# 启动时缓存：记录应用默认字体，避免 Minecraft 字体切出后残留。
_cached_base_font: QFont | None = None


def cache_default_style(app: QApplication) -> None:
    """在首次对应用设置任何 ``setStyleSheet`` 之前调用，缓存 Qt 默认/平台样式。"""
    _ensure_base_style(app)


def _ensure_base_style(app: QApplication) -> None:
    global _cached_base_style, _cached_base_font
    if _cached_base_font is None:
        _cached_base_font = QFont(app.font())
    if _cached_base_style is not None:
        return
    keys = QStyleFactory.keys()
    name = app.style().objectName()
    if name and name in keys:
        _cached_base_style = QStyleFactory.create(name)
        if _cached_base_style is not None:
            return
    for preferred in ("windowsvista", "windows11", "windows", "macos", "fusion"):
        if preferred in keys:
            _cached_base_style = QStyleFactory.create(preferred)
            if _cached_base_style is not None:
                return
    if keys:
        _cached_base_style = QStyleFactory.create(keys[0])

_THEME_BUILDERS = {t.theme_id: t.build_qss for t in THEMES}

_THEME_WIDGET_STYLE_SUPPORT = {
    "QTDefault": set(),
    **{t.theme_id: t.widget_support for t in THEMES},
}
_loaded_font_files: set[str] = set()


def _qss_escaped_font_family(name: str) -> str:
    return name.replace("\\", "\\\\").replace('"', '\\"')


def _default_font_reset_qss() -> str:
    if _cached_base_font is None:
        return ""
    fam = _cached_base_font.family().strip()
    if not fam:
        return ""
    # 显式在顶层重置 font-family，避免从 Minecraft 切换时残留。
    return f'QWidget {{ font-family: "{_qss_escaped_font_family(fam)}"; }}\n'


def _try_load_theme_font(path: Path) -> None:
    s = str(path.resolve())
    if s in _loaded_font_files:
        return
    if not path.is_file():
        return
    fid = QFontDatabase.addApplicationFont(s)
    if fid != -1:
        _loaded_font_files.add(s)


def _ensure_theme_fonts(theme_id: str) -> None:
    if theme_id != "Minecraft":
        return
    font_dir = ui_dir() / "resources" / "font" / "minecraft"
    for name in ("unifont.ttf", "unifont.otf", "unifont-*.ttf", "unifont-*.otf"):
        for fp in font_dir.glob(name):
            _try_load_theme_font(fp)


def normalize_theme_id(theme_id: str) -> str:
    tid = theme_id if theme_id in VALID_THEMES else DEFAULT_THEME
    return tid if tid in _THEME_BUILDERS or tid == "QTDefault" else "QTDefault"


def theme_supports_widget(theme_id: str, widget_key: str) -> bool:
    tid = normalize_theme_id(theme_id)
    return widget_key in _THEME_WIDGET_STYLE_SUPPORT.get(tid, set())


def current_theme_id(app: QApplication | None) -> str:
    if app is None:
        return DEFAULT_THEME
    v = app.property(THEME_PROP)
    if isinstance(v, str):
        return normalize_theme_id(v)
    return DEFAULT_THEME


def pick_workspace_dim_qss(theme_id: str) -> str:
    """拣选主界面展开侧栏时，上下栏之间工作区遮罩：亮色主题用暗色，暗色主题用亮色。"""
    tid = normalize_theme_id(theme_id)
    if tid == "Minecraft":
        return "background-color: rgba(255, 255, 255, 0.38);"
    return "background-color: rgba(0, 0, 0, 0.42);"


def pick_side_collapsed_strip_width_px(theme_id: str) -> int:
    """拣选主界面右侧收起条宽度：Metro8/Metro10 为 31px，Minecraft 为 48px。"""
    tid = normalize_theme_id(theme_id)
    if tid == "Minecraft":
        return 48
    if tid in ("Metro8", "Metro10"):
        return 31
    return 31


def theme_stylesheet(theme_id: str) -> str:
    tid = normalize_theme_id(theme_id)
    if tid == "QTDefault":
        return ""
    builder = _THEME_BUILDERS.get(tid)
    if builder is None:
        return ""
    qss = builder()
    extra = list_view_supplement_qss(tid)
    if extra:
        qss = qss.rstrip() + "\n" + extra + "\n"
    return qss


def apply_theme(app: QApplication | None, theme_id: str) -> None:
    if app is None:
        return
    tid = normalize_theme_id(theme_id)
    prop = app.property(THEME_PROP)
    if isinstance(prop, str) and normalize_theme_id(prop) == tid:
        return
    _ensure_base_style(app)
    _ensure_theme_fonts(tid)
    app.setProperty(THEME_PROP, tid)
    qss = theme_stylesheet(tid)
    # Minecraft 会通过 QSS 指定像素风字体；切换到其他主题时主动恢复应用默认字体。
    if tid != "Minecraft" and _cached_base_font is not None:
        app.setFont(QFont(_cached_base_font))
        qss = _default_font_reset_qss() + qss
    if tid == "QTDefault" or not qss:
        app.setStyleSheet("")
        if _cached_base_style is not None:
            app.setStyle(_cached_base_style)
        return
    app.setStyleSheet(qss)
