from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

import litemapy


MC_DATA_VERSION = 3953
FACING_ORDER = ["north", "east", "south", "west", "up", "down"]
POWERED_ORDER = ["false", "true"]
CELL_W = 12
CELL_D = 10
OBSERVER_Y = 1
POWER_MARKER = {
    "false": "minecraft:light_gray_concrete",
    "true": "minecraft:red_concrete",
}


@dataclass(frozen=True)
class Sample:
    sample_id: str
    facing: str
    powered: str
    x: int
    y: int
    z: int


def build_samples() -> list[Sample]:
    samples: list[Sample] = []
    sample_index = 1
    for row, powered in enumerate(POWERED_ORDER):
        cell_z = row * CELL_D
        for col, facing in enumerate(FACING_ORDER):
            cell_x = col * CELL_W
            samples.append(
                Sample(
                    sample_id=f"O{sample_index:02}",
                    facing=facing,
                    powered=powered,
                    x=cell_x + 6,
                    y=OBSERVER_Y,
                    z=cell_z + 5,
                )
            )
            sample_index += 1
    return samples


def add_power_marker(region: litemapy.Region, sample: Sample) -> None:
    marker_block = POWER_MARKER[sample.powered]
    region[sample.x + 2, 0, sample.z] = litemapy.BlockState(marker_block)


def build_fixture() -> tuple[litemapy.Schematic, str]:
    samples = build_samples()
    width = (len(FACING_ORDER) - 1) * CELL_W + 11
    length = (len(POWERED_ORDER) - 1) * CELL_D + 8
    height = OBSERVER_Y + 1
    region = litemapy.Region(0, 0, 0, width, height, length)

    lines = [
        "Observer experiment fixture layout",
        "",
        "Purpose:",
        "  Full observer state baseline for texture debugging.",
        "  Covers facing up/down/north/south/east/west and powered false/true.",
        "",
        "Layout:",
        f"  columns (x step={CELL_W}): {' | '.join(FACING_ORDER)}",
        f"  rows (z step={CELL_D}): powered=false at z=0, powered=true at z={CELL_D}",
        "  marker block: light_gray_concrete=false, red_concrete=true",
        "",
        "Face numbering used by renderer-side overlay:",
        "  1=up 2=down 3=north 4=south 5=west 6=east",
        "",
        "Columns:",
        "sample_id\tfacing\tpowered\tanchor_x\tanchor_y\tanchor_z",
    ]

    for sample in samples:
        add_power_marker(region, sample)
        region[sample.x, sample.y, sample.z] = litemapy.BlockState(
            "minecraft:observer",
            facing=sample.facing,
            powered=sample.powered,
        )
        lines.append(
            "\t".join(
                [
                    sample.sample_id,
                    sample.facing,
                    sample.powered,
                    str(sample.x),
                    str(sample.y),
                    str(sample.z),
                ]
            )
        )

    schematic = region.as_schematic(
        name="Observer Experiment Fixture",
        author="Litematica Nova",
        description=(
            "Observer-only fixture covering 12 states: facing north/east/south/west/up/down "
            "crossed with powered false/true."
        ),
        mc_version=MC_DATA_VERSION,
    )
    return schematic, "\n".join(lines)


def main() -> None:
    workspace_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Generate an observer-only experiment .litematic fixture."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=workspace_root / "_full_mode_observer_experiment_fixture.litematic",
    )
    parser.add_argument(
        "--layout-output",
        type=Path,
        default=workspace_root / "_full_mode_observer_layout.txt",
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
