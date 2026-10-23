#!/usr/bin/env python3
"""Package verified outputs from one actual vanilla runtime, never a version alias."""
import argparse
import hashlib
import json
import pathlib
import zipfile


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--runtime-cache", type=pathlib.Path, default=pathlib.Path(".tmp/game-data/version-runtime"))
    parser.add_argument("--oracle-root", type=pathlib.Path, default=pathlib.Path(".tmp/game-data/oracle-exports"))
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("data/game-data/version-adapters"))
    args = parser.parse_args()
    runtime = json.loads((args.runtime_cache / args.version / "runtime.json").read_bytes())
    inventory_bytes = (args.runtime_cache / args.version / "inventory.json").read_bytes()
    inventory = json.loads(inventory_bytes)
    oracle_root = args.oracle_root / args.version
    manifest_bytes = (oracle_root / "oracle-manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    for version in (runtime["minecraft_version"], inventory["minecraft_version"], manifest["minecraft"]):
        if version != args.version:
            raise ValueError("Input data belongs to a different Minecraft version")
    if {inventory["client_sha256"], manifest["client_jar_sha256"]} != {runtime["client_sha256"]}:
        raise ValueError("Inventory and oracle were not extracted from the same original client")
    if manifest["client_jar_size"] != runtime["client_bytes"]:
        raise ValueError("Oracle client size differs from the verified runtime source")
    required = {"block_states", "block_state_semantics", "face_occlusion_shapes", "skip_render_rows", "fluid_states", "block_entity_types"}
    if not required.issubset({entry["name"] for entry in manifest["artifacts"]}):
        raise ValueError("The runtime export is missing required production semantics")
    payloads = {"inventory.json": inventory_bytes, "oracle/oracle-manifest.json": manifest_bytes}
    for entry in manifest["artifacts"]:
        relative = pathlib.PurePosixPath(entry["path"])
        if len(relative.parts) != 1 or relative.suffix != ".jsonl":
            raise ValueError("Unexpected oracle artifact path")
        data = (oracle_root / relative).read_bytes()
        if sha(data) != entry["records_sha256"] or sum(bool(line.strip()) for line in data.splitlines()) != entry["record_count"]:
            raise ValueError(f"Incomplete or changed runtime artifact: {relative}")
        payloads["oracle/" + str(relative)] = data
    files = {name: {"sha256": sha(data), "bytes": len(data)} for name, data in sorted(payloads.items())}
    content = hashlib.sha256()
    for name, record in files.items():
        content.update(f"{name}\0{record['sha256']}\0{record['bytes']}\n".encode())
    adapter = {
        "schema_version": 1,
        "game_version": args.version,
        "content_id": content.hexdigest(),
        "source_jar_sha256": runtime["client_sha256"],
        "source_url": runtime["source_url"],
        "source_metadata_sha1": runtime["metadata_sha1"],
        "files": files,
        "oracle_identity": {
            "minecraft": args.version,
            "world_version": manifest["world_version"],
            "protocol_version": manifest["protocol_version"],
            "java_major": manifest["java_major"],
            "client_jar_sha256": runtime["client_sha256"],
            "client_jar_size": runtime["client_bytes"],
            "manifest_sha256": sha(manifest_bytes),
        },
    }
    payloads["adapter.json"] = (json.dumps(adapter, sort_keys=True, separators=(",", ":")) + "\n").encode()
    args.output.mkdir(parents=True, exist_ok=True)
    output = args.output / (args.version + ".zip")
    if output.is_file():
        with zipfile.ZipFile(output) as existing:
            if json.loads(existing.read("adapter.json")) == adapter:
                print(json.dumps({"version": args.version, "status": "reused-exact-package", "bytes": output.stat().st_size}))
                return
    pending = output.with_suffix(".zip.pending")
    with zipfile.ZipFile(pending, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in sorted(payloads.items()):
            info = zipfile.ZipInfo(name)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data, compresslevel=9)
    pending.replace(output)
    print(json.dumps({"version": args.version, "files": len(files), "bytes": output.stat().st_size,
                      "block_states": manifest["block_state_semantics"]["state_count"], "output": str(output)}))


if __name__ == "__main__":
    main()
