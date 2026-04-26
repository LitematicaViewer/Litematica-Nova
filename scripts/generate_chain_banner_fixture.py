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
    add(items, "CB01", "minecraft:chain", 0, 1, 0, "chain axis=y", axis="y", waterlogged="false")
    add(items, "CB02", "minecraft:chain", 4, 1, 0, "chain axis=x", axis="x", waterlogged="false")
    add(items, "CB03", "minecraft:chain", 8, 1, 0, "chain axis=z", axis="z", waterlogged="false")
    add(items, "CB04", "minecraft:white_banner", 12, 1, 0, "standing banner rotation=0; flag geometry should cover current and lower block", rotation="0")
    add(items, "CB05", "minecraft:red_banner", 16, 1, 0, "standing red banner rotation=8; flag geometry should cover current and lower block", rotation="8")
    add(items, "CB06", "minecraft:blue_wall_banner", 20, 1, 0, "wall banner facing=north; flag geometry should cover current and lower block", facing="north")
    add(items, "CB06", "minecraft:stone", 20, 1, -1, "support wall for wall banner")
    add(items, "CB07", "minecraft:blue_wall_banner", 24, 1, 0, "wall banner facing=east; flag geometry should cover current and lower block", facing="east")
    add(items, "CB07", "minecraft:stone", 25, 1, 0, "support wall for east-facing wall banner")
    add(items, "CB08", "minecraft:blue_wall_banner", 28, 1, 0, "wall banner facing=south; flag geometry should cover current and lower block", facing="south")
    add(items, "CB08", "minecraft:stone", 28, 1, 1, "support wall for south-facing wall banner")
    add(items, "CB09", "minecraft:blue_wall_banner", 32, 1, 0, "wall banner facing=west; flag geometry should cover current and lower block", facing="west")
    add(items, "CB09", "minecraft:stone", 31, 1, 0, "support wall for west-facing wall banner")
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
        "Chain/banner fixture layout",
        "",
        "Numbered samples:",
        "  CB01 = chain axis=y",
        "  CB02 = chain axis=x",
        "  CB03 = chain axis=z",
        "  CB04 = white standing banner rotation=0",
        "  CB05 = red standing banner rotation=8",
        "  CB06 = blue wall banner facing=north plus stone support wall",
        "  CB07 = blue wall banner facing=east plus stone support wall",
        "  CB08 = blue wall banner facing=south plus stone support wall",
        "  CB09 = blue wall banner facing=west plus stone support wall",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))
    schematic = region.as_schematic(
        name="Chain Banner Fixture",
        author="Litematica Nova",
        description="Minimal chain and banner base geometry fixture.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a chain/banner full-mode fixture.")
    parser.add_argument("--output", type=Path, default=workspace_root / "_full_mode_chain_banner_fixture.litematic")
    parser.add_argument("--layout-output", type=Path, default=workspace_root / "_full_mode_chain_banner_fixture_layout.txt")
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
