from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953

LABEL_FG = "minecraft:white_concrete"
LABEL_BG = "minecraft:black_concrete"
PLATFORM_BASE = "minecraft:smooth_stone"
PLATFORM_GEOM = "minecraft:stone_bricks"
PLATFORM_MIXED = "minecraft:deepslate_tiles"
AIR = "minecraft:air"

CELL_W = 12
CELL_D = 11
SECTION_GAP_Z = 6

GLYPHS = {
    "0": ["111", "101", "101", "101", "111"],
    "1": ["010", "110", "010", "010", "111"],
    "2": ["111", "001", "111", "100", "111"],
    "3": ["111", "001", "111", "001", "111"],
    "4": ["101", "101", "111", "001", "001"],
    "5": ["111", "100", "111", "001", "111"],
    "6": ["111", "100", "111", "101", "111"],
    "7": ["111", "001", "001", "001", "001"],
    "8": ["111", "101", "111", "101", "111"],
    "9": ["111", "101", "111", "001", "111"],
    "L": ["100", "100", "100", "100", "111"],
    "N": ["101", "111", "111", "111", "101"],
    "G": ["111", "100", "101", "101", "111"],
    "M": ["101", "111", "111", "101", "101"],
}


@dataclass(frozen=True)
class BlockPlacement:
    sample_id: str
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    note: str


@dataclass(frozen=True)
class Scenario:
    sample_id: str
    section: str
    title: str
    anchor_x: int
    anchor_y: int
    anchor_z: int
    note: str


