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


def family_prefix(block_id: str) -> str | None:
    local = block_id.removeprefix("minecraft:")
    if local.endswith("_trapdoor"):
        return "T"
    if local.endswith("_banner"):
        return "B"
    if local == "lever":
        return "L"
    return None


def with_sample_ids(items: list[Placement]) -> list[Placement]:
    targets = [item for item in items if family_prefix(item.block_id)]
    counts: dict[str, int] = {}
    labels: dict[tuple[int, int, int, str], str] = {}
    for item in sorted(targets, key=lambda entry: (family_prefix(entry.block_id), entry.z, entry.y, entry.x)):
        prefix = family_prefix(item.block_id)
        assert prefix is not None
        counts[prefix] = counts.get(prefix, 0) + 1
        labels[(item.x, item.y, item.z, item.block_id)] = f"{prefix}{counts[prefix]:02}"
    return [
        Placement(
            item.block_id,
            item.x,
            item.y,
            item.z,
            item.properties,
            item.note,
            labels.get((item.x, item.y, item.z, item.block_id), "-"),
        )
        for item in items
    ]


def placements() -> list[Placement]:
    items: list[Placement] = []

    x_step = 3
    z_step = 3
    index = 0
    for waterlogged in ["false", "true"]:
        for half in ["bottom", "top"]:
            for open_value in ["false", "true"]:
                for powered in ["false", "true"]:
                    for facing in ["north", "east", "south", "west"]:
                        x = (index % 8) * x_step
                        z = (index // 8) * z_step
                        items.append(
                            Placement(
                                "minecraft:oak_trapdoor",
                                x,
                                1,
                                z,
                                {
                                    "facing": facing,
                                    "half": half,
                                    "open": open_value,
                                    "powered": powered,
                                    "waterlogged": waterlogged,
                                },
                                "trapdoor full state sample",
                            )
                        )
                        index += 1

    banner_z = ((index + 7) // 8 + 2) * z_step
    for rotation in range(16):
        items.append(
            Placement(
                "minecraft:white_banner",
                (rotation % 8) * x_step,
                1,
                banner_z + (rotation // 8) * z_step,
                {"rotation": str(rotation)},
                "standing banner rotation sample",
            )
        )
    wall_banner_z = banner_z + 3 * z_step
    for i, facing in enumerate(["north", "east", "south", "west"]):
        x = i * 6
        items.append(
            Placement(
                "minecraft:blue_wall_banner",
                x,
                1,
                wall_banner_z,
                {"facing": facing},
                "wall banner facing sample",
            )
        )
        support_offsets = {
            "north": (0, 0, -1),
            "east": (1, 0, 0),
            "south": (0, 0, 1),
            "west": (-1, 0, 0),
        }
        dx, dy, dz = support_offsets[facing]
        items.append(
            Placement(
                "minecraft:stone",
                x + dx,
                1 + dy,
                wall_banner_z + dz,
                {},
                f"support block for wall banner facing={facing}",
            )
        )

    lever_z = wall_banner_z + 3 * z_step
    lever_index = 0
    for face in ["floor", "wall", "ceiling"]:
        for powered in ["false", "true"]:
            for facing in ["north", "south", "east", "west"]:
                x = (lever_index % 8) * x_step
                z = lever_z + (lever_index // 8) * z_step
                items.append(
                    Placement(
                        "minecraft:lever",
                        x,
                        1,
                        z,
                        {"face": face, "facing": facing, "powered": powered},
                        "lever face/facing/powered sample",
                    )
                )
                if face == "floor":
                    items.append(Placement("minecraft:stone", x, 0, z, {}, "lever floor support"))
                elif face == "ceiling":
                    items.append(Placement("minecraft:stone", x, 2, z, {}, "lever ceiling support"))
                else:
                    offsets = {
                        "north": (0, 0, -1),
                        "south": (0, 0, 1),
                        "east": (1, 0, 0),
                        "west": (-1, 0, 0),
                    }
                    dx, dy, dz = offsets[facing]
                    items.append(Placement("minecraft:stone", x + dx, 1 + dy, z + dz, {}, "lever wall support"))
                lever_index += 1

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
        "Trapdoor / banner / lever debug fixture layout",
        "",
        "Overlay is renderer-side only. Enable with LBA_FULL_MODE_V2_TBL_DEBUG=1.",
        "The sample_id labels are not stored in the litematic file.",
        "",
        "Coverage:",
        "  Txx = oak_trapdoor facing north/east/south/west, half top/bottom, open true/false, powered true/false, waterlogged true/false",
        "  Bxx = white_banner rotation 0..15 plus blue_wall_banner facing north/east/south/west",
        "  Lxx = lever face floor/wall/ceiling, facing north/south/east/west, powered true/false",
        "",
        "Columns:",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{k}={v}" for k, v in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))

    schematic = region.as_schematic(
        name="Trapdoor Banner Lever Debug Fixture",
        author="Litematica Nova",
        description="Full-state trapdoor/banner/lever fixture for renderer-side debug label overlay.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Generate trapdoor/banner/lever full-state debug fixture.")
    parser.add_argument("--output", type=Path, default=workspace_root / "_full_mode_tbl_debug_fixture.litematic")
    parser.add_argument("--layout-output", type=Path, default=workspace_root / "_full_mode_tbl_debug_fixture_layout.txt")
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
