#!/usr/bin/env python3
"""Build the offline exact-version adapter store from existing verified oracle exports."""
import argparse
import hashlib
import io
import json
import re
import stat
import zipfile
import zlib
from contextlib import ExitStack
from pathlib import Path, PurePosixPath


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def put(archive, name, data):
    # No source metadata, paths, timestamp, OS or iteration-order dependence.
    info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


MIN_CHUNK = 16 * 1024
MAX_GROUP = 128 * 1024
PACK_BYTES = 256 * 1024
RANGES_KEY = b'"neighbor_catalog_index_ranges":'
INVENTORY_KEY = re.compile(rb'"minecraft:[a-z0-9_./-]+":')


def payload_chunks(name, data):
    """Split only at byte offsets; the exact source bytes (including newlines) are authoritative."""
    if len(data) <= MIN_CHUNK:
        yield data
        return
    start = 0
    if name.endswith(".jsonl"):
        stream = io.BytesIO(data)
        for row in stream:
            end = stream.tell()
            # Modern skip-render rows repeat the same 100-300 KiB range array
            # across directions/states, beyond DEFLATE's 32 KiB history. Keep
            # just the array as a reusable blob, not the row-specific fields.
            if name == "oracle/skip_render_rows.jsonl" and len(row) > 32768:
                key = row.find(RANGES_KEY)
                first = key + len(RANGES_KEY)
                last = row.find(b"]", first) + 1
                if key >= 0 and row[first:first + 1] == b"[" and last - first > 32768:
                    a, b = end - len(row) + first, end - len(row) + last
                    if a > start:
                        yield data[start:a]
                    yield data[a:b]
                    start = b
            # Row-content anchors let unchanged suffixes resynchronize after
            # insertions. Group small records so ZIP metadata stays modest.
            if end - start >= MIN_CHUNK and (zlib.crc32(row) & 31 == 0 or end - start >= MAX_GROUP):
                yield data[start:end]
                start = end
    elif name == "inventory.json":
        # The inventory is one compact JSON line. Anchor on registry keys
        # instead of positional offsets or changing a single byte of the JSON.
        for match in INVENTORY_KEY.finditer(data):
            end = match.start()
            if end - start >= MIN_CHUNK and (zlib.crc32(match[0]) & 63 == 0 or end - start >= MAX_GROUP):
                yield data[start:end]
                start = end
    if start < len(data):
        yield data[start:]


