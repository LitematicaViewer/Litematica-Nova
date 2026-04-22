from __future__ import annotations

import json
import math
import zipfile
from dataclasses import dataclass
from io import BytesIO
from pathlib import Path
from typing import Any

from PIL import Image, ImageDraw, ImageFont
from litematicaba.ui.minecraft_top_sprite import (
    BLOCK_26_FLAT_ROOT,
    FAITHFUL_64_ROOT,
    VANILLA_26_JAR,
)

XK_REDSTONE_DISPLAY_ROOT = Path(r"C:\Users\USER\Downloads\XK redstone display 26.0.1")
CLEAN_GLASS_ROOT = Path(r"C:\Users\USER\Downloads\CleanGlass_1.20.2")


FACE_NAMES = ("down", "up", "north", "south", "west", "east", "cross")
FACE_DIRECTIONS = ("down", "up", "north", "south", "west", "east")
DEFAULT_ALPHA_MODE = "opaque"
MATERIAL_CACHE_FORMAT = "lba_full_mode_material_cache_v2"
MODEL_GEOMETRY_VERSION = 4
GLASS_OVERRIDES = {
    "glass",
    "white_stained_glass",
    "light_gray_stained_glass",
    "gray_stained_glass",
    "black_stained_glass",
    "brown_stained_glass",
    "red_stained_glass",
    "orange_stained_glass",
    "yellow_stained_glass",
    "lime_stained_glass",
    "green_stained_glass",
    "cyan_stained_glass",
    "light_blue_stained_glass",
    "blue_stained_glass",
    "purple_stained_glass",
    "magenta_stained_glass",
    "pink_stained_glass",
    "tinted_glass",
    "glass_pane",
    "white_stained_glass_pane",
    "light_gray_stained_glass_pane",
    "gray_stained_glass_pane",
    "black_stained_glass_pane",
    "brown_stained_glass_pane",
    "red_stained_glass_pane",
    "orange_stained_glass_pane",
    "yellow_stained_glass_pane",
    "lime_stained_glass_pane",
    "green_stained_glass_pane",
    "cyan_stained_glass_pane",
    "light_blue_stained_glass_pane",
    "blue_stained_glass_pane",
    "purple_stained_glass_pane",
    "magenta_stained_glass_pane",
    "pink_stained_glass_pane",
}


@dataclass(slots=True)
class _ModelRef:
    model: str
    y: int = 0
    x: int = 0


@dataclass(slots=True)
class _ResolvedModel:
    elements: list[dict[str, Any]]
    textures: dict[str, Any]


@dataclass(slots=True)
class _MaterialImage:
    key: str
    image: Image.Image
    alpha_mode: str


@dataclass(slots=True)
class _FaceMaterialSpec:
    texture_id: str
    uv: tuple[float, float, float, float] | None = None
    rotation: int = 0


def _namespace_path(value: str, *, default_prefix: str) -> tuple[str, str]:
    if ":" in value:
        namespace, path = value.split(":", 1)
    else:
        namespace, path = "minecraft", value
    if not path.startswith(default_prefix):
        path = f"{default_prefix}{path}"
    return namespace, path


def _blockstate_path(block_id: str) -> str:
    namespace, local = (block_id.split(":", 1) if ":" in block_id else ("minecraft", block_id))
    return f"assets/{namespace}/blockstates/{local}.json"


def _model_path(model_ref: str) -> str:
    namespace, path = _namespace_path(model_ref, default_prefix="block/")
    return f"assets/{namespace}/models/{path}.json"


def _texture_asset_path(texture_id: str) -> str:
    namespace, path = _namespace_path(texture_id, default_prefix="block/")
    return f"assets/{namespace}/textures/{path}.png"


def _texture_value_to_id(value: Any) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        for key in ("sprite", "id", "texture"):
            found = value.get(key)
            if isinstance(found, str):
                return found
    return None


def _variant_key_matches(key: str, properties: dict[str, str]) -> tuple[bool, int]:
    key = key.strip()
    if not key:
        return True, 0
    score = 0
    for part in key.split(","):
        if "=" not in part:
            continue
        name, value = part.split("=", 1)
        if properties.get(name.strip()) != value.strip():
            return False, 0
        score += 1
    return True, score


def _when_matches(when: Any, properties: dict[str, str]) -> bool:
    if not isinstance(when, dict):
        return True
    if "OR" in when:
        values = when.get("OR")
        return isinstance(values, list) and any(_when_matches(item, properties) for item in values)
    if "AND" in when:
        values = when.get("AND")
        return isinstance(values, list) and all(_when_matches(item, properties) for item in values)
    for key, expected in when.items():
        actual = properties.get(str(key))
        allowed = [v.strip() for v in str(expected).split("|")]
        if actual not in allowed:
            return False
    return True


def _model_refs_from_apply(value: Any) -> list[_ModelRef]:
    values = value if isinstance(value, list) else [value]
    refs: list[_ModelRef] = []
    for item in values:
        if not isinstance(item, dict):
            continue
        model = item.get("model")
        if not isinstance(model, str) or not model:
            continue
        refs.append(
            _ModelRef(
                model=model,
                y=int(item.get("y", 0) or 0) % 360,
                x=int(item.get("x", 0) or 0) % 360,
            )
        )
    return refs


