from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953
FROGLIGHTS = [
    ("F01", "minecraft:ochre_froglight", "Ochre"),
    ("F02", "minecraft:verdant_froglight", "Verdant"),
    ("F03", "minecraft:pearlescent_froglight", "Pearlescent"),
]


@dataclass(frozen=True)
class BlockPlacement:
    sample_id: str
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    note: str


def block_state(item: BlockPlacement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def add_single_sandwich(
    items: list[BlockPlacement],
    sample_id: str,
    block_id: str,
    origin_x: int,
    origin_y: int,
    origin_z: int,
    label: str,
) -> None:
    items.append(
        BlockPlacement(
            sample_id=sample_id,
            block_id=block_id,
            x=origin_x,
            y=origin_y,
            z=origin_z,
            properties={"axis": "y"},
            note=f"{label} single froglight center.",
        )
    )
    items.extend(
        [
            BlockPlacement(
                sample_id=sample_id,
                block_id="minecraft:glass",
                x=origin_x - 1,
                y=origin_y,
                z=origin_z,
                properties={},
                note=f"{label} single west glass.",
            ),
            BlockPlacement(
                sample_id=sample_id,
                block_id="minecraft:glass",
                x=origin_x + 1,
                y=origin_y,
                z=origin_z,
                properties={},
                note=f"{label} single east glass.",
            ),
        ]
    )


def add_grid_sandwich(
    items: list[BlockPlacement],
    sample_id: str,
    block_id: str,
    origin_x: int,
    origin_y: int,
    origin_z: int,
    label: str,
) -> None:
    for dx in range(3):
        for dz in range(3):
            items.append(
                BlockPlacement(
                    sample_id=sample_id,
                    block_id=block_id,
                    x=origin_x + dx,
                    y=origin_y,
                    z=origin_z + dz,
                    properties={"axis": "y"},
                    note=f"{label} 3x3 froglight body.",
                )
            )
    for dz in range(3):
        items.extend(
            [
                BlockPlacement(
                    sample_id=sample_id,
                    block_id="minecraft:glass",
                    x=origin_x - 1,
                    y=origin_y,
                    z=origin_z + dz,
                    properties={},
                    note=f"{label} 3x3 west glass wall.",
                ),
                BlockPlacement(
                    sample_id=sample_id,
                    block_id="minecraft:glass",
                    x=origin_x + 3,
                    y=origin_y,
                    z=origin_z + dz,
                    properties={},
                    note=f"{label} 3x3 east glass wall.",
                ),
            ]
        )


def placements() -> list[BlockPlacement]:
    items: list[BlockPlacement] = []
    base_y = 1
    single_z = 1
    grid_z = 7
    x_step = 10
    for index, (sample_id, block_id, label) in enumerate(FROGLIGHTS):
        origin_x = 1 + index * x_step
        add_single_sandwich(items, sample_id, block_id, origin_x, base_y, single_z, label)
        add_grid_sandwich(items, sample_id, block_id, origin_x - 1, base_y, grid_z, label)
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
        "Froglight family sandwich fixture layout",
        "",
        "Purpose:",
        "  Froglight family baseline for model-quad culling under glass sandwich.",
        "",
        "Samples:",
        "  F01 = ochre_froglight",
        "  F02 = verdant_froglight",
        "  F03 = pearlescent_froglight",
        "",
        "Each sample contains:",
        "  1. single froglight + west/east glass",
        "  2. 3x3 froglight + west/east glass walls",
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
        name="Froglight Family Sandwich Fixture",
        author="Litematica Nova",
        description="Froglight family fixture for full-mode model-quad culling checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Generate a froglight family glass sandwich .litematic fixture."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_froglight_sandwich_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_froglight_sandwich_fixture_layout.txt",
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
