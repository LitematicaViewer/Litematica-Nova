"""Build the exact block -> block-entity type relations used for FallingBlock.TileEntityData."""
import argparse
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=pathlib.Path, default=ROOT/".tmp/game-data/oracle-exports")
    parser.add_argument("--current-oracle", type=pathlib.Path, default=ROOT/"pack-in/game-data/default/derived/oracle")
    parser.add_argument("--dfu", type=pathlib.Path, default=ROOT.parent/"Litematica-Nova-upstreams/minecraft-26.3-decompiled")
    parser.add_argument("--output", type=pathlib.Path, default=ROOT/"tools/viewer-core/assets/version_falling_block.json")
    a = parser.parse_args()
    catalog = json.loads((ROOT/"tools/viewer-core/assets/version_entity_registry.json").read_bytes())
    versions = sorted((dv, version) for dv, version, *_ in catalog["versions"])
    intervals = {}
    for index, (dv, version) in enumerate(versions):
        path = a.current_oracle if version == "26.3" else a.oracle/version
        assigned = {}
        for line in (path/"block_entity_types.jsonl").read_bytes().splitlines():
            row = json.loads(line)
            for block in row["valid_block_ids"]:
                if block in assigned and assigned[block] != row["type_id"]:
                    raise ValueError((version,block,"ambiguous"))
                assigned[block] = row["type_id"]
        for block, typ in assigned.items():
            sequence = intervals.setdefault((block,typ), [])
            if index and sequence and sequence[-1][1] == versions[index-1][0]:
                sequence[-1][1] = dv
            else:
                sequence.append([dv,dv])
    source = (a.dfu/"net/minecraft/util/datafix/fixes/EntityBlockStateFix.java").read_text("utf-8")
    numeric = dict((name,int(num)) for name,num in re.findall(r'map\.put\("(minecraft:[^"]+)", (\d+)\)',source))
    if len(numeric) < 200: raise ValueError("legacy block ID table missing")
    data = {"holders":[[b,t,r] for (b,t),r in sorted(intervals.items())],"numeric":numeric}
    a.output.write_text(json.dumps(data,sort_keys=True,separators=(",",":"))+"\n","utf-8")
    print(f"{len(versions)} version registries, {len(intervals)} block/type pairs, {len(numeric)} legacy block IDs")


if __name__ == "__main__":
    main()
