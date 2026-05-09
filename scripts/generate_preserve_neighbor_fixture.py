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
    for dx in range(3):
        for dy in range(3):
            add_block(
                items,
                sample_id,
                "minecraft:stone",
                origin_x + dx,
                origin_y + dy,
                origin_z,
                "stone wall backing for preserve-neighbor-face check",
            )
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
    for dx in range(3):
        for dz in range(3):
            add_block(
                items,
                sample_id,
                "minecraft:stone",
                origin_x + dx,
                origin_y,
                origin_z + dz,
                "stone floor backing for preserve-neighbor-face check",
            )
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
    for dx in range(3):
        for dz in range(3):
            add_block(
                items,
                sample_id,
                "minecraft:stone",
                origin_x + dx,
                origin_y + 2,
                origin_z + dz,
                "stone ceiling backing for preserve-neighbor-face check",
            )
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


def placements() -> list[Placement]:
    items: list[Placement] = []

    add_wall_sample(
        items,
        "W01",
        "minecraft:dandelion",
        0,
        1,
        0,
        "plant sample in front of stone wall; wall face behind must be preserved",
    )
    add_floor_sample(
        items,
        "F01",
        "minecraft:torch",
        6,
        1,
        0,
        "torch sample above stone floor; top face below torch must be preserved",
    )
    add_floor_sample(
        items,
        "F02",
        "minecraft:redstone_torch",
        12,
        1,
        0,
        "redstone torch sample above stone floor; top face below torch must be preserved",
        lit="true",
    )
    add_ceiling_sample(
        items,
        "C01",
        "minecraft:lantern",
        18,
        1,
        0,
        "lantern sample below stone ceiling; bottom face above lantern must be preserved",
        hanging="true",
        waterlogged="false",
    )
    add_ceiling_sample(
        items,
        "C02",
        "minecraft:soul_lantern",
        24,
        1,
        0,
        "soul lantern sample below stone ceiling; bottom face above lantern must be preserved",
        hanging="true",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "W02",
        "minecraft:amethyst_cluster",
        30,
        1,
        0,
        "amethyst cluster sample in front of stone wall; wall face behind must be preserved",
        facing="south",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "F03",
        "minecraft:tube_coral_fan",
        36,
        1,
        0,
        "coral fan sample above stone floor; top face below coral must be preserved",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "W03",
        "minecraft:tube_coral_wall_fan",
        42,
        1,
        0,
        "coral wall fan sample in front of stone wall; wall face behind must be preserved",
        facing="south",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "F04",
        "minecraft:cherry_trapdoor",
        48,
        1,
        0,
        "cherry trapdoor above stone floor; top face below thin trapdoor must be preserved",
        facing="north",
        half="bottom",
        open="false",
        powered="false",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "F05",
        "minecraft:bamboo_trapdoor",
        54,
        1,
        0,
        "bamboo trapdoor above stone floor; top face below thin trapdoor must be preserved",
        facing="north",
        half="bottom",
        open="false",
        powered="false",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "F06",
        "minecraft:copper_trapdoor",
        60,
        1,
        0,
        "copper trapdoor above stone floor; top face below thin trapdoor must be preserved",
        facing="north",
        half="bottom",
        open="false",
        powered="false",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "F07",
        "minecraft:cherry_pressure_plate",
        66,
        1,
        0,
        "cherry pressure plate above stone floor; top face below plate must be preserved",
        powered="false",
    )
    add_floor_sample(
        items,
        "F08",
        "minecraft:polished_blackstone_pressure_plate",
        72,
        1,
        0,
        "polished blackstone pressure plate above stone floor; top face below plate must be preserved",
        powered="false",
    )
    add_floor_sample(
        items,
        "F09",
        "minecraft:light_weighted_pressure_plate",
        78,
        1,
        0,
        "light weighted pressure plate above stone floor; top face below plate must be preserved",
        power="0",
    )
    add_wall_sample(
        items,
        "W04",
        "minecraft:cherry_sign",
        84,
        1,
        0,
        "cherry standing sign near stone wall; wall face behind sign must be preserved",
        rotation="0",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "W05",
        "minecraft:cherry_wall_sign",
        90,
        1,
        0,
        "cherry wall sign on stone wall; wall face behind sign must be preserved",
        facing="south",
        waterlogged="false",
    )
    add_ceiling_sample(
        items,
        "C03",
        "minecraft:cherry_hanging_sign",
        96,
        1,
        0,
        "cherry hanging sign below stone ceiling; bottom face above sign must be preserved",
        attached="false",
        rotation="0",
        waterlogged="false",
    )
    add_wall_sample(
        items,
        "W06",
        "minecraft:cherry_wall_hanging_sign",
        102,
        1,
        0,
        "cherry wall hanging sign on stone wall; wall face behind sign must be preserved",
        facing="south",
        waterlogged="false",
    )
    add_floor_sample(
        items,
        "F10",
        "minecraft:lightning_rod",
        108,
        1,
        0,
        "lightning rod above stone floor; top face below rod must be preserved",
        facing="up",
        powered="false",
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
        "Preserve-neighbor-face whitelist fixture layout",
        "",
        "Purpose:",
        "  Minimal full-mode fixture for neighbor-face preservation checks.",
        "",
        "Families covered:",
        "  plants, torch family, lantern family, amethyst cluster family, coral family,",
        "  trapdoors, pressure plates, signs, hanging signs, lightning rod",
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
        name="Preserve Neighbor Face Whitelist Fixture",
        author="Litematica Nova",
        description="Minimal whitelist fixture for preserve-neighbor-face checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Generate a preserve-neighbor-face whitelist .litematic fixture."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_preserve_neighbor_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_preserve_neighbor_fixture_layout.txt",
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
