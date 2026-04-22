"""方块图标资源：内建 2D 种子目录、Vault 登记目录；索引为 ``block_2d/installed.json`` 与 ``block_icon/installed.json``。"""

from __future__ import annotations

import json
import shutil
import zipfile
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path

from litematicaba.core.config import project_root, user_data_dir

BLOCK_SOURCE_BUILTIN = "builtin"
BLOCK_SOURCE_VAULT = "vault"

BLOCK_TYPE_2D = "2D"
BLOCK_TYPE_ICON = "Icon"

VAULT_ENTRY_ID = f"{BLOCK_SOURCE_VAULT}:ccvaults"
VAULT_SITE_LABEL = "https://ccvaults.com/"

INSTALLED_FILENAME = "installed.json"
_LEGACY_SUBDIR_REGISTRY = "block_visuals_installed.json"


@dataclass
class InstalledBlockVisual:
    id: str
    version_label: str
    kind: str
    source_label: str
    source_key: str
    file_relpath: str
    active_material_list: bool = False
    active_layering: bool = False
    installed_at: str = ""


def _minecraft_assets_root() -> Path:
    return user_data_dir() / "minecraft-assets"


def _bundled_block_zip() -> Path:
    return project_root() / "pack-in" / "arr-private" / "block.zip"


def block_2d_initial_dir() -> Path:
    return _minecraft_assets_root() / "block_2d" / "initial"


def block_icon_vault_dir() -> Path:
    return _minecraft_assets_root() / "block_icon" / "vault"


def _block_2d_installed_path() -> Path:
    return _minecraft_assets_root() / "block_2d" / INSTALLED_FILENAME


def _block_icon_installed_path() -> Path:
    return _minecraft_assets_root() / "block_icon" / INSTALLED_FILENAME


def _legacy_installed_path() -> Path:
    """旧版：单文件登记在 minecraft-assets 根目录。"""
    return _minecraft_assets_root() / "block_visuals_installed.json"


def _migrate_subdir_registry_filename(parent_dir: Path) -> None:
    """将 ``block_visuals_installed.json`` 重命名为 ``installed.json``。"""
    new_p = parent_dir / INSTALLED_FILENAME
    if new_p.is_file():
        return
    old_p = parent_dir / _LEGACY_SUBDIR_REGISTRY
    if not old_p.is_file():
        return
    try:
        old_p.rename(new_p)
    except OSError:
        pass


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# 随安装包内 ``block/`` 大版本变更时递增，以触发重新种子化。
_BLOCK_2D_INITIAL_SEED_VERSION = "1"
_BLOCK_2D_SEED_SENTINEL = ".lba_block_2d_seed"


def ensure_initial_block_2d_seeded() -> None:
    """首次将安装包内 ``pack-in/arr-private/block.zip`` 解压到 ``block_2d/initial``。

    已完成后写入哨兵；后续调用立即返回，避免每次解压导致长时间主线程卡顿。
    """
    dst_root = block_2d_initial_dir()
    dst_root.mkdir(parents=True, exist_ok=True)
    sentinel = dst_root / _BLOCK_2D_SEED_SENTINEL
    try:
        if sentinel.is_file():
            ver = sentinel.read_text(encoding="utf-8").strip()
            if ver == _BLOCK_2D_INITIAL_SEED_VERSION:
                return
    except OSError:
        pass

    src_zip = _bundled_block_zip()
    if not src_zip.is_file():
        try:
            sentinel.write_text(_BLOCK_2D_INITIAL_SEED_VERSION, encoding="utf-8")
        except OSError:
            pass
        return

    try:
        with zipfile.ZipFile(src_zip, "r") as zf:
            zf.extractall(dst_root)
        sentinel.write_text(_BLOCK_2D_INITIAL_SEED_VERSION, encoding="utf-8")
    except (OSError, zipfile.BadZipFile):
        pass


