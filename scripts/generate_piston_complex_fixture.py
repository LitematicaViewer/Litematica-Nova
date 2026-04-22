from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


FACING_ORDER = ["north", "east", "south", "west", "up", "down"]
MC_DATA_VERSION = 3953
BASE_Y = 3
CELL_STEP = 4


@dataclass(frozen=True)
class Placement:
    group: str
    role: str
    block_id: str
    x: int
    y: int
    z: int
    facing: str
    extended: str = "-"
    short: str = "-"
    type: str = "-"
    paired: str = "no"
    note: str = ""


def facing_offset(facing: str) -> tuple[int, int, int]:
    return {
        "north": (0, 0, -1),
        "south": (0, 0, 1),
        "east": (1, 0, 0),
        "west": (-1, 0, 0),
        "up": (0, 1, 0),
        "down": (0, -1, 0),
    }[facing]


def block_state(item: Placement) -> litemapy.BlockState:
    if item.block_id in {"minecraft:piston", "minecraft:sticky_piston"}:
        return litemapy.BlockState(
            item.block_id,
            facing=item.facing,
            extended=item.extended,
        )
    if item.block_id == "minecraft:piston_head":
        return litemapy.BlockState(
            item.block_id,
            facing=item.facing,
            short=item.short,
            type=item.type,
        )
    raise ValueError(f"unsupported block id: {item.block_id}")


def add_base_matrix(out: list[Placement]) -> None:
    rows = [
        ("minecraft:piston", "false"),
        ("minecraft:piston", "true"),
        ("minecraft:sticky_piston", "false"),
        ("minecraft:sticky_piston", "true"),
    ]
    base_x = 0
    base_z = 0
    index = 1
    for row, (block_id, extended) in enumerate(rows):
        z = base_z + row * CELL_STEP
        for col, facing in enumerate(FACING_ORDER):
            out.append(
                Placement(
                    group=f"B{index:02}",
                    role="base_matrix",
                    block_id=block_id,
                    x=base_x + col * CELL_STEP,
                    y=BASE_Y,
                    z=z,
                    facing=facing,
                    extended=extended,
                    note="Base standalone matrix",
                )
            )
            index += 1


def add_head_matrix(out: list[Placement]) -> None:
    rows = [
        ("normal", "false"),
        ("normal", "true"),
        ("sticky", "false"),
        ("sticky", "true"),
    ]
    base_x = 34
    base_z = 0
    index = 1
    for row, (head_type, short) in enumerate(rows):
        z = base_z + row * CELL_STEP
        for col, facing in enumerate(FACING_ORDER):
            out.append(
                Placement(
                    group=f"H{index:02}",
                    role="head_matrix",
                    block_id="minecraft:piston_head",
                    x=base_x + col * CELL_STEP,
                    y=BASE_Y,
                    z=z,
                    facing=facing,
                    short=short,
                    type=head_type,
                    note="Head standalone matrix",
                )
            )
            index += 1


def add_pair_matrix(out: list[Placement]) -> None:
    rows = [
        ("minecraft:piston", "normal", "false"),
        ("minecraft:piston", "normal", "true"),
        ("minecraft:sticky_piston", "sticky", "false"),
        ("minecraft:sticky_piston", "sticky", "true"),
    ]
    base_x = 0
    base_z = 26
    index = 1
    for row, (base_id, head_type, short) in enumerate(rows):
        z = base_z + row * (CELL_STEP + 1)
        for col, facing in enumerate(FACING_ORDER):
            x = base_x + col * CELL_STEP
            group = f"C{index:02}"
            out.append(
                Placement(
                    group=group,
                    role="paired_base",
                    block_id=base_id,
                    x=x,
                    y=BASE_Y,
                    z=z,
                    facing=facing,
                    extended="true",
                    paired="yes",
                    note="Base+head pair: base",
                )
            )
            dx, dy, dz = facing_offset(facing)
            out.append(
                Placement(
                    group=group,
                    role="paired_head",
                    block_id="minecraft:piston_head",
                    x=x + dx,
                    y=BASE_Y + dy,
                    z=z + dz,
                    facing=facing,
                    short=short,
                    type=head_type,
                    paired="yes",
                    note="Base+head pair: head placed one block in facing direction",
                )
            )
            index += 1


