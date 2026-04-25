"""分层物品图标 32×32 预载缓存（主线程读写；与当前图标包 ``layering_item_icon_resolution_tag`` 绑定）。"""

from __future__ import annotations

from PySide6.QtGui import QPixmap

from litematicaba.core.game_resource_item_icon import layering_item_icon_resolution_tag

_tag: str = ""
_pixmaps: dict[str, QPixmap] = {}


def sync_layering_item_icon_pixmap_cache_tag() -> None:
    """若图标包或根目录变更则清空预载表。"""
    global _tag, _pixmaps
    t = layering_item_icon_resolution_tag()
    if t != _tag:
        _tag = t
        _pixmaps.clear()


def prewarmed_scaled_layering_item_pixmap(local_id: str | None) -> QPixmap | None:
    if not local_id:
        return None
    sync_layering_item_icon_pixmap_cache_tag()
    pm = _pixmaps.get(local_id)
    if pm is None or pm.isNull():
        return None
    return pm


def layering_item_icon_prewarm_cache_has_entries() -> bool:
    sync_layering_item_icon_pixmap_cache_tag()
    return bool(_pixmaps)


def store_prewarmed_layering_item_pixmap(local_id: str, pm: QPixmap) -> None:
    if not local_id or pm.isNull():
        return
    sync_layering_item_icon_pixmap_cache_tag()
    if local_id not in _pixmaps:
        _pixmaps[local_id] = pm