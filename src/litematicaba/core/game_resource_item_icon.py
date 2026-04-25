"""物品图标资源：内建 2D 种子目录、Vault 登记目录；索引为 ``item/installed.json``。"""

from __future__ import annotations

import concurrent.futures
import http.cookiejar
import json
import shutil
import threading
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

from litematicaba.core.config import project_root, user_data_dir

ITEM_SOURCE_BUILTIN = "builtin"
ITEM_SOURCE_VAULT = "vault"

VAULT_ENTRY_ID = f"{ITEM_SOURCE_VAULT}:ccvaults"
VAULT_SITE_LABEL = "https://ccvaults.com/"
VAULT_API_TOKEN_PATH = "/api/token"
VAULT_API_ITEMS_PATH = "/api/assets/10.%20Items"
VAULT_API_ALL_ASSETS_PATH = "/api/assets/all"
VAULT_API_KEY = "mcicons-apikey-0201osaiudx-24493534"

INSTALLED_FILENAME = "installed.json"


@dataclass
class InstalledItemIcon:
    id: str
    version_label: str
    source_label: str
    source_key: str
    file_relpath: str
    active_layering: bool = False
    installed_at: str = ""


def _minecraft_assets_root() -> Path:
    return user_data_dir() / "minecraft-assets"


def _bundled_item_zip() -> Path:
    return project_root() / "pack-in" / "arr-private" / "item.zip"


def item_initial_dir() -> Path:
    return _minecraft_assets_root() / "item" / "initial"


def item_vault_dir() -> Path:
    return _minecraft_assets_root() / "item" / "vault"


def _item_installed_path() -> Path:
    return _minecraft_assets_root() / "item" / INSTALLED_FILENAME


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# 随安装包内 ``item/`` 大版本变更时递增，以触发重新种子化。
_ITEM_INITIAL_SEED_VERSION = "1"
_ITEM_SEED_SENTINEL = ".lba_item_seed"


def ensure_initial_item_seeded() -> None:
    """首次将安装包内 ``pack-in/arr-private/item.zip`` 解压到 ``item/initial``。"""
    dst_root = item_initial_dir()
    dst_root.mkdir(parents=True, exist_ok=True)
    sentinel = dst_root / _ITEM_SEED_SENTINEL
    try:
        if sentinel.is_file():
            ver = sentinel.read_text(encoding="utf-8").strip()
            if ver == _ITEM_INITIAL_SEED_VERSION:
                return
    except OSError:
        pass

    src_zip = _bundled_item_zip()
    if not src_zip.is_file():
        try:
            sentinel.write_text(_ITEM_INITIAL_SEED_VERSION, encoding="utf-8")
        except OSError:
            pass
        return

    try:
        with zipfile.ZipFile(src_zip, "r") as zf:
            zf.extractall(dst_root)
        sentinel.write_text(_ITEM_INITIAL_SEED_VERSION, encoding="utf-8")
    except (OSError, zipfile.BadZipFile):
        pass


def _entry_from_dict(row: dict) -> InstalledItemIcon | None:
    try:
        active = bool(row.get("active_layering", False))
        if "active" in row and "active_layering" not in row:
            active = bool(row.get("active"))
        item = InstalledItemIcon(
            id=str(row.get("id", "")),
            version_label=str(row.get("version_label", "")),
            source_label=str(row.get("source_label", "")),
            source_key=str(row.get("source_key", "")),
            file_relpath=str(row.get("file_relpath", "")),
            active_layering=active,
            installed_at=str(row.get("installed_at", "")),
        )
    except Exception:
        return None
    if not item.id or item.source_key in ("", ITEM_SOURCE_BUILTIN):
        return None
    return item


def _load_registry(path: Path) -> list[InstalledItemIcon]:
    if not path.is_file():
        return []
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return []
    if not isinstance(raw, list):
        return []
    out: list[InstalledItemIcon] = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        item = _entry_from_dict(row)
        if item is not None:
            out.append(item)
    return out


