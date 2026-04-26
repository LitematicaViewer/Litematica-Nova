from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953


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


def placements() -> list[BlockPlacement]:
    base_y = 1
    base_z = 1
    gap = 6
    center_positions = {
        "G01": 1,
        "G02": 1 + gap,
        "G03": 1 + gap * 2,
        "G04": 1 + gap * 3,
    }

    items: list[BlockPlacement] = []
    for sample_id, center_x in center_positions.items():
        items.append(
            BlockPlacement(
                sample_id=sample_id,
                block_id="minecraft:stone",
                x=center_x,
                y=base_y,
                z=base_z,
                properties={},
                note="Center full cube under comparison.",
            )
        )

    items.extend(
        [
            BlockPlacement(
                sample_id="G02",
                block_id="minecraft:glass",
                x=center_positions["G02"] - 1,
                y=base_y,
                z=base_z,
                properties={},
                note="Single glass on west side.",
            ),
            BlockPlacement(
                sample_id="G03",
                block_id="minecraft:glass",
                x=center_positions["G03"] + 1,
                y=base_y,
                z=base_z,
                properties={},
                note="Single glass on east side.",
            ),
            BlockPlacement(
                sample_id="G04",
                block_id="minecraft:glass",
                x=center_positions["G04"] - 1,
                y=base_y,
                z=base_z,
                properties={},
                note="Sandwich glass on west side.",
            ),
            BlockPlacement(
                sample_id="G04",
                block_id="minecraft:glass",
                x=center_positions["G04"] + 1,
                y=base_y,
                z=base_z,
                properties={},
                note="Sandwich glass on east side.",
            ),
        ]
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
        "Glass sandwich fixture layout",
        "",
        "Purpose:",
        "  Minimal glass-whitelist sandwich fixture for mesh/culling verification.",
        "",
        "Samples:",
        "  G01 = center stone only",
        "  G02 = center stone with west glass",
        "  G03 = center stone with east glass",
        "  G04 = center stone with west+east glass sandwich",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append(
            "\t".join(
                [item.sample_id, item.block_id, anchor, properties, item.note]
            )
        )

    schematic = region.as_schematic(
        name="Glass Sandwich Fixture",
        author="Litematica Nova",
        description="Minimal fixture for glass sandwich culling regressions.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Generate a minimal glass sandwich .litematic fixture."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_glass_sandwich_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_glass_sandwich_fixture_layout.txt",
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
