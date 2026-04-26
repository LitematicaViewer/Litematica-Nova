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
    rows = [
        ("enabled=true", "true", 1),
        ("enabled=false", "false", 5),
    ]
    columns = [
        ("down", 1),
        ("north", 4),
        ("south", 7),
        ("east", 10),
        ("west", 13),
    ]

    items: list[HopperPlacement] = []
    index = 1
    for section, enabled, z in rows:
        for facing, x in columns:
            sample_id = f"H{index:02}"
            items.append(
                HopperPlacement(
                    sample_id=sample_id,
                    block_id="minecraft:hopper",
                    x=x,
                    y=1,
                    z=z,
                    properties={"enabled": enabled, "facing": facing},
                    title=f"Hopper facing={facing} {section}",
                    note=f"Vanilla-model hopper sample with facing={facing}, enabled={enabled}.",
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
        "Hopper fixture layout",
        "",
        "Purpose:",
        "  Minimal hopper-only fixture for facing/enabled family texture checks under full mode.",
        "",
        "Columns:",
        "sample_id\tsection\ttitle\tanchor\tblock_id\tproperties\tnote",
    ]
    for item in items:
        properties = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        section = f"enabled={item.properties['enabled']}"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append(
            "\t".join(
                [
                    item.sample_id,
                    section,
                    item.title,
                    anchor,
                    item.block_id,
                    properties,
                    item.note,
                ]
            )
        )

    schematic = region.as_schematic(
        name="Hopper Fixture",
        author="Litematica Nova",
        description="Minimal hopper-only fixture for family-local texture validation.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate a minimal hopper-only .litematic fixture.")
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_hopper_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_hopper_fixture_layout.txt",
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