def _entry_from_dict(row: dict) -> InstalledBlockVisual | None:
    try:
        kind = str(row.get("kind", ""))
        aml = bool(row.get("active_material_list", False))
        al = bool(row.get("active_layering", False))
        if "active" in row and "active_material_list" not in row and "active_layering" not in row:
            legacy_active = bool(row.get("active"))
            if legacy_active:
                aml = True
                if kind == BLOCK_TYPE_2D:
                    al = True
        if kind == BLOCK_TYPE_ICON:
            al = False
        item = InstalledBlockVisual(
            id=str(row.get("id", "")),
            version_label=str(row.get("version_label", "")),
            kind=kind,
            source_label=str(row.get("source_label", "")),
            source_key=str(row.get("source_key", "")),
            file_relpath=str(row.get("file_relpath", "")),
            active_material_list=aml,
            active_layering=al,
            installed_at=str(row.get("installed_at", "")),
        )
    except Exception:
        return None
    if not item.id or item.source_key in ("", BLOCK_SOURCE_BUILTIN):
        return None
    return item


def _load_registry(path: Path) -> list[InstalledBlockVisual]:
    if not path.is_file():
        return []
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return []
    if not isinstance(raw, list):
        return []
    out: list[InstalledBlockVisual] = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        item = _entry_from_dict(row)
        if item is not None:
            out.append(item)
    return out


