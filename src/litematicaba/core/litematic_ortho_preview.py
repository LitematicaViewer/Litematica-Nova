"""单区域正交剖面 RGBA 位图（legacy 2D，供分层页等使用）。

使用与 ``litematic_block_scan`` 相同的 litemapy 私有访问约定；不依赖 Qt。
"""

from __future__ import annotations

import hashlib
from enum import Enum, auto
from pathlib import Path

_AIR = frozenset(
    {
        "minecraft:air",
        "minecraft:cave_air",
        "minecraft:void_air",
    }
)

# 单维上限，避免超大投影一次性分配过大缓冲
_MAX_DIM = 640
_MAX_CELLS = 50_000_000


class OrthoViewKind(Enum):
    """九向：顶视 + 四向立面 + 四向 45° 俯视（XZ 旋转）。"""

    TOP = auto()
    NORTH = auto()
    SOUTH = auto()
    WEST = auto()
    EAST = auto()
    TOP_NE = auto()
    TOP_SE = auto()
    TOP_SW = auto()
    TOP_NW = auto()


def _is_air(block_id: str) -> bool:
    return block_id in _AIR


def _block_id(region: object, x: int, y: int, z: int) -> str:
    blocks = region._Region__blocks  # noqa: SLF001
    palette = region._Region__palette  # noqa: SLF001
    idx = blocks[x, y, z]
    return palette[idx]._BlockState__block_id  # noqa: SLF001


def _rgba_for_block(block_id: str) -> tuple[int, int, int, int]:
    if _is_air(block_id):
        return (40, 44, 52, 255)
    h = hashlib.md5(block_id.encode(), usedforsecurity=False).hexdigest()
    r, g, b = int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)
    return (r, g, b, 255)


def _write_pixel(buf: bytearray, w: int, x: int, y: int, rgba: tuple[int, int, int, int]) -> None:
    i = (y * w + x) * 4
    r, g, b, a = rgba
    buf[i] = r
    buf[i + 1] = g
    buf[i + 2] = b
    buf[i + 3] = a


def _top_color(region: object, sx: int, sy: int, sz: int, ix: int, iz: int) -> tuple[int, int, int, int]:
    for iy in range(sy - 1, -1, -1):
        bid = _block_id(region, ix, iy, iz)
        if not _is_air(bid):
            return _rgba_for_block(bid)
    return (40, 44, 52, 255)


