"""从 misode/mcmeta 拉取 NBT Viewer 所需游戏资源（blocks / models / atlas / UV）。

供选项页「更新游戏资源」与 ``scripts/fetch_nbt_mcmeta_assets.py`` 共用；不依赖 Node。
"""

from __future__ import annotations

import io
import json
import re
import shutil
import tarfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

MCMETA_RAW = "https://raw.githubusercontent.com/misode/mcmeta"
GITHUB_TARBALL = "https://github.com/misode/mcmeta/tarball"
UNPKG_CODICONS = "https://unpkg.com/vscode-codicons@0.0.14/dist"


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "LitematicaBA-nbt-mcmeta-fetch/1.0"})
    with urllib.request.urlopen(req, timeout=120) as resp:
        return resp.read()


@dataclass(frozen=True)
class McmetaVersionEntry:
    """misode/mcmeta ``versions/data.min.json`` 中单条版本信息（用于 UI 列表）。"""

    id: str
    name: str
    type: str
    stable: bool
    data_version: int


def fetch_mcmeta_version_catalog() -> list[McmetaVersionEntry]:
    """拉取并解析版本目录，按 ``data_version`` 降序（新在前）。"""
    raw = json.loads(_fetch(f"{MCMETA_RAW}/summary/versions/data.min.json").decode("utf-8"))
    out: list[McmetaVersionEntry] = []
    for v in raw:
        if not isinstance(v, dict):
            continue
        vid = v.get("id")
        if not isinstance(vid, str) or not vid:
            continue
        name = v.get("name")
        if not isinstance(name, str):
            name = vid
        typ = v.get("type")
        if typ not in ("release", "snapshot"):
            typ = "snapshot"
        stable = bool(v.get("stable"))
        try:
            dv = int(v.get("data_version", 0))
        except (TypeError, ValueError):
            dv = 0
        out.append(McmetaVersionEntry(id=vid, name=name, type=str(typ), stable=stable, data_version=dv))
    out.sort(key=lambda e: e.data_version, reverse=True)
    return out


def filter_mcmeta_catalog_for_picker(catalog: list[McmetaVersionEntry]) -> list[McmetaVersionEntry]:
    """版本选择列表：仅保留 **data 序号最高的一条**（快照或正式均可）+ **所有稳定版**；其余快照全部排除。"""
    if not catalog:
        return []
    max_dv = max(e.data_version for e in catalog)
    want_ids: set[str] = set()
    for e in catalog:
        if e.data_version == max_dv:
            want_ids.add(e.id)
    for e in catalog:
        if e.stable:
            want_ids.add(e.id)
    picked = [e for e in catalog if e.id in want_ids]
    picked.sort(key=lambda e: e.data_version, reverse=True)
    return picked


def latest_stable_release_id_from_catalog(catalog: list[McmetaVersionEntry]) -> str | None:
    """目录内「最新稳定正式版」的 id（按 data_version 最高）。"""
    best: str | None = None
    best_dv = -1
    for e in catalog:
        if e.type == "release" and e.stable and e.data_version >= best_dv:
            best_dv = e.data_version
            best = e.id
    return best


def is_safe_mcmeta_version_dir_name(version_id: str) -> bool:
    s = (version_id or "").strip()
    if not s or "/" in s or "\\" in s or s in (".", ".."):
        return False
    return True


def remove_versioned_mcmeta_assets(out_base: Path, version_id: str) -> bool:
    """删除 ``out_base/<version_id>/``（若存在且名为安全版本 id）。返回是否删除了目录。"""
    if not is_safe_mcmeta_version_dir_name(version_id):
        return False
    try:
        base = out_base.resolve()
    except OSError:
        return False
    root = (base / version_id.strip()).resolve()
    if not root.is_dir():
        return False
    try:
        root.relative_to(base)
    except ValueError:
        return False
    shutil.rmtree(root, ignore_errors=True)
    return True


def resolve_mcmeta_version(version_spec: str | None) -> tuple[str, str | None]:
    """返回 ``(版本 id, 警告文案)``；指定版本不存在时回退最新稳定版并附带警告。"""
    data = json.loads(_fetch(f"{MCMETA_RAW}/summary/versions/data.min.json").decode("utf-8"))
    warn: str | None = None
    if version_spec and version_spec.strip():
        w = version_spec.strip()
        for v in data:
            if v.get("id") == w:
                return w, None
        warn = f"未找到版本 {w!r}，已改用最新稳定版。"
    for v in data:
        if v.get("type") == "release" and v.get("stable"):
            return str(v["id"]), warn
    return str(data[0]["id"]), warn


