from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953
FACING_ORDER = ["north", "south", "west", "east"]
CELL_W = 14
CELL_D = 12
SECTION_GAP = 8
CHEST_Y = 1
LABEL_Y = 0
LABEL_FG = "minecraft:white_concrete"
LABEL_BG = "minecraft:black_concrete"
PLATFORM_NORMAL = "minecraft:smooth_stone"
PLATFORM_TRAPPED = "minecraft:deepslate_tiles"
STATE_MARKER = {
    "closed": "minecraft:light_gray_concrete",
    "open": "minecraft:lime_concrete",
}
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
    "C": ["111", "100", "100", "100", "111"],
    "T": ["111", "010", "010", "010", "010"],
    "D": ["110", "101", "101", "101", "110"],
    "H": ["101", "101", "111", "101", "101"],
}

ROW_SPECS = [
    ("single", "standalone", "closed"),
    ("single", "standalone", "open"),
    ("left", "standalone", "closed"),
    ("left", "standalone", "open"),
    ("right", "standalone", "closed"),
    ("right", "standalone", "open"),
    ("left", "paired", "closed"),
    ("left", "paired", "open"),
    ("right", "paired", "closed"),
    ("right", "paired", "open"),
]


@dataclass(frozen=True)
class Sample:
    sample_id: str
    block_id: str
    family: str
    shape: str
    arrangement: str
    target_state: str
    facing: str
    x: int
    y: int
    z: int
    label_x: int
    label_y: int
    label_z: int


@dataclass(frozen=True)
class BlockPlacement:
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str] | None = None


def block_state(item: BlockPlacement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **(item.properties or {}))


def double_partner_offset(facing: str, shape: str) -> tuple[int, int]:
    mapping = {
        ("north", "left"): (1, 0),
        ("north", "right"): (-1, 0),
        ("south", "left"): (-1, 0),
        ("south", "right"): (1, 0),
        ("west", "left"): (0, -1),
        ("west", "right"): (0, 1),
        ("east", "left"): (0, 1),
        ("east", "right"): (0, -1),
    }
    return mapping[(facing, shape)]


def add_label_blocks(out: list[BlockPlacement], text: str, x: int, y: int, z: int) -> None:
    width = len(text) * 4 - 1
    for dx in range(width):
        for dz in range(5):
            out.append(BlockPlacement(LABEL_BG, x + dx, y, z + dz))
    cursor = x
    for char in text:
        glyph = GLYPHS[char]
        for dz, row in enumerate(glyph):
            for dx, cell in enumerate(row):
                if cell == "1":
                    out.append(BlockPlacement(LABEL_FG, cursor + dx, y, z + dz))
        cursor += 4


def add_platform(out: list[BlockPlacement], x: int, z: int, platform_block: str) -> None:
    for dx in range(5):
        for dz in range(5):
            out.append(BlockPlacement(platform_block, x + dx, 0, z + dz))


def add_state_marker(out: list[BlockPlacement], state: str, arrangement: str, x: int, z: int) -> None:
    marker = STATE_MARKER[state]
    out.append(BlockPlacement(marker, x, 1, z))
    out.append(BlockPlacement(marker, x, 2, z))
    if arrangement == "paired":
        out.append(BlockPlacement(marker, x + 1, 2, z))


def add_chest_sample(out: list[BlockPlacement], sample: Sample) -> None:
    platform = PLATFORM_NORMAL if sample.family == "normal" else PLATFORM_TRAPPED
    add_label_blocks(out, sample.sample_id, sample.label_x, sample.label_y, sample.label_z)
    add_platform(out, sample.x - 1, sample.z - 1, platform)
    add_state_marker(out, sample.target_state, sample.arrangement, sample.x + 2, sample.z + 1)

    props = {
        "facing": sample.facing,
        "type": sample.shape,
        "waterlogged": "false",
    }
    out.append(BlockPlacement(sample.block_id, sample.x, sample.y, sample.z, props))

    if sample.arrangement != "paired" or sample.shape == "single":
        return

    dx, dz = double_partner_offset(sample.facing, sample.shape)
    partner_type = "right" if sample.shape == "left" else "left"
    out.append(
        BlockPlacement(
            sample.block_id,
            sample.x + dx,
            sample.y,
            sample.z + dz,
            {
                "facing": sample.facing,
                "type": partner_type,
                "waterlogged": "false",
            },
        )
    )


def build_samples() -> list[Sample]:
    samples: list[Sample] = []
    section_specs = [
        ("C", "minecraft:chest", "normal", 0),
        ("T", "minecraft:trapped_chest", "trapped", 4 * CELL_W + SECTION_GAP),
    ]
    for prefix, block_id, family, base_x in section_specs:
        index = 1
        for row, (shape, arrangement, target_state) in enumerate(ROW_SPECS):
            cell_z = row * CELL_D
            for col, facing in enumerate(FACING_ORDER):
                cell_x = base_x + col * CELL_W
                sample_id = f"{prefix}{index:02}"
                samples.append(
                    Sample(
                        sample_id=sample_id,
                        block_id=block_id,
                        family=family,
                        shape=shape,
                        arrangement=arrangement,
                        target_state=target_state,
                        facing=facing,
                        x=cell_x + 6,
                        y=CHEST_Y,
                        z=cell_z + 8,
                        label_x=cell_x + 1,
                        label_y=LABEL_Y,
                        label_z=cell_z + 1,
                    )
                )
                index += 1
    return samples


def build_fixture() -> tuple[litemapy.Schematic, str]:
    samples = build_samples()
    placements: list[BlockPlacement] = []
    for sample in samples:
        add_chest_sample(placements, sample)

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
        "Chest experiment fixture layout",
        "",
        "Purpose:",
        "  Unified chest family verification baseline for Full Mode V2.",
        "  Includes visible sample IDs, normal/trapped chest families, single chests, standalone large-chest halves,",
        "  and paired double chests across four horizontal facings plus closed/open validation slots.",
        "",
        "Notes:",
        "  - sample_id is rendered into the fixture as a visible floor label",
        "  - arrangement=standalone means a single placed blockstate only, including isolated left/right halves",
        "  - arrangement=paired means the opposite half is placed too, so the full double chest is visible",
        "  - target_state=open/closed is a validation slot annotation for later chest-family work",
        "",
        "Columns:",
        "sample_id\tfamily\tblock_id\tshape\tarrangement\ttarget_state\tfacing\tanchor_x\tanchor_y\tanchor_z\tlabel_x\tlabel_y\tlabel_z",
    ]
    for sample in samples:
        lines.append(
            "\t".join(
                [
                    sample.sample_id,
                    sample.family,
                    sample.block_id,
                    sample.shape,
                    sample.arrangement,
                    sample.target_state,
                    sample.facing,
                    str(sample.x - min_x),
                    str(sample.y - min_y),
                    str(sample.z - min_z),
                    str(sample.label_x - min_x),
                    str(sample.label_y - min_y),
                    str(sample.label_z - min_z),
                ]
            )
        )

    schematic = region.as_schematic(
        name="Chest Experiment Fixture",
        author="Litematica Nova",
        description=(
            "Chest family verification fixture with visible sample IDs, normal/trapped sections, "
            "single chests, standalone left/right halves, paired double chests, and closed/open validation slots."
        ),
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a chest family experiment .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_chest_experiment_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_chest_experiment_fixture_layout.txt",
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
