from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953


@dataclass(frozen=True)
class BlockPlacement:
    group: str
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    note: str


def block_state(item: BlockPlacement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def placements() -> list[BlockPlacement]:
    return [
        BlockPlacement(
            group="C01",
            block_id="minecraft:chest",
            x=1,
            y=1,
            z=1,
            properties={"facing": "south", "type": "single", "waterlogged": "false"},
            note="Single chest sample.",
        ),
        BlockPlacement(
            group="C02",
            block_id="minecraft:chest",
            x=5,
            y=1,
            z=1,
            properties={"facing": "south", "type": "left", "waterlogged": "false"},
            note="Left half of double chest.",
        ),
        BlockPlacement(
            group="C02",
            block_id="minecraft:chest",
            x=6,
            y=1,
            z=1,
            properties={"facing": "south", "type": "right", "waterlogged": "false"},
            note="Right half of double chest.",
        ),
        BlockPlacement(
            group="C03",
            block_id="minecraft:barrel",
            x=10,
            y=1,
            z=1,
            properties={"facing": "up", "open": "false"},
            note="Barrel smoke sample.",
        ),
        BlockPlacement(
            group="C04",
            block_id="minecraft:hopper",
            x=13,
            y=1,
            z=1,
            properties={"facing": "down", "enabled": "true"},
            note="Hopper smoke sample.",
        ),
    ]


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
        "Chest fixture layout",
        "",
        "Purpose:",
        "  Minimal chest family fixture for chest open-face top/bottom semantic remap checks, with barrel and hopper smoke samples.",
        "",
        "Columns:",
        "group\tblock_id\tx\ty\tz\tproperties\tnote",
    ]
    for item in items:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        lines.append(
            "\t".join(
                [
                    item.group,
                    item.block_id,
                    str(item.x - min_x),
                    str(item.y - min_y),
                    str(item.z - min_z),
                    properties,
                    item.note,
                ]
            )
        )

    schematic = region.as_schematic(
        name="Chest Fixture",
        author="Litematica Nova",
        description="Minimal chest fixture for chest open-face semantic remap checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a minimal chest .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_chest_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_chest_fixture_layout.txt",
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
