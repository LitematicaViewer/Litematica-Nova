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


def state(item: Placement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def add(
    items: list[Placement],
    sample_id: str,
    block_id: str,
    x: int,
    y: int,
    z: int,
    note: str,
    **properties: str,
) -> None:
    items.append(Placement(sample_id, block_id, x, y, z, properties, note))


def pane_props(**overrides: str) -> dict[str, str]:
    props = {
        "north": "false",
        "south": "false",
        "west": "false",
        "east": "false",
        "waterlogged": "false",
    }
    props.update(overrides)
    return props


def placements() -> list[Placement]:
    items: list[Placement] = []
    add(
        items,
        "P01",
        "minecraft:glass_pane",
        0,
        1,
        0,
        "single glass pane center post should render",
        **pane_props(),
    )
    add(
        items,
        "P02",
        "minecraft:glass_pane",
        4,
        1,
        0,
        "connected pane west half of pair",
        **pane_props(east="true"),
    )
    add(
        items,
        "P02",
        "minecraft:glass_pane",
        5,
        1,
        0,
        "connected pane east half of pair",
        **pane_props(west="true"),
    )
    add(
        items,
        "P03",
        "minecraft:glass_pane",
        9,
        1,
        0,
        "pane adjacent to stone should still render its pane geometry",
        **pane_props(east="true"),
    )
    add(
        items,
        "P03",
        "minecraft:stone",
        10,
        1,
        0,
        "opaque neighbor for pane culling check",
    )
    add(
        items,
        "P04",
        "minecraft:white_stained_glass_pane",
        14,
        1,
        0,
        "stained glass pane checks pane_top texture alias",
        **pane_props(),
    )
    add(
        items,
        "P05",
        "minecraft:glass_pane",
        18,
        1,
        0,
        "cross-connected pane center",
        **pane_props(north="true", south="true", west="true", east="true"),
    )
    add(items, "P05", "minecraft:glass_pane", 18, 1, -1, "north arm", **pane_props(south="true"))
    add(items, "P05", "minecraft:glass_pane", 18, 1, 1, "south arm", **pane_props(north="true"))
    add(items, "P05", "minecraft:glass_pane", 17, 1, 0, "west arm", **pane_props(east="true"))
    add(items, "P05", "minecraft:glass_pane", 19, 1, 0, "east arm", **pane_props(west="true"))
    add(
        items,
        "P06",
        "minecraft:glass_pane",
        24,
        1,
        0,
        "waterlogged pane should render pane geometry and water overlay",
        **pane_props(waterlogged="true"),
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

    region = litemapy.Region(0, 0, 0, max_x - min_x + 1, max_y - min_y + 1, max_z - min_z + 1)
    for item in items:
        region[item.x - min_x, item.y - min_y, item.z - min_z] = state(item)

    lines = [
        "Glass pane fixture layout",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))

    schematic = region.as_schematic(
        name="Glass Pane Fixture",
        author="Litematica Nova",
        description="Minimal glass pane render/culling fixture for full-mode checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a glass pane full-mode fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_glass_pane_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_glass_pane_fixture_layout.txt",
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
