"""Litematic 属性区（Metadata）读写。

通过 amulet_nbt 读写压缩 NBT，与属性页 ``PropertiesPage`` 使用的 ``SnbtProperties`` 对齐。
当前保存路径会改写：``Metadata.Name`` / ``Author`` / ``Description`` / ``PreviewImageData`` / ``TimeModified``；
若 ``SnbtProperties.regions`` 非空，还会按属性页表格顺序与名称重写根级 ``Regions``（仅重命名子复合键，子树内容保持引用不变）。
其余键保持文件原有内容（如 ``TotalBlocks``、``EnclosingSize`` 等由游戏或其它工具维护）。
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any

def _to_int(value, default: int = 0) -> int:
    try:
        if hasattr(value, "py_int"):
            return int(value.py_int)
        return int(str(value))
    except Exception:
        return default


def _to_str(value, default: str = "") -> str:
    if value is None:
        return default
    if hasattr(value, "py_str"):
        try:
            return str(value.py_str)
        except Exception:
            pass
    text = str(value)
    if len(text) >= 2 and text[0] == '"' and text[-1] == '"':
        return text[1:-1]
    return text


def _to_int32_signed(value: int) -> int:
    """Normalize Python int to signed int32 range."""
    v = int(value) & 0xFFFFFFFF
    if v >= 0x80000000:
        v -= 0x100000000
    return v


@dataclass(slots=True)
class RegionInfo:
    """根级 ``Regions`` 下的一个子区域在 UI 中的行。

    ``source_key``：最近一次自磁盘成功加载时该子复合在 NBT 中的键名，用于保存时定位子树；
    ``name``：当前显示名及保存目标键名（用户可编辑）。
    """

    source_key: str
    name: str
    size: tuple[int, int, int] = (0, 0, 0)
    position: tuple[int, int, int] = (0, 0, 0)


@dataclass(slots=True)
class SnbtProperties:
    file_path: Path | None = None
    # 加载时设为 path.name；UI「文件名称」编辑框绑定此字段，另存为默认名等使用，**非** SNBT 中的 Name
    file_name: str = ""
    # 对应 Metadata compound 的 ``Name`` 字符串（Litematica 内部/材料列表等引用）
    internal_name: str = ""
    author: str = ""
    description: str = ""
    created_unix: int = 0
    modified_unix: int = 0
    enclosing_size: tuple[int, int, int] = (0, 0, 0)
    total_blocks: int = 0
    total_volume: int = 0
    litematic_version: int = 0
    minecraft_data_version: int = 0
    preview_image_data: list[int] = field(default_factory=list)
    regions: list[RegionInfo] = field(default_factory=list)
    # 分阶段加载标记：阶段一（元数据）和阶段二（区域几何信息）已加载，阶段三（方块数据）按需加载
    _metadata_loaded: bool = field(default=False, repr=False)
    _regions_geo_loaded: bool = field(default=False, repr=False)
    # 保留原始 NBT 根标签引用，供阶段三按需加载区域详细数据
    _root_tag: Any = field(default=None, repr=False)


def copy_snbt_properties(data: SnbtProperties) -> SnbtProperties:
    """返回 ``SnbtProperties`` 的独立副本。

    ``preview_image_data`` 必须 ``list(...)`` 复制，否则快照与当前编辑会共享同一列表引用；
    ``regions`` 逐行复制为新的 ``RegionInfo``；
    ``_root_tag`` 置为 None 避免共享 NBT 引用（快照不需要阶段三数据）。
    用于「恢复默认值」基线与 ``_current_data`` 分离。
    """
    return replace(
        data,
        preview_image_data=list(data.preview_image_data),
        regions=[RegionInfo(r.source_key, r.name, r.size, r.position) for r in data.regions],
        _root_tag=None,
    )


def snbt_properties_to_dict(data: SnbtProperties) -> dict[str, Any]:
    return {
        "file_path": str(data.file_path) if data.file_path is not None else "",
        "file_name": data.file_name,
        "internal_name": data.internal_name,
        "author": data.author,
        "description": data.description,
        "created_unix": int(data.created_unix),
        "modified_unix": int(data.modified_unix),
        "enclosing_size": list(data.enclosing_size),
        "total_blocks": int(data.total_blocks),
        "total_volume": int(data.total_volume),
        "litematic_version": int(data.litematic_version),
        "minecraft_data_version": int(data.minecraft_data_version),
        "preview_image_data": [int(value) for value in data.preview_image_data],
        "regions": [
            {
                "source_key": region.source_key,
                "name": region.name,
                "size": list(region.size),
                "position": list(region.position),
            }
            for region in data.regions
        ],
    }


def snbt_properties_from_dict(raw: dict[str, Any]) -> SnbtProperties:
    file_path = str(raw.get("file_path", "") or "").strip()
    return SnbtProperties(
        file_path=Path(file_path) if file_path else None,
        file_name=str(raw.get("file_name", "") or ""),
        internal_name=str(raw.get("internal_name", "") or ""),
        author=str(raw.get("author", "") or ""),
        description=str(raw.get("description", "") or ""),
        created_unix=int(raw.get("created_unix", 0) or 0),
        modified_unix=int(raw.get("modified_unix", 0) or 0),
        enclosing_size=tuple(int(v or 0) for v in list(raw.get("enclosing_size", [0, 0, 0]))[:3]) or (0, 0, 0),
        total_blocks=int(raw.get("total_blocks", 0) or 0),
        total_volume=int(raw.get("total_volume", 0) or 0),
        litematic_version=int(raw.get("litematic_version", 0) or 0),
        minecraft_data_version=int(raw.get("minecraft_data_version", 0) or 0),
        preview_image_data=[int(value or 0) for value in list(raw.get("preview_image_data", []))],
        regions=[
            RegionInfo(
                source_key=str(item.get("source_key", "") or ""),
                name=str(item.get("name", "") or ""),
                size=tuple(int(v or 0) for v in list(item.get("size", [0, 0, 0]))[:3]) or (0, 0, 0),
                position=tuple(int(v or 0) for v in list(item.get("position", [0, 0, 0]))[:3]) or (0, 0, 0),
            )
            for item in list(raw.get("regions", []))
            if isinstance(item, dict)
        ],
        _metadata_loaded=True,
        _regions_geo_loaded=True,
        _root_tag=None,
    )
def _import_amulet_nbt():
    try:
        from amulet_nbt import (
            CompoundTag,
            IntArrayTag,
            IntTag,
            LongTag,
            NamedTag,
            StringTag,
            load,
        )
    except Exception as exc:
        raise RuntimeError(
            "缺少依赖 amulet_nbt，请先安装 requirements.txt 后再使用 SNBT 读写。"
        ) from exc
    return CompoundTag, IntArrayTag, IntTag, LongTag, NamedTag, StringTag, load


def _read_xyz_compound(comp: Any) -> tuple[int, int, int]:
    if comp is None:
        return (0, 0, 0)
    try:
        return (
            _to_int(comp.get("x"), 0),
            _to_int(comp.get("y"), 0),
            _to_int(comp.get("z"), 0),
        )
    except Exception:
        return (0, 0, 0)


def _parse_regions_list(root: Any) -> list[RegionInfo]:
    """从根标签读取 ``Regions``，顺序为 NBT 复合中的键迭代顺序（amulet 与 Python 3.7+ 通常稳定）。"""
    regions_tag = root.get("Regions")
    if regions_tag is None:
        return []
    try:
        keys = list(regions_tag.keys())
    except Exception:
        return []
    out: list[RegionInfo] = []
    for key in keys:
        key_str = _to_str(key, str(key))
        sub = regions_tag.get(key)
        if sub is None:
            continue
        size_c = sub.get("Size")
        if size_c is None:
            size_c = sub.get("size")
        pos_c = sub.get("Position")
        if pos_c is None:
            pos_c = sub.get("position")
        sz = _read_xyz_compound(size_c)
        pos = _read_xyz_compound(pos_c)
        out.append(RegionInfo(source_key=key_str, name=key_str, size=sz, position=pos))
    return out


def regions_after_save_commit(data: SnbtProperties) -> SnbtProperties:
    """保存成功后把 ``source_key`` 与 ``name`` 对齐，便于同一会话内再次保存。"""
    fixed = [
        RegionInfo(
            source_key=r.name.strip(),
            name=r.name.strip(),
            size=r.size,
            position=r.position,
        )
        for r in data.regions
    ]
    return replace(data, regions=fixed)


def _apply_region_renames(root: Any, regions: list[RegionInfo], CompoundTag: Any) -> None:
    if not regions:
        return
    regions_tag = root.get("Regions")
    if regions_tag is None:
        raise ValueError("文件中缺少 Regions 复合标签，无法写回区域名称。")
    names = [r.name.strip() for r in regions]
    if any(not n for n in names):
        raise ValueError("区域名称不能为空。")
    if len(set(names)) != len(names):
        raise ValueError("区域名称不能重复。")
    extracted: list[Any] = []
    for r in regions:
        sk = r.source_key
        if sk not in regions_tag:
            raise ValueError(
                f"找不到区域「{sk}」，文件可能已在外部被修改，请重新打开后再试。"
            )
        extracted.append(regions_tag[sk])
    new_compound = CompoundTag()
    for name, tag in zip(names, extracted):
        new_compound[name] = tag
    root["Regions"] = new_compound


def _load_stage1_metadata(root: Any, path: Path) -> SnbtProperties:
    """阶段一：读取头部元数据（除 Regions 外的所有字段）。"""
    metadata = root.get("Metadata", {})
    enclosing = metadata.get("EnclosingSize", {})

    preview_tag = metadata.get("PreviewImageData")
    preview_data: list[int] = []
    if preview_tag is not None:
        try:
            preview_data = [int(v) for v in preview_tag]
        except Exception:
            preview_data = []

    mc_data_ver = _to_int(metadata.get("MinecraftDataVersion"), 0)
    if mc_data_ver == 0:
        mc_data_ver = _to_int(root.get("MinecraftDataVersion"), 0)

    return SnbtProperties(
        file_path=path,
        file_name=path.name,
        internal_name=_to_str(metadata.get("Name"), ""),
        author=_to_str(metadata.get("Author"), ""),
        description=_to_str(metadata.get("Description"), ""),
        created_unix=_to_int(metadata.get("TimeCreated"), 0),
        modified_unix=_to_int(metadata.get("TimeModified"), 0),
        enclosing_size=(
            _to_int(enclosing.get("x"), 0),
            _to_int(enclosing.get("y"), 0),
            _to_int(enclosing.get("z"), 0),
        ),
        total_blocks=_to_int(metadata.get("TotalBlocks"), 0),
        total_volume=_to_int(metadata.get("TotalVolume"), 0),
        litematic_version=_to_int(root.get("Version"), 0),
        minecraft_data_version=mc_data_ver,
        preview_image_data=preview_data,
        regions=[],
        _metadata_loaded=True,
        _regions_geo_loaded=False,
        _root_tag=None,
    )


def _load_stage2_regions_geo(root: Any, props: SnbtProperties) -> None:
    """阶段二：读取区域几何信息（仅 Position 和 Size）。"""
    props.regions = _parse_regions_list(root)
    props._regions_geo_loaded = True


def load_snbt_properties(file_path: str | Path) -> SnbtProperties:
    """读取 .litematic，抽出 ``Metadata`` 与根级 ``Version`` 等到 ``SnbtProperties``。

    采用分阶段读取策略（逻辑分层）：
    - 阶段一：读取头部元数据（除 Regions 外）
    - 阶段二：读取区域几何信息（Position 和 Size）
    - 阶段三：按需使用 ``_root_tag`` 访问 BlockStates、Entities 等

    注意：``amulet_nbt.load(compressed=True)`` 会一次性解压并解析**整份** NBT（含各区域方块数据），
    大文件的主要耗时在此。若在同进程后台线程中调用，仍可能与 Qt 主线程争夺 CPython GIL 导致整窗假死；
    属性页对大文件改用 ``mp_load_snbt_properties_for_ui`` 在**子进程**中执行本函数。
    """
    _, _, _, _, NamedTag, _, load = _import_amulet_nbt()
    path = Path(file_path)
    nbt: NamedTag = load(str(path), compressed=True)
    root = nbt.tag

    # 阶段一：加载头部元数据
    props = _load_stage1_metadata(root, path)

    # 阶段二：加载区域几何信息
    _load_stage2_regions_geo(root, props)

    # 保留原始 NBT 根标签引用，供阶段三按需加载
    props._root_tag = root

    return props


def mp_load_snbt_properties_for_ui(path_str: str, result_queue: Any) -> None:
    """多进程 ``spawn`` 目标函数（须为本模块顶层，供 Windows pickle）。

    在**独立解释器**中调用 :func:`load_snbt_properties`，再 ``dataclasses.replace`` 去掉 ``_root_tag``
    后 ``pickle`` 写回队列，仅传元数据与区域几何，避免把大块 NBT 树带回主进程。
    """
    import pickle

    try:
        props = load_snbt_properties(Path(path_str))
        props = replace(props, _root_tag=None)
        result_queue.put(("ok", pickle.dumps(props)))
    except Exception as exc:
        try:
            result_queue.put(("err", str(exc)))
        except Exception:
            pass


def save_snbt_properties(data: SnbtProperties, output_path: str | Path | None = None) -> Path:
    """从 ``data.file_path`` 读入完整 NBT，覆写可编辑 Metadata 字段后写入 ``output_path``（默认原路径）。"""
    CompoundTag, IntArrayTag, IntTag, LongTag, NamedTag, StringTag, load = _import_amulet_nbt()
    if data.file_path is None:
        raise ValueError("file_path is empty, cannot save.")
    src = Path(data.file_path)
    dst = Path(output_path) if output_path is not None else src

    nbt: NamedTag = load(str(src), compressed=True)
    root = nbt.tag
    metadata = root.get("Metadata")
    if metadata is None:
        raise ValueError("Invalid litematic: missing Metadata compound.")

    # Name：与 UI「内部名称」一致；缺失时由本调用补写 StringTag
    metadata["Name"] = StringTag(data.internal_name)
    metadata["Author"] = StringTag(data.author)
    metadata["Description"] = StringTag(data.description)
    metadata["PreviewImageData"] = IntArrayTag([_to_int32_signed(v) for v in data.preview_image_data])

    if "TimeModified" in metadata:
        metadata["TimeModified"] = LongTag(int(data.modified_unix))
    else:
        metadata["TimeModified"] = IntTag(int(data.modified_unix))

    _apply_region_renames(root, data.regions, CompoundTag)

    nbt.save_to(str(dst), compressed=True)
    return dst