def _save_registry(path: Path, items: list[InstalledItemIcon]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = [asdict(i) for i in items]
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")


def load_installed_item_icons() -> list[InstalledItemIcon]:
    ensure_initial_item_seeded()
    return _load_registry(_item_installed_path())


def set_active_layering_item_icon(installed_id: str) -> list[InstalledItemIcon]:
    items = _load_registry(_item_installed_path())
    if installed_id == ITEM_SOURCE_BUILTIN:
        for i in range(len(items)):
            items[i].active_layering = False
    else:
        found = False
        for i in range(len(items)):
            on = items[i].id == installed_id
            items[i].active_layering = on
            if on:
                found = True
        if not found:
            return load_installed_item_icons()
    _save_registry(_item_installed_path(), items)
    return load_installed_item_icons()


def clear_active_layering_item_icon() -> list[InstalledItemIcon]:
    return set_active_layering_item_icon(ITEM_SOURCE_BUILTIN)


def delete_installed_item_icon(installed_id: str) -> list[InstalledItemIcon]:
    path = _item_installed_path()
    items = _load_registry(path)
    new_items = [e for e in items if e.id != installed_id]
    if len(new_items) != len(items):
        for item in items:
            if item.id == installed_id and item.source_key == ITEM_SOURCE_VAULT:
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
        _save_registry(path, new_items)
    return load_installed_item_icons()


def register_vault_item_icon_slot() -> InstalledItemIcon:
    """准备 ``item/vault`` 目录并登记 Vault 来源。"""
    vault = item_vault_dir()
    vault.mkdir(parents=True, exist_ok=True)
    rel = vault.relative_to(_minecraft_assets_root()).as_posix()
    entry = InstalledItemIcon(
        id=VAULT_ENTRY_ID,
        version_label="未知",
        source_label=VAULT_SITE_LABEL,
        source_key=ITEM_SOURCE_VAULT,
        file_relpath=rel,
        active_layering=True,
        installed_at=_now_iso(),
    )
    items = _load_registry(_item_installed_path())
    found = False
    for i, old in enumerate(items):
        if old.id == VAULT_ENTRY_ID:
            items[i] = entry
            found = True
            break
    if not found:
        items.append(entry)
    for i in range(len(items)):
        items[i].active_layering = items[i].id == VAULT_ENTRY_ID
    _save_registry(_item_installed_path(), items)
    return entry


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


def download_and_process_vault_item_icons(
    progress_callback: Callable[[int, int, str], bool] | None = None,
) -> None:
    """通过 Vault API 索引下载物品图标并处理为 32x32 PNG（邻近采样）。"""
    from PySide6.QtCore import Qt
    from PySide6.QtGui import QImage

    def _emit_progress(current: int, total: int, status: str) -> bool:
        if progress_callback is None:
            return True
        return bool(progress_callback(current, total, status))

    if not _emit_progress(0, 1, "正在连接到 ccvaults.com..."):
        return

    dst_dir = item_vault_dir()
    dst_dir.mkdir(parents=True, exist_ok=True)

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

    if not _emit_progress(0, 1, "正在获取物品图标索引..."):
        return

    entries: list[tuple[str, str]] = []
    seen_item_ids: set[str] = set()
    category_name = "10. Items"

    items_assets_url = urllib.parse.urljoin(VAULT_SITE_LABEL, VAULT_API_ITEMS_PATH)
    try:
        payload = _fetch_json_with_auth(opener, items_assets_url, auth_headers)
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"获取 Vault 物品索引失败：HTTP {exc.code}") from exc

    if isinstance(payload, list):
        for row in payload:
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
                    item_id = Path(file_name).stem.lower()
                    if not item_id or item_id in seen_item_ids:
                        continue
                    rel_path = "/".join(
                        urllib.parse.quote(seg, safe="")
                        for seg in ("assets", category_name, sub_name, file_name)
                    )
                    file_url = urllib.parse.urljoin(VAULT_SITE_LABEL, rel_path)
                    entries.append((item_id, file_url))
                    seen_item_ids.add(item_id)

    # items 专用索引失败时，回退全量索引。
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
                    item_id = Path(file_name).stem.lower()
                    if not item_id or item_id in seen_item_ids:
                        continue
                    rel_parts = ["assets", category]
                    if subcategory:
                        rel_parts.append(subcategory)
                    rel_parts.append(file_name)
                    rel_path = "/".join(urllib.parse.quote(seg, safe="") for seg in rel_parts)
                    file_url = urllib.parse.urljoin(VAULT_SITE_LABEL, rel_path)
                    entries.append((item_id, file_url))
                    seen_item_ids.add(item_id)

    if not entries:
        raise RuntimeError("未解析到任何物品图标索引")

    total_count = len(entries)
    if not _emit_progress(0, total_count, f"准备并行下载 {total_count} 个图标..."):
        return

    downloaded_count = 0
    completed_count = 0
    cancel_event = threading.Event()
    max_workers = min(12, max(4, total_count))

    def _download_one(entry: tuple[str, str]) -> bool:
        if cancel_event.is_set():
            return False
        item_id, file_url = entry
        try:
            req = urllib.request.Request(
                file_url,
                headers={
                    "User-Agent": "LitematicaBA-item-icon-manager/1.0",
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
                Qt.TransformationMode.FastTransformation,
            )

        return bool(img.save(str(dst_dir / f"{item_id}.png"), "PNG"))

    executor = concurrent.futures.ThreadPoolExecutor(max_workers=max_workers)
    future_to_item: dict[concurrent.futures.Future[bool], str] = {}
    try:
        for entry in entries:
            fut = executor.submit(_download_one, entry)
            future_to_item[fut] = entry[0]

        for fut in concurrent.futures.as_completed(future_to_item):
            item_id = future_to_item[fut]
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
                f"正在下载图标: {item_id} ({completed_count}/{total_count})，成功 {downloaded_count}",
            ):
                cancel_event.set()
                for pending in future_to_item:
                    pending.cancel()
                break
    finally:
        executor.shutdown(wait=not cancel_event.is_set(), cancel_futures=cancel_event.is_set())

    if cancel_event.is_set():
        return

    if downloaded_count <= 0:
        raise RuntimeError("未成功下载任何物品图标")

    _emit_progress(total_count, total_count, f"下载完成：{downloaded_count}/{total_count}")


