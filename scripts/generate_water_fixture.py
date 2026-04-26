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

    # W01: single source water
    out.append(
        BlockPlacement(
            group="W01",
            block_id="minecraft:water",
            x=1,
            y=1,
            z=1,
            properties={"level": "0"},
            note="Single source water with open air around it.",
        )
    )

    # W02: adjacent waters with different heights
    for offset, level in enumerate(["0", "3", "6"]):
        out.append(
            BlockPlacement(
                group="W02",
                block_id="minecraft:water",
                x=6 + offset,
                y=1,
                z=1,
                properties={"level": level},
                note=f"Three adjacent water blocks with level={level}.",
            )
        )

    # W03: water against a full opaque block
    out.extend(
        [
            BlockPlacement(
                group="W03",
                block_id="minecraft:water",
                x=1,
                y=1,
                z=6,
                properties={"level": "0"},
                note="Water adjacent to stone on the east face.",
            ),
            BlockPlacement(
                group="W03",
                block_id="minecraft:stone",
                x=2,
                y=1,
                z=6,
                properties={},
                note="Opaque full cube neighbor for W03.",
            ),
        ]
    )

    # W04: water against glass
    out.extend(
        [
            BlockPlacement(
                group="W04",
                block_id="minecraft:water",
                x=6,
                y=1,
                z=6,
                properties={"level": "0"},
                note="Water adjacent to glass on the east face.",
            ),
            BlockPlacement(
                group="W04",
                block_id="minecraft:glass",
                x=7,
                y=1,
                z=6,
                properties={},
                note="Glass neighbor for W04.",
            ),
        ]
    )

    # W05: water against rail
    out.extend(
        [
            BlockPlacement(
                group="W05",
                block_id="minecraft:water",
                x=11,
                y=1,
                z=6,
                properties={"level": "0"},
                note="Water adjacent to rail on the east face.",
            ),
            BlockPlacement(
                group="W05",
                block_id="minecraft:stone",
                x=12,
                y=0,
                z=6,
                properties={},
                note="Support block for rail.",
            ),
            BlockPlacement(
                group="W05",
                block_id="minecraft:rail",
                x=12,
                y=1,
                z=6,
                properties={"shape": "north_south", "waterlogged": "false"},
                note="Rail neighbor for W05.",
            ),
        ]
    )

    # W06: water against redstone wire
    out.extend(
        [
            BlockPlacement(
                group="W06",
                block_id="minecraft:water",
                x=16,
                y=1,
                z=6,
                properties={"level": "0"},
                note="Water adjacent to redstone wire on the east face.",
            ),
            BlockPlacement(
                group="W06",
                block_id="minecraft:stone",
                x=17,
                y=0,
                z=6,
                properties={},
                note="Support block for redstone wire.",
            ),
            BlockPlacement(
                group="W06",
                block_id="minecraft:redstone_wire",
                x=17,
                y=1,
                z=6,
                properties={
                    "power": "7",
                    "north": "none",
                    "south": "none",
                    "east": "none",
                    "west": "none",
                },
                note="Redstone wire neighbor for W06.",
            ),
        ]
    )

    # W07: water against a waterlogged special plant block
    out.extend(
        [
            BlockPlacement(
                group="W07",
                block_id="minecraft:water",
                x=21,
                y=1,
                z=6,
                properties={"level": "0"},
                note="Water adjacent to waterlogged big dripleaf stem on the east face.",
            ),
            BlockPlacement(
                group="W07",
                block_id="minecraft:big_dripleaf_stem",
                x=22,
                y=1,
                z=6,
                properties={"facing": "north", "waterlogged": "true"},
                note="Waterlogged special plant neighbor for W07.",
            ),
        ]
    )

    # W08: source water pair for same-fluid internal face culling
    out.extend(
        [
            BlockPlacement(
                group="W08",
                block_id="minecraft:water",
                x=26,
                y=1,
                z=6,
                properties={"level": "0"},
                note="First source water in same-fluid pair.",
            ),
            BlockPlacement(
                group="W08",
                block_id="minecraft:water",
                x=27,
                y=1,
                z=6,
                properties={"level": "0"},
                note="Second source water in same-fluid pair.",
            ),
        ]
    )

    # W09: compact source pool for visual block-boundary checks
    for dx in range(3):
        for dz in range(3):
            out.append(
                BlockPlacement(
                    group="W09",
                    block_id="minecraft:water",
                    x=32 + dx,
                    y=1,
                    z=5 + dz,
                    properties={"level": "0"},
                    note="3x3 source-water pool; same-fluid internal sides should not create visible grid seams.",
                )
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

    layout_lines = [
        "Water fixture layout",
        "",
        "Purpose:",
        "  Minimal Full Mode V2 water fixture covering height, adjacency, glass, rail, and redstone wire neighbors.",
        "",
        "Columns:",
        "group\tblock_id\tx\ty\tz\tproperties\tnote",
    ]
    for item in items:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        layout_lines.append(
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
    layout_lines.extend(
        [
            "",
            "Groups:",
            "  W01: single water",
            "  W02: adjacent different heights",
            "  W03: water next to stone",
            "  W04: water next to glass",
            "  W05: water next to rail",
            "  W06: water next to redstone wire",
            "  W07: water next to waterlogged big dripleaf stem",
            "  W08: adjacent source water pair",
            "",
            f"Placed blocks: {len(items)}",
            f"Bounding box: width={width}, height={height}, length={length}",
        ]
    )

    schematic = region.as_schematic(
        name="Water Fixture",
        author="Litematica Nova",
        description=(
            "Minimal water special-family fixture for Full Mode V2: height, adjacency, glass, rail, and redstone wire coverage."
        ),
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(layout_lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a minimal water .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_water_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_water_fixture_layout.txt",
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
