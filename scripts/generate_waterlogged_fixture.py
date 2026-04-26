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


def add(items: list[Placement], sample_id: str, block_id: str, x: int, y: int, z: int, note: str, **properties: str) -> None:
    items.append(Placement(sample_id, block_id, x, y, z, properties, note))


def state(item: Placement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def placements() -> list[Placement]:
    items: list[Placement] = []
    add(
        items,
        "WL01",
        "minecraft:oak_trapdoor",
        0,
        1,
        0,
        "waterlogged trapdoor should get internal water overlay",
        facing="north",
        half="bottom",
        open="false",
        powered="false",
        waterlogged="true",
    )
    add(
        items,
        "WL02",
        "minecraft:oak_stairs",
        4,
        1,
        0,
        "waterlogged stairs should get internal water overlay",
        facing="north",
        half="bottom",
        shape="straight",
        waterlogged="true",
    )
    add(
        items,
        "WL03",
        "minecraft:oak_fence",
        8,
        1,
        0,
        "waterlogged fence should get internal water overlay",
        north="false",
        south="false",
        west="false",
        east="false",
        waterlogged="true",
    )
    add(
        items,
        "WL04",
        "minecraft:glass_pane",
        12,
        1,
        0,
        "waterlogged glass pane should show pane plus water overlay",
        north="false",
        south="false",
        west="false",
        east="false",
        waterlogged="true",
    )
    add(
        items,
        "WL05",
        "minecraft:water",
        16,
        1,
        0,
        "source water next to waterlogged pane should not draw same-fluid internal face",
        level="0",
    )
    add(
        items,
        "WL05",
        "minecraft:glass_pane",
        17,
        1,
        0,
        "waterlogged pane adjacent to source water",
        north="false",
        south="false",
        west="true",
        east="false",
        waterlogged="true",
    )
    add(
        items,
        "WL06",
        "minecraft:big_dripleaf_stem",
        22,
        1,
        0,
        "waterlogged big dripleaf stem should get internal water overlay",
        facing="north",
        waterlogged="true",
    )
    add(
        items,
        "WL07",
        "minecraft:water",
        26,
        1,
        0,
        "source water next to waterlogged big dripleaf stem",
        level="0",
    )
    add(
        items,
        "WL07",
        "minecraft:big_dripleaf_stem",
        27,
        1,
        0,
        "waterlogged big dripleaf stem adjacent to source water",
        facing="north",
        waterlogged="true",
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

    lines = ["Waterlogged fixture layout", "", "sample_id\tblock_id\tanchor\tproperties\tnote"]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))
    schematic = region.as_schematic(
        name="Waterlogged Fixture",
        author="Litematica Nova",
        description="Minimal waterlogged overlay fixture for full-mode checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a waterlogged full-mode fixture.")
    parser.add_argument("--output", type=Path, default=workspace_root / "_full_mode_waterlogged_fixture.litematic")
    parser.add_argument("--layout-output", type=Path, default=workspace_root / "_full_mode_waterlogged_fixture_layout.txt")
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
