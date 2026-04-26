from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953


@dataclass(frozen=True)
class HopperPlacement:
    sample_id: str
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    title: str
    note: str


def block_state(item: HopperPlacement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def placements() -> list[HopperPlacement]:
    columns = [
        ("down", 1),
        ("north", 8),
        ("south", 15),
        ("east", 22),
        ("west", 29),
    ]
    enabled_rows = [
        ("true", 1),
        ("false", 8),
    ]

    items: list[HopperPlacement] = []
    index = 1
    for enabled, z in enabled_rows:
        for facing, x in columns:
            sample_id = f"HF{index:02}"
            items.append(
                HopperPlacement(
                    sample_id=sample_id,
                    block_id="minecraft:hopper",
                    x=x,
                    y=1,
                    z=z,
                    properties={"enabled": enabled, "facing": facing},
                    title=f"Hopper face-split facing={facing} enabled={enabled}",
                    note=(
                        "Face-split paired sample for direct locked/unlocked comparison. "
                        "Top row keeps enabled=true, bottom row keeps enabled=false."
                    ),
                )
            )
            index += 1
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
        "Hopper face-split fixture layout",
        "",
        "Purpose:",
        "  Hopper-only face-split paired fixture for per-face numbering and enabled=true/false comparison.",
        "  Columns keep the same five core facings; rows separate unlocked and locked hopper states.",
        "",
        "Rows:",
        "  z=0 -> enabled=true",
        "  z=7 -> enabled=false",
        "",
        "Columns:",
        "  x=0 down, x=7 north, x=14 south, x=21 east, x=28 west",
        "",
        "Samples:",
        "sample_id\tsection\ttitle\tanchor\tblock_id\tproperties\tnote",
    ]
    for item in items:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append(
            "\t".join(
                [
                    item.sample_id,
                    "face_split_core",
                    item.title,
                    anchor,
                    item.block_id,
                    properties,
                    item.note,
                ]
            )
        )

    schematic = region.as_schematic(
        name="Hopper Face Split Fixture",
        author="Litematica Nova",
        description="Hopper-only face-split paired fixture for per-face overlay inspection and enabled=true/false comparison.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Generate a hopper face-split .litematic fixture for per-face numbering."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_hopper_face_split_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_hopper_face_split_fixture_layout.txt",
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