def _raster_top(region: object, sx: int, sy: int, sz: int, scale: int) -> tuple[bytearray, int, int]:
    w, h = sx // scale, sz // scale
    buf = bytearray(w * h * 4)
    for ox in range(w):
        for oz in range(h):
            ix = min(sx - 1, ox * scale + scale // 2)
            iz = min(sz - 1, oz * scale + scale // 2)
            c = _top_color(region, sx, sy, sz, ix, iz)
            _write_pixel(buf, w, ox, oz, c)
    return buf, w, h


def _raster_north(region: object, sx: int, sy: int, sz: int, scale: int) -> tuple[bytearray, int, int]:
    w, h = sx // scale, sy // scale
    buf = bytearray(w * h * 4)
    for ox in range(w):
        for oy in range(h):
            ix = min(sx - 1, ox * scale + scale // 2)
            iy = min(sy - 1, oy * scale + scale // 2)
            c = (40, 44, 52, 255)
            for iz in range(sz):
                bid = _block_id(region, ix, iy, iz)
                if not _is_air(bid):
                    c = _rgba_for_block(bid)
                    break
            _write_pixel(buf, w, ox, oy, c)
    return buf, w, h


def _raster_south(region: object, sx: int, sy: int, sz: int, scale: int) -> tuple[bytearray, int, int]:
    w, h = sx // scale, sy // scale
    buf = bytearray(w * h * 4)
    for ox in range(w):
        for oy in range(h):
            ix = min(sx - 1, ox * scale + scale // 2)
            iy = min(sy - 1, oy * scale + scale // 2)
            c = (40, 44, 52, 255)
            for iz in range(sz - 1, -1, -1):
                bid = _block_id(region, ix, iy, iz)
                if not _is_air(bid):
                    c = _rgba_for_block(bid)
                    break
            _write_pixel(buf, w, ox, oy, c)
    return buf, w, h


def _raster_west(region: object, sx: int, sy: int, sz: int, scale: int) -> tuple[bytearray, int, int]:
    w, h = sz // scale, sy // scale
    buf = bytearray(w * h * 4)
    for oz in range(w):
        for oy in range(h):
            iz = min(sz - 1, oz * scale + scale // 2)
            iy = min(sy - 1, oy * scale + scale // 2)
            c = (40, 44, 52, 255)
            for ix in range(sx):
                bid = _block_id(region, ix, iy, iz)
                if not _is_air(bid):
                    c = _rgba_for_block(bid)
                    break
            _write_pixel(buf, w, oz, oy, c)
    return buf, w, h


def _raster_east(region: object, sx: int, sy: int, sz: int, scale: int) -> tuple[bytearray, int, int]:
    w, h = sz // scale, sy // scale
    buf = bytearray(w * h * 4)
    for oz in range(w):
        for oy in range(h):
            iz = min(sz - 1, oz * scale + scale // 2)
            iy = min(sy - 1, oy * scale + scale // 2)
            c = (40, 44, 52, 255)
            for ix in range(sx - 1, -1, -1):
                bid = _block_id(region, ix, iy, iz)
                if not _is_air(bid):
                    c = _rgba_for_block(bid)
                    break
            _write_pixel(buf, w, oz, oy, c)
    return buf, w, h


def _raster_top_diagonal(
    region: object,
    sx: int,
    sy: int,
    sz: int,
    scale: int,
    *,
    flip_x: bool,
    flip_z: bool,
) -> tuple[bytearray, int, int]:
    u_min = -(sz - 1)
    u_max = sx - 1
    v_min = 0
    v_max = sx + sz - 2
    uw = u_max - u_min + 1
    vh = v_max - v_min + 1
    buf = bytearray(uw * vh * 4)
    void = (40, 44, 52, 255)
    for u in range(u_min, u_max + 1):
        for v in range(v_min, v_max + 1):
            if (u + v) & 1:
                _write_pixel(buf, uw, u - u_min, v - v_min, void)
                continue
            ix = (u + v) // 2
            iz = (v - u) // 2
            if flip_x:
                ix = sx - 1 - ix
            if flip_z:
                iz = sz - 1 - iz
            if not (0 <= ix < sx and 0 <= iz < sz):
                _write_pixel(buf, uw, u - u_min, v - v_min, void)
                continue
            c = _top_color(region, sx, sy, sz, ix, iz)
            _write_pixel(buf, uw, u - u_min, v - v_min, c)
    if scale <= 1:
        return buf, uw, vh
    nw, nh = max(1, uw // scale), max(1, vh // scale)
    out = bytearray(nw * nh * 4)
    for ox in range(nw):
        for oy in range(nh):
            su = min(uw - 1, ox * scale + scale // 2)
            sv = min(vh - 1, oy * scale + scale // 2)
            si = (sv * uw + su) * 4
            di = (oy * nw + ox) * 4
            out[di : di + 4] = buf[si : si + 4]
    return out, nw, nh


def _pick_scale(sx: int, sy: int, sz: int, kind: OrthoViewKind) -> int:
    if kind in (OrthoViewKind.TOP_NE, OrthoViewKind.TOP_SE, OrthoViewKind.TOP_SW, OrthoViewKind.TOP_NW):
        uw = sx + sz - 1
        vh = sx + sz - 1
        m = max(uw, vh)
    elif kind == OrthoViewKind.TOP:
        m = max(sx, sz)
    elif kind in (OrthoViewKind.NORTH, OrthoViewKind.SOUTH):
        m = max(sx, sy)
    else:
        m = max(sz, sy)
    if m <= _MAX_DIM:
        return 1
    s = (m + _MAX_DIM - 1) // _MAX_DIM
    return max(1, s)


def render_region_ortho_rgba(
    path: str | Path,
    region_name: str,
    kind: OrthoViewKind,
) -> tuple[bytes | None, int, int, str]:
    """返回 ``(rgba_bytes, width, height, error)``；成功时 error 为空串。"""
    from litemapy import Schematic

    p = Path(path)
    try:
        schematic = Schematic.load(str(p))
    except Exception as exc:
        return None, 0, 0, f"无法加载投影：{exc}"

    region = schematic.regions.get(region_name)
    if region is None:
        return None, 0, 0, f"找不到子区域：{region_name!r}"

    sx = region.maxx() - region.minx() + 1
    sy = region.maxy() - region.miny() + 1
    sz = region.maxz() - region.minz() + 1
    if sx <= 0 or sy <= 0 or sz <= 0:
        return None, 0, 0, "子区域尺寸无效。"

    if sx * sy * sz > _MAX_CELLS:
        return None, 0, 0, f"子区域体素过多（{sx * sy * sz}），请换较小区域或后续版本再试。"

    scale = _pick_scale(sx, sy, sz, kind)

    try:
        if kind == OrthoViewKind.TOP:
            buf, w, h = _raster_top(region, sx, sy, sz, scale)
        elif kind == OrthoViewKind.NORTH:
            buf, w, h = _raster_north(region, sx, sy, sz, scale)
        elif kind == OrthoViewKind.SOUTH:
            buf, w, h = _raster_south(region, sx, sy, sz, scale)
        elif kind == OrthoViewKind.WEST:
            buf, w, h = _raster_west(region, sx, sy, sz, scale)
        elif kind == OrthoViewKind.EAST:
            buf, w, h = _raster_east(region, sx, sy, sz, scale)
        elif kind == OrthoViewKind.TOP_NE:
            buf, w, h = _raster_top_diagonal(region, sx, sy, sz, scale, flip_x=False, flip_z=False)
        elif kind == OrthoViewKind.TOP_SE:
            buf, w, h = _raster_top_diagonal(region, sx, sy, sz, scale, flip_x=False, flip_z=True)
        elif kind == OrthoViewKind.TOP_SW:
            buf, w, h = _raster_top_diagonal(region, sx, sy, sz, scale, flip_x=True, flip_z=True)
        else:  # TOP_NW
            buf, w, h = _raster_top_diagonal(region, sx, sy, sz, scale, flip_x=True, flip_z=False)
    except Exception as exc:
        return None, 0, 0, f"渲染失败：{exc}"

    note = ""
    if scale > 1:
        note = f"（已按 {scale}× 降采样以控制图像尺寸）"
    return bytes(buf), w, h, note
