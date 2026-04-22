from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path
from typing import Any


MAGIC = b"LBALPK1\0"
VERSION = 1
HEADER_STRUCT = struct.Struct("<8sIQ")


def layer_pack_path(cache_file: str | Path) -> Path:
    cache = Path(cache_file).resolve()
    return cache.with_name(f"{cache.name}.layers.lpack")


def layer_pack_temp_path(cache_file: str | Path) -> Path:
    return layer_pack_path(cache_file).with_suffix(".lpack.tmp")


def write_layer_pack_atomic(cache_file: str | Path, meta: dict[str, Any], layers: list[dict[str, Any]], stats: dict[str, Any]) -> Path:
    pack_path = layer_pack_path(cache_file)
    tmp_path = layer_pack_temp_path(cache_file)
    tmp_path.parent.mkdir(parents=True, exist_ok=True)

    encoded_layers: list[tuple[int, bytes, int]] = []
    for layer in layers:
        y = int(layer.get("y", len(encoded_layers)) or 0)
        blocks = layer.get("blocks", []) if isinstance(layer, dict) else []
        payload = {
            "metadata": meta.get("metadata", {}),
            "chunk_size": (meta.get("visual") or {}).get("chunk_size"),
            "y": y,
            "blocks": blocks,
        }
        raw = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        encoded_layers.append((y, zlib.compress(raw, level=3), len(blocks) if isinstance(blocks, list) else 0))

    layer_index: list[dict[str, int]] = []
    offset = 0
    for y, data, block_count in encoded_layers:
        layer_index.append({"y": y, "offset": offset, "length": len(data), "block_count": block_count})
        offset += len(data)

    header = {"version": VERSION, "meta": meta, "stats": stats, "compression": "zlib", "layers": layer_index}
    raw_offsets = [int(item["offset"]) for item in layer_index]
    header_len = 0
    for _ in range(16):
        for item, raw_offset in zip(layer_index, raw_offsets):
            item["offset"] = HEADER_STRUCT.size + header_len + raw_offset
        next_header_bytes = json.dumps(header, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        if len(next_header_bytes) == header_len:
            break
        header_len = len(next_header_bytes)
    for item, raw_offset in zip(layer_index, raw_offsets):
        item["offset"] = HEADER_STRUCT.size + header_len + raw_offset
    header_bytes = json.dumps(header, ensure_ascii=False, separators=(",", ":")).encode("utf-8")

    with tmp_path.open("wb") as handle:
        handle.write(HEADER_STRUCT.pack(MAGIC, VERSION, len(header_bytes)))
        handle.write(header_bytes)
        for _y, data, _block_count in encoded_layers:
            handle.write(data)
        handle.flush()
    tmp_path.replace(pack_path)
    return pack_path


def read_layer_pack_header(cache_file: str | Path) -> dict[str, Any]:
    pack_path = layer_pack_path(cache_file)
    with pack_path.open("rb") as handle:
        magic, version, header_len = HEADER_STRUCT.unpack(handle.read(HEADER_STRUCT.size))
        if magic != MAGIC or version != VERSION:
            raise ValueError("unsupported layer pack format")
        header = json.loads(handle.read(header_len).decode("utf-8"))
    if not isinstance(header, dict):
        raise ValueError("invalid layer pack header")
    return header


def read_layer_pack_meta(cache_file: str | Path) -> dict[str, Any]:
    header = read_layer_pack_header(cache_file)
    meta = header.get("meta")
    if not isinstance(meta, dict):
        raise ValueError("invalid layer pack meta")
    return meta


def read_layer_pack_layer(cache_file: str | Path, y: int) -> dict[str, Any]:
    pack_path = layer_pack_path(cache_file)
    header = read_layer_pack_header(cache_file)
    layers = header.get("layers") if isinstance(header.get("layers"), list) else []
    row = next((item for item in layers if isinstance(item, dict) and int(item.get("y", -1)) == int(y)), None)
    if row is None:
        raise KeyError(f"layer {int(y)} is not present in pack")
    with pack_path.open("rb") as handle:
        handle.seek(int(row.get("offset", 0) or 0))
        compressed = handle.read(int(row.get("length", 0) or 0))
    payload = json.loads(zlib.decompress(compressed).decode("utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("invalid layer payload")
    return payload


def cleanup_layer_pack_temp(cache_file: str | Path) -> None:
    try:
        layer_pack_temp_path(cache_file).unlink(missing_ok=True)
    except Exception:
        pass
