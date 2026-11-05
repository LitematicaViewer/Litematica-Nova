#!/usr/bin/env python3
"""Generate native conversion type identities. JARs/bytecode never enter the product.

1.8--1.13.2: javap the exact client registration calls, locating classes with the
same MCP mappings used by export-legacy-oracles.py/export-version-oracles.py.
Later releases: enumerate that version's oracle registry, never spawn eggs.
A DFU name alias only affects translation; it never creates a registry entry.
"""
import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import zipfile
from concurrent.futures import ThreadPoolExecutor
from version_entity_traits import living_ids, scalar_evidence

ROOT = pathlib.Path(__file__).resolve().parents[3]
KINDS = {"entity": "entity_type", "block_entity": "block_entity_type"}


def registries(rows):
    sets = {kind: set() for kind in KINDS}
    for line in rows:
        row = json.loads(line)
        for kind, registry in KINDS.items():
            if row["registry_id"] == "minecraft:" + registry:
                sets[kind].add(row["entry_id"])
    if any(not ids for ids in sets.values()):
        raise ValueError("missing typed builtin registry")
    return sets


def old_registry(version, jar, mcp, javap):
    flat = version.startswith("1.13")
    namespaced = flat or version.startswith(("1.11", "1.12"))
    mapping_version = "1.10" if version == "1.10.1" else version
    with zipfile.ZipFile(mcp / (mapping_version + ("-config.zip" if flat else "-srg.zip"))) as z:
        mapping = z.read("config/joined.tsrg" if flat else "joined.srg").decode().replace("\r\n", "\n")
    result = {}
    for kind in KINDS:
        if kind == "entity":
            mapped = "net/minecraft/entity/" + ("EntityType" if flat else "EntityList")
            sentinel = "pig" if namespaced else "Pig"
        else:
            mapped = "net/minecraft/tileentity/" + ("TileEntityType" if flat else "TileEntity")
            sentinel = "furnace" if namespaced else "Furnace"
        owner, = re.findall(r"^(?:CL: )?(\w+) " + mapped + "$", mapping, re.M)
        code = subprocess.check_output([javap, "-p", "-c", "-classpath", str(jar), owner], text=True)
        # Select the bootstrap method, not accessors, spawn-egg builders or the
        # internal registration overload that delegates with non-constant args.
        methods = [m for m in re.split(r"(?=^  \S[^\n]+[;{]$)", code, flags=re.M)
                   if re.search("// String " + sentinel + "$", m, re.M)]
        method, = methods
        if flat:
            desc = "(Ljava/lang/String;L" + owner + "$a;)L" + owner + ";"
        elif kind == "entity" and namespaced:
            desc = "(ILjava/lang/String;Ljava/lang/Class;Ljava/lang/String;)V"
        elif kind == "entity":
            desc = ["(Ljava/lang/Class;Ljava/lang/String;I)V", "(Ljava/lang/Class;Ljava/lang/String;III)V"]
        else:
            desc = "(Ljava/lang/String;Ljava/lang/Class;)V" if namespaced else "(Ljava/lang/Class;Ljava/lang/String;)V"
        descriptors = [desc] if isinstance(desc, str) else desc
        ids = set()
        strings = []
        enums = {}
        for instruction in method.splitlines():
            match = re.search(r"// String ([^ ]+)$", instruction)
            if match:
                strings.append(match[1])
            if not flat and kind == "entity" and "getstatic" in instruction:
                field = re.search(r"// Field ([\w$]+)\.(\w+):L\1;", instruction)
                if field:
                    # Old minecart registrations get the save name from an enum
                    # constant. Read its actual constructor argument, not the
                    # ordinal, translation key or class name.
                    cls, key = field.groups()
                    if cls not in enums:
                        enum = subprocess.check_output([javap, "-p", "-c", "-classpath", str(jar), cls], text=True)
                        enums[cls] = {key: name for name, key in re.findall(
                            r'// String (\w+)\n[^\n]+invokespecial[^\n]+\n[^\n]+putstatic[^\n]+// Field (\w+):L',
                            enum.split("static {};")[-1])}
                    strings.append(enums[cls][key])
            if "invokestatic" not in instruction or not any(instruction.endswith(":" + d) for d in descriptors):
                continue
            # The second string in 1.11/1.12 entity registrations is the old
            # localization name, NOT the persisted registry ID.
            count = 2 if kind == "entity" and namespaced and not flat else 1
            if len(strings) != count:
                raise ValueError(f"{version} {kind}: unexpected registration args {strings}")
            raw = ("minecraft:" if namespaced else "") + strings[0]
            if raw in ids:
                raise ValueError(f"{version} {kind}: duplicate registration {raw}")
            ids.add(raw)
            strings = []
        if not ids or ("minecraft:" if namespaced else "") + sentinel not in ids:
            raise ValueError(f"{version} {kind}: empty/incomplete registration")
        result[kind] = ids
    return result


