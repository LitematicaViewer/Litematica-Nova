from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable


@dataclass(frozen=True)
class ThemeDef:
    theme_id: str
    build_qss: Callable[[], str]
    widget_support: set[str] = field(default_factory=set)


def ui_dir() -> Path:
    return Path(__file__).resolve().parents[1]


def resource_dir(theme_id: str) -> Path:
    return ui_dir() / "resources" / "theme" / theme_id


def icon_dir() -> Path:
    return ui_dir() / "resources" / "icon"


def qss_url(path: Path) -> str:
    return path.as_posix()
