"""为 Deepslate ``VoxelRenderer`` 生成体素列表（与正交预览相同的方块着色策略）。"""

from __future__ import annotations

from pathlib import Path

from litematicaba.core.litematic_ortho_preview import _block_id, _is_air, _rgba_for_block

_MAX_REGION_CELLS = 50_000_000
_DEFAULT_MAX_VOXELS = 220_000


def _block_id_at_region_coords(region: object, lx: int, ly: int, lz: int) -> str:
    """使用 Region 区域坐标（与 ``range_x/y/z`` / ``block_positions`` 一致）取方块 ID。"""
    st = region[lx, ly, lz]
    return st._BlockState__block_id  # noqa: SLF001


def _merged_schematic_bounds(schematic: object) -> tuple[int, int, int, int, int, int]:
    gx0 = gx1 = gy0 = gy1 = gz0 = gz1 = 0
    first = True
    for reg in schematic.regions.values():
        rx0, rx1 = reg.min_schem_x(), reg.max_schem_x()
        ry0, ry1 = reg.min_schem_y(), reg.max_schem_y()
        rz0, rz1 = reg.min_schem_z(), reg.max_schem_z()
        if first:
            gx0, gx1, gy0, gy1, gz0, gz1 = rx0, rx1, ry0, ry1, rz0, rz1
            first = False
        else:
            gx0, gx1 = min(gx0, rx0), max(gx1, rx1)
            gy0, gy1 = min(gy0, ry0), max(gy1, ry1)
            gz0, gz1 = min(gz0, rz0), max(gz1, rz1)
    return gx0, gx1, gy0, gy1, gz0, gz1


def build_region_voxels_payload(
    path: str | Path,
    region_name: str | None,
    *,
    max_voxels: int = _DEFAULT_MAX_VOXELS,
) -> tuple[dict | None, str]:
    """返回 ``(payload, err)``。

    ``region_name`` 为 ``None`` 时合并**所有**子区域到投影统一坐标系（与 vscode-nbt /
    Litematica 多区域组合一致）：同一格后遍历的 Region 覆盖先遍历的。

    ``payload`` 含 ``voxels``、``camDist``、可选 ``note``；成功时 ``err`` 为空串。
    """
    from litemapy import Schematic

    p = Path(path)
    try:
        schematic = Schematic.load(str(p))
    except Exception as exc:
        return None, f"无法加载投影：{exc}"

    if not schematic.regions:
        return None, "该文件没有子区域。"

    note_parts: list[str] = []

    if region_name is None:
        gx0, gx1, gy0, gy1, gz0, gz1 = _merged_schematic_bounds(schematic)
        bw = gx1 - gx0 + 1
        bh = gy1 - gy0 + 1
        bl = gz1 - gz0 + 1
        if bw <= 0 or bh <= 0 or bl <= 0:
            return None, "合并边界无效。"
        if bw * bh * bl > _MAX_REGION_CELLS:
            return None, f"合并后包围盒体素过多（{bw * bh * bl}），请换较小投影或后续版本再试。"

        merged: dict[tuple[int, int, int], str] = {}
        for reg in schematic.regions.values():
            for lx in reg.range_x():
                for ly in reg.range_y():
                    for lz in reg.range_z():
                        sx = reg.x + lx
                        sy = reg.y + ly
                        sz = reg.z + lz
                        merged[(sx, sy, sz)] = _block_id_at_region_coords(reg, lx, ly, lz)

        total_cells = bw * bh * bl
        step = 1
        if total_cells > max_voxels * 6:
            step = max(1, int(round((total_cells / max_voxels) ** (1.0 / 3.0))))
        if step > 1:
            note_parts.append(f"体素过密，已按 {step} 格步进抽样")
        note_parts.append(f"已合并 {len(schematic.regions)} 个子区域")

        ox = (gx0 + gx1) / 2.0
        oy = (gy0 + gy1) / 2.0
        oz = (gz0 + gz1) / 2.0

        solid: list[tuple[int, int, int, str]] = [
            (sx, sy, sz, bid) for (sx, sy, sz), bid in merged.items() if not _is_air(bid)
        ]
        if step > 1:
            solid = [
                t
                for t in solid
                if (t[0] - gx0) % step == 0
                and (t[1] - gy0) % step == 0
                and (t[2] - gz0) % step == 0
            ]

        voxels: list[dict[str, object]] = []
        for sx, sy, sz, bid in solid:
            r, g, b, _a = _rgba_for_block(bid)
            voxels.append(
                {
                    "x": int(round(sx - ox)),
                    "y": int(round(sy - oy)),
                    "z": int(round(sz - oz)),
                    "color": [r, g, b],
                }
            )

        if len(voxels) > max_voxels:
            k = max(1, (len(voxels) + max_voxels - 1) // max_voxels)
            voxels = voxels[::k][:max_voxels]
            note_parts.append(f"已截断至约 {max_voxels} 个体素")

        m = max(bw, bh, bl)
        cam_dist = max(24.0, float(m) * 1.85)
        note = "；".join(note_parts) if note_parts else ""
        payload: dict[str, object] = {
            "voxels": voxels,
            "camDist": cam_dist,
            "bounds": {"sx": bw, "sy": bh, "sz": bl},
        }
        if note:
            payload["note"] = note
        return payload, ""

    region = schematic.regions.get(region_name)
    if region is None:
        return None, f"找不到子区域：{region_name!r}"

    sx = region.maxx() - region.minx() + 1
    sy = region.maxy() - region.miny() + 1
    sz = region.maxz() - region.minz() + 1
    if sx <= 0 or sy <= 0 or sz <= 0:
        return None, "子区域尺寸无效。"

    if sx * sy * sz > _MAX_REGION_CELLS:
        return None, f"子区域体素过多（{sx * sy * sz}），请换较小区域或后续版本再试。"

    total_cells = sx * sy * sz
    step = 1
    if total_cells > max_voxels * 6:
        step = max(1, int(round((total_cells / max_voxels) ** (1.0 / 3.0))))

    voxels = []
    if step > 1:
        note_parts.append(f"体素过密，已按 {step} 格步进抽样")

    ox = (sx - 1) / 2.0
    oy = (sy - 1) / 2.0
    oz = (sz - 1) / 2.0

    for ix in range(0, sx, step):
        for iy in range(0, sy, step):
            for iz in range(0, sz, step):
                bid = _block_id(region, ix, iy, iz)
                if _is_air(bid):
                    continue
                r, g, b, _a = _rgba_for_block(bid)
                voxels.append(
                    {
                        "x": int(round(ix - ox)),
                        "y": int(round(iy - oy)),
                        "z": int(round(iz - oz)),
                        "color": [r, g, b],
                    }
                )

    if len(voxels) > max_voxels:
        k = max(1, (len(voxels) + max_voxels - 1) // max_voxels)
        voxels = voxels[::k][:max_voxels]
        note_parts.append(f"已截断至约 {max_voxels} 个体素")

    m = max(sx, sy, sz)
    cam_dist = max(24.0, float(m) * 1.85)

    note = "；".join(note_parts) if note_parts else ""
    payload = {
        "voxels": voxels,
        "camDist": cam_dist,
        "bounds": {"sx": sx, "sy": sy, "sz": sz},
    }
    if note:
        payload["note"] = note
    return payload, ""
