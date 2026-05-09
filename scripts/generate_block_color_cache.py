from __future__ import annotations

import argparse
import json
import os
import struct
import time
import zlib
from dataclasses import dataclass
from pathlib import Path
from statistics import median
from typing import Any
from zipfile import ZipFile


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = REPO_ROOT / "data" / "blockColorCache.json"

VANILLA_TINTS: dict[str, tuple[float, float, float]] = {
    "grass": (0.55, 0.76, 0.32),
    "foliage": (0.45, 0.68, 0.30),
    "water": (0.25, 0.45, 0.95),
}


@dataclass(frozen=True)
class TextureColor:
    rgb: tuple[float, float, float]
    pixels: int


class MinecraftAssets:
    def __init__(self, jar_path: Path) -> None:
        self.jar_path = jar_path
        self.jar = ZipFile(jar_path)
        self.names = set(self.jar.namelist())
        self.json_cache: dict[str, Any | None] = {}
        self.texture_cache: dict[str, TextureColor | None] = {}

    def close(self) -> None:
        self.jar.close()

    def read_json(self, asset_path: str) -> Any | None:
        if asset_path in self.json_cache:
            return self.json_cache[asset_path]
        if asset_path not in self.names:
            self.json_cache[asset_path] = None
            return None
        with self.jar.open(asset_path) as handle:
            payload = json.loads(handle.read().decode("utf-8"))
        self.json_cache[asset_path] = payload
        return payload

    def texture_color(self, texture_id: str, block_id: str) -> TextureColor | None:
        texture_id = normalize_texture_id(texture_id)
        if texture_id in self.texture_cache:
            return self.texture_cache[texture_id]
        path = f"assets/minecraft/textures/{texture_id}.png"
        if path not in self.names:
            self.texture_cache[texture_id] = None
            return None
        try:
            with self.jar.open(path) as handle:
                pixels = decode_png_rgba(handle.read())
        except Exception:
            self.texture_cache[texture_id] = None
            return None
        tint = tint_for_texture(block_id, texture_id)
        color = representative_color(pixels, tint)
        self.texture_cache[texture_id] = color
        return color

    def blockstate_names(self) -> list[str]:
        prefix = "assets/minecraft/blockstates/"
        suffix = ".json"
        return sorted(
            name[len(prefix) : -len(suffix)]
            for name in self.names
            if name.startswith(prefix) and name.endswith(suffix)
        )


def normalize_texture_id(texture_id: str) -> str:
    texture_id = texture_id.removeprefix("minecraft:")
    if not texture_id.startswith("block/") and not texture_id.startswith("entity/"):
        texture_id = f"block/{texture_id}"
    return texture_id


def decode_png_rgba(data: bytes) -> list[tuple[int, int, int, int]]:
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("not a png")
    offset = 8
    width = height = color_type = bit_depth = interlace = None
    idat = bytearray()
    palette: list[tuple[int, int, int]] = []
    transparency: bytes = b""
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        chunk_type = data[offset + 4 : offset + 8]
        chunk_data = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type, _compression, _filter, interlace = struct.unpack(
                ">IIBBBBB", chunk_data
            )
        elif chunk_type == b"PLTE":
            palette = [
                tuple(chunk_data[index : index + 3])  # type: ignore[arg-type]
                for index in range(0, len(chunk_data), 3)
            ]
        elif chunk_type == b"tRNS":
            transparency = chunk_data
        elif chunk_type == b"IDAT":
            idat.extend(chunk_data)
        elif chunk_type == b"IEND":
            break
    if width is None or height is None or interlace != 0:
        raise ValueError("unsupported png format")
    if color_type != 3 and bit_depth != 8:
        raise ValueError("unsupported png bit depth")
    if color_type == 3 and bit_depth not in {1, 2, 4, 8}:
        raise ValueError("unsupported indexed png bit depth")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(color_type)
    if channels is None:
        raise ValueError(f"unsupported png color type: {color_type}")
    raw = zlib.decompress(bytes(idat))
    stride = (width * bit_depth + 7) // 8 if color_type == 3 else width * channels
    rows: list[bytes] = []
    cursor = 0
    prev = bytearray(stride)
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        scanline = bytearray(raw[cursor : cursor + stride])
        cursor += stride
        recon = unfilter_scanline(scanline, prev, 1 if color_type == 3 else channels, filter_type)
        rows.append(bytes(recon))
        prev = recon
    pixels: list[tuple[int, int, int, int]] = []
    for row in rows:
        palette_indices = unpack_palette_indices(row, width, bit_depth) if color_type == 3 else None
        for x in range(width):
            i = x * channels
            if color_type == 2:
                pixels.append((row[i], row[i + 1], row[i + 2], 255))
            elif color_type == 6:
                pixels.append((row[i], row[i + 1], row[i + 2], row[i + 3]))
            elif color_type == 0:
                pixels.append((row[i], row[i], row[i], 255))
            elif color_type == 3:
                index = palette_indices[x] if palette_indices is not None else row[i]
                r, g, b = palette[index]
                alpha = transparency[index] if index < len(transparency) else 255
                pixels.append((r, g, b, alpha))
            elif color_type == 4:
                pixels.append((row[i], row[i], row[i], row[i + 1]))
    return pixels