def _save_registry(path: Path, items: list[InstalledBlockVisual]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = [asdict(i) for i in items]
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")


def _migrate_legacy_root_index() -> None:
    """将根目录 ``block_visuals_installed.json`` 按类型拆到 block_2d / block_icon 后删除旧文件。"""
    legacy = _legacy_installed_path()
    if not legacy.is_file():
        return
    try:
        raw = json.loads(legacy.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        try:
            legacy.unlink(missing_ok=True)
        except OSError:
            pass
        return
    if not isinstance(raw, list):
        try:
            legacy.unlink(missing_ok=True)
        except OSError:
            pass
        return
    into_2d: list[InstalledBlockVisual] = []
    into_icon: list[InstalledBlockVisual] = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        item = _entry_from_dict(row)
        if item is None:
            continue
        if item.kind == BLOCK_TYPE_ICON:
            into_icon.append(item)
        else:
            into_2d.append(item)
    if into_2d:
        existing = _load_registry(_block_2d_installed_path())
        ids = {e.id for e in existing}
        for e in into_2d:
            if e.id not in ids:
                existing.append(e)
                ids.add(e.id)
        _save_registry(_block_2d_installed_path(), existing)
    if into_icon:
        existing = _load_registry(_block_icon_installed_path())
        ids = {e.id for e in existing}
        for e in into_icon:
            if e.id not in ids:
                existing.append(e)
                ids.add(e.id)
        _save_registry(_block_icon_installed_path(), existing)
    try:
        legacy.unlink(missing_ok=True)
    except OSError:
        pass


def load_installed_block_visuals() -> list[InstalledBlockVisual]:
    ensure_initial_block_2d_seeded()
    root_a = _minecraft_assets_root()
    _migrate_subdir_registry_filename(root_a / "block_2d")
    _migrate_subdir_registry_filename(root_a / "block_icon")
    _migrate_legacy_root_index()
    return _load_registry(_block_2d_installed_path()) + _load_registry(_block_icon_installed_path())


def _save_both_registries(items_2d: list[InstalledBlockVisual], items_icon: list[InstalledBlockVisual]) -> None:
    _save_registry(_block_2d_installed_path(), items_2d)
    _save_registry(_block_icon_installed_path(), items_icon)


def set_active_material_list(installed_id: str) -> list[InstalledBlockVisual]:
    items_2d = _load_registry(_block_2d_installed_path())
    items_icon = _load_registry(_block_icon_installed_path())
    if installed_id == BLOCK_SOURCE_BUILTIN:
        for i in range(len(items_2d)):
            items_2d[i].active_material_list = False
        for i in range(len(items_icon)):
            items_icon[i].active_material_list = False
    else:
        found = False
        for i in range(len(items_2d)):
            on = items_2d[i].id == installed_id
            items_2d[i].active_material_list = on
            if on:
                found = True
        for i in range(len(items_icon)):
            on = items_icon[i].id == installed_id
            items_icon[i].active_material_list = on
            if on:
                found = True
        if not found:
            return load_installed_block_visuals()
    _save_both_registries(items_2d, items_icon)
    return load_installed_block_visuals()


def set_active_layering(installed_id: str) -> list[InstalledBlockVisual]:
    items_2d = _load_registry(_block_2d_installed_path())
    items_icon = _load_registry(_block_icon_installed_path())
    if installed_id == BLOCK_SOURCE_BUILTIN:
        for i in range(len(items_2d)):
            items_2d[i].active_layering = False
        for i in range(len(items_icon)):
            items_icon[i].active_layering = False
    else:
        found_2d = False
        for i in range(len(items_2d)):
            on = items_2d[i].id == installed_id and items_2d[i].kind == BLOCK_TYPE_2D
            items_2d[i].active_layering = on
            if on:
                found_2d = True
        for i in range(len(items_icon)):
            items_icon[i].active_layering = False
        if not found_2d:
            return load_installed_block_visuals()
    _save_both_registries(items_2d, items_icon)
    return load_installed_block_visuals()


def clear_active_material_list() -> list[InstalledBlockVisual]:
    return set_active_material_list(BLOCK_SOURCE_BUILTIN)


def clear_active_layering() -> list[InstalledBlockVisual]:
    return set_active_layering(BLOCK_SOURCE_BUILTIN)


def delete_installed_block_visual(installed_id: str) -> list[InstalledBlockVisual]:
    path_2d = _block_2d_installed_path()
    path_icon = _block_icon_installed_path()
    items_2d = _load_registry(path_2d)
    items_icon = _load_registry(path_icon)
    new_2d = [e for e in items_2d if e.id != installed_id]
    new_icon = [e for e in items_icon if e.id != installed_id]
    if len(new_2d) != len(items_2d) or len(new_icon) != len(items_icon):
        for item in items_icon:
            if item.id == installed_id and item.source_key == BLOCK_SOURCE_VAULT and item.id == VAULT_ENTRY_ID:
                p = _minecraft_assets_root() / item.file_relpath
                try:
                    if p.is_dir():
                        shutil.rmtree(p, ignore_errors=True)
                except OSError:
                    pass
                try:
                    p.mkdir(parents=True, exist_ok=True)
                except OSError:
                    pass
        _save_both_registries(new_2d, new_icon)
    return load_installed_block_visuals()


def register_vault_block_icon_slot() -> InstalledBlockVisual:
    """准备 ``block_icon/vault`` 目录并登记 Vault 来源。"""
    _migrate_legacy_root_index()
    vault = block_icon_vault_dir()
    vault.mkdir(parents=True, exist_ok=True)
    rel = vault.relative_to(_minecraft_assets_root())
    rel_s = rel.as_posix()
    entry = InstalledBlockVisual(
        id=VAULT_ENTRY_ID,
        version_label="未知",
        kind=BLOCK_TYPE_ICON,
        source_label=VAULT_SITE_LABEL,
        source_key=BLOCK_SOURCE_VAULT,
        file_relpath=rel_s,
        active_material_list=True,
        active_layering=False,
        installed_at=_now_iso(),
    )
    items_2d = _load_registry(_block_2d_installed_path())
    items_icon = _load_registry(_block_icon_installed_path())
    for i in range(len(items_2d)):
        items_2d[i].active_material_list = False
    found = False
    for i, old in enumerate(items_icon):
        if old.id == VAULT_ENTRY_ID:
            entry.active_layering = False
            items_icon[i] = entry
            found = True
            break
    if not found:
        items_icon.append(entry)
    for j in range(len(items_icon)):
        items_icon[j].active_material_list = items_icon[j].id == VAULT_ENTRY_ID
        items_icon[j].active_layering = False
    _save_both_registries(items_2d, items_icon)
    return entry


def normalize_material_list_block_local_id(block_id: str) -> str | None:
    """自材料列表方块键提取用于匹配贴图文件的本地 id（不含命名空间与方块状态）。"""
    raw = (block_id or "").strip()
    if not raw or raw.startswith("E/"):
        return None
    base = raw.split("[", 1)[0].strip()
    if ":" in base:
        return base.split(":", 1)[1].strip() or None
    return base or None


def _installed_json_fingerprint() -> str:
    parts: list[str] = []
    for path in (_block_2d_installed_path(), _block_icon_installed_path()):
        try:
            parts.append(str(path.stat().st_mtime_ns) if path.is_file() else "-")
        except OSError:
            parts.append("x")
    return "|".join(parts)


_ml_search_root_fp: str = ""
_ml_search_root_cached: Path | None = None


def material_list_icon_search_root() -> Path:
    """当前「应用到材料列表」所使用资源目录；无登记激活项时为内建 ``block_2d/initial``。"""
    global _ml_search_root_fp, _ml_search_root_cached
    ensure_initial_block_2d_seeded()
    root_a = _minecraft_assets_root()
    _migrate_subdir_registry_filename(root_a / "block_2d")
    _migrate_subdir_registry_filename(root_a / "block_icon")
    _migrate_legacy_root_index()
    fp = _installed_json_fingerprint()
    if fp == _ml_search_root_fp and _ml_search_root_cached is not None:
        return _ml_search_root_cached
    _ml_search_root_fp = fp
    for it in _load_registry(_block_2d_installed_path()) + _load_registry(_block_icon_installed_path()):
        if it.active_material_list and it.file_relpath.strip():
            p = _minecraft_assets_root() / it.file_relpath
            if p.is_dir():
                _ml_search_root_cached = p
                return p
    _ml_search_root_cached = block_2d_initial_dir()
    return _ml_search_root_cached


def _find_block_png_under(root: Path, local_id: str) -> Path | None:
    if not root.is_dir() or not local_id:
        return None
    safe = local_id.replace("\\", "/").split("/")[-1]
    if not safe or safe in (".", ".."):
        return None
    direct = [
        root / f"{safe}.png",
        root / "assets" / "minecraft" / "textures" / "block" / f"{safe}.png",
        root / "textures" / "block" / f"{safe}.png",
        root / "block" / f"{safe}.png",
    ]
    for c in direct:
        try:
            if c.is_file():
                return c
        except OSError:
            continue
    n = 0
    try:
        for p in root.rglob(f"{safe}.png"):
            n += 1
            if n > 600:
                break
            if p.is_file():
                return p
    except OSError:
        pass
    return None


_ml_icon_cache: dict[tuple[str, str], Path | None] = {}
_ml_icon_cache_tag: str = ""


def _material_list_icon_resolution_state() -> tuple[str, Path]:
    """缓存失效标签 + 当前搜索根目录。"""
    root = material_list_icon_search_root()
    parts = [str(root.resolve())]
    try:
        parts.append(str(root.stat().st_mtime_ns) if root.is_dir() else "0")
    except OSError:
        parts.append("0")
    parts.append(_installed_json_fingerprint())
    return "|".join(parts), root


def material_list_icon_resolution_tag() -> str:
    """与 ``resolve_material_list_icon_path`` 的环境标签一致；用于失效材料列表 pixmap 预载缓存。"""
    return _material_list_icon_resolution_state()[0]


def resolve_material_list_icon_path(block_id: str) -> Path | None:
    """解析材料列表用方块 PNG 路径；找不到则返回 ``None``（由 UI 回退占位图）。"""
    global _ml_icon_cache_tag, _ml_icon_cache
    tag, root = _material_list_icon_resolution_state()
    if tag != _ml_icon_cache_tag:
        _ml_icon_cache_tag = tag
        _ml_icon_cache.clear()
    lid = normalize_material_list_block_local_id(block_id)
    if not lid:
        return None
    key = (tag, lid)
    if key in _ml_icon_cache:
        return _ml_icon_cache[key]
    found = _find_block_png_under(root, lid)
    _ml_icon_cache[key] = found
    return found