def dfu_aliases(src):
    fixes = src / "net/minecraft/util/datafix/fixes"
    pairs = r'\.put\("([^"]+)", "([^"]+)"\)'
    def extract(name):
        return dict(re.findall(pairs, (fixes/(name+".java")).read_text(encoding="utf-8")))
    entity = extract("EntityIdFix")
    # EntityHorse.Type was split before the namespace migration; the stable
    # identity denotes its default form, never a decision about subtype data.
    entity["EntityHorse"] = entity["Horse"]
    entity.update(dict(re.findall(pairs, (fixes/"EntityTheRenameningFix.java").read_text().split("RENAMED_BLOCKS")[0])))
    for name in ["EntityZombifiedPiglinRenameFix", "EntityPufferfishRenameFix", "EntityRavagerRenameFix"]:
        old, new = re.search(r'Objects\.equals\("([^"]+)", name\) \? "([^"]+)"', (fixes/(name+".java")).read_text()).groups()
        entity[old] = new
    entity.update({a:b for a,b in extract("EntityCodSalmonFix").items() if not a.endswith("_spawn_egg")})
    block = extract("BlockEntityIdFix")
    block["minecraft:suspicious_sand"] = "minecraft:brushable_block"  # DataFixers v3438
    return {"entity": entity, "block_entity": block}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cache", type=pathlib.Path, default=ROOT/".tmp/game-data/version-runtime")
    p.add_argument("--adapters", type=pathlib.Path, default=ROOT/"data/game-data/version-adapters")
    p.add_argument("--default-oracle", type=pathlib.Path, default=ROOT/"data/mc26/26.3/oracle")
    p.add_argument("--dfu-source", type=pathlib.Path, default=ROOT.parent/"Litematica-Nova-upstreams/minecraft-26.3-decompiled")
    p.add_argument("--current-jar", type=pathlib.Path, default=pathlib.Path("D:/.minecraft/versions/26.3-Fabric/26.3-Fabric.jar"))
    p.add_argument("--javap", default="javap")
    p.add_argument("--output", type=pathlib.Path, default=ROOT/"tools/viewer-core/assets/version_entity_registry.json")
    p.add_argument("--check", action="store_true")
    a = p.parse_args()
    catalog = [{"version":"1.8", "dataVersion":0}] + json.loads((ROOT/"data/minecraft-versions.json").read_bytes())["versions"]
    aliases = dfu_aliases(a.dfu_source)
    def intervals(indexes):
        ranges = []
        for idx in indexes:
            if ranges and ranges[-1][1] + 1 == idx:
                ranges[-1][1] = idx
            else:
                ranges.append([idx, idx])
        return [[versions[b][0], versions[e][0]] for b,e in ranges]
    history = {kind:{} for kind in KINDS}
    versions = []
    for i, entry in enumerate(catalog):
        version, dv = entry["version"], entry["dataVersion"]
        if version == "26.3":
            manifest = json.loads((a.default_oracle/"oracle-manifest.json").read_bytes())
            with (a.default_oracle/"builtin_registries.jsonl").open("rb") as rows:
                sets = registries(rows)
            source = "oracle"
        else:
            with zipfile.ZipFile(a.adapters / (version + ".zip")) as z:
                manifest = json.loads(z.read("oracle/oracle-manifest.json"))
                adapter = json.loads(z.read("adapter.json"))
                if adapter["game_version"] != version or adapter["source_jar_sha256"] != manifest["client_jar_sha256"]:
                    raise ValueError(f"{version}: mismatched adapter")
                if "oracle/builtin_registries.jsonl" in z.namelist():
                    with z.open("oracle/builtin_registries.jsonl") as rows:
                        sets = registries(rows)
                    source = "oracle"
                else:
                    jar = a.cache/version/"client.jar"
                    if hashlib.sha256(jar.read_bytes()).hexdigest() != manifest["client_jar_sha256"]:
                        raise ValueError(f"{version}: JAR differs from version manifest")
                    sets = old_registry(version, jar, a.cache/"mcp", a.javap)
                    source = "client_registration"
        if (manifest["minecraft"], manifest["world_version"]) != (version, dv):
            raise ValueError(f"{version}: mismatched oracle identity")
        versions.append([dv, version, source, manifest["client_jar_sha256"]])
        for kind, ids in sets.items():
            for raw in ids:
                history[kind].setdefault(raw, []).append(i)
        print(version, source, *(len(sets[k]) for k in KINDS), flush=True)
    result = {"versions": versions}
    for kind in KINDS:
        rows = []
        for raw, indexes in sorted(history[kind].items()):
            name = raw
            visited = set()
            while name in aliases[kind]:
                if name in visited:
                    raise ValueError(f"cyclic {kind} rename {name}")
                visited.add(name)
                name = aliases[kind][name]
            # Compress contiguous presence, but only along the *listed* releases.
            rows.append([raw, name, intervals(indexes)])
        result[kind] = rows
    if hashlib.sha256(a.current_jar.read_bytes()).hexdigest() != versions[-1][3]:
        raise ValueError("26.3 JAR identity differs from oracle")
    living = living_ids(a.dfu_source, a.current_jar, a.cache, sets["entity"])
    if set(living) != {row[1] for row in result["entity"]}:
        raise ValueError("living classification lacks a canonical entity")
    result["living"] = sorted(name for name, value in living.items() if value)
    def inspect_release(entry):
        _, v, _, digest = entry
        jar = a.current_jar if v == "26.3" else pathlib.Path(json.loads((a.cache/v/"runtime.json").read_bytes())["client_jar"])
        if hashlib.sha256(jar.read_bytes()).hexdigest() != digest:
            raise ValueError(f"{v}: scalar codec JAR identity differs from type registry")
        return scalar_evidence(a.cache, v, jar, a.javap)
    scalar_versions = {}
    with ThreadPoolExecutor(max_workers=4) as workers:
        for idx, evidence in enumerate(workers.map(inspect_release, versions)):
            for field in evidence:
                scalar_versions.setdefault(field, []).append(idx)
    result["scalars"] = [[*field, intervals(indexes)] for field, indexes in sorted(scalar_versions.items())]
    # One independently reviewable row per identity/release.
    output = '{\n' + ',\n'.join('  "' + key + '":[\n' + ',\n'.join(
        '    ' + json.dumps(row, separators=(',', ':')) for row in value) + '\n  ]'
        for key, value in result.items()) + '\n}\n'
    if a.check:
        if a.output.read_bytes() != output.encode():
            raise SystemExit("generated registries differ")
    else:
        a.output.write_bytes(output.encode())


if __name__ == "__main__":
    main()
