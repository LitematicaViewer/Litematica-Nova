"""方块图标资源：内建 2D 种子目录、Vault 登记目录；索引为 ``block_2d/installed.json`` 与 ``block_icon/installed.json``。"""

from __future__ import annotations

import json
import shutil
import urllib.request
import zipfile
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

from litematicaba.core.config import project_root, user_data_dir

BLOCK_SOURCE_BUILTIN = "builtin"
BLOCK_SOURCE_VAULT = "vault"

BLOCK_TYPE_2D = "2D"
BLOCK_TYPE_ICON = "Icon"

VAULT_ENTRY_ID = f"{BLOCK_SOURCE_VAULT}:ccvaults"
VAULT_SITE_LABEL = "https://ccvaults.com/"
VAULT_API_TOKEN_PATH = "/api/token"
VAULT_API_BLOCKS_PATH = "/api/assets/20.%20Blocks"
VAULT_API_ALL_ASSETS_PATH = "/api/assets/all"
VAULT_API_KEY = "mcicons-apikey-0201osaiudx-24493534"

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


def download_and_process_vault_icons(
    progress_callback: Callable[[int, int, str], bool] | None = None
) -> None:
    """通过 Vault API 索引下载方块图标并处理为 32x32 PNG。"""
    from PySide6.QtCore import Qt
    from PySide6.QtGui import QImage
    import concurrent.futures
    import http.cookiejar
    import urllib.error
    import urllib.parse
    import urllib.request

    def _emit_progress(current: int, total: int, status: str) -> bool:
        if progress_callback is None:
            return True
        return bool(progress_callback(current, total, status))

    def _fetch_json_with_auth(
        opener: urllib.request.OpenerDirector, url: str, headers: dict[str, str]
    ) -> object:
        req = urllib.request.Request(url, headers=headers)
        with opener.open(req, timeout=30) as resp:
            payload = resp.read().decode("utf-8", "ignore")
        try:
            return json.loads(payload)
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"Vault 返回非 JSON 数据：{url}") from exc

    if not _emit_progress(0, 1, "正在连接到 ccvaults.com..."):
        return

    vault_dir = block_icon_vault_dir()
    vault_dir.mkdir(parents=True, exist_ok=True)

    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    opener.addheaders = [
        (
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
            "(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
    ]

    try:
        opener.open(VAULT_SITE_LABEL, timeout=20).read()
    except OSError as exc:
        raise RuntimeError("无法连接 Vault 首页") from exc

    if not _emit_progress(0, 1, "正在获取访问令牌..."):
        return

    token_url = urllib.parse.urljoin(VAULT_SITE_LABEL, VAULT_API_TOKEN_PATH)
    token_req = urllib.request.Request(
        token_url,
        data=b"{}",
        method="POST",
        headers={
            "x-api-key": VAULT_API_KEY,
            "Content-Type": "application/json;charset=UTF-8",
            "Accept": "application/json, text/plain, */*",
            "Origin": VAULT_SITE_LABEL.rstrip("/"),
            "Referer": VAULT_SITE_LABEL,
            "X-Requested-With": "XMLHttpRequest",
        },
    )

    try:
        with opener.open(token_req, timeout=20) as token_resp:
            token_payload = token_resp.read().decode("utf-8", "ignore")
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"获取 Vault 令牌失败：HTTP {exc.code}") from exc
    except OSError as exc:
        raise RuntimeError("获取 Vault 令牌失败") from exc

    try:
        token = str(json.loads(token_payload).get("token", "")).strip()
    except json.JSONDecodeError as exc:
        raise RuntimeError("Vault 令牌响应解析失败") from exc
    if not token:
        raise RuntimeError("Vault 未返回可用访问令牌")

    auth_headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/json, text/plain, */*",
        "Origin": VAULT_SITE_LABEL.rstrip("/"),
        "Referer": VAULT_SITE_LABEL,
    }

    if not _emit_progress(0, 1, "正在获取方块图标索引..."):
        return

    block_assets_url = urllib.parse.urljoin(VAULT_SITE_LABEL, VAULT_API_BLOCKS_PATH)
    try:
        blocks_payload = _fetch_json_with_auth(opener, block_assets_url, auth_headers)
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"获取 Vault 方块索引失败：HTTP {exc.code}") from exc

    entries: list[tuple[str, str]] = []
    seen_block_ids: set[str] = set()
    category_name = "20. Blocks"

    if isinstance(blocks_payload, list):
        for row in blocks_payload:
            if not isinstance(row, dict):
                continue
            subcategories = row.get("subcategories")
            if not isinstance(subcategories, list):
                continue
            for sub in subcategories:
                if not isinstance(sub, dict):
                    continue
                sub_name = str(sub.get("name", "")).strip()
                files = sub.get("files")
                if not sub_name or not isinstance(files, list):
                    continue
                for file_name in files:
                    if not isinstance(file_name, str) or not file_name.lower().endswith(".png"):
                        continue
                    block_id = Path(file_name).stem.lower()
                    if not block_id or block_id in seen_block_ids:
                        continue
                    rel_path = "/".join(
                        urllib.parse.quote(seg, safe="")
                        for seg in ("assets", category_name, sub_name, file_name)
                    )
                    file_url = urllib.parse.urljoin(VAULT_SITE_LABEL, rel_path)
                    entries.append((block_id, file_url))
                    seen_block_ids.add(block_id)

    # blocks 专用索引失败时，回退全量索引。
    if not entries:
        all_assets_url = urllib.parse.urljoin(VAULT_SITE_LABEL, VAULT_API_ALL_ASSETS_PATH)
        try:
            all_payload = _fetch_json_with_auth(opener, all_assets_url, auth_headers)
        except urllib.error.HTTPError as exc:
            raise RuntimeError(f"获取 Vault 全量索引失败：HTTP {exc.code}") from exc

        if isinstance(all_payload, list):
            for row in all_payload:
                if not isinstance(row, dict):
                    continue
                files = row.get("files")
                if not isinstance(files, list):
                    continue
                for file_row in files:
                    if not isinstance(file_row, dict):
                        continue
                    file_name = str(file_row.get("file", "")).strip()
                    if not file_name.lower().endswith(".png"):
                        continue
                    category = str(file_row.get("category", "")).strip()
                    if category != category_name:
                        continue
                    subcategory = str(file_row.get("subcategory", "")).strip()
                    block_id = Path(file_name).stem.lower()
                    if not block_id or block_id in seen_block_ids:
                        continue
                    rel_parts = ["assets", category]
                    if subcategory:
                        rel_parts.append(subcategory)
                    rel_parts.append(file_name)
                    rel_path = "/".join(urllib.parse.quote(seg, safe="") for seg in rel_parts)
                    file_url = urllib.parse.urljoin(VAULT_SITE_LABEL, rel_path)
                    entries.append((block_id, file_url))
                    seen_block_ids.add(block_id)

    if not entries:
        raise RuntimeError("未解析到任何方块图标索引")

    total_count = len(entries)
    if not _emit_progress(0, total_count, f"准备并行下载 {total_count} 个图标..."):
        return

    downloaded_count = 0
    completed_count = 0

    import threading

    cancel_event = threading.Event()
    max_workers = min(12, max(4, total_count))

    def _download_one(entry: tuple[str, str]) -> bool:
        if cancel_event.is_set():
            return False
        block_id, file_url = entry
        try:
            req = urllib.request.Request(
                file_url,
                headers={
                    "User-Agent": "LitematicaBA-icon-manager/1.0",
                    "Referer": VAULT_SITE_LABEL,
                },
            )
            with urllib.request.urlopen(req, timeout=20) as resp:
                img_data = resp.read()
        except (urllib.error.HTTPError, OSError):
            return False

        if cancel_event.is_set():
            return False

        img = QImage.fromData(img_data)
        if img.isNull():
            return False

        if img.width() != 32 or img.height() != 32:
            img = img.scaled(
                32,
                32,
                Qt.AspectRatioMode.IgnoreAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )

        return bool(img.save(str(vault_dir / f"{block_id}.png"), "PNG"))

    executor = concurrent.futures.ThreadPoolExecutor(max_workers=max_workers)
    future_to_block: dict[concurrent.futures.Future[bool], str] = {}
    try:
        for entry in entries:
            fut = executor.submit(_download_one, entry)
            future_to_block[fut] = entry[0]

        for fut in concurrent.futures.as_completed(future_to_block):
            block_id = future_to_block[fut]
            completed_count += 1
            ok = False
            try:
                ok = bool(fut.result())
            except Exception:
                ok = False
            if ok:
                downloaded_count += 1

            if not _emit_progress(
                completed_count,
                total_count,
                f"正在下载图标: {block_id} ({completed_count}/{total_count})，成功 {downloaded_count}",
            ):
                cancel_event.set()
                for pending in future_to_block:
                    pending.cancel()
                break
    finally:
        executor.shutdown(wait=not cancel_event.is_set(), cancel_futures=cancel_event.is_set())

    if cancel_event.is_set():
        return

    if downloaded_count <= 0:
        raise RuntimeError("未成功下载任何方块图标")

    _emit_progress(total_count, total_count, f"下载完成：{downloaded_count}/{total_count}")


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


def resolve_material_list_icon_path_in_root(block_id: str, root: Path) -> Path | None:
    """在指定根目录下解析材料列表方块 PNG 路径（不带全局缓存）。"""
    lid = normalize_material_list_block_local_id(block_id)
    if not lid:
        return None
    return _find_block_png_under(root, lid)


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