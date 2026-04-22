from __future__ import annotations

import json
import math
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from PySide6.QtCore import QRectF, Qt
from PySide6.QtGui import QColor, QPainter, QPixmap


VANILLA_26_JAR = Path(r"E:\.minecraft\versions\26.1\26.1.jar")
FAITHFUL_64_ROOT = Path(r"C:\Users\USER\Downloads\Faithful 64x - Release 13")
BLOCK_26_FLAT_ROOT = Path(r"C:\Users\USER\Downloads\block-26.1")


@dataclass(slots=True)
class _ModelRef:
    model: str
    y: int = 0
    x: int = 0


@dataclass(slots=True)
class _ResolvedModel:
    elements: list[dict[str, Any]]
    textures: dict[str, Any]


class MinecraftTopSpriteResolver:
    """Small state-aware top-view renderer for the complete layer mode.

    The model decision chain is intentionally vanilla-first:
    26.1.jar blockstates/models decide geometry, Faithful only overrides
    textures, and the flat block-26.1 dump is the final texture fallback.
    """

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
        self._zip: zipfile.ZipFile | None = None
        self._names: set[str] | None = None
        self._json_cache: dict[str, Any] = {}
        self._model_cache: dict[str, _ResolvedModel | None] = {}
        self._texture_cache: dict[str, QPixmap | None] = {}
        self._sprite_cache: dict[str, QPixmap | None] = {}

    def is_available(self) -> bool:
        return self._vanilla_jar.is_file()

    def sprite_for_block(self, block_id: str, properties: dict[str, str]) -> QPixmap | None:
        if not self.is_available():
            return None
        state_key = _state_cache_key(block_id, properties)
        cached = self._sprite_cache.get(state_key)
        if state_key in self._sprite_cache:
            return QPixmap(cached) if cached is not None else None

        refs = self._select_model_refs(block_id, properties)
        if not refs:
            self._sprite_cache[state_key] = None
            return None

        sprite = QPixmap(32, 32)
        sprite.fill(Qt.GlobalColor.transparent)
        painter = QPainter(sprite)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
        painted = False
        for ref in refs:
            model = self._resolve_model(ref.model)
            if model is None:
                continue
            if self._paint_model_top(painter, model, ref, block_id, properties):
                painted = True
        painter.end()

        if not painted:
            fallback = self._fallback_full_tile(refs, block_id)
            if fallback is None:
                self._sprite_cache[state_key] = None
                return None
            self._sprite_cache[state_key] = fallback
            return QPixmap(fallback)

        if _local_id(block_id) == "redstone_wire":
            sprite = _tint_redstone(sprite, properties)
        self._sprite_cache[state_key] = sprite
        return QPixmap(sprite)

    def _zip_file(self) -> zipfile.ZipFile:
        if self._zip is None:
            self._zip = zipfile.ZipFile(self._vanilla_jar)
        return self._zip

    def _zip_names(self) -> set[str]:
        if self._names is None:
            self._names = set(self._zip_file().namelist())
        return self._names

    def _read_json(self, path: str) -> Any | None:
        cached = self._json_cache.get(path)
        if path in self._json_cache:
            return cached
        if path not in self._zip_names():
            self._json_cache[path] = None
            return None
        with self._zip_file().open(path) as fh:
            payload = json.load(fh)
        self._json_cache[path] = payload
        return payload

    def _select_model_refs(self, block_id: str, properties: dict[str, str]) -> list[_ModelRef]:
        bs_path = _blockstate_path(block_id)
        payload = self._read_json(bs_path)
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
            elements = [e for e in own_elements if isinstance(e, dict)]
        resolved = _ResolvedModel(elements=elements, textures=textures)
        self._model_cache[path] = resolved
        return resolved

    def _paint_model_top(
        self,
        painter: QPainter,
        model: _ResolvedModel,
        ref: _ModelRef,
        block_id: str,
        properties: dict[str, str],
    ) -> bool:
        painted = False
        for element in model.elements:
            faces = element.get("faces")
            if not isinstance(faces, dict):
                continue
            face = faces.get("up")
            if not isinstance(face, dict):
                continue
            texture_id = self._resolve_texture_id(face.get("texture"), model.textures)
            texture = self._texture_pixmap(texture_id)
            if texture is None or texture.isNull():
                continue
            dst = _element_top_rect(element)
            if dst is None or dst.width() <= 0 or dst.height() <= 0:
                continue
            dst = _rotate_rect(dst, ref.y)
            src = _face_source_rect(face, texture)
            painter.drawPixmap(dst, texture, src)
            painted = True
        return painted

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

    def _texture_pixmap(self, texture_id: str | None) -> QPixmap | None:
        if not texture_id:
            return None
        if texture_id in self._texture_cache:
            cached = self._texture_cache[texture_id]
            return QPixmap(cached) if cached is not None else None
        rel = _texture_asset_path(texture_id)
        pixmap = self._load_texture(rel)
        self._texture_cache[texture_id] = pixmap
        return QPixmap(pixmap) if pixmap is not None else None

    def _load_texture(self, rel: str) -> QPixmap | None:
        overlay = self._overlay_root / rel
        if overlay.is_file():
            pixmap = QPixmap(str(overlay))
            if not pixmap.isNull():
                return pixmap
        if rel in self._zip_names():
            data = self._zip_file().read(rel)
            pixmap = QPixmap()
            if pixmap.loadFromData(data) and not pixmap.isNull():
                return pixmap
        flat = self._flat_fallback_root / Path(rel).name
        if flat.is_file():
            pixmap = QPixmap(str(flat))
            if not pixmap.isNull():
                return pixmap
        return None

    def _fallback_full_tile(self, refs: list[_ModelRef], block_id: str) -> QPixmap | None:
        for ref in refs:
            model = self._resolve_model(ref.model)
            if model is None:
                continue
            for key in ("top", "all", "side", "particle"):
                texture_id = self._resolve_texture_id(f"#{key}", model.textures)
                texture = self._texture_pixmap(texture_id)
                if texture is not None and not texture.isNull():
                    out = QPixmap(32, 32)
                    out.fill(Qt.GlobalColor.transparent)
                    painter = QPainter(out)
                    painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
                    painter.drawPixmap(QRectF(0, 0, 32, 32), texture, QRectF(0, 0, texture.width(), texture.height()))
                    painter.end()
                    return out
        texture = self._texture_pixmap(f"minecraft:block/{_local_id(block_id)}")
        if texture is None:
            return None
        out = QPixmap(32, 32)
        out.fill(Qt.GlobalColor.transparent)
        painter = QPainter(out)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
        painter.drawPixmap(QRectF(0, 0, 32, 32), texture, QRectF(0, 0, texture.width(), texture.height()))
        painter.end()
        return out


