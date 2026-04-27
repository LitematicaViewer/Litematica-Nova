from __future__ import annotations

import itertools
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from litematicaba.core.config import project_root
from litematicaba.core.layer_pack import (
    cleanup_layer_pack_temp,
    layer_pack_path,
    read_layer_pack_layer,
    read_layer_pack_meta,
)


_CREATE_NO_WINDOW = 0x08000000 if os.name == "nt" else 0
_BELOW_NORMAL_PRIORITY_CLASS = 0x00004000 if os.name == "nt" else 0
_CHUNK_SIZE = 32
_CACHE_MAGIC_V2 = b"LPNC2\0\0\0"
_CACHE_MAGIC_V3 = b"LPNC3\0\0\0"
RENDER_BUILD_MODE_NORMAL = "normal"
RENDER_BUILD_MODE_FAST_EXPERIMENTAL = "fast_experimental"
RENDER_BUILD_MODE_FULL = "full"
VALID_RENDER_BUILD_MODES = (
    RENDER_BUILD_MODE_NORMAL,
    RENDER_BUILD_MODE_FAST_EXPERIMENTAL,
    RENDER_BUILD_MODE_FULL,
)
_FULL_MODE_COLOR_CACHE_SUFFIX = ".full_mode_colors.json"
_FULL_MODE_MATERIAL_CACHE_SUFFIX = ".full_mode_materials.json"
_FULL_MODE_MATERIAL_ATLAS_SUFFIX = ".full_mode_materials.png"
_FULL_MODE_MATERIAL_CACHE_FORMAT = "lba_full_mode_material_cache_v2"


@dataclass(slots=True)
class AnalyzeResult:
    file_path: str
    output: dict[str, Any]


@dataclass(slots=True)
class LoadAnalyzeResult:
    file_path: str
    output: dict[str, Any]
    snbt: dict[str, Any]


@dataclass(slots=True)
class ViewerLaunch:
    launch_id: int
    file_path: str
    progress_file: str
    cache_file: str


@dataclass(slots=True)
class LayerPrecacheLaunch:
    launch_id: int
    cache_file: str
    status_file: str


@dataclass(slots=True)
class ViewerProgress:
    ready: bool
    total_chunks: int
    built_chunks: int
    renderable_chunks: int | None
    empty_mesh_chunks: int | None
    uploaded_chunks: int
    resident_chunks: int
    cache_file_bytes: int | None
    percent: float
    phase: str
    elapsed_ms: int
    eta_seconds: int | None
    scene_size_x: int | None
    scene_size_y: int | None
    scene_size_z: int | None


@dataclass(slots=True)
class ViewerProgressSnapshot:
    launch_id: int
    running: bool
    progress: ViewerProgress | None


@dataclass(slots=True)
class NativeRenderDiagnostics:
    target: str
    renderer_path: str
    cache_path: str
    cache_exists: bool
    model_chunks: int
    mesh_count: int
    vertex_count: int
    index_count: int
    has_vertex_colors: bool
    vertex_color_bytes: int
    vertex_color_count: int
    sample_block_id: str
    sample_color_raw: str
    sample_color_after_normalize: str
    color_source: str
    shading_mode: str
    palette_hits: int
    fallback_hits: int
    vertex_color_attribute_enabled: bool
    default_gray_fallback: bool


@dataclass(slots=True)
class _BuildRecord:
    process: subprocess.Popen[str]
    progress_file: Path
    cache_file: Path
    file_path: Path


@dataclass(slots=True)
class _LayerPrecacheRecord:
    process: subprocess.Popen[str]
    cache_file: Path
    status_file: Path


_build_lock = threading.Lock()
_build_records: dict[int, _BuildRecord] = {}
_layer_precache_records: dict[int, _LayerPrecacheRecord] = {}
_next_launch_id = itertools.count(1)
_next_layer_precache_id = itertools.count(1)
_child_process_lock = threading.Lock()
_child_processes: set[subprocess.Popen[str]] = set()


class BackendUnavailableError(RuntimeError):
    pass


def _register_child_process(process: subprocess.Popen[str]) -> subprocess.Popen[str]:
    with _child_process_lock:
        _child_processes.add(process)
    return process


def _discard_child_process(process: subprocess.Popen[str] | None) -> None:
    if process is None:
        return
    with _child_process_lock:
        _child_processes.discard(process)


def _kill_process_tree(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=_CREATE_NO_WINDOW,
            check=False,
        )
        return
    process.kill()


def cleanup_backend_processes() -> None:
    with _build_lock:
        launch_ids = list(_build_records.keys())
    for launch_id in launch_ids:
        try:
            cancel_cache_build(launch_id)
        except Exception:
            pass

    with _child_process_lock:
        processes = list(_child_processes)
        _child_processes.clear()
    with _build_lock:
        layer_records = list(_layer_precache_records.values())
        _layer_precache_records.clear()
    processes.extend(record.process for record in layer_records)
    for process in processes:
        try:
            if process.poll() is None:
                _kill_process_tree(process)
        except Exception:
            pass
    for record in layer_records:
        _cleanup_layer_precache_runtime_files(record.cache_file)
        try:
            record.status_file.unlink(missing_ok=True)
        except Exception:
            pass


