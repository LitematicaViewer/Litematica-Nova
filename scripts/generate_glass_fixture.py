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


def placements() -> list[Placement]:
    return [
        Placement("G01", "minecraft:glass", 0, 1, 0, {}, "single clear glass block; backface should not be double-emitted"),
        Placement("G02", "minecraft:glass", 4, 1, 0, {}, "west glass block of adjacent pair"),
        Placement("G02", "minecraft:glass", 5, 1, 0, {}, "east glass block of adjacent pair"),
        Placement("G03", "minecraft:white_stained_glass", 9, 1, 0, {}, "single stained glass block"),
        Placement("G04", "minecraft:glass", 14, 1, 0, {}, "glass beside stone, glass face should remain single-sided"),
        Placement("G04", "minecraft:stone", 15, 1, 0, {}, "opaque neighbor for glass adjacency check"),
    ]


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
        "Glass fixture layout",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))

    schematic = region.as_schematic(
        name="Glass Fixture",
        author="Litematica Nova",
        description="Minimal glass transparency fixture for full-mode single-sided checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a glass full-mode fixture.")
    parser.add_argument("--output", type=Path, default=workspace_root / "_full_mode_glass_fixture.litematic")
    parser.add_argument("--layout-output", type=Path, default=workspace_root / "_full_mode_glass_fixture_layout.txt")
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
