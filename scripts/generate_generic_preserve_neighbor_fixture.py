from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953


@dataclass(frozen=True)
class Placement:
    sample_id: str
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    note: str


def block_state(item: Placement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def add_block(
    items: list[Placement],
    sample_id: str,
    block_id: str,
    x: int,
    y: int,
    z: int,
    note: str,
    **properties: str,
) -> None:
    items.append(
        Placement(
            sample_id=sample_id,
            block_id=block_id,
            x=x,
            y=y,
            z=z,
            properties=properties,
            note=note,
        )
    )


def add_wall_backing(items: list[Placement], sample_id: str, origin_x: int, origin_y: int, origin_z: int) -> None:
    for dx in range(3):
        for dy in range(3):
            add_block(
                items,
                sample_id,
                "minecraft:stone",
                origin_x + dx,
                origin_y + dy,
                origin_z,
                "stone wall backing for neighbor-face preservation check",
            )


def add_floor_backing(items: list[Placement], sample_id: str, origin_x: int, origin_y: int, origin_z: int) -> None:
    for dx in range(3):
        for dz in range(3):
            add_block(
                items,
                sample_id,
                "minecraft:stone",
                origin_x + dx,
                origin_y,
                origin_z + dz,
                "stone floor backing for neighbor-face preservation check",
            )


def add_ceiling_backing(items: list[Placement], sample_id: str, origin_x: int, origin_y: int, origin_z: int) -> None:
    for dx in range(3):
        for dz in range(3):
            add_block(
                items,
                sample_id,
                "minecraft:stone",
                origin_x + dx,
                origin_y + 2,
                origin_z + dz,
                "stone ceiling backing for neighbor-face preservation check",
            )


def add_wall_sample(
    items: list[Placement],
    sample_id: str,
    block_id: str,
    origin_x: int,
    origin_y: int,
    origin_z: int,
    note: str,
    **properties: str,
) -> None:
    add_wall_backing(items, sample_id, origin_x, origin_y, origin_z)
    add_block(
        items,
        sample_id,
        block_id,
        origin_x + 1,
        origin_y + 1,
        origin_z + 1,
        note,
        **properties,
    )


def add_floor_sample(
    items: list[Placement],
    sample_id: str,
    block_id: str,
    origin_x: int,
    origin_y: int,
    origin_z: int,
    note: str,
    **properties: str,
) -> None:
    add_floor_backing(items, sample_id, origin_x, origin_y, origin_z)
    add_block(
        items,
        sample_id,
        block_id,
        origin_x + 1,
        origin_y + 1,
        origin_z + 1,
        note,
        **properties,
    )


def add_ceiling_sample(
    items: list[Placement],
    sample_id: str,
    block_id: str,
    origin_x: int,
    origin_y: int,
    origin_z: int,
    note: str,
    **properties: str,
) -> None:
    add_ceiling_backing(items, sample_id, origin_x, origin_y, origin_z)
    add_block(
        items,
        sample_id,
        block_id,
        origin_x + 1,
        origin_y + 1,
        origin_z + 1,
        note,
        **properties,
    )


def add_identical_pair_sample(
    items: list[Placement],
    sample_id: str,
    block_id: str,
    origin_x: int,
    origin_y: int,
    origin_z: int,
    note: str,
    **properties: str,
) -> None:
    add_wall_backing(items, sample_id, origin_x, origin_y, origin_z)
    add_block(
        items,
        sample_id,
        block_id,
        origin_x + 1,
        origin_y + 1,
        origin_z + 1,
        note,
        **properties,
    )
    add_block(
        items,
        sample_id,
        block_id,
        origin_x + 2,
        origin_y + 1,
        origin_z + 1,
        "identical-state neighbor for generic preserve shortcut bypass check",
        **properties,
    )


def placements() -> list[Placement]:
    items: list[Placement] = []

    add_wall_sample(
        items,
        "A01",
        "minecraft:dandelion",
        0,
        1,
        0,
        "plant in front of wall; wall face toward plant should be preserved",
    )
    add_floor_sample(
        items,
        "A02",
        "minecraft:torch",
        6,
        1,
        0,
        "torch over floor; top face below torch should be preserved",
    )
    add_ceiling_sample(
        items,
        "A03",
        "minecraft:lantern",
        12,
        1,
        0,
        "lantern under ceiling; bottom face above lantern should be preserved",
        hanging="true",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A04",
        "minecraft:amethyst_cluster",
        18,
        1,
        0,
        "amethyst cluster in front of wall; wall face behind cluster should be preserved",
        facing="south",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "A05",
        "minecraft:tube_coral_fan",
        24,
        1,
        0,
        "coral fan over floor; top face below coral should be preserved",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A06",
        "minecraft:oak_slab",
        30,
        1,
        0,
        "bottom slab in front of wall; wall face behind slab should be preserved by generic rule",
        type="bottom",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A07",
        "minecraft:oak_stairs",
        36,
        1,
        0,
        "stairs in front of wall; wall face behind stairs should be preserved by generic rule",
        facing="south",
        half="bottom",
        shape="straight",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A08",
        "minecraft:oak_trapdoor",
        0,
        6,
        0,
        "trapdoor in front of wall; wall face toward trapdoor should be preserved",
        facing="north",
        half="bottom",
        open="false",
        powered="false",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A09",
        "minecraft:oak_door",
        6,
        6,
        0,
        "lower door half in front of wall; wall face toward door should be preserved",
        facing="north",
        half="lower",
        hinge="left",
        open="false",
        powered="false",
    )
    add_wall_sample(
        items,
        "A10",
        "minecraft:oak_leaves",
        12,
        6,
        0,
        "leaves in front of wall; wall face toward transparent leaves should be preserved",
        distance="1",
        persistent="true",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A11",
        "minecraft:end_rod",
        18,
        6,
        0,
        "end rod in front of wall; wall face toward end rod should be preserved",
        facing="south",
    )
    add_floor_sample(
        items,
        "A12",
        "minecraft:pink_petals",
        24,
        6,
        0,
        "pink petals on floor; top face below flower cluster should be preserved",
        facing="north",
        flower_amount="4",
    )
    add_floor_sample(
        items,
        "A13",
        "minecraft:wildflowers",
        30,
        6,
        0,
        "wildflowers on floor; top face below flower cluster should be preserved if assets exist",
        facing="north",
        flower_amount="4",
    )
    add_floor_sample(
        items,
        "A14",
        "minecraft:cave_vines",
        0,
        6,
        12,
        "cave_vines with berries; neighbor face should be preserved",
        age="25",
        berries="true",
    )
    add_floor_sample(
        items,
        "A15",
        "minecraft:cave_vines_plant",
        6,
        6,
        12,
        "cave_vines_plant without berries; family exact id check",
        berries="false",
    )
    add_floor_sample(
        items,
        "A16",
        "minecraft:big_dripleaf",
        12,
        6,
        12,
        "big_dripleaf; dripleaf family preserve-neighbor check",
        facing="north",
        tilt="none",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "A17",
        "minecraft:big_dripleaf_stem",
        18,
        6,
        12,
        "big_dripleaf_stem; dripleaf family preserve-neighbor check",
        facing="north",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "A18",
        "minecraft:small_dripleaf",
        24,
        6,
        12,
        "small_dripleaf; dripleaf family preserve-neighbor check",
        facing="north",
        half="lower",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "A19",
        "minecraft:azalea",
        30,
        6,
        12,
        "azalea bush in front of wall; wall face toward azalea should be preserved",
    )
    add_wall_sample(
        items,
        "A20",
        "minecraft:flowering_azalea",
        36,
        6,
        12,
        "flowering azalea bush in front of wall; wall face toward azalea should be preserved",
    )

    add_identical_pair_sample(
        items,
        "B01",
        "minecraft:oak_slab",
        44,
        1,
        0,
        "identical bottom slab pair; generic preserve shortcut should not force interior neighbor face",
        type="bottom",
        waterlogged="false",
    )
    add_identical_pair_sample(
        items,
        "B02",
        "minecraft:oak_stairs",
        50,
        1,
        0,
        "identical stair pair; generic preserve shortcut should not force interior neighbor face",
        facing="south",
        half="bottom",
        shape="straight",
        waterlogged="false",
    )

    return items


def build_fixture() -> tuple[litemapy.Schematic, str]:
    items = placements()
    min_x = min(item.x for item in items)
    min_y = min(item.y for item in items)
    min_z = min(item.z for item in items)
    max_x = max(item.x for item in items)
    max_y = max(item.y for item in items)
    max_z = max(item.z for item in items)

    width = max_x - min_x + 1
    height = max_y - min_y + 1
    length = max_z - min_z + 1

    region = litemapy.Region(0, 0, 0, width, height, length)
    for item in items:
        region[item.x - min_x, item.y - min_y, item.z - min_z] = block_state(item)

    lines = [
        "Generic preserve-neighbor-face fixture layout",
        "",
        "Purpose:",
        "  Minimal full-mode fixture for generic preserve-neighbor-face rule checks.",
        "",
        "Coverage:",
        "  a. non-full block + full block neighbor should preserve neighbor face",
        "  b. identical exact-state non-full block pair should skip generic shortcut",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append(
            "\t".join([item.sample_id, item.block_id, anchor, properties, item.note])
        )

    schematic = region.as_schematic(
        name="Generic Preserve Neighbor Fixture",
        author="Litematica Nova",
        description="Minimal generic preserve-neighbor-face fixture for full-mode checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Generate a generic preserve-neighbor-face .litematic fixture."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_generic_preserve_neighbor_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_generic_preserve_neighbor_fixture_layout.txt",
    )
    args = parser.parse_args()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.layout_output.parent.mkdir(parents=True, exist_ok=True)
    schematic, layout = build_fixture()
    schematic.save(str(args.output))
    args.layout_output.write_text(layout + "\n", encoding="utf-8")
    print(f"fixture={args.output}")
    print(f"layout={args.layout_output}")


if __name__ == "__main__":
    main()
