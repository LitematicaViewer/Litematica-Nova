from __future__ import annotations

import argparse
from pathlib import Path

import litemapy


FACING_ORDER = ["north", "east", "south", "west", "up", "down"]
CELL_STEP = 4
BLOCK_Y = 1
MC_DATA_VERSION = 3953


def build_layout_rows() -> list[dict[str, str]]:
    return [
        {"kind": "minecraft:piston", "extended": "false", "z": "0"},
        {"kind": "minecraft:piston", "extended": "true", "z": "4"},
        {"kind": "minecraft:sticky_piston", "extended": "false", "z": "10"},
        {"kind": "minecraft:sticky_piston", "extended": "true", "z": "14"},
        {"kind": "minecraft:piston_head", "type": "normal", "short": "false", "z": "22"},
        {"kind": "minecraft:piston_head", "type": "normal", "short": "true", "z": "26"},
        {"kind": "minecraft:piston_head", "type": "sticky", "short": "false", "z": "30"},
        {"kind": "minecraft:piston_head", "type": "sticky", "short": "true", "z": "34"},
    ]


def block_state_for(row: dict[str, str], facing: str) -> litemapy.BlockState:
    kind = row["kind"]
    if kind in {"minecraft:piston", "minecraft:sticky_piston"}:
        return litemapy.BlockState(kind, facing=facing, extended=row["extended"])
    return litemapy.BlockState(
        kind,
        facing=facing,
        short=row["short"],
        type=row["type"],
    )


def build_fixture_schematic() -> tuple[litemapy.Schematic, str]:
    rows = build_layout_rows()
    width = (len(FACING_ORDER) - 1) * CELL_STEP + 1
    length = int(rows[-1]["z"]) + 1
    region = litemapy.Region(0, 0, 0, width, BLOCK_Y + 1, length)

    layout_lines = [
        "Piston family fixture layout",
        "",
        f"Columns (x step={CELL_STEP}):",
    ]
    for index, facing in enumerate(FACING_ORDER):
        layout_lines.append(f"  x={index * CELL_STEP:>2}: facing={facing}")

    layout_lines.append("")
    layout_lines.append("Rows:")

    placed = 0
    for row in rows:
        z = int(row["z"])
        row_desc = " ".join(f"{key}={value}" for key, value in row.items() if key != "z")
        layout_lines.append(f"  z={z:>2}: {row_desc}")
        for index, facing in enumerate(FACING_ORDER):
            x = index * CELL_STEP
            region[x, BLOCK_Y, z] = block_state_for(row, facing)
            placed += 1

    schematic = region.as_schematic(
        name="Piston Family Fixture",
        author="Codex",
        description=(
            "Minimal piston family fixture covering piston, sticky_piston, piston_head; "
            "facings north/east/south/west/up/down; piston extended true/false; "
            "piston_head short true/false and type normal/sticky."
        ),
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(
        layout_lines
        + [
            "",
            f"Placed blocks: {placed}",
            f"Bounding box: width={width}, height={BLOCK_Y + 1}, length={length}",
        ]
    )


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a minimal piston family .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_piston_fixture.litematic",
        help="Output .litematic path.",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_piston_fixture_layout.txt",
        help="Output layout description path.",
    )
    args = parser.parse_args()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.layout_output.parent.mkdir(parents=True, exist_ok=True)

    schematic, layout = build_fixture_schematic()
    schematic.save(str(args.output))
    args.layout_output.write_text(layout + "\n", encoding="utf-8")

    print(f"fixture={args.output}")
    print(f"layout={args.layout_output}")


if __name__ == "__main__":
    main()