def unpack_palette_indices(row: bytes, width: int, bit_depth: int) -> list[int]:
    if bit_depth == 8:
        return list(row[:width])
    mask = (1 << bit_depth) - 1
    out: list[int] = []
    for byte in row:
        for shift in range(8 - bit_depth, -1, -bit_depth):
            out.append((byte >> shift) & mask)
            if len(out) == width:
                return out
    return out


def unfilter_scanline(
    scanline: bytearray, prev: bytearray, bpp: int, filter_type: int
) -> bytearray:
    out = bytearray(len(scanline))
    for i, value in enumerate(scanline):
        left = out[i - bpp] if i >= bpp else 0
        up = prev[i]
        up_left = prev[i - bpp] if i >= bpp else 0
        if filter_type == 0:
            predictor = 0
        elif filter_type == 1:
            predictor = left
        elif filter_type == 2:
            predictor = up
        elif filter_type == 3:
            predictor = (left + up) // 2
        elif filter_type == 4:
            predictor = paeth(left, up, up_left)
        else:
            raise ValueError(f"unsupported png filter: {filter_type}")
        out[i] = (value + predictor) & 0xFF
    return out


def paeth(left: int, up: int, up_left: int) -> int:
    p = left + up - up_left
    pa = abs(p - left)
    pb = abs(p - up)
    pc = abs(p - up_left)
    if pa <= pb and pa <= pc:
        return left
    if pb <= pc:
        return up
    return up_left


def representative_color(
    pixels: list[tuple[int, int, int, int]], tint: tuple[float, float, float] | None
) -> TextureColor | None:
    opaque = [(r, g, b, a) for r, g, b, a in pixels if a >= 32]
    if not opaque:
        return None
    channels = []
    for channel in range(3):
        values = []
        for pixel in opaque:
            value = pixel[channel]
            if tint:
                value = int(round(value * tint[channel]))
            values.append(max(0, min(255, value)))
        channels.append(median(values) / 255.0)
    return TextureColor((channels[0], channels[1], channels[2]), len(opaque))


def tint_for_texture(block_id: str, texture_id: str) -> tuple[float, float, float] | None:
    local = block_id.removeprefix("minecraft:")
    if "water" in texture_id or local == "water":
        return VANILLA_TINTS["water"]
    if "grass" in texture_id or local in {"grass_block", "short_grass", "tall_grass", "fern"}:
        return VANILLA_TINTS["grass"]
    if local.endswith("_leaves") or "leaves" in texture_id or "vine" in texture_id:
        return VANILLA_TINTS["foliage"]
    return None


def build_color_cache(assets: MinecraftAssets) -> tuple[dict[str, list[float]], dict[str, int]]:
    colors: dict[str, list[float]] = {}
    stats = {"blocks": 0, "resolved": 0, "missing": 0, "textures_sampled": 0}
    for local_name in assets.blockstate_names():
        block_id = f"minecraft:{local_name}"
        stats["blocks"] += 1
        blockstate = assets.read_json(f"assets/minecraft/blockstates/{local_name}.json")
        model_ids = model_ids_from_blockstate(blockstate)
        texture_colors: list[TextureColor] = []
        for model_id in model_ids:
            texture_ids = texture_ids_from_model(assets, model_id)
            for texture_id in texture_ids:
                color = assets.texture_color(texture_id, block_id)
                if color:
                    texture_colors.append(color)
        if not texture_colors:
            stats["missing"] += 1
            continue
        rgb = combine_texture_colors(texture_colors)
        state_keys = state_keys_from_blockstate(block_id, blockstate)
        for key in {block_id, local_name, *state_keys}:
            colors[key] = [round(rgb[0], 4), round(rgb[1], 4), round(rgb[2], 4)]
        stats["resolved"] += 1
        stats["textures_sampled"] += len(texture_colors)
    return colors, stats


def model_ids_from_blockstate(blockstate: Any) -> list[str]:
    out: list[str] = []
    if not isinstance(blockstate, dict):
        return out
    variants = blockstate.get("variants")
    if isinstance(variants, dict):
        for value in variants.values():
            collect_model_ids(value, out)
    multipart = blockstate.get("multipart")
    if isinstance(multipart, list):
        for entry in multipart:
            if isinstance(entry, dict):
                collect_model_ids(entry.get("apply"), out)
    return sorted(set(out))