def _write_js_const(path: Path, name: str, raw_inner: str) -> None:
    inner = raw_inner.replace("`", "\\`").replace("${", "\\${")
    path.write_text(f"const {name} = `{inner}`\n", encoding="utf-8")


def _assets_from_tarball(version: str) -> str:
    url = f"{GITHUB_TARBALL}/{version}-assets-json"
    buf = io.BytesIO(_fetch(url))
    blockstates: dict[str, object] = {}
    models: dict[str, object] = {}
    re_bs = re.compile(r"/assets/minecraft/blockstates/([a-z0-9/_]+)\.json$")
    re_md = re.compile(r"/assets/minecraft/models/([a-z0-9/_]+)\.json$")
    with tarfile.open(fileobj=buf, mode="r:gz") as tar:
        for m in tar.getmembers():
            if not m.isfile():
                continue
            name = m.name.replace("\\", "/")
            raw = tar.extractfile(m)
            if raw is None:
                continue
            text = raw.read().decode("utf-8")
            mb = re_bs.search(name)
            if mb:
                blockstates[mb.group(1)] = json.loads(text)
                continue
            mm = re_md.search(name)
            if mm:
                models[mm.group(1)] = json.loads(text)
    payload = {"blockstates": blockstates, "models": models}
    return json.dumps(payload, separators=(",", ":"), ensure_ascii=False)


@dataclass
class McmetaFetchResult:
    """``ok`` 为 True 时 ``version_id`` 为实际写入的 MC 版本 id。"""

    ok: bool
    version_id: str
    mcmeta_dir: Path
    message: str
    warning: str | None = None


def run_mcmeta_fetch(
    *,
    out_base: Path,
    version_spec: str | None = None,
    with_ui_extras: bool = False,
    copy_packaged_ui: bool = False,
    repo_root: Path | None = None,
) -> McmetaFetchResult:
    """写入 ``out_base/<版本 id>/mcmeta/`` 下四件套 + ``version.txt``。

    ``version_spec`` 非空时尝试匹配该 id；无效则回退最新稳定版（与旧脚本警告行为一致可后续再加）。
    ``with_ui_extras`` / ``copy_packaged_ui`` 需 ``repo_root`` 或默认可解析的源码树。
    """
    root = repo_root if repo_root is not None else _repo_root()
    out_base = out_base.resolve()
    try:
        ver, ver_warn = resolve_mcmeta_version(version_spec)
        out_mc = out_base / ver / "mcmeta"
        out_mc.mkdir(parents=True, exist_ok=True)

        blocks_url = f"{MCMETA_RAW}/{ver}-summary/blocks/data.min.json"
        atlas_url = f"{MCMETA_RAW}/{ver}-atlas/all/atlas.png"
        uv_url = f"{MCMETA_RAW}/{ver}-atlas/all/data.min.json"

        blocks_text = _fetch(blocks_url).decode("utf-8")
        _write_js_const(out_mc / "blocks.js", "stringifiedBlocks", blocks_text)

        uv_text = _fetch(uv_url).decode("utf-8")
        _write_js_const(out_mc / "uvmapping.js", "stringifiedUvmapping", uv_text)

        assets_inner = _assets_from_tarball(ver)
        _write_js_const(out_mc / "assets.js", "stringifiedAssets", assets_inner)

        (out_mc / "atlas.png").write_bytes(_fetch(atlas_url))
        (out_mc / "version.txt").write_text(ver + "\n", encoding="utf-8")

        notes: list[str] = []

        if with_ui_extras:
            cod_css = _fetch(f"{UNPKG_CODICONS}/codicon.css").decode("utf-8")
            (out_base / "codicon.css").write_text(cod_css, encoding="utf-8")
            (out_base / "codicon.ttf").write_bytes(_fetch(f"{UNPKG_CODICONS}/codicon.ttf"))
            editor_css = root / "third_party" / "vscode-nbt" / "res" / "editor.css"
            if editor_css.is_file():
                (out_base / "editor.css").write_bytes(editor_css.read_bytes())
            else:
                notes.append("未写入 editor.css（缺少 third_party/vscode-nbt）")

        if copy_packaged_ui:
            packaged = root / "src" / "litematicaba" / "resources" / "web" / "nbt-viewer"
            for name in ("index.html", "editor.js"):
                src = packaged / name
                if src.is_file():
                    shutil.copy2(src, out_base / name)
                else:
                    notes.append(f"未复制 {name}")

        msg = f"已写入 mcmeta，版本 {ver}。"
        if notes:
            msg += " " + "；".join(notes)
        return McmetaFetchResult(True, ver, out_mc, msg, warning=ver_warn)
    except Exception as exc:
        return McmetaFetchResult(False, "", out_base / "mcmeta", str(exc), warning=None)
