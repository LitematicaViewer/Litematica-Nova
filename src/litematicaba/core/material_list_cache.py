"""材料列表磁盘缓存（design §2.8.2）：``data/cache``，主键为路径 + 区域 + 是否含实体。"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from litematicaba.core.config import user_data_dir


def _cache_dir() -> Path:
    d = user_data_dir() / "cache"
    d.mkdir(parents=True, exist_ok=True)
    return d


def cache_key_path(source: Path, *, region_name: str | None, include_entities: bool) -> Path:
    raw = f"{source.resolve()}|{region_name if region_name is not None else ''}|{include_entities}".encode(
        "utf-8"
    )
    h = hashlib.sha256(raw).hexdigest()[:28]
    return _cache_dir() / f"material_{h}.json"


def file_mtime_ns(path: Path) -> int:
    return path.stat().st_mtime_ns


def load_material_cache(path: Path, *, region_name: str | None, include_entities: bool) -> tuple[dict[str, int], int] | None:
    """返回 ``(counts, cached_mtime_ns)``；文件不存在或损坏则 ``None``。"""
    cf = cache_key_path(path, region_name=region_name, include_entities=include_entities)
    if not cf.is_file():
        return None
    try:
        data: dict[str, Any] = json.loads(cf.read_text(encoding="utf-8"))
        counts = data.get("counts")
        mns = data.get("mtime_ns")
        if not isinstance(counts, dict) or mns is None:
            return None
        out: dict[str, int] = {}
        for k, v in counts.items():
            try:
                out[str(k)] = int(v)
            except (TypeError, ValueError):
                continue
        return out, int(mns)
    except (OSError, json.JSONDecodeError, ValueError):
        return None


def save_material_cache(
    path: Path,
    *,
    region_name: str | None,
    include_entities: bool,
    counts: dict[str, int],
    mtime_ns: int,
) -> None:
    cf = cache_key_path(path, region_name=region_name, include_entities=include_entities)
    payload = {
        "source_path": str(path.resolve()),
        "region_name": region_name if region_name is not None else "",
        "include_entities": include_entities,
        "mtime_ns": mtime_ns,
        "counts": counts,
    }
    cf.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def cache_is_stale(path: Path, cached_mtime_ns: int) -> bool:
    try:
        return file_mtime_ns(path) != cached_mtime_ns
    except OSError:
        return True
