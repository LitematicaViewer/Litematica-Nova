"""材料列表方块图标 32×32 预载缓存（主线程读写；与当前图标包 ``material_list_icon_resolution_tag`` 绑定）。"""

from __future__ import annotations

from PySide6.QtCore import Qt
from PySide6.QtGui import QPixmap

from litematicaba.core.game_resource_block_icon import material_list_icon_resolution_tag

_tag: str = ""
_pixmaps: dict[str, QPixmap] = {}


def sync_material_list_icon_pixmap_cache_tag() -> None:
    """若图标包或根目录变更则清空预载表。"""
    global _tag, _pixmaps
    t = material_list_icon_resolution_tag()
    if t != _tag:
        _tag = t
        _pixmaps.clear()


def prewarmed_scaled_pixmap_for_local_id(local_id: str | None) -> QPixmap | None:
    """``local_id`` 为 ``normalize_material_list_block_local_id`` 的结果。"""
    if not local_id:
        return None
    sync_material_list_icon_pixmap_cache_tag()
    pm = _pixmaps.get(local_id)
    if pm is None or pm.isNull():
        return None
    return pm


def material_list_icon_prewarm_cache_has_entries() -> bool:
    """当前图标包环境下是否已有预载 pixmap（用于选项切换时的提示）。"""
    sync_material_list_icon_pixmap_cache_tag()
    return bool(_pixmaps)


def store_prewarmed_scaled_pixmap(local_id: str, pm: QPixmap) -> None:
    """仅主线程调用；``local_id`` 为方块本地名（与 PNG 主文件名一致）。"""
    if not local_id or pm.isNull():
        return
    sync_material_list_icon_pixmap_cache_tag()
    if local_id not in _pixmaps:
        _pixmaps[local_id] = pm
