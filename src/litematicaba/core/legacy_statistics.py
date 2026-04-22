"""与旧项目 ``script/LitematicaViewer.start_analysis`` 对齐的方块统计（design §2.4.3）。

方块遍历逻辑见 ``litematic_block_scan``；分类关键词来自 ``lang/category.json``。
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from litematicaba.core.config import project_root
from litematicaba.core.litematic_block_scan import scan_litematic


def _load_category_ordered() -> list[tuple[str, list[str]]]:
    from litematicaba.core.game_resource_language import _legacy_dir, ensure_initial_language_seeded
    ensure_initial_language_seeded()
    path = _legacy_dir() / "category.json"
    if not path.is_file():
        return []
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        return []
    return [(str(k), list(v) if isinstance(v, list) else []) for k, v in raw.items()]


def category_for_local_id(local_id: str, ordered: list[tuple[str, list[str]]]) -> str:
    """等价于 ``script/Litmatool.Category_Tran``：ID 本地名按 ``_`` 分段，按表顺序首命中大类。"""
    parts = local_id.split("_")
    for cat_name, tokens in ordered:
        for prop in parts:
            if prop in tokens:
                return cat_name
    return ""


def material_label_zh(redly_ratio: float, num_non_air: int) -> str:
    """旧 UI 由红石偏度比例与 ``num`` 映射的「材质」中文文案。"""
    if num_non_air <= 10:
        return "方块太少"
    if redly_ratio > 0.5:
        return "红石机器"
    if redly_ratio >= 0.3:
        return "生电红石"
    if redly_ratio >= 0.1:
        return "生电机器"
    if redly_ratio >= 0.01:
        return "结构性机器"
    return "建筑"


def _metadata_total_blocks(path: Path) -> int | None:
    try:
        from amulet_nbt import load as nbt_load
    except Exception:
        return None
    try:
        nbt_file = nbt_load(str(path), compressed=True)
        meta = nbt_file.tag.get("Metadata")
        if meta is None:
            return None
        tb = meta.get("TotalBlocks")
        if tb is None:
            return None
        if hasattr(tb, "py_int"):
            return int(tb.py_int)
        return int(str(tb))
    except Exception:
        return None


@dataclass(slots=True)
class LegacyStatisticsResult:
    """一次统计的输出；比值为 0~1，界面层自行乘 100 与格式化。"""

    source_path: Path
    num_non_air: int
    distinct_block_keys: int
    last_region_size: tuple[int, int, int]
    last_region_cell_count: int
    density_ratio: float | None
    redstone_skew_ratio: float | None
    material_label_zh: str
    fluid_ratio: float | None
    fluid_units: int
    redstone_plus_container_units: int
    metadata_total_blocks: int | None
    metadata_matches_computed_num: bool | None


def compute_legacy_statistics(
    path: str | Path,
    *,
    include_entities: bool = False,
) -> LegacyStatisticsResult:
    """解析 ``.litematic`` 并计算与旧查看器一致的统计指标。"""
    p = Path(path)
    ordered_cat = _load_category_ordered()
    snap = scan_litematic(p, include_entities=include_entities)
    block = snap.block
    num = snap.num_non_air
    size_x, size_y, size_z = snap.last_region_size

    red_u = 0
    chest_u = 0
    fluid_u = 0
    for val, cnt in block.items():
        if val.split("/")[0] == "E":
            continue
        id_local = val.split("[")[0].split(":")[-1]
        cat = category_for_local_id(id_local, ordered_cat)
        if cat == "红石":
            red_u += cnt
        elif cat == "容器":
            chest_u += cnt
        elif cat == "液体":
            fluid_u += cnt

    vol = size_x * size_y * size_z
    density_ratio = (num / vol) if vol > 0 else None

    sorted_block = sorted(block.items(), key=lambda x: x[1], reverse=True)
    if len(block) > 5 and sorted_block:
        denom = num - sorted_block[0][1]
    else:
        denom = num
    if denom <= 0:
        redly = None
    else:
        redly = (red_u + chest_u) / denom

    if num > 0:
        fluid_ratio = fluid_u / num
    else:
        fluid_ratio = None

    label = material_label_zh(redly if redly is not None else 0.0, num)
    meta_tb = _metadata_total_blocks(p)
    meta_ok = meta_tb is not None and meta_tb == num if meta_tb is not None else None

    return LegacyStatisticsResult(
        source_path=p.resolve(),
        num_non_air=num,
        distinct_block_keys=len(block),
        last_region_size=(size_x, size_y, size_z),
        last_region_cell_count=vol,
        density_ratio=density_ratio,
        redstone_skew_ratio=redly,
        material_label_zh=label,
        fluid_ratio=fluid_ratio,
        fluid_units=fluid_u,
        redstone_plus_container_units=red_u + chest_u,
        metadata_total_blocks=meta_tb,
        metadata_matches_computed_num=meta_ok,
    )
