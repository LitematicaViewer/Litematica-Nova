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


def block_state(item: Placement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def add_column(
    items: list[Placement],
    sample_id: str,
    x: int,
    z: int,
    blocks: list[str],
    note: str,
    top_properties: dict[str, str] | None = None,
) -> None:
    for y, block_id in enumerate(blocks):
        properties = {"snowy": "false"} if block_id == "minecraft:grass_block" else {}
        if top_properties is not None and y == len(blocks) - 1:
            properties.update(top_properties)
        add_block(items, sample_id, block_id, x, y, z, note, **properties)


def placements() -> list[Placement]:
    items: list[Placement] = []

    add_column(items, "T01", 0, 0, ["minecraft:dirt"], "single dirt baseline")
    add_column(
        items,
        "T02",
        3,
        0,
        ["minecraft:grass_block"],
        "single grass_block baseline",
    )
    add_column(
        items,
        "T03",
        6,
        0,
        ["minecraft:dirt", "minecraft:grass_block"],
        "grass_block over dirt, vertical two-layer side check",
    )
    add_column(
        items,
        "T04",
        9,
        0,
        ["minecraft:dirt", "minecraft:dirt"],
        "1x2 dirt vertical stack side check",
    )
    add_column(
        items,
        "T05",
        12,
        0,
        ["minecraft:dirt", "minecraft:dirt", "minecraft:grass_block"],
        "1x3 terrain column side check",
    )

    for dx in range(2):
        for dy, block_id in enumerate(["minecraft:dirt", "minecraft:grass_block"]):
            add_block(
                items,
                "T06",
                block_id,
                0 + dx,
                dy,
                5,
                "2x2 cliff face, outside side should be on outer x/z boundary",
                **({"snowy": "false"} if block_id == "minecraft:grass_block" else {}),
            )

    for dx in range(3):
        for dy, block_id in enumerate(["minecraft:dirt", "minecraft:grass_block"]):
            add_block(
                items,
                "T07",
                block_id,
                4 + dx,
                dy,
                5,
                "3x2 cliff wall, middle inner sides should be culled",
                **({"snowy": "false"} if block_id == "minecraft:grass_block" else {}),
            )

    add_column(
        items,
        "T08",
        9,
        5,
        ["minecraft:dirt", "minecraft:podzol"],
        "podzol terrain-stack contrast",
        top_properties={"snowy": "false"},
    )
    add_column(
        items,
        "T09",
        12,
        5,
        ["minecraft:dirt", "minecraft:mycelium"],
        "mycelium terrain-stack contrast",
        top_properties={"snowy": "false"},
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

    region = litemapy.Region(
        0,
        0,
        0,
        max_x - min_x + 1,
        max_y - min_y + 1,
        max_z - min_z + 1,
    )
    for item in items:
        region[item.x - min_x, item.y - min_y, item.z - min_z] = block_state(item)

    lines = [
        "Terrain stack fixture layout",
        "",
        "Purpose:",
        "  Minimal full-mode fixture for stacked dirt/grass side generation and final culling.",
        "  Sample IDs live only in this layout and LBA_TERRAIN_STACK_TRACE output.",
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
        name="Terrain Stack Fixture",
        author="Litematica Nova",
        description="Minimal terrain stack fixture for full-mode side placement checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate terrain-stack full-mode fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_terrain_stack_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_terrain_stack_fixture_layout.txt",
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
