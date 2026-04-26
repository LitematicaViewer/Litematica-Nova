from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953


@dataclass(frozen=True)
class Placement:
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    note: str
    sample_id: str = "-"


def state(item: Placement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def support_offset(facing: str) -> tuple[int, int, int]:
    return {
        "north": (0, 0, -1),
        "east": (1, 0, 0),
        "south": (0, 0, 1),
        "west": (-1, 0, 0),
    }[facing]


def with_sample_ids(items: list[Placement]) -> list[Placement]:
    trapdoors = [item for item in items if item.block_id.endswith("_trapdoor")]
    labels: dict[tuple[int, int, int], str] = {}
    for index, item in enumerate(sorted(trapdoors, key=lambda entry: (entry.z, entry.y, entry.x)), start=1):
        labels[(item.x, item.y, item.z)] = f"T{index:02}"
    return [
        Placement(
            item.block_id,
            item.x,
            item.y,
            item.z,
            item.properties,
            item.note,
            labels.get((item.x, item.y, item.z), "-"),
        )
        for item in items
    ]


def placements() -> list[Placement]:
    items: list[Placement] = []
    index = 0
    for half in ["bottom", "top"]:
        for open_value in ["false", "true"]:
            for powered in ["false", "true"]:
                for facing in ["north", "east", "south", "west"]:
                    x = (index % 8) * 4
                    z = (index // 8) * 4
                    props = {
                        "facing": facing,
                        "half": half,
                        "open": open_value,
                        "powered": powered,
                        "waterlogged": "false",
                    }
                    items.append(
                        Placement(
                            "minecraft:oak_trapdoor",
                            x,
                            1,
                            z,
                            props,
                            "trapdoor attachment sample with side and vertical reference blocks",
                        )
                    )
                    dx, dy, dz = support_offset(facing)
                    items.append(
                        Placement(
                            "minecraft:stone",
                            x + dx,
                            1 + dy,
                            z + dz,
                            {},
                            f"side reference block for facing={facing}",
                        )
                    )
                    items.append(
                        Placement(
                            "minecraft:stone",
                            x,
                            0 if half == "bottom" else 2,
                            z,
                            {},
                            f"vertical reference block for half={half}",
                        )
                    )
                    index += 1

    water_z = ((index + 7) // 8 + 2) * 4
    for i, facing in enumerate(["north", "east", "south", "west"]):
        x = i * 4
        props = {
            "facing": facing,
            "half": "bottom",
            "open": "true",
            "powered": "false",
            "waterlogged": "true",
        }
        items.append(
            Placement(
                "minecraft:oak_trapdoor",
                x,
                1,
                water_z,
                props,
                "representative waterlogged open trapdoor attachment sample",
            )
        )
        dx, dy, dz = support_offset(facing)
        items.append(Placement("minecraft:stone", x + dx, 1 + dy, water_z + dz, {}, f"waterlogged side reference facing={facing}"))
        items.append(Placement("minecraft:stone", x, 0, water_z, {}, "waterlogged bottom reference"))

    return with_sample_ids(items)


def build_fixture() -> tuple[litemapy.Schematic, str]:
    items = placements()
    min_x = min(item.x for item in items)
    min_y = min(item.y for item in items)
    min_z = min(item.z for item in items)
    max_x = max(item.x for item in items)
    max_y = max(item.y for item in items)
    max_z = max(item.z for item in items)

    region = litemapy.Region(0, 0, 0, max_x - min_x + 1, max_y - min_y + 1, max_z - min_z + 1)
    for item in items:
        region[item.x - min_x, item.y - min_y, item.z - min_z] = state(item)

    lines = [
        "Trapdoor attachment fixture layout",
        "",
        "Overlay is renderer-side only. Enable with LBA_FULL_MODE_V2_TBL_DEBUG=1.",
        "Stone blocks are references/supports, not sample labels.",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))

    schematic = region.as_schematic(
        name="Trapdoor Attachment Fixture",
        author="Litematica Nova",
        description="Trapdoor attachment/reference fixture with renderer-side Txx overlay labels.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate trapdoor attachment full-mode fixture.")
    parser.add_argument("--output", type=Path, default=workspace_root / "_full_mode_trapdoor_attachment_fixture.litematic")
    parser.add_argument("--layout-output", type=Path, default=workspace_root / "_full_mode_trapdoor_attachment_fixture_layout.txt")
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