def normalize_layering_item_local_id(item_id: str) -> str | None:
    """提取用于匹配贴图文件的物品本地 id（不含命名空间与状态）。"""
    raw = (item_id or "").strip()
    if not raw or raw.startswith("E/"):
        return None
    base = raw.split("[", 1)[0].split("{", 1)[0].strip()
    if ":" in base:
        return base.split(":", 1)[1].strip() or None
    return base or None


def _installed_json_fingerprint() -> str:
    path = _item_installed_path()
    try:
        return str(path.stat().st_mtime_ns) if path.is_file() else "-"
    except OSError:
        return "x"


_layer_search_root_fp: str = ""
_layer_search_root_cached: Path | None = None


def layering_item_icon_search_root() -> Path:
    """当前「应用到分层」所使用资源目录；无登记激活项时为内建 ``item/initial``。"""
    global _layer_search_root_fp, _layer_search_root_cached
    ensure_initial_item_seeded()
    fp = _installed_json_fingerprint()
    if fp == _layer_search_root_fp and _layer_search_root_cached is not None:
        return _layer_search_root_cached
    _layer_search_root_fp = fp
    for it in _load_registry(_item_installed_path()):
        if it.active_layering and it.file_relpath.strip():
            p = _minecraft_assets_root() / it.file_relpath
            if p.is_dir():
                _layer_search_root_cached = p
                return p
    _layer_search_root_cached = item_initial_dir()
    return _layer_search_root_cached


def _find_item_png_under(root: Path, local_id: str) -> Path | None:
    if not root.is_dir() or not local_id:
        return None
    safe = local_id.replace("\\", "/").split("/")[-1]
    if not safe or safe in (".", ".."):
        return None
    direct = [
        root / f"{safe}.png",
        root / "assets" / "minecraft" / "textures" / "item" / f"{safe}.png",
        root / "textures" / "item" / f"{safe}.png",
        root / "item" / f"{safe}.png",
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


def resolve_layering_item_icon_path_in_root(item_id: str, root: Path) -> Path | None:
    """在指定根目录下解析物品 PNG 路径（不带全局缓存）。"""
    lid = normalize_layering_item_local_id(item_id)
    if not lid:
        return None
    return _find_item_png_under(root, lid)


_layer_icon_cache: dict[tuple[str, str], Path | None] = {}
_layer_icon_cache_tag: str = ""


def _layering_item_icon_resolution_state() -> tuple[str, Path]:
    """缓存失效标签 + 当前搜索根目录。"""
    root = layering_item_icon_search_root()
    parts = [str(root.resolve())]
    try:
        parts.append(str(root.stat().st_mtime_ns) if root.is_dir() else "0")
    except OSError:
        parts.append("0")
    parts.append(_installed_json_fingerprint())
    return "|".join(parts), root


def layering_item_icon_resolution_tag() -> str:
    """与 ``resolve_layering_item_icon_path`` 的环境标签一致。"""
    return _layering_item_icon_resolution_state()[0]


def resolve_layering_item_icon_path(item_id: str) -> Path | None:
    """解析分层用物品 PNG 路径；找不到则返回 ``None``。"""
    global _layer_icon_cache_tag, _layer_icon_cache
    tag, root = _layering_item_icon_resolution_state()
    if tag != _layer_icon_cache_tag:
        _layer_icon_cache_tag = tag
        _layer_icon_cache.clear()
    lid = normalize_layering_item_local_id(item_id)
    if not lid:
        return None
    key = (tag, lid)
    if key in _layer_icon_cache:
        return _layer_icon_cache[key]
    found = _find_item_png_under(root, lid)
    _layer_icon_cache[key] = found
    return found