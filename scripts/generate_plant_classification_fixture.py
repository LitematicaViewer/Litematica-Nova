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
    add(items, "PL01", "minecraft:dandelion", 0, 1, 0, "true crossed-plane plant representative")
    add(items, "PL02", "minecraft:cave_vines", 4, 1, 0, "explicit crossed-plane vine representative", age="25", berries="true")
    add(items, "PL03", "minecraft:big_dripleaf", 8, 1, 0, "special plant typed model, not generic X", facing="north", tilt="none", waterlogged="false")
    add(items, "PL04", "minecraft:big_dripleaf_stem", 12, 1, 0, "special plant typed model stem", facing="north", waterlogged="false")
    add(items, "PL05", "minecraft:big_dripleaf_stem", 16, 1, 0, "waterlogged stem should get water overlay", facing="north", waterlogged="true")
    add(items, "PL06", "minecraft:small_dripleaf", 20, 1, 0, "special plant typed lower half", facing="north", half="lower", waterlogged="true")
    add(items, "PL07", "minecraft:small_dripleaf", 24, 1, 0, "special plant typed upper half", facing="north", half="upper", waterlogged="false")
    add(items, "PL08", "minecraft:azalea", 28, 1, 0, "azalea bush typed model, not generic X")
    add(items, "PL09", "minecraft:flowering_azalea", 32, 1, 0, "flowering azalea bush typed model, not generic X")
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
        "Plant classification fixture layout",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))
    schematic = region.as_schematic(
        name="Plant Classification Fixture",
        author="Litematica Nova",
        description="Minimal fixture for crossed-plane vs special plant full-mode templates.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a plant classification full-mode fixture.")
    parser.add_argument("--output", type=Path, default=workspace_root / "_full_mode_plant_classification_fixture.litematic")
    parser.add_argument("--layout-output", type=Path, default=workspace_root / "_full_mode_plant_classification_fixture_layout.txt")
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