def collect_model_ids(value: Any, out: list[str]) -> None:
    if isinstance(value, list):
        for item in value:
            collect_model_ids(item, out)
    elif isinstance(value, dict):
        model = value.get("model")
        if isinstance(model, str):
            out.append(model)


def texture_ids_from_model(assets: MinecraftAssets, model_id: str) -> list[str]:
    model_path = f"assets/minecraft/models/{model_id.removeprefix('minecraft:')}.json"
    model = resolve_model(assets, model_path)
    textures = model.get("textures", {})
    if not isinstance(textures, dict):
        return []
    ordered_keys = [
        "top",
        "side",
        "all",
        "texture",
        "front",
        "end",
        "bottom",
        "particle",
    ]
    values: list[str] = []
    for key in ordered_keys:
        value = textures.get(key)
        if isinstance(value, str):
            values.append(resolve_texture_ref(value, textures))
    if not values:
        for value in textures.values():
            if isinstance(value, str):
                values.append(resolve_texture_ref(value, textures))
    return [value for value in dict.fromkeys(values) if not value.startswith("minecraft:entity/")]


def resolve_model(assets: MinecraftAssets, model_path: str) -> dict[str, Any]:
    model = assets.read_json(model_path)
    if not isinstance(model, dict):
        return {}
    parent_ref = model.get("parent")
    merged: dict[str, Any] = {}
    if isinstance(parent_ref, str):
        parent_path = f"assets/minecraft/models/{parent_ref.removeprefix('minecraft:')}.json"
        merged = resolve_model(assets, parent_path)
    merged_textures = dict(merged.get("textures", {}))
    if isinstance(model.get("textures"), dict):
        merged_textures.update(model["textures"])
    merged["textures"] = merged_textures
    if "elements" in model:
        merged["elements"] = model["elements"]
    return merged


def resolve_texture_ref(value: str, textures: dict[str, Any]) -> str:
    seen = set()
    while value.startswith("#"):
        key = value[1:]
        if key in seen:
            break
        seen.add(key)
        next_value = textures.get(key)
        if not isinstance(next_value, str):
            break
        value = next_value
    if not value.startswith("minecraft:"):
        value = f"minecraft:{value}"
    return value


def combine_texture_colors(colors: list[TextureColor]) -> tuple[float, float, float]:
    total = sum(color.pixels for color in colors)
    if total <= 0:
        return (0.6, 0.6, 0.6)
    return tuple(
        sum(color.rgb[channel] * color.pixels for color in colors) / total
        for channel in range(3)
    )


def state_keys_from_blockstate(block_id: str, blockstate: Any) -> list[str]:
    if not isinstance(blockstate, dict):
        return []
    keys: list[str] = []
    variants = blockstate.get("variants")
    if isinstance(variants, dict):
        for key in variants.keys():
            if key:
                keys.append(f"{block_id}[{key}]")
    return keys


def discover_jar(explicit: str | None) -> Path | None:
    candidates: list[Path] = []
    if explicit:
        candidates.append(Path(explicit))
    if os.environ.get("LBA_MINECRAFT_JAR"):
        candidates.append(Path(os.environ["LBA_MINECRAFT_JAR"]))
    candidates.extend(Path("D:/.minecraft/versions").glob("1.21*/1.21*.jar"))
    candidates.extend((Path.home() / "AppData" / "Roaming" / ".minecraft" / "versions").glob("1.21*/1.21*.jar"))
    candidates.extend((REPO_ROOT / "third_party" / "render-assets" / "vanilla").glob("*.jar"))
    for candidate in candidates:
        if candidate.exists() and jar_has_required_assets(candidate):
            return candidate
    return None


def jar_has_required_assets(path: Path) -> bool:
    try:
        with ZipFile(path) as jar:
            names = set(jar.namelist())
            return (
                "assets/minecraft/blockstates/stone.json" in names
                and "assets/minecraft/models/block/stone.json" in names
                and "assets/minecraft/textures/block/stone.png" in names
            )
    except Exception:
        return False


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate normal/fast block color cache from Minecraft textures.")
    parser.add_argument("--jar", help="Path to a Minecraft client jar. Overrides discovery.")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    jar_path = discover_jar(args.jar)
    if jar_path is None:
        print(
            "failed: Minecraft jar/assets not found. Set LBA_MINECRAFT_JAR or pass --jar <path>.",
            flush=True,
        )
        return 2

    assets = MinecraftAssets(jar_path)
    try:
        colors, stats = build_color_cache(assets)
    finally:
        assets.close()

    payload = {
        "metadata": {
            "source": str(jar_path),
            "minecraft_version": jar_path.parent.name,
            "generated_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
            "entries": len(colors),
            **stats,
        },
        "colors": colors,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps(payload["metadata"], ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