def add_mixed_zone(out: list[Placement]) -> None:
    specs = [
        ("minecraft:piston", "normal", "false", "north"),
        ("minecraft:sticky_piston", "sticky", "true", "east"),
        ("minecraft:piston", "normal", "true", "south"),
        ("minecraft:sticky_piston", "sticky", "false", "west"),
        ("minecraft:piston", "normal", "false", "up"),
        ("minecraft:sticky_piston", "sticky", "true", "down"),
        ("minecraft:sticky_piston", "sticky", "false", "north"),
        ("minecraft:piston", "normal", "true", "east"),
        ("minecraft:piston", "normal", "false", "west"),
        ("minecraft:sticky_piston", "sticky", "true", "south"),
        ("minecraft:piston", "normal", "true", "down"),
        ("minecraft:sticky_piston", "sticky", "false", "up"),
    ]
    base_x = 34
    base_z = 26
    for index, (base_id, head_type, short, facing) in enumerate(specs, start=1):
        col = (index - 1) % 4
        row = (index - 1) // 4
        y_layer = row % 2
        x = base_x + col * 6 + (row % 2)
        y = BASE_Y + y_layer * 3
        z = base_z + row * 7 + (col % 2)
        group = f"M{index:02}"
        out.append(
            Placement(
                group=group,
                role="mixed_base",
                block_id=base_id,
                x=x,
                y=y,
                z=z,
                facing=facing,
                extended="true",
                paired="yes",
                note="Mixed composition zone: base",
            )
        )
        dx, dy, dz = facing_offset(facing)
        out.append(
            Placement(
                group=group,
                role="mixed_head",
                block_id="minecraft:piston_head",
                x=x + dx,
                y=y + dy,
                z=z + dz,
                facing=facing,
                short=short,
                type=head_type,
                paired="yes",
                note="Mixed composition zone: paired head",
            )
        )


def build_fixture() -> tuple[litemapy.Schematic, str]:
    placements: list[Placement] = []
    add_base_matrix(placements)
    add_head_matrix(placements)
    add_pair_matrix(placements)
    add_mixed_zone(placements)

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
        "Piston complex fixture layout",
        "",
        "Purpose:",
        "  Expose static piston family issues in matrix, pair, and mixed arrangements before changing render rules.",
        "",
        "Face/facing order used in matrix columns:",
        f"  {', '.join(FACING_ORDER)}",
        "",
        "Areas:",
        "  Bxx: Base standalone matrix at x=0, z=0",
        "  Hxx: Head standalone matrix at x=34, z=0",
        "  Cxx: Base+Head pair matrix at x=0, z=26",
        "  Mxx: Mixed composition zone at x=34, z=26",
        "",
        "Columns:",
        "group\trole\tblock_id\tfacing\textended\tshort\ttype\tpaired\tx\ty\tz\tnote",
    ]
    for item in placements:
        lines.append(
            "\t".join(
                [
                    item.group,
                    item.role,
                    item.block_id,
                    item.facing,
                    item.extended,
                    item.short,
                    item.type,
                    item.paired,
                    str(item.x - min_x),
                    str(item.y - min_y),
                    str(item.z - min_z),
                    item.note,
                ]
            )
        )
    lines.extend(
        [
            "",
            f"Placed blocks: {len(placements)}",
            f"Bounding box: width={width}, height={height}, length={length}",
        ]
    )

    schematic = region.as_schematic(
        name="Piston Complex Fixture",
        author="Codex",
        description=(
            "Complex piston family fixture: base matrix, head matrix, base+head pair matrix, "
            "and mixed composition zone for Full Mode V2 visual debugging."
        ),
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a complex piston family .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_piston_complex_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_piston_complex_fixture_layout.txt",
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