def build(inputs, output):
    sources = {}
    for path in sorted(inputs.iterdir()):
        require(not path.is_symlink(), f"symlink input: {path}")
        is_zip = path.is_file() and path.suffix == ".zip"
        version = path.stem if is_zip else path.name
        require((is_zip or path.is_dir()) and re.fullmatch(r"[A-Za-z0-9_.-]{1,100}", version)
                and version not in (".", ".."), f"unexpected adapter input: {path}")
        if is_zip or version not in sources:
            sources[version] = path
    require(sources, f"no exact-version adapters: {inputs}")
    blobs = set()
    seen_payloads = set()
    index = {"schema_version": 3, "format": "nova.version-adapter-store", "versions": {},
             "legacy_archives": {}, "recipes": {}, "groups": [], "locators": {}}
    group = bytearray()
    shards = {}
    logical = 0
    with zipfile.ZipFile(output, "w") as out:
        def flush_group():
            if group:
                put(out, f"groups/{len(index['groups'])}", group)
                index["groups"].append(digest(group))
                group.clear()

        def store_piece(sha, piece):
            if not piece or len(piece) > PACK_BYTES:
                put(out, "blobs/" + sha, piece)
                return
            if len(group) + len(piece) > PACK_BYTES:
                flush_group()
            shards.setdefault(sha[:2], {})[sha] = [len(index["groups"]), len(group), len(piece)]
            group.extend(piece)

        for version, path in sorted(sources.items()):
            with ExitStack() as stack:
                if path.is_file():
                    z = stack.enter_context(zipfile.ZipFile(path))
                    names = [i.filename for i in z.infolist() if not i.is_dir()]
                    require(all(not stat.S_ISLNK(i.external_attr >> 16) for i in z.infolist()), f"{version}: symlink")
                    read = z.read
                    with path.open("rb") as f:
                        index["legacy_archives"][path.name] = {"sha256": hashlib.file_digest(f, "sha256").hexdigest(),
                                                              "bytes": path.stat().st_size}
                else:
                    paths = list(path.rglob("*"))
                    require(not any(p.is_symlink() for p in paths), f"{version}: symlink")
                    names = [p.relative_to(path).as_posix() for p in paths if p.is_file()]
                    read = lambda name: (path / name).read_bytes()
                raw = read("adapter.json")
                identity = json.loads(raw)
                files = identity["files"]
                require(identity["schema_version"] == 1 and identity["game_version"] == version,
                        f"{version}: wrong adapter identity")
                require(len(names) == len(set(names)) and set(names) == set(files) | {"adapter.json"},
                        f"{version}: unrecorded, missing or duplicate logical entry")
                require("adapter.json" not in files, f"{version}: recursive identity")
                content = hashlib.sha256()
                oracle = inventory = None
                for name, record in sorted(files.items()):
                    parts = name.split("/")
                    require(all(part not in ("", ".", "..") for part in parts) and "\\" not in name
                            and ":" not in name and not PurePosixPath(name).is_absolute(), f"{version}: invalid path {name}")
                    data = read(name)
                    sha = digest(data)
                    require(record["bytes"] == len(data) and record["sha256"] == sha,
                            f"{version}/{name}: content differs from the original identity")
                    content.update(f"{name}\0{sha}\0{len(data)}\n".encode())
                    if sha not in seen_payloads:
                        chunks = []
                        for piece in payload_chunks(name, data):
                            chunk_sha = sha if len(piece) == len(data) else digest(piece)
                            if chunk_sha not in blobs:
                                store_piece(chunk_sha, piece)
                                blobs.add(chunk_sha)
                            chunks.append({"sha256": chunk_sha, "bytes": len(piece)})
                        if len(chunks) > 1:
                            recipe = json.dumps({"chunks": chunks}, separators=(",", ":")).encode()
                            require(len(chunks) <= 131072 and len(recipe) <= 16 * 1024 * 1024,
                                    f"{version}/{name}: chunk recipe too large")
                            recipe_sha = digest(recipe)
                            put(out, "recipes/" + recipe_sha, recipe)
                            index["recipes"][sha] = recipe_sha
                        seen_payloads.add(sha)
                    if name == "oracle/oracle-manifest.json":
                        oracle = json.loads(data)
                    elif name == "inventory.json":
                        inventory = json.loads(data)
                    logical += 1
                require(content.hexdigest() == identity["content_id"], f"{version}: content identity mismatch")
                provenance = identity["oracle_identity"]
                require(oracle and inventory and version == inventory["minecraft_version"] == oracle["minecraft"]
                        and identity["source_jar_sha256"] == inventory["client_sha256"] == oracle["client_jar_sha256"]
                        and all(provenance[k] == oracle[k] for k in
                                ("minecraft", "world_version", "protocol_version", "java_major", "client_jar_sha256", "client_jar_size"))
                        and files["oracle/oracle-manifest.json"]["sha256"] == provenance["manifest_sha256"],
                        f"{version}: oracle/inventory provenance mismatch")
                for artifact in oracle["artifacts"]:
                    require(files["oracle/" + artifact["path"]]["sha256"] == artifact["records_sha256"],
                            f"{version}: oracle payload differs from manifest")
                put(out, f"versions/{version}.json", raw)
                index["versions"][version] = digest(raw)
        flush_group()
        for prefix, rows in sorted(shards.items()):
            raw = json.dumps(rows, separators=(",", ":"), sort_keys=True).encode()
            require(len(raw) <= 256 * 1024, "adapter locator shard is too large")
            put(out, "locators/" + prefix, raw)
            index["locators"][prefix] = digest(raw)
        require(len(out.infolist()) <= 1000000, "adapter store contains too many entries")
    return index, logical, len(blobs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="shared.zip within a private staging directory")
    args = parser.parse_args()
    output = args.output
    require(output.name == "shared.zip" and not output.parent.resolve().is_relative_to(args.input.resolve()),
            "write shared.zip in a staging directory, separate from input")
    output.parent.mkdir(parents=True, exist_ok=True)
    pending = output.with_suffix(".zip.pending")
    index_path = output.with_name("index.json")
    pending_index = index_path.with_suffix(".json.pending")
    try:
        index, logical, count = build(args.input, pending)
        pending_index.write_bytes((json.dumps(index, separators=(",", ":"), sort_keys=True) + "\n").encode())
        pending.replace(output)
        pending_index.replace(index_path)
        print(json.dumps({"versions": len(index["versions"]), "logical_payloads": logical, "unique_blobs": count,
                          "recipes": len(index["recipes"]),
                          "bytes": output.stat().st_size, "archive": str(output)}))
    finally:
        pending.unlink(missing_ok=True)
        pending_index.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
