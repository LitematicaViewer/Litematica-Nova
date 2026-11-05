"""Losslessly encode official oracle JSONL inside generated default-adapter staging only."""
import hashlib
import json
import os
import shutil
from pathlib import Path
import struct
import sys
import zlib

# Columns alternate literal text and the exact spelling of an independently varying value.
# The skip table's middle column is the pool consumed directly by the terrain compiler.
TABLES = {
    "skip_render_rows": (1, ("neighbor_catalog_index_ranges",)),
    "block_state_semantics": (2, ("block_id", "catalog_state_index", "state_id")),
    "block_states": (3, ("block_id", "properties", "raw_id", "state_id")),
    "tags": (4, ("member_id", "member_index")),
}
U32 = struct.Struct("<I")
DECODER = json.JSONDecoder()


def exact_cells(line, keys):
    pos = 0
    values = []
    for key in keys:
        marker = '"' + key + '":'
        start = line.index(marker, pos) + len(marker)
        _, end = DECODER.raw_decode(line, start)
        values.extend((line[pos:start], line[start:end]))
        pos = end
    return [*values, line[pos:]]


def pack(directory, output, name, artifact):
    code, keys = TABLES[name]
    source = directory / artifact["path"]
    if source != directory / (name + ".jsonl"):
        raise ValueError("unexpected oracle path")
    columns = [{} for _ in range(2 * len(keys) + 1)]
    refs = bytearray()
    source_hash = hashlib.sha256()
    total = rows = 0
    with source.open("rb") as stream:
        for line in stream:
            source_hash.update(line)
            total += len(line)
            rows += 1
            for column, cell in zip(columns, exact_cells(line.decode("utf-8"), keys), strict=True):
                refs.extend(U32.pack(column.setdefault(cell, len(column))))
    if rows != artifact["record_count"] or source_hash.hexdigest() != artifact["records_sha256"]:
        raise ValueError(f"{source}: source data differs from the oracle manifest")
    body = bytearray(struct.pack("<BIQ", code, rows, total)) + source_hash.digest()
    for column in columns:
        body.extend(U32.pack(len(column)))
        for text in column:
            data = text.encode("utf-8")
            body.extend(U32.pack(len(data)))
            body.extend(data)
    body.extend(refs)
    destination = output / (name + ".novatab")
    temporary = destination.with_suffix(".novatab.part")
    with temporary.open("wb") as out:
        out.write(b"NVTAB001" + U32.pack(len(body)) + hashlib.sha256(body).digest())
        out.write(zlib.compress(body, 9))
    os.replace(temporary, destination)


if __name__ == "__main__":
    directory = Path(sys.argv[1]).resolve(strict=True)
    output = Path(sys.argv[2]).resolve()
    if output == directory or directory in output.parents or output in directory.parents:
        raise ValueError("oracle staging must be separate from original inputs")
    manifest = json.loads((directory / "oracle-manifest.json").read_bytes())
    artifacts = {row["name"]: row for row in manifest["artifacts"]}
    compact = {name for name in TABLES if name in artifacts}
    excluded = {name + ".jsonl" for name in compact}
    # Create an empty output, never delete/edit the trusted source or overwrite an old output.
    shutil.copytree(directory, output, ignore=lambda where, names: excluded if Path(where) == directory else set())
    for name in sorted(compact):
        pack(directory, output, name, artifacts[name])
