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
    out: list[BlockPlacement] = []

    rail_x = 2
    rail_y = 1
    rail_z = 2
    out.extend(
        [
            BlockPlacement(
                group="R01",
                block_id="minecraft:rail",
                x=rail_x,
                y=rail_y,
                z=rail_z,
                properties={"shape": "north_south", "waterlogged": "false"},
                note="Central rail.",
            ),
            BlockPlacement(
                group="R01",
                block_id="minecraft:stone",
                x=rail_x,
                y=rail_y - 1,
                z=rail_z,
                properties={},
                note="Down neighbor / support under rail.",
            ),
            BlockPlacement(
                group="R01",
                block_id="minecraft:stone",
                x=rail_x,
                y=rail_y + 1,
                z=rail_z,
                properties={},
                note="Up neighbor above rail.",
            ),
            BlockPlacement(
                group="R01",
                block_id="minecraft:stone",
                x=rail_x - 1,
                y=rail_y,
                z=rail_z,
                properties={},
                note="West neighbor facing rail.",
            ),
            BlockPlacement(
                group="R01",
                block_id="minecraft:stone",
                x=rail_x + 1,
                y=rail_y,
                z=rail_z,
                properties={},
                note="East neighbor facing rail.",
            ),
            BlockPlacement(
                group="R01",
                block_id="minecraft:stone",
                x=rail_x,
                y=rail_y,
                z=rail_z - 1,
                properties={},
                note="North neighbor facing rail.",
            ),
            BlockPlacement(
                group="R01",
                block_id="minecraft:stone",
                x=rail_x,
                y=rail_y,
                z=rail_z + 1,
                properties={},
                note="South neighbor facing rail.",
            ),
        ]
    )

    smoke_x = 8
    smoke_z = 1
    out.extend(
        [
            BlockPlacement(
                group="S01",
                block_id="minecraft:glass",
                x=smoke_x + 0,
                y=1,
                z=smoke_z,
                properties={},
                note="Glass smoke sample.",
            ),
            BlockPlacement(
                group="S02",
                block_id="minecraft:stone",
                x=smoke_x + 3,
                y=0,
                z=smoke_z,
                properties={},
                note="Support for repeater.",
            ),
            BlockPlacement(
                group="S02",
                block_id="minecraft:repeater",
                x=smoke_x + 3,
                y=1,
                z=smoke_z,
                properties={"facing": "east", "delay": "1", "locked": "false", "powered": "false"},
                note="Repeater smoke sample.",
            ),
            BlockPlacement(
                group="S03",
                block_id="minecraft:stone",
                x=smoke_x + 6,
                y=0,
                z=smoke_z,
                properties={},
                note="Support for redstone wire.",
            ),
            BlockPlacement(
                group="S03",
                block_id="minecraft:redstone_wire",
                x=smoke_x + 6,
                y=1,
                z=smoke_z,
                properties={
                    "power": "5",
                    "north": "none",
                    "south": "none",
                    "east": "none",
                    "west": "none",
                },
                note="Redstone wire smoke sample.",
            ),
            BlockPlacement(
                group="S04",
                block_id="minecraft:hopper",
                x=smoke_x + 9,
                y=1,
                z=smoke_z,
                properties={"facing": "down", "enabled": "true"},
                note="Hopper smoke sample.",
            ),
            BlockPlacement(
                group="S05",
                block_id="minecraft:chest",
                x=smoke_x + 12,
                y=1,
                z=smoke_z,
                properties={"facing": "south", "type": "single", "waterlogged": "false"},
                note="Chest smoke sample.",
            ),
            BlockPlacement(
                group="S06",
                block_id="minecraft:barrel",
                x=smoke_x + 15,
                y=1,
                z=smoke_z,
                properties={"facing": "up", "open": "false"},
                note="Barrel smoke sample.",
            ),
            BlockPlacement(
                group="S07",
                block_id="minecraft:piston",
                x=smoke_x + 18,
                y=1,
                z=smoke_z,
                properties={"facing": "east", "extended": "false"},
                note="Static piston smoke sample.",
            ),
        ]
    )
    return out


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
        "Rail neighbor fixture layout",
        "",
        "Purpose:",
        "  Validate rail preserve-neighbor-faces semantics and smoke-check nearby special families.",
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
        name="Rail Neighbor Fixture",
        author="Litematica Nova",
        description="Minimal rail preserve-neighbor fixture plus smoke samples for nearby special families.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a rail preserve-neighbor .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_rail_neighbor_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_rail_neighbor_fixture_layout.txt",
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