def block_state(item: BlockPlacement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def add_label_blocks(out: list[BlockPlacement], sample_id: str, x: int, y: int, z: int) -> None:
    width = len(sample_id) * 4 - 1
    for dx in range(width):
        for dz in range(5):
            out.append(BlockPlacement(sample_id, LABEL_BG, x + dx, y, z + dz, {}, "Label background"))
    cursor = x
    for char in sample_id:
        glyph = GLYPHS[char]
        for dz, row in enumerate(glyph):
            for dx, cell in enumerate(row):
                if cell == "1":
                    out.append(BlockPlacement(sample_id, LABEL_FG, cursor + dx, y, z + dz, {}, "Label foreground"))
        cursor += 4


def add_platform(
    out: list[BlockPlacement],
    sample_id: str,
    x: int,
    z: int,
    width: int,
    depth: int,
    block_id: str,
    note: str,
) -> None:
    for dx in range(width):
        for dz in range(depth):
            out.append(BlockPlacement(sample_id, block_id, x + dx, 0, z + dz, {}, note))


def add_water(
    out: list[BlockPlacement],
    sample_id: str,
    x: int,
    y: int,
    z: int,
    level: int,
    note: str,
) -> None:
    out.append(
        BlockPlacement(
            sample_id,
            "minecraft:water",
            x,
            y,
            z,
            {"level": str(level)},
            note,
        )
    )


def add_block(
    out: list[BlockPlacement],
    sample_id: str,
    block_id: str,
    x: int,
    y: int,
    z: int,
    properties: dict[str, str] | None,
    note: str,
) -> None:
    out.append(BlockPlacement(sample_id, block_id, x, y, z, properties or {}, note))


def level_section(out: list[BlockPlacement], scenarios: list[Scenario], base_x: int, base_z: int) -> None:
    for idx, level in enumerate(range(8)):
        row = idx // 4
        col = idx % 4
        cell_x = base_x + col * CELL_W
        cell_z = base_z + row * CELL_D
        sample_id = f"L{idx + 1:02}"
        add_label_blocks(out, sample_id, cell_x + 1, 0, cell_z + 1)
        add_platform(out, sample_id, cell_x, cell_z, 9, 9, PLATFORM_BASE, "Level section platform")
        add_water(out, sample_id, cell_x + 5, 1, cell_z + 5, level, f"Single water block, level={level}")
        scenarios.append(
            Scenario(
                sample_id,
                "levels",
                f"Single water level {level}",
                cell_x + 5,
                1,
                cell_z + 5,
                f"Standalone water block with level={level}.",
            )
        )


def adjacency_section(out: list[BlockPlacement], scenarios: list[Scenario], base_x: int, base_z: int) -> None:
    specs = [
        ("N01", "water to air edge", lambda ox, oz: add_water(out, "N01", ox + 4, 1, oz + 4, 0, "Source water with open air on every side")),
        (
            "N02",
            "water next to full block",
            lambda ox, oz: (
                add_water(out, "N02", ox + 4, 1, oz + 4, 0, "Water adjacent to stone on east"),
                add_block(out, "N02", "minecraft:stone", ox + 5, 1, oz + 4, {}, "Opaque full block neighbor"),
            ),
        ),
        (
            "N03",
            "water next to glass",
            lambda ox, oz: (
                add_water(out, "N03", ox + 4, 1, oz + 4, 0, "Water adjacent to glass on east"),
                add_block(out, "N03", "minecraft:glass", ox + 5, 1, oz + 4, {}, "Glass full block neighbor"),
            ),
        ),
        (
            "N04",
            "water next to rail",
            lambda ox, oz: (
                add_water(out, "N04", ox + 4, 1, oz + 4, 0, "Water adjacent to rail on east"),
                add_block(out, "N04", "minecraft:stone", ox + 5, 0, oz + 4, {}, "Rail support block"),
                add_block(
                    out,
                    "N04",
                    "minecraft:rail",
                    ox + 5,
                    1,
                    oz + 4,
                    {"shape": "north_south", "waterlogged": "false"},
                    "Rail preserve-neighbor sample",
                ),
            ),
        ),
        (
            "N05",
            "water next to redstone wire",
            lambda ox, oz: (
                add_water(out, "N05", ox + 4, 1, oz + 4, 0, "Water adjacent to redstone wire on east"),
                add_block(out, "N05", "minecraft:stone", ox + 5, 0, oz + 4, {}, "Wire support block"),
                add_block(
                    out,
                    "N05",
                    "minecraft:redstone_wire",
                    ox + 5,
                    1,
                    oz + 4,
                    {"power": "7", "north": "none", "south": "none", "east": "none", "west": "none"},
                    "Redstone wire preserve-neighbor sample",
                ),
            ),
        ),
        (
            "N06",
            "water next to repeater",
            lambda ox, oz: (
                add_water(out, "N06", ox + 4, 1, oz + 4, 0, "Water adjacent to repeater on east"),
                add_block(out, "N06", "minecraft:stone", ox + 5, 0, oz + 4, {}, "Repeater support block"),
                add_block(
                    out,
                    "N06",
                    "minecraft:repeater",
                    ox + 5,
                    1,
                    oz + 4,
                    {"delay": "1", "facing": "east", "locked": "false", "powered": "false"},
                    "Repeater preserve-neighbor sample",
                ),
            ),
        ),
        (
            "N07",
            "water next to hopper",
            lambda ox, oz: (
                add_water(out, "N07", ox + 4, 1, oz + 4, 0, "Water adjacent to hopper on east"),
                add_block(
                    out,
                    "N07",
                    "minecraft:hopper",
                    ox + 5,
                    1,
                    oz + 4,
                    {"enabled": "true", "facing": "down"},
                    "Hopper preserve-neighbor sample",
                ),
            ),
        ),
        (
            "N08",
            "adjacent water low-high line",
            lambda ox, oz: (
                add_water(out, "N08", ox + 3, 1, oz + 4, 0, "Level 0"),
                add_water(out, "N08", ox + 4, 1, oz + 4, 3, "Level 3"),
                add_water(out, "N08", ox + 5, 1, oz + 4, 7, "Level 7"),
            ),
        ),
        (
            "N09",
            "adjacent water side pair",
            lambda ox, oz: (
                add_water(out, "N09", ox + 4, 1, oz + 4, 1, "Level 1 west"),
                add_water(out, "N09", ox + 5, 1, oz + 4, 6, "Level 6 east"),
            ),
        ),
        (
            "N10",
            "water wall edge mix",
            lambda ox, oz: (
                add_water(out, "N10", ox + 4, 1, oz + 4, 2, "Center level 2"),
                add_water(out, "N10", ox + 4, 1, oz + 5, 5, "South level 5"),
                add_block(out, "N10", "minecraft:stone", ox + 5, 1, oz + 4, {}, "East wall"),
                add_block(out, "N10", "minecraft:glass", ox + 3, 1, oz + 4, {}, "West glass"),
            ),
        ),
    ]

    for idx, (sample_id, title, builder) in enumerate(specs):
        row = idx // 5
        col = idx % 5
        cell_x = base_x + col * CELL_W
        cell_z = base_z + row * CELL_D
        add_label_blocks(out, sample_id, cell_x + 1, 0, cell_z + 1)
        add_platform(out, sample_id, cell_x, cell_z, 9, 9, PLATFORM_BASE, "Adjacency section platform")
        builder(cell_x, cell_z)
        scenarios.append(
            Scenario(sample_id, "adjacency", title, cell_x + 4, 1, cell_z + 4, title),
        )


def geometry_section(out: list[BlockPlacement], scenarios: list[Scenario], base_x: int, base_z: int) -> None:
    def g01(ox: int, oz: int) -> None:
        for dx, level in enumerate([0, 1, 3, 5]):
            add_water(out, "G01", ox + 2 + dx, 1, oz + 4, level, f"Straight line level={level}")

    def g02(ox: int, oz: int) -> None:
        add_water(out, "G02", ox + 4, 1, oz + 4, 0, "Corner center")
        add_water(out, "G02", ox + 5, 1, oz + 4, 3, "Corner east arm")
        add_water(out, "G02", ox + 4, 1, oz + 5, 6, "Corner south arm")

    def g03(ox: int, oz: int) -> None:
        add_water(out, "G03", ox + 4, 1, oz + 4, 0, "Cross center")
        add_water(out, "G03", ox + 4, 1, oz + 3, 2, "North arm")
        add_water(out, "G03", ox + 4, 1, oz + 5, 4, "South arm")
        add_water(out, "G03", ox + 3, 1, oz + 4, 6, "West arm")
        add_water(out, "G03", ox + 5, 1, oz + 4, 7, "East arm")

    def g04(ox: int, oz: int) -> None:
        for dx in range(2):
            for dz in range(2):
                add_water(out, "G04", ox + 4 + dx, 1, oz + 4 + dz, 0, "2x2 source platform")

    def g05(ox: int, oz: int) -> None:
        add_water(out, "G05", ox + 3, 1, oz + 5, 0, "Step 1")
        add_water(out, "G05", ox + 4, 1, oz + 4, 2, "Step 2")
        add_water(out, "G05", ox + 5, 1, oz + 3, 4, "Step 3")
        add_water(out, "G05", ox + 6, 1, oz + 2, 6, "Step 4")

    def g06(ox: int, oz: int) -> None:
        for x in range(3, 7):
            add_block(out, "G06", "minecraft:stone", ox + x, 1, oz + 3, {}, "Pit north wall")
            add_block(out, "G06", "minecraft:stone", ox + x, 1, oz + 6, {}, "Pit south wall")
        for z in range(4, 6):
            add_block(out, "G06", "minecraft:stone", ox + 3, 1, oz + z, {}, "Pit west wall")
            add_block(out, "G06", "minecraft:stone", ox + 6, 1, oz + z, {}, "Pit east wall")
        add_water(out, "G06", ox + 4, 1, oz + 4, 0, "Pit water north-west")
        add_water(out, "G06", ox + 5, 1, oz + 4, 5, "Pit water north-east")
        add_water(out, "G06", ox + 4, 1, oz + 5, 7, "Pit water south-west")
        add_water(out, "G06", ox + 5, 1, oz + 5, 3, "Pit water south-east")

    def g07(ox: int, oz: int) -> None:
        add_block(out, "G07", "minecraft:stone", ox + 6, 1, oz + 3, {}, "Raised edge wall")
        add_block(out, "G07", "minecraft:stone", ox + 6, 1, oz + 4, {}, "Raised edge wall")
        add_block(out, "G07", "minecraft:stone", ox + 6, 1, oz + 5, {}, "Raised edge wall")
        add_water(out, "G07", ox + 4, 1, oz + 3, 0, "Edge north")
        add_water(out, "G07", ox + 4, 1, oz + 4, 2, "Edge center")
        add_water(out, "G07", ox + 4, 1, oz + 5, 6, "Edge south")

    def g08(ox: int, oz: int) -> None:
        add_water(out, "G08", ox + 3, 1, oz + 4, 0, "Channel start")
        add_water(out, "G08", ox + 4, 1, oz + 4, 2, "Channel mid")
        add_water(out, "G08", ox + 5, 1, oz + 4, 4, "Channel turn")
        add_water(out, "G08", ox + 5, 1, oz + 5, 6, "Channel south")

    builders = [
        ("G01", "straight line", g01),
        ("G02", "corner", g02),
        ("G03", "cross", g03),
        ("G04", "2x2 platform", g04),
        ("G05", "stair-step surface", g05),
        ("G06", "pit/basin", g06),
        ("G07", "edge against wall", g07),
        ("G08", "turning channel", g08),
    ]
    for idx, (sample_id, title, builder) in enumerate(builders):
        row = idx // 4
        col = idx % 4
        cell_x = base_x + col * CELL_W
        cell_z = base_z + row * CELL_D
        add_label_blocks(out, sample_id, cell_x + 1, 0, cell_z + 1)
        add_platform(out, sample_id, cell_x, cell_z, 9, 9, PLATFORM_GEOM, "Geometry section platform")
        builder(cell_x, cell_z)
        scenarios.append(
            Scenario(sample_id, "geometry", title, cell_x + 4, 1, cell_z + 4, title),
        )


def mixed_section(out: list[BlockPlacement], scenarios: list[Scenario], base_x: int, base_z: int) -> None:
    sample_id = "M01"
    add_label_blocks(out, sample_id, base_x + 1, 0, base_z + 1)
    add_platform(out, sample_id, base_x, base_z, 16, 14, PLATFORM_MIXED, "Mixed section platform")

    for x in range(3, 12):
        add_water(out, sample_id, base_x + x, 1, base_z + 5, min((x - 3), 7), f"Mixed straight run level={min((x - 3), 7)}")
    add_water(out, sample_id, base_x + 10, 1, base_z + 6, 4, "Turn south")
    add_water(out, sample_id, base_x + 10, 1, base_z + 7, 6, "Turn south deep")
    add_water(out, sample_id, base_x + 9, 1, base_z + 7, 3, "Corner fill")
    add_water(out, sample_id, base_x + 8, 1, base_z + 7, 1, "Corner fill")
    add_water(out, sample_id, base_x + 6, 1, base_z + 8, 0, "Pocket 1")
    add_water(out, sample_id, base_x + 7, 1, base_z + 8, 5, "Pocket 2")
    add_water(out, sample_id, base_x + 8, 1, base_z + 8, 7, "Pocket 3")
    add_water(out, sample_id, base_x + 8, 1, base_z + 9, 2, "Pocket 4")

    for z in range(4, 10):
        add_block(out, sample_id, "minecraft:glass", base_x + 12, 1, base_z + z, {}, "Glass wall beside water")
    add_block(out, sample_id, "minecraft:stone", base_x + 4, 1, base_z + 3, {}, "Opaque obstruction")
    add_block(out, sample_id, "minecraft:stone", base_x + 5, 1, base_z + 3, {}, "Opaque obstruction")
    add_block(out, sample_id, "minecraft:stone", base_x + 5, 0, base_z + 10, {}, "Rail support")
    add_block(
        out,
        sample_id,
        "minecraft:rail",
        base_x + 5,
        1,
        base_z + 10,
        {"shape": "east_west", "waterlogged": "false"},
        "Rail mixed neighbor",
    )
    add_block(out, sample_id, "minecraft:stone", base_x + 7, 0, base_z + 10, {}, "Repeater support")
    add_block(
        out,
        sample_id,
        "minecraft:repeater",
        base_x + 7,
        1,
        base_z + 10,
        {"delay": "1", "facing": "north", "locked": "false", "powered": "false"},
        "Repeater mixed neighbor",
    )
    add_block(out, sample_id, "minecraft:stone", base_x + 9, 0, base_z + 10, {}, "Redstone support")
    add_block(
        out,
        sample_id,
        "minecraft:redstone_wire",
        base_x + 9,
        1,
        base_z + 10,
        {"power": "5", "north": "none", "south": "none", "east": "none", "west": "none"},
        "Redstone mixed neighbor",
    )
    add_block(
        out,
        sample_id,
        "minecraft:hopper",
        base_x + 11,
        1,
        base_z + 10,
        {"enabled": "true", "facing": "down"},
        "Hopper mixed neighbor",
    )
    scenarios.append(
        Scenario(
            sample_id,
            "mixed",
            "Continuous multi-block mixed sample",
            base_x + 8,
            1,
            base_z + 7,
            "Continuous water run with turn, pocket, glass wall, opaque blocks, rail, repeater, wire, and hopper.",
        )
    )


def build_fixture() -> tuple[litemapy.Schematic, str]:
    placements: list[BlockPlacement] = []
    scenarios: list[Scenario] = []

    level_section(placements, scenarios, 0, 0)
    adjacency_base_z = 2 * CELL_D + SECTION_GAP_Z
    adjacency_section(placements, scenarios, 0, adjacency_base_z)
    geometry_base_z = adjacency_base_z + 2 * CELL_D + SECTION_GAP_Z
    geometry_section(placements, scenarios, 0, geometry_base_z)
    mixed_base_z = geometry_base_z + 2 * CELL_D + SECTION_GAP_Z
    mixed_section(placements, scenarios, 0, mixed_base_z)

    min_x = min(item.x for item in placements)
    min_y = min(item.y for item in placements)
    min_z = min(item.z for item in placements)
    max_x = max(item.x for item in placements)
    max_y = max(item.y for item in placements)
    max_z = max(item.z for item in placements)
    width = max_x - min_x + 1
    height = max_y - min_y + 1
    length = max_z - min_z + 1

    region = litemapy.Region(0, 0, 0, width, height, length)
    for item in placements:
        region[item.x - min_x, item.y - min_y, item.z - min_z] = block_state(item)

    lines = [
        "Water complex fixture layout",
        "",
        "Purpose:",
        "  Complex Full Mode V2 water verification fixture with visible sample IDs.",
        "  Covers level 0..7, adjacency rules, geometry patterns, and one mixed composition zone.",
        "",
        "Notes:",
        "  - sample_id is rendered into the fixture as a visible floor label",
        "  - numbering is baked into the schematic, so numbered runs do not need a renderer-side water debug mode",
        "",
        "Scenario Columns:",
        "sample_id\tsection\ttitle\tanchor_x\tanchor_y\tanchor_z\tnote",
    ]
    for scenario in scenarios:
        lines.append(
            "\t".join(
                [
                    scenario.sample_id,
                    scenario.section,
                    scenario.title,
                    str(scenario.anchor_x - min_x),
                    str(scenario.anchor_y - min_y),
                    str(scenario.anchor_z - min_z),
                    scenario.note,
                ]
            )
        )

    lines.extend(
        [
            "",
            "Block Columns:",
            "sample_id\tblock_id\tx\ty\tz\tproperties\tnote",
        ]
    )
    for item in placements:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        lines.append(
            "\t".join(
                [
                    item.sample_id,
                    item.block_id,
                    str(item.x - min_x),
                    str(item.y - min_y),
                    str(item.z - min_z),
                    properties,
                    item.note,
                ]
            )
        )

    lines.extend(
        [
            "",
            f"Scenario count: {len(scenarios)}",
            f"Placed blocks: {len(placements)}",
            f"Bounding box: width={width}, height={height}, length={length}",
        ]
    )

    schematic = region.as_schematic(
        name="Water Complex Fixture",
        author="Litematica Nova",
        description=(
            "Complex water verification fixture for Full Mode V2 with visible IDs, level matrix, adjacency matrix, "
            "geometry patterns, and a mixed local scene."
        ),
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a complex water .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_water_complex_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_water_complex_fixture_layout.txt",
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
