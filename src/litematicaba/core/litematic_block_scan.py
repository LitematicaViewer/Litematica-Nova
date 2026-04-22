"""Litematic 方块计数扫描：与旧版分析一致，供统计与材料列表复用。"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any


_AIR = frozenset(
    {
        "minecraft:air",
        "minecraft:cave_air",
        "minecraft:void_air",
    }
)
_SKIP_BLOCK_AGG = frozenset(
    {
        "minecraft:piston_head",
        "minecraft:nether_portal",
        "minecraft:moving_piston",
        "minecraft:bedrock",
    }
)
_ENTITY_SKIP = frozenset(
    {
        "E/minecraft:item",
        "E/minecraft:bat",
        "E/minecraft:experience_orb",
        "E/minecraft:shulker_bullet",
    }
)


def _coerce_prop_int(value: Any) -> int:
    if hasattr(value, "py_int"):
        return int(value.py_int)
    return int(str(value))


def _iter_block_states(region: Any):
    size_x = region.maxx() - region.minx() + 1
    size_y = region.maxy() - region.miny() + 1
    size_z = region.maxz() - region.minz() + 1
    palette = region._Region__palette
    blocks = region._Region__blocks
    for x in range(size_x):
        for y in range(size_y):
            for z in range(size_z):
                idx = blocks[x, y, z]
                yield palette[idx]


@dataclass(slots=True)
class LitematicScanSnapshot:
    """一次完整扫描结果（与 ``compute_legacy_statistics`` 使用的量一致）。"""

    block: dict[str, int]
    num_non_air: int
    last_region_size: tuple[int, int, int]


def scan_litematic(
    path: str | Path,
    *,
    include_entities: bool = False,
    region_name: str | None = None,
) -> LitematicScanSnapshot:
    """遍历投影；``region_name`` 为 ``None`` 时统计全部子区域。"""
    from litemapy import Schematic

    p = Path(path)
    schematic = Schematic.load(str(p))
    block: dict[str, int] = {}
    num_non_air = 0
    size_x = size_y = size_z = 0

    if region_name is None:
        region_iter = list(schematic.regions.values())
    else:
        reg = schematic.regions.get(region_name)
        if reg is None:
            return LitematicScanSnapshot(block={}, num_non_air=0, last_region_size=(0, 0, 0))
        region_iter = [reg]

    for region in region_iter:
        size_x = region.maxx() - region.minx() + 1
        size_y = region.maxy() - region.miny() + 1
        size_z = region.maxz() - region.minz() + 1

        for block_state in _iter_block_states(region):
            bid = block_state._BlockState__block_id
            props = getattr(block_state, "_BlockState__properties", None) or {}
            if bid not in _AIR:
                num_non_air += 1
                if bid not in _SKIP_BLOCK_AGG:
                    _process_one_block_body(bid, props, block)

        if include_entities:
            entities = getattr(region, "_Region__entities", None)
            if entities:
                for entity in entities:
                    entity_type = "E/" + str(entity.id)
                    if entity_type in _ENTITY_SKIP:
                        continue
                    if entity_type not in block:
                        block[entity_type] = 1
                    else:
                        block[entity_type] += 1

    return LitematicScanSnapshot(
        block=block,
        num_non_air=num_non_air,
        last_region_size=(size_x, size_y, size_z),
    )


def _process_one_block_body(block_id: str, block_property: dict[str, Any], block: dict[str, int]) -> None:
    """在已判定非空气且非 ``_SKIP_BLOCK_AGG`` 时更新 ``block`` 聚合（与旧脚本一致）。"""
    output = block_id
    mbb = ["potted_", "_cake", "wall_", "_cauldron"]
    analysis = {
        "minecraft:farmland": "minecraft:dirt",
        "minecraft:dirt_path": "minecraft:dirt",
        "minecraft:bubble_column": "minecraft:water",
        "minecraft:soul_fire": "minecraft:fire",
    }
    prop_list: list[tuple[str, str, str | None, int]] = [
        ("waterlogged", "true", "minecraft:water", 1),
        ("type", "double", None, 2),
        ("half", "upper", None, -1),
        ("part", "head", None, -1),
        ("eggs", "", "minecraft:turtle_egg", 0),
        ("pickles", "", "minecraft:sea_pickle", 0),
        ("charges", "", "minecraft:glowstone", 0),
        ("flower_amount", "", "minecraft:pink_petals", 0),
    ]
    for a in analysis:
        output = analysis[a] if block_id == a else block_id
    for root in mbb:
        if root in block_id:
            output = block_id.replace(root, "")
    for pt, pv, pf, pn in prop_list:
        if pt in block_property:
            use_pn = pn
            if not use_pn:
                use_pn = _coerce_prop_int(block_property[pt])
            if block_property[pt] == pv or not pv:
                if not pf:
                    block[output] = block[output] + use_pn if output in block else use_pn
                elif pf not in block:
                    block[pf] = use_pn
                else:
                    block[pf] = block[pf] + use_pn
                continue
    block[output] = block[output] + 1 if output in block else 1


def scan_litematic_block_counts(
    path: str | Path,
    *,
    include_entities: bool = False,
    region_name: str | None = None,
) -> dict[str, int]:
    """仅返回方块/实体计数字典。"""
    return scan_litematic(path, include_entities=include_entities, region_name=region_name).block


def sorted_block_counts(block: dict[str, int]) -> list[tuple[str, int]]:
    """按数量降序、再按 ID 升序的稳定展示顺序。"""
    return sorted(block.items(), key=lambda x: (-x[1], x[0]))