def _spawn_managed_process(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
) -> subprocess.Popen[str]:
    return _register_child_process(
        subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
            cwd=str(cwd or project_root()),
            env=env,
            creationflags=_CREATE_NO_WINDOW,
        )
    )


def backend_repo_root() -> Path:
    candidates: list[Path] = []
    env_root = os.environ.get("LITEMATICA_VIEWER_BACKEND_ROOT", "").strip()
    if env_root:
        candidates.append(Path(env_root))
    project = project_root()
    candidates.extend(
        [
            project.parent / "Litematica-viewer",
            project / "vendor" / "Litematica-viewer",
        ]
    )
    for candidate in candidates:
        if (candidate / "core").is_dir() and (candidate / "desktop-ui").is_dir():
            return candidate.resolve()
    searched = "\n".join(str(path) for path in candidates)
    raise BackendUnavailableError(
        "鏈壘鍒扮幇鏈夊悗绔粨搴?Litematica-viewer銆俓n"
        "鍙€氳繃鐜鍙橀噺 LITEMATICA_VIEWER_BACKEND_ROOT 鎸囧悜瀹冪殑鐩綍銆俓n"
        f"宸叉鏌ワ細\n{searched}"
    )


def _resolve_binary(name: str) -> Path:
    filename = f"{name}.exe" if os.name == "nt" else name
    project = project_root()
    candidates = []
    if name == "litematica_core":
        release_filename = "litematica_core_release.exe" if os.name == "nt" else "litematica_core_release"
        candidates.append(project / "bin" / "viewer-backend" / release_filename)
    if name == "litematica_native_viewer":
        release_filenames = (
            [
                "litematica_native_viewer.exe",
                "litematica_native_viewer_runtimefix.exe",
                "litematica_native_viewer_viewportfix.exe",
                "litematica_native_viewer_pivotfix.exe",
                "litematica_native_viewer_release.exe",
            ]
            if os.name == "nt"
            else [
                "litematica_native_viewer",
                "litematica_native_viewer_runtimefix",
                "litematica_native_viewer_viewportfix",
                "litematica_native_viewer_pivotfix",
                "litematica_native_viewer_release",
            ]
        )
        candidates.extend(project / "bin" / "viewer-backend" / filename for filename in release_filenames)
    candidates.append(project / "bin" / "viewer-backend" / filename)
    try:
        repo = backend_repo_root()
    except BackendUnavailableError:
        repo = None
    if repo is not None:
        candidates.extend(
            [
                repo / "core" / "target" / "debug" / filename,
                repo / "core" / "target" / "release" / filename,
                repo / "desktop-ui" / "tauri" / "target" / "debug" / filename,
                repo / "desktop-ui" / "tauri" / "target" / "release" / filename,
                repo / "desktop-ui" / "tauri" / "binaries" / filename,
                repo / "desktop-ui" / "src-tauri" / "target" / "debug" / filename,
                repo / "desktop-ui" / "src-tauri" / "target" / "release" / filename,
                repo / "desktop-ui" / "src-tauri" / "binaries" / filename,
            ]
        )
    for candidate in candidates:
        if candidate.is_file():
            return candidate.resolve()
    checked = "\n".join(str(path) for path in candidates)
    raise BackendUnavailableError(f"鏈壘鍒板彲鎵ц鏂囦欢 {name}銆俓n宸叉鏌ワ細\n{checked}")


def ensure_backend_ready() -> None:
    _resolve_binary("litematica_core")
    _resolve_binary("litematica_native_viewer")


def _run_json_command(binary: Path, args: list[str]) -> dict[str, Any]:
    command = [str(binary), *args]
    completed = subprocess.run(
        command,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        creationflags=_CREATE_NO_WINDOW,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        raise RuntimeError(f"{binary.name} 执行失败：{detail}")
    text = completed.stdout.strip()
    if not text:
        raise RuntimeError(f"{binary.name} 未返回 JSON 输出")
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"{binary.name} 返回了无法解析的 JSON：{exc}") from exc


def _analyze_litematic_direct(file_path: str | Path, *, include_entities: bool = True) -> AnalyzeResult:
    binary = _resolve_binary("litematica_core")
    resolved = Path(file_path).resolve()
    args = ["analyze", str(resolved)]
    if include_entities:
        args.append("--include-entities")
    payload = _run_json_command(binary, args)
    return AnalyzeResult(file_path=str(resolved), output=payload)


