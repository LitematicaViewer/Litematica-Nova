from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953


@dataclass(frozen=True)
class Placement:
    sample_id: str
    block_id: str
    x: int
    y: int
    z: int
    properties: dict[str, str]
    note: str


def block_state(item: Placement) -> litemapy.BlockState:
    return litemapy.BlockState(item.block_id, **item.properties)


def add_support(items: list[Placement], sample_id: str, x: int, y: int, z: int, facing: str, note: str) -> None:
    offsets = {
        "north": (0, 0, -1),
        "east": (1, 0, 0),
        "south": (0, 0, 1),
        "west": (-1, 0, 0),
    }
    dx, dy, dz = offsets[facing]
    items.append(Placement(sample_id + "S", "minecraft:stone", x + dx, y + dy, z + dz, {}, note))


def placements() -> list[Placement]:
    items: list[Placement] = []

    # Wall-attached orientation samples. North/south are included only as no-regression controls.
    sample = 1
    for block_id in ["minecraft:oak_wall_sign", "minecraft:spruce_wall_sign"]:
        for facing in ["north", "east", "south", "west"]:
            sample_id = f"WS{sample:02}"
            x = (sample - 1) * 4
            z = 0 if block_id == "minecraft:oak_wall_sign" else 4
            items.append(Placement(sample_id, block_id, x, 1, z, {"facing": facing, "waterlogged": "false"}, f"{block_id} facing={facing}"))
            add_support(items, sample_id, x, 1, z, facing, f"support for {sample_id}")
            sample += 1

    sample = 1
    for block_id in ["minecraft:stone_button", "minecraft:oak_button", "minecraft:spruce_button"]:
        for facing in ["north", "east", "south", "west"]:
            sample_id = f"BT{sample:02}"
            x = (sample - 1) * 4
            z = 10 + (0 if block_id == "minecraft:stone_button" else 4 if block_id == "minecraft:oak_button" else 8)
            items.append(Placement(sample_id, block_id, x, 1, z, {"face": "wall", "facing": facing, "powered": "false"}, f"{block_id} wall facing={facing}"))
            add_support(items, sample_id, x, 1, z, facing, f"support for {sample_id}")
            sample += 1

    # Banner east/west is kept as a guard sample, but this fixture's target is wall_sign + button.
    items.append(Placement("WA01", "minecraft:blue_wall_banner", 0, 1, 24, {"facing": "east"}, "guard wall_banner facing=east"))
    add_support(items, "WA01", 0, 1, 24, "east", "support for WA01")
    items.append(Placement("WA02", "minecraft:blue_wall_banner", 5, 1, 24, {"facing": "west"}, "guard wall_banner facing=west"))
    add_support(items, "WA02", 5, 1, 24, "west", "support for WA02")

    # Preserve-neighbor samples: stone is the neighbor whose face must remain visible.
    items.append(Placement("PN01", "minecraft:oak_sign", 10, 1, 24, {"rotation": "0", "waterlogged": "false"}, "standing sign preserve-neighbor target"))
    items.append(Placement("PN01S", "minecraft:stone", 10, 1, 23, {}, "stone neighbor for PN01"))
    items.append(Placement("PN02", "minecraft:oak_hanging_sign", 14, 1, 24, {"attached": "false", "rotation": "0", "waterlogged": "false"}, "hanging_sign preserve-neighbor target"))
    items.append(Placement("PN02S", "minecraft:stone", 14, 1, 23, {}, "stone neighbor for PN02"))
    items.append(Placement("PN03", "minecraft:oak_wall_hanging_sign", 18, 1, 24, {"facing": "east", "waterlogged": "false"}, "wall_hanging_sign preserve-neighbor target"))
    add_support(items, "PN03", 18, 1, 24, "east", "support for PN03")

    # Stripped log/wood axis samples.
    for index, axis in enumerate(["x", "y", "z"]):
        items.append(Placement(f"SL{index + 1:02}", "minecraft:stripped_oak_log", index * 4, 1, 32, {"axis": axis}, f"stripped_oak_log axis={axis}"))
        items.append(Placement(f"SW{index + 1:02}", "minecraft:stripped_oak_wood", index * 4, 1, 36, {"axis": axis}, f"stripped_oak_wood axis={axis}"))

    return items


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
        region[item.x - min_x, item.y - min_y, item.z - min_z] = block_state(item)

    lines = [
        "Wall-attached sign/button/banner + stripped log fixture layout",
        "No sample labels are written into the litematic; use this layout and traces as the id map.",
        "",
        "sample_id\tblock_id\tanchor\tproperties\tnote",
    ]
    for item in items:
        props = ",".join(f"{key}={value}" for key, value in sorted(item.properties.items())) or "-"
        anchor = f"({item.x - min_x},{item.y - min_y},{item.z - min_z})"
        lines.append("\t".join([item.sample_id, item.block_id, anchor, props, item.note]))

    schematic = region.as_schematic(
        name="Wall Attached Sign Button Stripped Fixture",
        author="Litematica BA",
        description="Minimal fixture for wall-attached east/west orientation, preserve-neighbor, and stripped log axis checks.",
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    output = root / "_full_mode_wall_attached_sign_button_stripped_fixture.litematic"
    layout_output = root / "_full_mode_wall_attached_sign_button_stripped_fixture_layout.txt"
    schematic, layout = build_fixture()
    schematic.save(str(output))
    layout_output.write_text(layout + "\n", encoding="utf-8")
    print(f"fixture={output}")
    print(f"layout={layout_output}")


if __name__ == "__main__":
    main()