def _state_cache_key(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return block_id
    pairs = ",".join(f"{k}={v}" for k, v in sorted(properties.items()))
    return f"{block_id}[{pairs}]"


def _local_id(block_id: str) -> str:
    return block_id.split(":", 1)[1] if ":" in block_id else block_id


def _rotate_direction(direction: str, x_degrees: int, y_degrees: int) -> str:
    for _ in range((x_degrees // 90) % 4):
        direction = {
            "down": "south",
            "south": "up",
            "up": "north",
            "north": "down",
        }.get(direction, direction)
    for _ in range((y_degrees // 90) % 4):
        direction = {
            "north": "east",
            "east": "south",
            "south": "west",
            "west": "north",
        }.get(direction, direction)
    return direction


def _face_vertices(face_name: str, from_xyz: list[float], to_xyz: list[float]) -> list[list[float]]:
    x0, y0, z0 = from_xyz
    x1, y1, z1 = to_xyz
    return {
        "down": [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
        "up": [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        "north": [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
        "south": [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
        "west": [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
        "east": [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
    }.get(face_name, [])


def _rotate_point_around_axis(
    point: list[float],
    origin: list[float],
    axis: str,
    angle_degrees: float,
    *,
    rescale: bool = False,
) -> list[float]:
    radians = math.radians(angle_degrees)
    sin_a = math.sin(radians)
    cos_a = math.cos(radians)
    x, y, z = point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]
    if rescale and abs(cos_a) > 0.0001:
        scale = 1.0 / abs(cos_a)
        if axis == "x":
            y *= scale
            z *= scale
        elif axis == "y":
            x *= scale
            z *= scale
        elif axis == "z":
            x *= scale
            y *= scale
    if axis == "x":
        rotated = [x, y * cos_a - z * sin_a, y * sin_a + z * cos_a]
    elif axis == "y":
        rotated = [x * cos_a + z * sin_a, y, -x * sin_a + z * cos_a]
    elif axis == "z":
        rotated = [x * cos_a - y * sin_a, x * sin_a + y * cos_a, z]
    else:
        rotated = [x, y, z]
    return [rotated[0] + origin[0], rotated[1] + origin[1], rotated[2] + origin[2]]


def _apply_model_rotations(vertices: list[list[float]], ref: _ModelRef) -> list[list[float]]:
    origin = [0.5, 0.5, 0.5]
    result = vertices
    if ref.x:
        result = [_rotate_point_around_axis(point, origin, "x", ref.x) for point in result]
    if ref.y:
        result = [_rotate_point_around_axis(point, origin, "y", ref.y) for point in result]
    return [[round(coord, 6) for coord in point] for point in result]


def _element_face_quad(element: dict[str, Any], face_name: str, ref: _ModelRef) -> list[list[float]] | None:
    raw_from = element.get("from")
    raw_to = element.get("to")
    if not isinstance(raw_from, list) or not isinstance(raw_to, list) or len(raw_from) < 3 or len(raw_to) < 3:
        return None
    try:
        from_xyz = [float(raw_from[0]) / 16.0, float(raw_from[1]) / 16.0, float(raw_from[2]) / 16.0]
        to_xyz = [float(raw_to[0]) / 16.0, float(raw_to[1]) / 16.0, float(raw_to[2]) / 16.0]
    except (TypeError, ValueError):
        return None
    vertices = _face_vertices(face_name, from_xyz, to_xyz)
    if not vertices:
        return None
    rotation = element.get("rotation")
    if isinstance(rotation, dict):
        raw_origin = rotation.get("origin")
        if isinstance(raw_origin, list) and len(raw_origin) >= 3:
            try:
                origin = [float(raw_origin[0]) / 16.0, float(raw_origin[1]) / 16.0, float(raw_origin[2]) / 16.0]
                axis = str(rotation.get("axis", ""))
                angle = float(rotation.get("angle", 0) or 0)
                rescale = bool(rotation.get("rescale", False))
                vertices = [
                    _rotate_point_around_axis(point, origin, axis, angle, rescale=rescale)
                    for point in vertices
                ]
            except (TypeError, ValueError):
                pass
    return _apply_model_rotations(vertices, ref)


def _detect_alpha_mode(image: Image.Image) -> str:
    alpha = image.getchannel("A")
    extrema = alpha.getextrema()
    if extrema == (255, 255):
        return "opaque"
    values = set(alpha.getdata())
    if values.issubset({0, 255}):
        return "cutout"
    return "translucent"


def _crop_first_animation_frame(image: Image.Image) -> Image.Image:
    if image.width <= 0 or image.height <= image.width:
        return image
    if image.height % image.width == 0:
        return image.crop((0, 0, image.width, image.width))
    return image


def _face_uv_tuple(value: Any) -> tuple[float, float, float, float] | None:
    if not isinstance(value, list) or len(value) < 4:
        return None
    try:
        return (float(value[0]), float(value[1]), float(value[2]), float(value[3]))
    except (TypeError, ValueError):
        return None


def _bake_face_image(
    image: Image.Image,
    uv: tuple[float, float, float, float] | None,
    rotation: int,
) -> Image.Image:
    baked = image.convert("RGBA")
    if uv is not None:
        left, top, right, bottom = uv
        width = baked.width
        height = baked.height
        box = (
            int(round(left / 16.0 * width)),
            int(round(top / 16.0 * height)),
            int(round(right / 16.0 * width)),
            int(round(bottom / 16.0 * height)),
        )
        x0 = max(0, min(width, min(box[0], box[2])))
        y0 = max(0, min(height, min(box[1], box[3])))
        x1 = max(0, min(width, max(box[0], box[2])))
        y1 = max(0, min(height, max(box[1], box[3])))
        if x1 > x0 and y1 > y0:
            baked = baked.crop((x0, y0, x1, y1))
    turns = (rotation // 90) % 4
    if turns:
        baked = baked.rotate(-90 * turns, expand=True)
    return baked


class MinecraftFullMaterialCacheBuilder:
    def __init__(
        self,
        *,
        vanilla_jar: Path = VANILLA_26_JAR,
        overlay_root: Path = FAITHFUL_64_ROOT,
        flat_fallback_root: Path = BLOCK_26_FLAT_ROOT,
    ) -> None:
        self._vanilla_jar = vanilla_jar
        self._overlay_root = overlay_root
        self._flat_fallback_root = flat_fallback_root
        self._xk_redstone_root = XK_REDSTONE_DISPLAY_ROOT
        self._clean_glass_root = CLEAN_GLASS_ROOT
        self._zip: zipfile.ZipFile | None = None
        self._names: set[str] | None = None
        self._json_cache: dict[str, Any] = {}
        self._model_cache: dict[str, _ResolvedModel | None] = {}
        self._texture_cache: dict[str, Image.Image | None] = {}
        self._tinted_cache: dict[str, Image.Image | None] = {}

    def is_available(self) -> bool:
        return self._vanilla_jar.is_file()

    def build_cache_payload(
        self,
        palette: list[dict[str, Any]],
        property_pool: list[dict[str, str]],
        source_litematic_path: Path | None = None,
    ) -> tuple[dict[str, Any], Image.Image]:
        if not self.is_available():
            raise RuntimeError(r"鐎瑰本鏆ｅΟ鈥崇础閹靛彞绗夐崚?vanilla 娑撴槒绁┃鎰剁窗E:\.minecraft\versions\26.1\26.1.jar")

        material_order: list[_MaterialImage] = []
        material_index: dict[str, int] = {}
        palette_keys: list[str] = []
        palette_materials: list[dict[str, Any]] = []
        palette_model_quads: list[list[dict[str, Any]]] = []
        block_model_quads: list[dict[str, Any]] = []
        baked_palette_count = 0
        fallback_palette_count = 0
        vanilla_model_quad_count = 0

        for row in palette:
            if not isinstance(row, dict):
                continue
            block_id = str(row.get("block_id", "") or "")
            if not block_id:
                continue
            property_id = int(row.get("property_id", 0) or 0)
            props_raw = property_pool[property_id] if 0 <= property_id < len(property_pool) else {}
            properties = {str(k): str(v) for k, v in dict(props_raw or {}).items()}
            state_key = _state_cache_key(block_id, properties)
            palette_keys.append(state_key)
            material_map, geometry_hint, used_fallback, model_quads = self._resolve_palette_materials(
                block_id,
                properties,
            )
            if used_fallback:
                fallback_palette_count += 1
            else:
                baked_palette_count += 1
            entry: dict[str, Any] = {"geometry_hint": geometry_hint}
            for face_name, material in material_map.items():
                if material is None:
                    continue
                slot = material_index.get(material.key)
                if slot is None:
                    slot = len(material_order)
                    material_index[material.key] = slot
                    material_order.append(material)
                entry[face_name] = slot
            quad_entries: list[dict[str, Any]] = []
            for quad in model_quads:
                material = quad.get("material")
                if not isinstance(material, _MaterialImage):
                    continue
                slot = material_index.get(material.key)
                if slot is None:
                    slot = len(material_order)
                    material_index[material.key] = slot
                    material_order.append(material)
                quad_entries.append(
                    {
                        "vertices": quad["vertices"],
                        "material": slot,
                        "double_sided": bool(quad.get("double_sided", False)),
                        "cullface": quad.get("cullface"),
                    }
                )
            vanilla_model_quad_count += len(quad_entries)
            palette_materials.append(entry)
            palette_model_quads.append(quad_entries)

        for override in self._build_block_model_overrides(source_litematic_path):
            quad_entries: list[dict[str, Any]] = []
            for quad in override.get("quads", []):
                material = quad.get("material")
                if not isinstance(material, _MaterialImage):
                    continue
                slot = material_index.get(material.key)
                if slot is None:
                    slot = len(material_order)
                    material_index[material.key] = slot
                    material_order.append(material)
                quad_entries.append(
                    {
                        "vertices": quad["vertices"],
                        "material": slot,
                        "double_sided": bool(quad.get("double_sided", False)),
                        "cullface": quad.get("cullface"),
                    }
                )
            if quad_entries:
                block_model_quads.append(
                    {
                        "x": int(override.get("x", 0)),
                        "y": int(override.get("y", 0)),
                        "z": int(override.get("z", 0)),
                        "replace": bool(override.get("replace", False)),
                        "quads": quad_entries,
                    }
                )

        if not material_order:
            raise RuntimeError("鐎瑰本鏆ｅΟ鈥崇础濞屸剝婀佺憴锝嗙€介崚棰佹崲娴ｆ洖褰查悽銊ф畱 face material")

        atlas, materials_payload = self._pack_materials(material_order)
        payload = {
            "format": MATERIAL_CACHE_FORMAT,
            "source": "26.1.jar blockstate/model + Faithful texture overlay + block-26.1 fallback",
            "source_litematic_path": str(source_litematic_path) if source_litematic_path is not None else "",
            "geometry_version": MODEL_GEOMETRY_VERSION,
            "palette_keys": palette_keys,
            "materials": materials_payload,
            "palette_materials": palette_materials,
            "palette_model_quads": palette_model_quads,
            "block_model_quads": block_model_quads,
            "stats": {
                "palette_entries": len(palette_materials),
                "material_slots": len(materials_payload),
                "baked_palette_entries": baked_palette_count,
                "fallback_palette_entries": fallback_palette_count,
                "vanilla_model_quads": vanilla_model_quad_count,
                "block_model_overrides": len(block_model_quads),
            },
        }
        return payload, atlas

    def _zip_file(self) -> zipfile.ZipFile:
        if self._zip is None:
            self._zip = zipfile.ZipFile(self._vanilla_jar)
        return self._zip

    def _zip_names(self) -> set[str]:
        if self._names is None:
            self._names = set(self._zip_file().namelist())
        return self._names

    def _read_json(self, path: str) -> Any | None:
        if path in self._json_cache:
            return self._json_cache[path]
        if path not in self._zip_names():
            self._json_cache[path] = None
            return None
        with self._zip_file().open(path) as fh:
            payload = json.load(fh)
        self._json_cache[path] = payload
        return payload

    def _select_model_refs(self, block_id: str, properties: dict[str, str]) -> list[_ModelRef]:
        payload = self._read_json(_blockstate_path(block_id))
        if not isinstance(payload, dict):
            return []
        refs: list[_ModelRef] = []
        variants = payload.get("variants")
        if isinstance(variants, dict):
            best_score = -1
            best_values: list[Any] = []
            for key, value in variants.items():
                matched, score = _variant_key_matches(str(key), properties)
                if not matched:
                    continue
                if score > best_score:
                    best_score = score
                    best_values = [value]
                elif score == best_score:
                    best_values.append(value)
            for value in best_values[:1]:
                refs.extend(_model_refs_from_apply(value))

        multipart = payload.get("multipart")
        if isinstance(multipart, list):
            for part in multipart:
                if not isinstance(part, dict):
                    continue
                when = part.get("when")
                if when is not None and not _when_matches(when, properties):
                    continue
                refs.extend(_model_refs_from_apply(part.get("apply")))
        return refs

    def _resolve_model(self, model_ref: str) -> _ResolvedModel | None:
        path = _model_path(model_ref)
        if path in self._model_cache:
            return self._model_cache[path]
        payload = self._read_json(path)
        if not isinstance(payload, dict):
            self._model_cache[path] = None
            return None
        parent = str(payload.get("parent", "") or "")
        parent_model = self._resolve_model(parent) if parent else None
        textures: dict[str, Any] = {}
        elements: list[dict[str, Any]] = []
        if parent_model is not None:
            textures.update(parent_model.textures)
            elements = parent_model.elements
        own_textures = payload.get("textures")
        if isinstance(own_textures, dict):
            textures.update(own_textures)
        own_elements = payload.get("elements")
        if isinstance(own_elements, list):
            elements = [element for element in own_elements if isinstance(element, dict)]
        resolved = _ResolvedModel(elements=elements, textures=textures)
        self._model_cache[path] = resolved
        return resolved

    def _resolve_texture_id(self, value: Any, textures: dict[str, Any]) -> str | None:
        if not isinstance(value, str):
            return None
        seen: set[str] = set()
        current = value
        while current.startswith("#"):
            key = current[1:]
            if key in seen:
                return None
            seen.add(key)
            replacement = textures.get(key)
            replacement_id = _texture_value_to_id(replacement)
            if not isinstance(replacement_id, str):
                return None
            current = replacement_id
        return current

    def _load_texture_image(self, texture_id: str) -> Image.Image | None:
        if texture_id in self._texture_cache:
            cached = self._texture_cache[texture_id]
            return cached.copy() if cached is not None else None
        rel = _texture_asset_path(texture_id)
        image = self._load_texture_rel(rel)
        self._texture_cache[texture_id] = image.copy() if image is not None else None
        return image

    def _load_texture_rel(self, rel: str) -> Image.Image | None:
        xk = self._load_xk_texture_rel(rel)
        if xk is not None:
            return xk
        overlay = self._overlay_root / rel
        if overlay.is_file():
            with Image.open(overlay) as image:
                return _crop_first_animation_frame(image.convert("RGBA"))
        if rel in self._zip_names():
            data = self._zip_file().read(rel)
            with Image.open(BytesIO(data)) as image:
                return _crop_first_animation_frame(image.convert("RGBA"))
        flat = self._flat_fallback_root / Path(rel).name
        if flat.is_file():
            with Image.open(flat) as image:
                return _crop_first_animation_frame(image.convert("RGBA"))
        return None

    def _load_xk_texture_rel(self, rel: str) -> Image.Image | None:
        xk = self._xk_redstone_root / rel
        if xk.is_file():
            with Image.open(xk) as image:
                return _crop_first_animation_frame(image.convert("RGBA"))
        return self._find_named_texture(self._xk_redstone_root, Path(rel).stem)

    def _find_named_texture(self, root: Path, name: str) -> Image.Image | None:
        if not root.is_dir() or not name:
            return None
        for path in (
            root / "assets" / "minecraft" / "textures" / "block" / f"{name}.png",
            root / "assets" / "create" / "textures" / "block" / f"{name}.png",
            root / "assets" / "create" / "textures" / "block" / "funnel" / f"{name}.png",
        ):
            if path.is_file():
                with Image.open(path) as image:
                    return _crop_first_animation_frame(image.convert("RGBA"))
        return None

    def _clean_glass_texture(self, block_id: str) -> Image.Image | None:
        local = _local_id(block_id)
        if local not in GLASS_OVERRIDES:
            return None
        ctm_dir = self._clean_glass_root / "assets" / "minecraft" / "optifine" / "ctm" / local
        for tile in ("4.png", "0.png", "2.png", "3.png", "1.png"):
            ctm = ctm_dir / tile
            if not ctm.is_file():
                continue
            with Image.open(ctm) as image:
                candidate = _crop_first_animation_frame(image.convert("RGBA"))
            alpha = candidate.getchannel("A").getextrema()
            if alpha != (0, 0):
                return candidate
        return self._find_named_texture(self._clean_glass_root, local)

    def _xk_texture_id_for_state(self, block_id: str, properties: dict[str, str], texture_id: str) -> str | None:
        local = _local_id(block_id)
        texture_name = Path(_texture_asset_path(texture_id)).stem
        if local == "repeater":
            if not texture_name.startswith("repeater"):
                return None
            delay = max(1, min(4, int(properties.get("delay", "1") or 1)))
            prefix = "repeater_on" if properties.get("powered") == "true" else "repeater"
            return f"minecraft:block/{prefix}{delay}"
        if local == "comparator":
            if not texture_name.startswith("comparator"):
                return None
            suffix = "2" if properties.get("mode") == "subtract" else ""
            on = "_on" if properties.get("powered") == "true" else ""
            return f"minecraft:block/comparator{suffix}{on}"
        if local in {"piston", "sticky_piston", "piston_head", "moving_piston"}:
            name = texture_name
            if not name.startswith("piston"):
                return None
            if (local == "sticky_piston" or properties.get("type") == "sticky") and name in {"piston_top", "piston_side", "piston_bottom"}:
                name = f"{name}_sticky"
            if properties.get("extended") == "true" and name in {"piston_top", "piston_side", "piston_bottom"}:
                name = f"{name}_on"
            return f"minecraft:block/{name}"
        if "redstone" in local or "redstone" in texture_id:
            name = texture_name
            if local == "redstone_wire":
                power = max(0, min(15, int(properties.get("power", "0") or 0)))
                return f"minecraft:block/redstone_dust_p{power:02d}"
            return f"minecraft:block/{name}"
        return None

    def _fallback_local_texture(self, block_id: str) -> Image.Image | None:
        if ":" in block_id:
            namespace, local = block_id.split(":", 1)
            texture = self._load_texture_image(f"{namespace}:block/{local}")
            if texture is not None:
                return texture
            xk_texture = self._find_named_texture(self._xk_redstone_root, local)
            if xk_texture is not None:
                return xk_texture
        texture = self._load_texture_image(f"minecraft:block/{_local_id(block_id)}")
        if texture is not None:
            return texture
        return None

    def _resolve_palette_materials(
        self,
        block_id: str,
        properties: dict[str, str],
    ) -> tuple[dict[str, _MaterialImage | None], str, bool, list[dict[str, Any]]]:
        suffix = _local_id(block_id)
        if suffix == "water":
            material = self._build_water_material(block_id, properties)
            return (
                {face_name: material for face_name in FACE_NAMES},
                "vanilla_model",
                False,
                self._build_water_quads(material),
            )

        refs = self._select_model_refs(block_id, properties)
        face_specs: dict[str, _FaceMaterialSpec] = {}
        fallback_ids: dict[str, str] = {}
        model_quads: list[dict[str, Any]] = []
        for ref in refs:
            model = self._resolve_model(ref.model)
            if model is None:
                continue
            self._collect_face_material_specs(face_specs, model, ref)
            self._collect_texture_fallback_ids(fallback_ids, model)
            model_quads.extend(self._collect_model_quads(block_id, properties, model, ref))

        materials: dict[str, _MaterialImage | None] = {}
        used_fallback = False
        for face_name in FACE_DIRECTIONS:
            spec = face_specs.get(face_name)
            if spec is None:
                texture_id = self._fallback_texture_id(face_name, fallback_ids)
                spec = _FaceMaterialSpec(texture_id) if texture_id else None
            material = self._material_from_texture_id(
                block_id,
                properties,
                face_name,
                spec,
            )
            if material is None:
                material = self._material_from_fallback_tile(block_id, properties, face_name)
                if material is not None:
                    used_fallback = True
            materials[face_name] = material

        materials["cross"] = (
            materials.get("north")
            or materials.get("south")
            or materials.get("west")
            or materials.get("east")
            or materials.get("up")
            or materials.get("down")
        )

        if all(material is None for material in materials.values()):
            fallback = self._material_from_fallback_tile(block_id, properties, "all")
            materials = {face_name: fallback for face_name in FACE_NAMES}
            used_fallback = True
        if suffix == "moving_piston" and not model_quads:
            model_quads.extend(self._build_moving_piston_quads(properties))
        if suffix.endswith("_sign") and not model_quads:
            model_quads.extend(self._build_sign_quads(block_id, properties, materials.get("cross") or materials.get("up")))
        return materials, "vanilla_model" if model_quads else "auto", used_fallback, model_quads

    def _build_moving_piston_quads(self, properties: dict[str, str]) -> list[dict[str, Any]]:
        head_props = {
            "facing": properties.get("facing", "north"),
            "short": "true",
            "type": "sticky" if properties.get("type") == "sticky" else "normal",
        }
        quads: list[dict[str, Any]] = []
        for ref in self._select_model_refs("minecraft:piston_head", head_props):
            model = self._resolve_model(ref.model)
            if model is not None:
                quads.extend(self._collect_model_quads("minecraft:moving_piston", head_props, model, ref))
        return quads

    def _build_moving_piston_progress_quads(self, properties: dict[str, str], progress: float) -> list[dict[str, Any]]:
        quads = self._build_moving_piston_quads(properties)
        facing = properties.get("facing", "north")
        offset = {
            "down": (0.0, -progress, 0.0),
            "up": (0.0, progress, 0.0),
            "north": (0.0, 0.0, -progress),
            "south": (0.0, 0.0, progress),
            "west": (-progress, 0.0, 0.0),
            "east": (progress, 0.0, 0.0),
        }.get(facing, (0.0, 0.0, -progress))
        return [
            {
                **quad,
                "vertices": [
                    [round(vertex[0] + offset[0], 6), round(vertex[1] + offset[1], 6), round(vertex[2] + offset[2], 6)]
                    for vertex in quad["vertices"]
                ],
            }
            for quad in quads
        ]

    def _build_block_model_overrides(self, source_litematic_path: Path | None) -> list[dict[str, Any]]:
        if source_litematic_path is None or not source_litematic_path.is_file():
            return []
        try:
            from amulet_nbt import load
        except Exception:
            return []
        try:
            root = load(str(source_litematic_path))
            regions = root["Regions"]
        except Exception:
            return []

        bounds_min = self._litematic_bounds_min(regions)
        overrides: list[dict[str, Any]] = []
        for region in regions.values():
            block_lookup = self._region_block_lookup(region, bounds_min)
            tile_entities = region.get("TileEntities") or region.get("BlockEntities") or []
            for tile in tile_entities:
                pos = self._tile_entity_scene_pos(tile, region, bounds_min)
                if pos is None:
                    continue
                block_id, properties = block_lookup.get(pos, ("", {}))
                local = _local_id(block_id)
                if local == "moving_piston":
                    progress = self._tile_float(tile, "Progress", "progress", default=0.0)
                    progress = max(0.0, min(1.0, progress))
                    quads = self._build_moving_piston_progress_quads(properties, progress)
                    if quads:
                        overrides.append({"x": pos[0], "y": pos[1], "z": pos[2], "replace": True, "quads": quads})
                    continue
                if local.endswith("_sign"):
                    text_lines = self._extract_sign_text_lines(tile)
                    if not any(text_lines):
                        continue
                    text_material = self._sign_text_material(block_id, properties, text_lines)
                    text_quad = self._sign_text_quad_for_state(properties, text_material)
                    overrides.append({"x": pos[0], "y": pos[1], "z": pos[2], "replace": False, "quads": [text_quad]})
        return overrides

    def _litematic_bounds_min(self, regions: Any) -> tuple[int, int, int]:
        mins: list[tuple[int, int, int]] = []
        for region in regions.values():
            position = region.get("Position") or {}
            size = region.get("Size") or {}
            px, py, pz = self._tag_int(position.get("x")), self._tag_int(position.get("y")), self._tag_int(position.get("z"))
            sx, sy, sz = self._tag_int(size.get("x")), self._tag_int(size.get("y")), self._tag_int(size.get("z"))
            mins.append((min(px, px + sx - 1), min(py, py + sy - 1), min(pz, pz + sz - 1)))
        if not mins:
            return (0, 0, 0)
        return (min(x for x, _, _ in mins), min(y for _, y, _ in mins), min(z for _, _, z in mins))

    def _region_block_lookup(self, region: Any, bounds_min: tuple[int, int, int]) -> dict[tuple[int, int, int], tuple[str, dict[str, str]]]:
        try:
            palette = region["BlockStatePalette"]
            block_states = list(region["BlockStates"])
            size = region["Size"]
            position = region["Position"]
        except Exception:
            return {}
        sx, sy, sz = abs(self._tag_int(size.get("x"))), abs(self._tag_int(size.get("y"))), abs(self._tag_int(size.get("z")))
        if sx <= 0 or sy <= 0 or sz <= 0:
            return {}
        bits = max(2, (len(palette) - 1).bit_length())
        mask = (1 << bits) - 1
        px, py, pz = self._tag_int(position.get("x")), self._tag_int(position.get("y")), self._tag_int(position.get("z"))
        lookup: dict[tuple[int, int, int], tuple[str, dict[str, str]]] = {}
        volume = sx * sy * sz
        for index in range(volume):
            bit = index * bits
            raw = int(block_states[bit // 64])
            if raw < 0:
                raw += 1 << 64
            palette_index = (raw >> (bit % 64)) & mask
            if bit % 64 + bits > 64 and bit // 64 + 1 < len(block_states):
                raw_next = int(block_states[bit // 64 + 1])
                if raw_next < 0:
                    raw_next += 1 << 64
                palette_index |= (raw_next << (64 - bit % 64)) & mask
            if palette_index <= 0 or palette_index >= len(palette):
                continue
            state = palette[palette_index]
            block_id = self._tag_str(state.get("Name"))
            properties = {str(k): self._tag_str(v) for k, v in dict(state.get("Properties") or {}).items()}
            y = index // (sx * sz)
            rem = index % (sx * sz)
            z = rem // sx
            x = rem % sx
            lookup[(px + x - bounds_min[0], py + y - bounds_min[1], pz + z - bounds_min[2])] = (block_id, properties)
        return lookup

    def _tile_entity_scene_pos(self, tile: Any, region: Any, bounds_min: tuple[int, int, int]) -> tuple[int, int, int] | None:
        if all(key in tile for key in ("x", "y", "z")):
            return (
                self._tag_int(tile.get("x")) - bounds_min[0],
                self._tag_int(tile.get("y")) - bounds_min[1],
                self._tag_int(tile.get("z")) - bounds_min[2],
            )
        if "Pos" in tile and len(tile["Pos"]) >= 3:
            return (
                self._tag_int(tile["Pos"][0]) - bounds_min[0],
                self._tag_int(tile["Pos"][1]) - bounds_min[1],
                self._tag_int(tile["Pos"][2]) - bounds_min[2],
            )
        if all(key in tile for key in ("LocalX", "LocalY", "LocalZ")):
            position = region.get("Position") or {}
            return (
                self._tag_int(position.get("x")) + self._tag_int(tile.get("LocalX")) - bounds_min[0],
                self._tag_int(position.get("y")) + self._tag_int(tile.get("LocalY")) - bounds_min[1],
                self._tag_int(position.get("z")) + self._tag_int(tile.get("LocalZ")) - bounds_min[2],
            )
        return None

    def _extract_sign_text_lines(self, tile: Any) -> list[str]:
        front_text = tile.get("front_text") or tile.get("FrontText")
        if front_text is not None:
            try:
                messages = front_text.get("messages")
            except Exception:
                messages = None
            if messages is not None:
                return [self._plain_text_from_component(self._tag_str(item)) for item in list(messages or [])[:4]]
        lines = []
        for key in ("Text1", "Text2", "Text3", "Text4"):
            lines.append(self._plain_text_from_component(self._tag_str(tile.get(key))))
        return lines[:4]

    def _plain_text_from_component(self, value: str) -> str:
        value = value.strip()
        if not value:
            return ""
        try:
            parsed = json.loads(value)
        except Exception:
            return value.strip('"')
        return self._component_to_text(parsed)

    def _component_to_text(self, value: Any) -> str:
        if isinstance(value, str):
            return value
        if isinstance(value, list):
            return "".join(self._component_to_text(item) for item in value)
        if isinstance(value, dict):
            text = str(value.get("text", "") or "")
            extra = value.get("extra")
            if isinstance(extra, list):
                text += "".join(self._component_to_text(item) for item in extra)
            return text
        return ""

    def _tag_int(self, value: Any, default: int = 0) -> int:
        if value is None:
            return default
        for attr in ("py_int", "py_float", "value"):
            if hasattr(value, attr):
                try:
                    return int(getattr(value, attr))
                except Exception:
                    pass
        try:
            return int(value)
        except Exception:
            return default

    def _tile_float(self, tile: Any, *keys: str, default: float = 0.0) -> float:
        for key in keys:
            if key not in tile:
                continue
            value = tile.get(key)
            for attr in ("py_float", "py_int", "value"):
                if hasattr(value, attr):
                    try:
                        return float(getattr(value, attr))
                    except Exception:
                        pass
            try:
                return float(value)
            except Exception:
                pass
        return default

    def _tag_str(self, value: Any, default: str = "") -> str:
        if value is None:
            return default
        if hasattr(value, "py_str"):
            try:
                return str(value.py_str)
            except Exception:
                pass
        if hasattr(value, "value"):
            try:
                return str(value.value)
            except Exception:
                pass
        return str(value)

    def _collect_model_quads(
        self,
        block_id: str,
        properties: dict[str, str],
        model: _ResolvedModel,
        ref: _ModelRef,
    ) -> list[dict[str, Any]]:
        quads: list[dict[str, Any]] = []
        for element in model.elements:
            faces = element.get("faces")
            if not isinstance(faces, dict):
                continue
            for face_name, face in faces.items():
                if not isinstance(face, dict):
                    continue
                resolved = self._resolve_texture_id(face.get("texture"), model.textures)
                if not resolved:
                    continue
                spec = _FaceMaterialSpec(
                    resolved,
                    _face_uv_tuple(face.get("uv")),
                    int(face.get("rotation", 0) or 0) % 360,
                )
                material = self._material_from_texture_id(block_id, properties, str(face_name), spec)
                if material is None:
                    material = self._material_from_fallback_tile(block_id, properties, str(face_name))
                vertices = _element_face_quad(element, str(face_name), ref)
                if material is None or vertices is None:
                    continue
                alpha = _detect_alpha_mode(material.image)
                quads.append(
                    {
                        "vertices": vertices,
                        "material": material,
                        "double_sided": alpha in {"cutout", "translucent"} or _local_id(block_id).endswith("_rail"),
                        "cullface": _rotate_direction(str(face.get("cullface")), ref.x, ref.y) if face.get("cullface") else None,
                    }
                )
        return quads

    def _build_water_material(self, block_id: str, properties: dict[str, str]) -> _MaterialImage:
        image = (
            self._load_texture_image("minecraft:block/water_still")
            or self._fallback_local_texture(block_id)
            or Image.new("RGBA", (16, 16), (52, 118, 255, 150))
        ).convert("RGBA")
        if _detect_alpha_mode(image) == "opaque":
            alpha = Image.new("L", image.size, 150)
            image.putalpha(alpha)
        return _MaterialImage(key=f"water:{_state_cache_key(block_id, properties)}", image=image, alpha_mode="translucent")

    def _build_water_quads(self, material: _MaterialImage) -> list[dict[str, Any]]:
        return [
            {
                "vertices": _face_vertices(face, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
                "material": material,
                "double_sided": False,
                "cullface": face,
            }
            for face in FACE_DIRECTIONS
        ]

    def _build_sign_quads(
        self,
        block_id: str,
        properties: dict[str, str],
        wood_material: _MaterialImage | None,
    ) -> list[dict[str, Any]]:
        if wood_material is None:
            wood_material = self._material_from_fallback_tile(block_id, properties, "sign")
        if wood_material is None:
            return []
        text_material = self._sign_text_material(block_id, properties, None)
        return [
            {
                "vertices": [[0.125, 0.25, 0.5], [0.875, 0.25, 0.5], [0.875, 0.75, 0.5], [0.125, 0.75, 0.5]],
                "material": wood_material,
                "double_sided": False,
            },
            {
                "vertices": [[0.16, 0.32, 0.497], [0.84, 0.32, 0.497], [0.84, 0.68, 0.497], [0.16, 0.68, 0.497]],
                "material": text_material,
                "double_sided": False,
            },
            {
                "vertices": [[0.4375, 0.0, 0.46875], [0.5625, 0.0, 0.46875], [0.5625, 0.25, 0.53125], [0.4375, 0.25, 0.53125]],
                "material": wood_material,
                "double_sided": False,
            },
        ]

    def _sign_text_quad_for_state(self, properties: dict[str, str], material: _MaterialImage) -> dict[str, Any]:
        vertices = [[0.16, 0.32, 0.497], [0.84, 0.32, 0.497], [0.84, 0.68, 0.497], [0.16, 0.68, 0.497]]
        facing = properties.get("facing")
        if facing in {"south", "west", "east"}:
            y_rotation = {"south": 180.0, "west": 270.0, "east": 90.0}[facing]
            vertices = [_rotate_point_around_axis(point, [0.5, 0.5, 0.5], "y", y_rotation) for point in vertices]
        elif "rotation" in properties:
            try:
                y_rotation = int(properties.get("rotation", "0") or 0) * 22.5
                vertices = [_rotate_point_around_axis(point, [0.5, 0.5, 0.5], "y", y_rotation) for point in vertices]
            except ValueError:
                pass
        return {
            "vertices": [[round(coord, 6) for coord in point] for point in vertices],
            "material": material,
            "double_sided": False,
        }

    def _sign_text_material(self, block_id: str, properties: dict[str, str], lines: list[str] | None) -> _MaterialImage:
        line_key = "|".join(lines or ["", "", "", ""])
        key = f"sign_text:{_state_cache_key(block_id, properties)}:{line_key}"
        cached = self._tinted_cache.get(key)
        if cached is not None:
            return _MaterialImage(key=key, image=cached.copy(), alpha_mode="cutout")
        image = Image.new("RGBA", (64, 32), (0, 0, 0, 0))
        draw = ImageDraw.Draw(image)
        font = ImageFont.load_default()
        if lines is None:
            lines = ["", "", "", ""]
        if any(lines):
            for line, y in zip(lines[:4], (4, 11, 18, 25), strict=False):
                text = line[:18]
                if not text:
                    continue
                try:
                    box = draw.textbbox((0, 0), text, font=font)
                    width = box[2] - box[0]
                except Exception:
                    width = len(text) * 6
                draw.text(((64 - width) / 2, y), text, fill=(38, 24, 12, 235), font=font)
        else:
            pixels = image.load()
            for row, y in enumerate((7, 13, 19, 25)):
                width = 40 - row * 4
                x0 = (64 - width) // 2
                for y2 in range(y, y + 2):
                    for x in range(x0, x0 + width):
                        pixels[x, y2] = (38, 24, 12, 220)
        self._tinted_cache[key] = image.copy()
        return _MaterialImage(key=key, image=image, alpha_mode="cutout")

    def _collect_face_material_specs(
        self,
        target: dict[str, _FaceMaterialSpec],
        model: _ResolvedModel,
        ref: _ModelRef,
    ) -> None:
        for element in model.elements:
            faces = element.get("faces")
            if not isinstance(faces, dict):
                continue
            for face_name, face in faces.items():
                if not isinstance(face, dict):
                    continue
                resolved = self._resolve_texture_id(face.get("texture"), model.textures)
                if not resolved:
                    continue
                rotated_face = _rotate_direction(str(face_name), ref.x, ref.y)
                uv = _face_uv_tuple(face.get("uv"))
                rotation = int(face.get("rotation", 0) or 0) % 360
                target.setdefault(rotated_face, _FaceMaterialSpec(resolved, uv, rotation))

    def _collect_texture_fallback_ids(self, target: dict[str, str], model: _ResolvedModel) -> None:
        for key in ("top", "bottom", "side", "end", "all", "particle"):
            resolved = self._resolve_texture_id(f"#{key}", model.textures)
            if resolved:
                target.setdefault(key, resolved)

    def _fallback_texture_id(self, face_name: str, fallback_ids: dict[str, str]) -> str | None:
        if face_name == "up":
            return (
                fallback_ids.get("top")
                or fallback_ids.get("end")
                or fallback_ids.get("all")
                or fallback_ids.get("particle")
            )
        if face_name == "down":
            return (
                fallback_ids.get("bottom")
                or fallback_ids.get("end")
                or fallback_ids.get("all")
                or fallback_ids.get("particle")
                or fallback_ids.get("top")
            )
        return (
            fallback_ids.get("side")
            or fallback_ids.get("end")
            or fallback_ids.get("all")
            or fallback_ids.get("particle")
            or fallback_ids.get("top")
        )

    def _material_from_texture_id(
        self,
        block_id: str,
        properties: dict[str, str],
        face_name: str,
        spec: _FaceMaterialSpec | None,
    ) -> _MaterialImage | None:
        if spec is None or not spec.texture_id:
            return None
        texture_id = spec.texture_id
        key_prefix = texture_id
        clean_glass = self._clean_glass_texture(block_id)
        if clean_glass is not None:
            image = clean_glass
            key_prefix = f"cleanglass:{_local_id(block_id)}"
        else:
            xk_texture_id = self._xk_texture_id_for_state(block_id, properties, texture_id)
            xk_image = self._load_texture_image(xk_texture_id) if xk_texture_id else None
            if xk_image is not None:
                image = xk_image
                key_prefix = f"xk:{xk_texture_id}"
            else:
                image = self._load_texture_image(texture_id)
        if image is None:
            return None
        image = _bake_face_image(image, spec.uv, spec.rotation)
        key = f"{key_prefix}@uv={spec.uv or 'full'}@rot={spec.rotation}"
        return _MaterialImage(
            key=f"{key}#{face_name}",
            image=image,
            alpha_mode=_detect_alpha_mode(image),
        )

    def _material_from_fallback_tile(
        self,
        block_id: str,
        properties: dict[str, str],
        face_name: str,
    ) -> _MaterialImage | None:
        clean_glass = self._clean_glass_texture(block_id)
        if clean_glass is not None:
            image = clean_glass
            key = f"cleanglass:{_local_id(block_id)}#{face_name}"
        else:
            image = self._fallback_local_texture(block_id)
            if block_id.startswith("create:") and self._find_named_texture(self._xk_redstone_root, _local_id(block_id)) is not None:
                key = f"xk:create:block/{_local_id(block_id)}#{face_name}"
            else:
                key = f"fallback:{block_id}#{face_name}"
        if image is None and _local_id(block_id).endswith("_sign"):
            wood = _local_id(block_id).removesuffix("_wall_sign").removesuffix("_hanging_sign").removesuffix("_sign")
            image = self._load_texture_image(f"minecraft:block/{wood}_planks")
        if image is None:
            return None
        if _local_id(block_id) == "redstone_wire":
            image = self._tinted_redstone_image(key, image, properties)
            key = f"{key}#power={properties.get('power', '0')}"
        return _MaterialImage(key=key, image=image, alpha_mode=_detect_alpha_mode(image))

    def _build_redstone_wire_material(
        self,
        block_id: str,
        properties: dict[str, str],
    ) -> _MaterialImage:
        state_key = _state_cache_key(block_id, properties)
        power = max(0, min(15, int(properties.get("power", "0") or 0)))
        xk_key = f"xk:minecraft:block/redstone_dust_p{power:02d}:{state_key}"
        cached = self._tinted_cache.get(state_key)
        if cached is not None:
            return _MaterialImage(
                key=xk_key,
                image=cached.copy(),
                alpha_mode=_detect_alpha_mode(cached),
            )
        image = (
            self._load_texture_image(f"minecraft:block/redstone_dust_p{power:02d}")
            or self._load_texture_image("minecraft:block/redstone_dust_dot")
            or self._load_texture_image("minecraft:block/redstone_dust_line0")
            or self._fallback_local_texture(block_id)
            or Image.new("RGBA", (16, 16), (255, 24, 24, 180))
        )
        image = _crop_first_animation_frame(image.convert("RGBA"))
        self._tinted_cache[state_key] = image.copy()
        return _MaterialImage(
            key=xk_key,
            image=image,
            alpha_mode=_detect_alpha_mode(image),
        )

    def _tinted_redstone_image(
        self,
        key: str,
        image: Image.Image,
        properties: dict[str, str],
    ) -> Image.Image:
        cache_key = f"{key}#{properties.get('power', '0')}"
        cached = self._tinted_cache.get(cache_key)
        if cached is not None:
            return cached.copy()
        power = max(0, min(15, int(properties.get("power", "15") or 15)))
        alpha = 80 + int(power / 15 * 80)
        overlay = Image.new("RGBA", image.size, (255, 24 + power * 6, 24, alpha))
        tinted = image.copy()
        tinted.alpha_composite(overlay)
        self._tinted_cache[cache_key] = tinted.copy()
        return tinted

    def _pack_materials(
        self,
        materials: list[_MaterialImage],
    ) -> tuple[Image.Image, list[dict[str, Any]]]:
        cell = max(max(material.image.width, material.image.height) for material in materials)
        cols = max(1, math.ceil(math.sqrt(len(materials))))
        rows = max(1, math.ceil(len(materials) / cols))
        atlas = Image.new("RGBA", (cols * cell, rows * cell), (0, 0, 0, 0))
        payload: list[dict[str, Any]] = []
        for index, material in enumerate(materials):
            col = index % cols
            row = index // cols
            x = col * cell
            y = row * cell
            atlas.alpha_composite(material.image, (x, y))
            payload.append(
                {
                    "key": material.key,
                    "uv_rect": [
                        round(x / atlas.width, 8),
                        round(y / atlas.height, 8),
                        round((x + material.image.width) / atlas.width, 8),
                        round((y + material.image.height) / atlas.height, 8),
                    ],
                    "alpha_mode": material.alpha_mode,
                    "pixel_size": [material.image.width, material.image.height],
                }
            )
        return atlas, payload


def build_full_mode_material_cache(
    cache_manifest_path: Path,
    layer_index_path: Path,
    output_json_path: Path,
    output_atlas_path: Path,
    source_litematic_path: Path | None = None,
) -> Path:
    try:
        payload = json.loads(layer_index_path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise RuntimeError(f"完整模式无法读取分层索引：{exc}") from exc
    visual = payload.get("visual") if isinstance(payload, dict) else {}
    palette = visual.get("palette") if isinstance(visual, dict) else []
    property_pool = visual.get("property_pool") if isinstance(visual, dict) else []
    if not isinstance(palette, list) or not isinstance(property_pool, list):
        raise RuntimeError("完整模式需要 cache 分层索引里的 palette/property_pool 数据；请重建 3D cache。")

    builder = MinecraftFullMaterialCacheBuilder()
    payload_json, atlas = builder.build_cache_payload(palette, property_pool, source_litematic_path)
    payload_json["cache_manifest"] = str(cache_manifest_path)
    payload_json["atlas_file"] = output_atlas_path.name
    output_json_path.parent.mkdir(parents=True, exist_ok=True)
    atlas_tmp = output_atlas_path.with_suffix(output_atlas_path.suffix + ".tmp")
    json_tmp = output_json_path.with_suffix(output_json_path.suffix + ".tmp")
    atlas.save(atlas_tmp, format="PNG")
    json_tmp.write_text(
        json.dumps(payload_json, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    atlas_tmp.replace(output_atlas_path)
    json_tmp.replace(output_json_path)
    return output_json_path