def get_cache_layer_meta(cache_file: str | Path) -> dict[str, Any]:
    binary = _resolve_binary("litematica_core")
    resolved = Path(cache_file).resolve()
    mark_layer_precache_activity(resolved)
    pack = layer_pack_path(resolved)
    if pack.is_file():
        try:
            return read_layer_pack_meta(resolved)
        except Exception:
            pass
    precached = layer_precache_dir(resolved) / "meta.json"
    if precached.is_file():
        try:
            return json.loads(precached.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            pass
    return _run_json_command(binary, ["cache-layer-meta", str(resolved)])


def get_cache_layer(cache_file: str | Path, y: int) -> dict[str, Any]:
    binary = _resolve_binary("litematica_core")
    resolved = Path(cache_file).resolve()
    mark_layer_precache_activity(resolved)
    pack = layer_pack_path(resolved)
    if pack.is_file():
        try:
            return read_layer_pack_layer(resolved, int(y))
        except Exception:
            pass
    precached = layer_precache_dir(resolved) / f"layer_{int(y)}.json"
    if precached.is_file():
        try:
            return json.loads(precached.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            pass
    return _run_json_command(binary, ["cache-layer", str(resolved), f"--y={int(y)}"])


def layer_precache_dir(cache_file: str | Path) -> Path:
    cache = Path(cache_file).resolve()
    return cache.with_name(f"{cache.name}.layer-precache")


def layer_precache_activity_file(cache_file: str | Path) -> Path:
    return layer_precache_dir(cache_file) / "viewing-active.flag"


def _cleanup_layer_precache_runtime_files(cache_file: str | Path) -> None:
    cleanup_layer_pack_temp(cache_file)
    for path in (
        layer_precache_activity_file(cache_file),
        layer_precache_dir(cache_file) / "status.json",
    ):
        try:
            path.unlink(missing_ok=True)
        except Exception:
            pass


def mark_layer_precache_activity(cache_file: str | Path) -> None:
    try:
        marker = layer_precache_activity_file(cache_file)
        marker.parent.mkdir(parents=True, exist_ok=True)
        marker.write_text(str(time.time()), encoding="utf-8")
    except Exception:
        pass


def _mark_layer_precache_activity_while_running(cache_file: Path, process: subprocess.Popen[str]) -> None:
    def run() -> None:
        while process.poll() is None:
            mark_layer_precache_activity(cache_file)
            time.sleep(1.0)

    threading.Thread(target=run, name="lba-layer-precache-yield-marker", daemon=True).start()


def start_layer_precache(cache_file: str | Path, *, timeout_seconds: int = 600) -> LayerPrecacheLaunch:
    resolved = Path(cache_file).resolve()
    if not resolved.is_file():
        raise RuntimeError(f"cache 鏂囦欢涓嶅瓨鍦紝鏃犳硶棰勭敓鎴愬垎灞傦細{resolved}")
    _cleanup_layer_precache_runtime_files(resolved)
    status_file = _temp_json_path("lba_layer_precache")
    activity_file = layer_precache_activity_file(resolved)
    env = dict(os.environ)
    env["PYTHONPATH"] = str(project_root() / "src")
    process = _register_child_process(
        subprocess.Popen(
            [
                sys.executable,
                "-m",
                "litematicaba.core.layer_precache_worker",
                str(resolved),
                str(status_file),
                str(int(timeout_seconds)),
                str(activity_file),
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
            cwd=str(project_root()),
            env=env,
            creationflags=_CREATE_NO_WINDOW | _BELOW_NORMAL_PRIORITY_CLASS,
        )
    )
    with _build_lock:
        launch_id = next(_next_layer_precache_id)
        _layer_precache_records[launch_id] = _LayerPrecacheRecord(
            process=process,
            cache_file=resolved,
            status_file=status_file,
        )
    return LayerPrecacheLaunch(launch_id=launch_id, cache_file=str(resolved), status_file=str(status_file))


def poll_layer_precache(launch_id: int) -> dict[str, Any]:
    with _build_lock:
        record = _layer_precache_records.get(int(launch_id))
    if record is None:
        return {"running": False, "phase": "missing", "ready": False}
    running = record.process.poll() is None
    payload: dict[str, Any] = {}
    if record.status_file.is_file():
        try:
            raw = json.loads(record.status_file.read_text(encoding="utf-8"))
            if isinstance(raw, dict):
                payload = raw
        except (OSError, json.JSONDecodeError):
            payload = {}
    payload["running"] = running
    payload["returncode"] = record.process.poll()
    if not running:
        with _build_lock:
            _layer_precache_records.pop(int(launch_id), None)
        _discard_child_process(record.process)
        _cleanup_layer_precache_runtime_files(record.cache_file)
        try:
            record.status_file.unlink(missing_ok=True)
        except Exception:
            pass
    return payload


def cancel_layer_precache(launch_id: int) -> None:
    with _build_lock:
        record = _layer_precache_records.pop(int(launch_id), None)
    if record is None:
        return
    try:
        if record.process.poll() is None:
            _kill_process_tree(record.process)
    finally:
        _discard_child_process(record.process)
        _cleanup_layer_precache_runtime_files(record.cache_file)
        try:
            record.status_file.unlink(missing_ok=True)
        except Exception:
            pass


def _run_backend_worker_json(args: list[str]) -> dict[str, Any]:
    env = dict(os.environ)
    env["LBA_BACKEND_WORKER"] = "1"
    env["PYTHONPATH"] = str(project_root() / "src")
    completed = subprocess.run(
        [sys.executable, "-m", "litematicaba.core.viewer_backend_worker", *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=env,
        creationflags=_CREATE_NO_WINDOW,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown backend worker error"
        raise RuntimeError(f"viewer backend worker 执行失败：{detail}")
    text = completed.stdout.strip()
    if not text:
        raise RuntimeError("viewer backend worker 未返回 JSON")
    return json.loads(text)


def _can_spawn_python_module_worker() -> bool:
    exe_name = Path(sys.executable).name.lower()
    return exe_name in {"python.exe", "pythonw.exe", "python"} or exe_name.startswith("python")


def analyze_litematic(file_path: str | Path, *, include_entities: bool = True) -> AnalyzeResult:
    if os.environ.get("LBA_BACKEND_WORKER") == "1" or not _can_spawn_python_module_worker():
        return _analyze_litematic_direct(file_path, include_entities=include_entities)
    resolved = Path(file_path).resolve()
    args = ["analyze", str(resolved)]
    if include_entities:
        args.append("--include-entities")
    payload = _run_backend_worker_json(args)
    return AnalyzeResult(file_path=str(resolved), output=payload["output"])


def load_and_analyze_litematic(
    file_path: str | Path,
    *,
    include_entities: bool = True,
) -> LoadAnalyzeResult:
    resolved = Path(file_path).resolve()
    if not _can_spawn_python_module_worker():
        result = _analyze_litematic_direct(resolved, include_entities=include_entities)
        from litematicaba.core.snbt_properties import load_snbt_properties, snbt_properties_to_dict

        snbt = load_snbt_properties(resolved)
        return LoadAnalyzeResult(
            file_path=str(resolved),
            output=result.output,
            snbt=snbt_properties_to_dict(snbt),
        )
    args = ["load-and-analyze", str(resolved)]
    if include_entities:
        args.append("--include-entities")
    payload = _run_backend_worker_json(args)
    return LoadAnalyzeResult(
        file_path=str(resolved),
        output=payload["output"],
        snbt=payload["snbt"],
    )


def material_counts_from_analysis(file_path: str | Path, *, include_entities: bool = False) -> dict[str, int]:
    result = analyze_litematic(file_path, include_entities=include_entities)
    payload = result.output
    counts: dict[str, int] = {}
    derived = payload.get("derived", {}) if isinstance(payload, dict) else {}
    for row in list(derived.get("material_counts", []) or []):
        if isinstance(row, dict) and row.get("key") is not None:
            counts[str(row["key"])] = int(row.get("count", 0) or 0)
    if include_entities:
        analysis = payload.get("analysis", {}) if isinstance(payload, dict) else {}
        for key, value in (analysis.get("entity_counts", {}) or {}).items():
            counts[f"E/{key}"] = int(value or 0)
    return counts


def _temp_json_path(prefix: str) -> Path:
    stamp = int(time.time() * 1000)
    return Path(tempfile.gettempdir()) / f"{prefix}_{os.getpid()}_{stamp}.json"


def build_mode_env(build_mode: str) -> dict[str, str]:
    mode = str(build_mode or RENDER_BUILD_MODE_NORMAL).strip().lower()
    if mode not in VALID_RENDER_BUILD_MODES:
        mode = RENDER_BUILD_MODE_NORMAL
    env = dict(os.environ)
    env["LBA_VIEWER_BUILD_MODE"] = mode
    if mode in (RENDER_BUILD_MODE_NORMAL, RENDER_BUILD_MODE_FULL):
        for key in (
            "LBA_ENABLE_COMPACT_CACHE_V2",
            "LBA_ENABLE_FAST_PATH_NON_OCCLUDING",
            "LBA_ENABLE_FAST_PATH_HALF_SLAB",
            "LBA_ENABLE_FAST_PATH_CARPET",
            "LBA_ENABLE_FAST_PATH_STAIR_HALF",
        ):
            env.pop(key, None)
    elif mode == RENDER_BUILD_MODE_FAST_EXPERIMENTAL:
        env["LBA_ENABLE_COMPACT_CACHE_V2"] = "1"
        env["LBA_ENABLE_FAST_PATH_NON_OCCLUDING"] = "1"
        env["LBA_ENABLE_FAST_PATH_HALF_SLAB"] = "1"
        env["LBA_ENABLE_FAST_PATH_CARPET"] = "1"
        env["LBA_ENABLE_FAST_PATH_STAIR_HALF"] = "1"
    return env


def _state_color_key(block_id: str, properties: dict[str, str]) -> str:
    if not properties:
        return block_id
    pairs = ",".join(f"{key}={value}" for key, value in sorted(properties.items()))
    return f"{block_id}[{pairs}]"


def _full_mode_color_cache_path(cache_file: Path) -> Path:
    return cache_file.with_name(f"{cache_file.name}{_FULL_MODE_COLOR_CACHE_SUFFIX}")


def _full_mode_material_cache_path(cache_file: Path) -> Path:
    return cache_file.with_name(f"{cache_file.name}{_FULL_MODE_MATERIAL_CACHE_SUFFIX}")


def _full_mode_material_atlas_path(cache_file: Path) -> Path:
    return cache_file.with_name(f"{cache_file.name}{_FULL_MODE_MATERIAL_ATLAS_SUFFIX}")


def _layer_index_path_from_cache(cache_file: Path) -> Path:
    try:
        manifest = json.loads(cache_file.read_text(encoding="utf-8"))
    except Exception as exc:
        raise RuntimeError(f"完整模式无法读取 3D cache manifest：{exc}") from exc
    layer_index_file = manifest.get("layer_index_file") if isinstance(manifest, dict) else None
    if not layer_index_file:
        raise RuntimeError("完整模式需要 3D cache 内的分层索引；请先重建 3D cache。")
    layer_path = (cache_file.parent / str(layer_index_file)).resolve()
    if not layer_path.is_file():
        raise RuntimeError(f"完整模式所需的分层索引不存在：{layer_path}")
    return layer_path


def _prepare_full_mode_color_cache(cache_file: Path) -> Path:
    """Build a per-state texture-derived color map for the native full display path.

    The 3D cache itself keeps the existing protocol. Complete mode reads the
    cache sidecar palette, resolves each state through the MC resource chain,
    then lets the native viewer use the resulting texture-derived colors while
    streaming the same model through its normal renderer.
    """

    layer_path = _layer_index_path_from_cache(cache_file)
    output_path = _full_mode_color_cache_path(cache_file)
    try:
        if output_path.is_file() and output_path.stat().st_mtime >= layer_path.stat().st_mtime:
            return output_path
    except OSError:
        pass

    try:
        payload = json.loads(layer_path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise RuntimeError(f"完整模式无法读取分层索引：{exc}") from exc
    visual = payload.get("visual") if isinstance(payload, dict) else {}
    palette = visual.get("palette") if isinstance(visual, dict) else []
    property_pool = visual.get("property_pool") if isinstance(visual, dict) else []
    if not isinstance(palette, list) or not isinstance(property_pool, list):
        raise RuntimeError("完整模式需要 cache 分层索引里的 palette/property_pool 数据；请重建 3D cache。")

    try:
        from litematicaba.ui.minecraft_top_sprite import (
            minecraft_top_view_average_color_for_block,
            minecraft_top_view_resources_available,
        )
    except Exception as exc:
        raise RuntimeError(f"完整模式资源解析器不可用：{exc}") from exc
    if not minecraft_top_view_resources_available():
        raise RuntimeError(r"完整模式找不到 vanilla 主资源：E:\.minecraft\versions\26.1\26.1.jar")

    fallback_colors = _load_viewer_color_cache()
    color_map: dict[str, list[float]] = {}
    sprite_hits = 0
    fallback_hits = 0
    for row in palette:
        if not isinstance(row, dict):
            continue
        block_id = str(row.get("block_id", "") or "")
        if not block_id:
            continue
        property_id = int(row.get("property_id", 0) or 0)
        props_raw = property_pool[property_id] if 0 <= property_id < len(property_pool) else {}
        properties = {str(k): str(v) for k, v in dict(props_raw or {}).items()}
        rgb = minecraft_top_view_average_color_for_block(block_id, properties)
        if rgb is None:
            stripped = block_id.split("[", 1)[0]
            normalized = stripped.rsplit(":", 1)[-1]
            fallback = fallback_colors.get(stripped) or fallback_colors.get(normalized)
            if fallback is None:
                continue
            rgb = (float(fallback[0]), float(fallback[1]), float(fallback[2]))
            fallback_hits += 1
        else:
            sprite_hits += 1
        packed = [round(float(channel), 6) for channel in rgb]
        color_map[_state_color_key(block_id, properties)] = packed
        color_map.setdefault(block_id, packed)
        color_map.setdefault(block_id.rsplit(":", 1)[-1], packed)

    if not color_map:
        raise RuntimeError("完整模式没有解析到任何可用的贴图派生色。")
    output = {
        "format": "lba_full_mode_texture_color_cache_v1",
        "source": "26.1.jar blockstate/model + Faithful texture overlay + block-26.1 fallback",
        "sprite_hits": sprite_hits,
        "fallback_hits": fallback_hits,
        "colors": color_map,
    }
    tmp_path = output_path.with_suffix(f"{output_path.suffix}.tmp")
    tmp_path.write_text(json.dumps(output, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    tmp_path.replace(output_path)
    print(
        "[LBA_FULL_MODE] texture_color_cache_ready "
        f"path={output_path} entries={len(color_map)} sprite_hits={sprite_hits} fallback_hits={fallback_hits}"
    )
    return output_path


def _prepare_full_mode_material_cache(cache_file: Path, source_litematic_path: Path | None = None) -> Path:
    layer_path = _layer_index_path_from_cache(cache_file)
    output_path = _full_mode_material_cache_path(cache_file)
    atlas_path = _full_mode_material_atlas_path(cache_file)
    try:
        output_fresh = output_path.is_file() and output_path.stat().st_mtime >= layer_path.stat().st_mtime
        atlas_fresh = atlas_path.is_file() and atlas_path.stat().st_mtime >= layer_path.stat().st_mtime
        if output_fresh and atlas_fresh and _full_mode_material_cache_is_current(output_path, atlas_path, source_litematic_path):
            return output_path
    except OSError:
        pass

    try:
        from litematicaba.core.full_mode_material_cache import build_full_mode_material_cache
    except Exception as exc:
        raise RuntimeError(f"完整模式材质 cache 构建器不可用：{exc}") from exc

    try:
        built_path = build_full_mode_material_cache(
            cache_file,
            layer_path,
            output_path,
            atlas_path,
            source_litematic_path,
        )
    except Exception as exc:
        raise RuntimeError(f"完整模式无法构建真实面贴图材质 cache：{exc}") from exc

    payload = {}
    try:
        payload = json.loads(output_path.read_text(encoding="utf-8"))
    except Exception:
        payload = {}
    stats = payload.get("stats") if isinstance(payload, dict) else {}
    print(
        "[LBA_FULL_MODE] material_cache_ready "
        f"path={built_path} atlas={atlas_path} palette_entries={int((stats or {}).get('palette_entries', 0) or 0)} "
        f"material_slots={int((stats or {}).get('material_slots', 0) or 0)} "
        f"fallback_palette_entries={int((stats or {}).get('fallback_palette_entries', 0) or 0)}"
    )
    return built_path


def _full_mode_material_cache_is_current(output_path: Path, atlas_path: Path, source_litematic_path: Path | None = None) -> bool:
    try:
        payload = json.loads(output_path.read_text(encoding="utf-8"))
    except Exception:
        return False
    if not isinstance(payload, dict):
        return False
    if payload.get("format") != _FULL_MODE_MATERIAL_CACHE_FORMAT:
        print(
            "[LBA_FULL_MODE] material_cache_rebuild_required "
            f"path={output_path} reason=format actual={payload.get('format')!r} expected={_FULL_MODE_MATERIAL_CACHE_FORMAT}"
        )
        return False
    if payload.get("atlas_file") != atlas_path.name:
        return False
    if source_litematic_path is not None and payload.get("source_litematic_path") != str(source_litematic_path):
        return False
    if not isinstance(payload.get("materials"), list) or not payload.get("materials"):
        return False
    if not isinstance(payload.get("palette_materials"), list) or not payload.get("palette_materials"):
        return False
    if int(payload.get("geometry_version", 0) or 0) < 4:
        return False
    if not isinstance(payload.get("palette_model_quads"), list) or not payload.get("palette_model_quads"):
        return False
    return True


def start_cache_build(file_path: str | Path, *, build_mode: str = RENDER_BUILD_MODE_NORMAL) -> ViewerLaunch:
    binary = _resolve_binary("litematica_native_viewer")
    resolved = Path(file_path).resolve()
    progress_file = _temp_json_path("lba_native_progress")
    cache_file = _temp_json_path("lba_native_cache")
    command = [
        str(binary),
        str(resolved),
        f"--chunk-size={_CHUNK_SIZE}",
        "--prebuild-before-show",
        "--prebuild-only",
        f"--ready-file={progress_file}",
        f"--cache-file={cache_file}",
    ]
    process = _spawn_managed_process(command, env=build_mode_env(build_mode))
    with _build_lock:
        launch_id = next(_next_launch_id)
        _build_records[launch_id] = _BuildRecord(
            process=process,
            progress_file=progress_file,
            cache_file=cache_file,
            file_path=resolved,
        )
    return ViewerLaunch(
        launch_id=launch_id,
        file_path=str(resolved),
        progress_file=str(progress_file),
        cache_file=str(cache_file),
    )


def poll_cache_build(launch_id: int) -> ViewerProgressSnapshot:
    with _build_lock:
        record = _build_records.get(int(launch_id))
    if record is None:
        raise RuntimeError("鎵句笉鍒板搴旂殑 3D cache 鏋勫缓浠诲姟")

    running = record.process.poll() is None
    progress = None
    if record.progress_file.is_file():
        try:
            payload = json.loads(record.progress_file.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            payload = None
        if isinstance(payload, dict):
            progress = ViewerProgress(
                ready=bool(payload.get("ready", False)),
                total_chunks=int(payload.get("total_chunks", 0) or 0),
                built_chunks=int(payload.get("built_chunks", 0) or 0),
                renderable_chunks=_opt_int(payload.get("renderable_chunks")),
                empty_mesh_chunks=_opt_int(payload.get("empty_mesh_chunks")),
                uploaded_chunks=int(payload.get("uploaded_chunks", 0) or 0),
                resident_chunks=int(payload.get("resident_chunks", 0) or 0),
                cache_file_bytes=_opt_int(payload.get("cache_file_bytes")),
                percent=float(payload.get("percent", 0.0) or 0.0),
                phase=str(payload.get("phase", "") or ""),
                elapsed_ms=int(payload.get("elapsed_ms", 0) or 0),
                eta_seconds=_opt_int(payload.get("eta_seconds")),
                scene_size_x=_opt_int(payload.get("scene_size_x")),
                scene_size_y=_opt_int(payload.get("scene_size_y")),
                scene_size_z=_opt_int(payload.get("scene_size_z")),
            )

    if not running and progress is not None and progress.ready:
        with _build_lock:
            _build_records.pop(int(launch_id), None)
        _discard_child_process(record.process)

    return ViewerProgressSnapshot(
        launch_id=int(launch_id),
        running=running,
        progress=progress,
    )


def cancel_cache_build(launch_id: int) -> None:
    with _build_lock:
        record = _build_records.pop(int(launch_id), None)
    if record is None:
        return
    if record.process.poll() is None:
        _kill_process_tree(record.process)
        record.process.wait(timeout=5)
    _discard_child_process(record.process)


def probe_native_render_cache(
    file_path: str | Path,
    cache_file: str | Path,
    *,
    target: str,
) -> NativeRenderDiagnostics:
    binary = _resolve_binary("litematica_native_viewer")
    resolved_file = Path(file_path).resolve()
    resolved_cache = Path(cache_file).resolve()
    manifest: dict[str, Any] = {}
    chunks: list[dict[str, Any]] = []
    vertex_count = 0
    index_count = 0
    has_vertex_colors = False
    vertex_color_bytes = 0
    vertex_color_count = 0
    sample_color_raw = "none"
    color_source = "cache_vertex_color"
    shading_mode = "vertex_color_flat"
    palette_hits = 0
    fallback_hits = 0
    sample_block_id, sample_color_after_normalize, sample_palette_hit = _sample_block_color(resolved_file)
    palette_hits += int(sample_palette_hit)
    fallback_hits += int(bool(sample_block_id) and not sample_palette_hit)

    if resolved_cache.is_file():
        try:
            manifest = json.loads(resolved_cache.read_text(encoding="utf-8"))
            chunks = [item for item in manifest.get("chunks", []) if isinstance(item, dict)]
            color_chain = str(manifest.get("color_chain", "") or "")
            color_source = f"prebuild_cache:{color_chain or 'unknown'}"
            for item in chunks:
                vertex_count += int(item.get("vertex_count", 0) or 0)
                index_count += int(item.get("index_count", 0) or 0)
            color_probe = _probe_cache_chunk_colors(resolved_cache, manifest, chunks)
            has_vertex_colors = bool(color_probe["has_vertex_colors"])
            vertex_color_bytes = int(color_probe["vertex_color_bytes"])
            vertex_color_count = int(color_probe["vertex_color_count"])
            sample_color_raw = str(color_probe["sample_color_raw"])
            fallback_hits += int(not has_vertex_colors)
        except Exception as exc:
            color_source = f"cache_probe_failed:{exc}"
            shading_mode = "unknown"
            fallback_hits += 1
    else:
        color_source = "cache_missing"
        shading_mode = "none"
        fallback_hits += 1

    diagnostics = NativeRenderDiagnostics(
        target=target,
        renderer_path="litematica_native_viewer.cache_input",
        cache_path=str(resolved_cache),
        cache_exists=resolved_cache.is_file(),
        model_chunks=len(chunks),
        mesh_count=sum(1 for item in chunks if int(item.get("index_count", 0) or 0) > 0),
        vertex_count=vertex_count,
        index_count=index_count,
        has_vertex_colors=has_vertex_colors,
        vertex_color_bytes=vertex_color_bytes,
        vertex_color_count=vertex_color_count,
        sample_block_id=sample_block_id,
        sample_color_raw=sample_color_raw,
        sample_color_after_normalize=sample_color_after_normalize,
        color_source=color_source,
        shading_mode=shading_mode,
        palette_hits=palette_hits,
        fallback_hits=fallback_hits,
        vertex_color_attribute_enabled=True,
        default_gray_fallback=fallback_hits > 0 or not has_vertex_colors,
    )
    return diagnostics


def _probe_cache_chunk_colors(
    manifest_path: Path,
    manifest: dict[str, Any],
    chunks: list[dict[str, Any]],
) -> dict[str, Any]:
    chunk_dir = manifest_path.parent / str(manifest.get("chunk_data_dir", ""))
    total_color_count = sum(int(item.get("vertex_count", 0) or 0) for item in chunks)
    total_color_bytes = sum((int(item.get("vertex_count", 0) or 0) + 3) // 4 * 3 for item in chunks)
    sample_color_raw = "none"
    for item in chunks[:8]:
        chunk_name = item.get("file")
        if not chunk_name:
            continue
        chunk_path = chunk_dir / str(chunk_name)
        if not chunk_path.is_file():
            continue
        if chunk_path.suffix.lower() == ".bin":
            data = chunk_path.read_bytes()[:32]
            if len(data) < 24:
                continue
            magic = data[: len(_CACHE_MAGIC_V2)]
            if magic == _CACHE_MAGIC_V2:
                vertex_count = int.from_bytes(data[20:24], "little", signed=False)
                total_color_count = max(total_color_count, vertex_count)
                total_color_bytes = max(total_color_bytes, vertex_count * 12)
                sample_color_raw = "v2:f32_vertex_colors"
                continue
            if magic == _CACHE_MAGIC_V3 and len(data) >= 32:
                vertex_count = int.from_bytes(data[20:24], "little", signed=False)
                face_color_count = int.from_bytes(data[28:32], "little", signed=False)
                if face_color_count > 0:
                    total_color_count = max(total_color_count, vertex_count)
                    total_color_bytes = max(total_color_bytes, face_color_count * 3)
                    color_offset = 32 + vertex_count * 12
                    full_data = chunk_path.read_bytes()
                    if len(full_data) >= color_offset + 3 and sample_color_raw == "none":
                        raw = list(full_data[color_offset : color_offset + 3])
                        sample_color_raw = f"rgb8({raw[0]},{raw[1]},{raw[2]})"
                    continue
        else:
            try:
                payload = json.loads(chunk_path.read_text(encoding="utf-8"))
            except Exception:
                continue
            colors = payload.get("colors") or []
            if colors:
                total_color_count += len(colors)
                total_color_bytes += len(colors) * 12
                if sample_color_raw == "none":
                    sample_color_raw = str(colors[0])
    return {
        "has_vertex_colors": total_color_count > 0,
        "vertex_color_bytes": total_color_bytes,
        "vertex_color_count": total_color_count,
        "sample_color_raw": sample_color_raw,
    }


def _sample_block_color(file_path: Path) -> tuple[str, str, bool]:
    sample_block_id = ""
    try:
        counts = material_counts_from_analysis(file_path, include_entities=False)
    except Exception:
        counts = {}
    if counts:
        sample_block_id = max(counts.items(), key=lambda item: item[1])[0]
    color_cache = _load_viewer_color_cache()
    if not sample_block_id:
        return "", "none", False
    stripped_state = sample_block_id.split("[", 1)[0]
    normalized_key = stripped_state.rsplit(":", 1)[-1]
    for candidate in (normalized_key, stripped_state):
        color = color_cache.get(candidate)
        if color is not None:
            return sample_block_id, f"{normalized_key}:{color}", True
    return sample_block_id, f"{normalized_key}:default_gray", False


def _load_viewer_color_cache() -> dict[str, list[float]]:
    cache_path = project_root() / "desktop-ui" / "src" / "data" / "blockColorCache.json"
    try:
        payload = json.loads(cache_path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    if not isinstance(payload, dict):
        return {}
    result: dict[str, list[float]] = {}
    for key, value in payload.items():
        if isinstance(value, list) and len(value) >= 3:
            result[str(key)] = [float(value[0]), float(value[1]), float(value[2])]
    return result


def _launch_native_renderer(
    file_path: str | Path,
    cache_file: str | Path,
    *,
    target: str,
    auto_exit_seconds: int | None = None,
    parent_hwnd: int | None = None,
    preview_mode: bool = False,
    preview_spin: bool = False,
    display_mode: str = RENDER_BUILD_MODE_NORMAL,
) -> subprocess.Popen[str]:
    binary = _resolve_binary("litematica_native_viewer")
    resolved_file = Path(file_path).resolve()
    resolved_cache = Path(cache_file).resolve()
    mode = str(display_mode or RENDER_BUILD_MODE_NORMAL).strip().lower()
    if mode not in VALID_RENDER_BUILD_MODES:
        mode = RENDER_BUILD_MODE_NORMAL
    mark_layer_precache_activity(resolved_cache)
    probe_native_render_cache(resolved_file, resolved_cache, target=target)
    command = [
        str(binary),
        str(resolved_file),
        f"--chunk-size={_CHUNK_SIZE}",
        f"--display-mode={mode}",
    ]
    env: dict[str, str] | None = None
    if mode == RENDER_BUILD_MODE_FULL:
        env = dict(os.environ)
        env["LBA_VIEWER_DISPLAY_MODE"] = RENDER_BUILD_MODE_FULL
        env.pop("LBA_VIEWER_FULL_MATERIAL_CACHE", None)
        env.pop("LBA_VIEWER_FULL_COLOR_CACHE", None)
        print(
            "[LBA_FULL_MODE_V2] native_viewer_live_scene "
            f"target={target} renderer=rust_runtime_state_templates cache={resolved_cache}"
        )
    else:
        command.append(f"--cache-input={resolved_cache}")
    launch_mode = "embedded" if parent_hwnd else "popup"
    if parent_hwnd:
        command.append(f"--embed-parent-hwnd={int(parent_hwnd)}")
    if auto_exit_seconds is not None and auto_exit_seconds > 0:
        command.append(f"--auto-exit-seconds={int(auto_exit_seconds)}")
    if preview_mode:
        command.append("--preview-mode")
    if preview_spin:
        command.append("--preview-spin")
    process = _spawn_managed_process(command, env=env)
    _mark_layer_precache_activity_while_running(resolved_cache, process)
    return process


def open_popup_viewer(
    file_path: str | Path,
    cache_file: str | Path,
    *,
    auto_exit_seconds: int | None = None,
    display_mode: str = RENDER_BUILD_MODE_NORMAL,
) -> subprocess.Popen[str]:
    return _launch_native_renderer(
        file_path,
        cache_file,
        target="popup",
        auto_exit_seconds=auto_exit_seconds,
        display_mode=display_mode,
    )


def open_embedded_viewer(
    file_path: str | Path,
    cache_file: str | Path,
    *,
    parent_hwnd: int,
    preview_mode: bool = False,
    preview_spin: bool = False,
    display_mode: str = RENDER_BUILD_MODE_NORMAL,
) -> subprocess.Popen[str]:
    return _launch_native_renderer(
        file_path,
        cache_file,
        target="embedded",
        parent_hwnd=parent_hwnd,
        preview_mode=preview_mode,
        preview_spin=preview_spin,
        display_mode=display_mode,
    )


def open_preview_capture_viewer(
    file_path: str | Path,
    cache_file: str | Path,
    *,
    auto_exit_seconds: int = 6,
) -> subprocess.Popen[str]:
    return _launch_native_renderer(
        file_path,
        cache_file,
        target="preview_capture",
        auto_exit_seconds=auto_exit_seconds,
        preview_mode=True,
        preview_spin=False,
    )


def render_cache_preview_to_file(
    file_path: str | Path,
    cache_file: str | Path,
    output_path: str | Path,
) -> None:
    binary = _resolve_binary("litematica_native_viewer")
    resolved_file = Path(file_path).resolve()
    resolved_cache = Path(cache_file).resolve()
    resolved_output = Path(output_path).resolve()
    mark_layer_precache_activity(resolved_cache)
    probe_native_render_cache(resolved_file, resolved_cache, target="offscreen_preview")
    command = [
        str(binary),
        str(resolved_file),
        f"--chunk-size={_CHUNK_SIZE}",
        f"--cache-input={resolved_cache}",
        f"--preview-output={resolved_output}",
    ]
    completed = subprocess.run(
        command,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        creationflags=_CREATE_NO_WINDOW,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown preview render error"
        raise RuntimeError(f"离屏预览渲染失败：{detail}")


def _opt_int(value: Any) -> int | None:
    if value is None:
        return None
    return int(value)