_RESOLVER: MinecraftTopSpriteResolver | None = None


def minecraft_top_view_pixmap_for_block(block_id: str, properties: dict[str, str]) -> QPixmap | None:
    global _RESOLVER
    if _RESOLVER is None:
        _RESOLVER = MinecraftTopSpriteResolver()
    return _RESOLVER.sprite_for_block(block_id, properties)


def minecraft_top_view_resources_available() -> bool:
    global _RESOLVER
    if _RESOLVER is None:
        _RESOLVER = MinecraftTopSpriteResolver()
    return _RESOLVER.is_available()


def minecraft_top_view_average_color_for_block(block_id: str, properties: dict[str, str]) -> tuple[float, float, float] | None:
    sprite = minecraft_top_view_pixmap_for_block(block_id, properties)
    if sprite is None or sprite.isNull():
        return None
    image = sprite.toImage()
    if image.isNull():
        return None
    red = green = blue = count = 0
    for y in range(image.height()):
        for x in range(image.width()):
            color = image.pixelColor(x, y)
            alpha = color.alpha()
            if alpha <= 0:
                continue
            red += color.red() * alpha
            green += color.green() * alpha
            blue += color.blue() * alpha
            count += alpha
    if count <= 0:
        return None
    return (red / count / 255.0, green / count / 255.0, blue / count / 255.0)


def _state_cache_key(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return block_id
    pairs = ",".join(f"{k}={v}" for k, v in sorted(properties.items()))
    return f"{block_id}[{pairs}]"


def _local_id(block_id: str) -> str:
    return block_id.split(":", 1)[1] if ":" in block_id else block_id


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


def _element_top_rect(element: dict[str, Any]) -> QRectF | None:
    from_xyz = element.get("from")
    to_xyz = element.get("to")
    if not (isinstance(from_xyz, list) and isinstance(to_xyz, list) and len(from_xyz) >= 3 and len(to_xyz) >= 3):
        return None
    x1 = float(from_xyz[0]) * 2.0
    z1 = float(from_xyz[2]) * 2.0
    x2 = float(to_xyz[0]) * 2.0
    z2 = float(to_xyz[2]) * 2.0
    return QRectF(min(x1, x2), min(z1, z2), abs(x2 - x1), abs(z2 - z1))


def _rotate_rect(rect: QRectF, degrees: int) -> QRectF:
    steps = (degrees // 90) % 4
    if steps == 0:
        return rect
    points = [
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
        (rect.right(), rect.bottom()),
        (rect.left(), rect.bottom()),
    ]
    rotated = [_rotate_point(x, y, steps) for x, y in points]
    xs = [p[0] for p in rotated]
    ys = [p[1] for p in rotated]
    return QRectF(min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys))


def _rotate_point(x: float, y: float, steps: int) -> tuple[float, float]:
    cx = cy = 16.0
    dx = x - cx
    dy = y - cy
    if steps == 1:
        return cx - dy, cy + dx
    if steps == 2:
        return cx - dx, cy - dy
    if steps == 3:
        return cx + dy, cy - dx
    return x, y


def _face_source_rect(face: dict[str, Any], texture: QPixmap) -> QRectF:
    uv = face.get("uv")
    if not (isinstance(uv, list) and len(uv) >= 4):
        return QRectF(0, 0, texture.width(), texture.height())
    u1, v1, u2, v2 = (float(uv[0]), float(uv[1]), float(uv[2]), float(uv[3]))
    left = min(u1, u2) / 16.0 * texture.width()
    top = min(v1, v2) / 16.0 * texture.height()
    width = max(1.0, abs(u2 - u1) / 16.0 * texture.width())
    height = max(1.0, abs(v2 - v1) / 16.0 * texture.height())
    return QRectF(left, top, width, height)


def _tint_redstone(source: QPixmap, properties: dict[str, str]) -> QPixmap:
    power = max(0, min(15, int(properties.get("power", "15") or 15)))
    alpha = 80 + int(power / 15 * 80)
    out = QPixmap(source)
    painter = QPainter(out)
    painter.setCompositionMode(QPainter.CompositionMode.CompositionMode_SourceAtop)
    painter.fillRect(out.rect(), QColor(255, 24 + power * 6, 24, alpha))
    painter.end()
    return out
