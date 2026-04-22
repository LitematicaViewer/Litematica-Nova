from __future__ import annotations

import json
import sys
import time
from pathlib import Path

from litematicaba.core.layer_pack import cleanup_layer_pack_temp, write_layer_pack_atomic
from litematicaba.core.native_backend_bridge import layer_precache_dir


STATUS_WRITE_INTERVAL_SECONDS = 0.5
STATUS_WRITE_INTERVAL_LAYERS = 8
VIEW_ACTIVITY_WINDOW_SECONDS = 3.0
VIEW_ACTIVITY_SLEEP_SECONDS = 0.25
SMALL_MODEL_NON_AIR_BLOCKS = 120_000
LARGE_MODEL_NON_AIR_BLOCKS = 500_000
HUGE_MODEL_NON_AIR_BLOCKS = 1_500_000
LARGE_MODEL_LAYER_CAP = 192
HUGE_MODEL_LAYER_CAP = 128


def _write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    tmp.replace(path)


def _cleanup_legacy_layer_files(output_dir: Path) -> None:
    for pattern in ("layer_*.json", "meta.json"):
        for path in output_dir.glob(pattern):
            try:
                path.unlink()
            except Exception:
                pass


def _read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def _layer_index_path(cache_file: Path) -> Path:
    manifest = _read_json(cache_file)
    layer_index_file = manifest.get("layer_index_file")
    if not layer_index_file:
        raise RuntimeError("cache does not contain layer_index_file; rebuild 3D cache before layer precache")
    return (cache_file.parent / str(layer_index_file)).resolve()


def _visual_meta_from_sidecar(sidecar: dict) -> dict:
    visual = sidecar.get("visual") if isinstance(sidecar.get("visual"), dict) else {}
    return {
        "metadata": sidecar.get("metadata", {}),
        "visual": {
            "chunk_size": visual.get("chunk_size"),
            "size_x": visual.get("size_x", 0),
            "size_y": visual.get("size_y", 0),
            "size_z": visual.get("size_z", 0),
            "palette": visual.get("palette", []),
            "property_pool": visual.get("property_pool", []),
        },
    }


def _non_air_block_count(layers: list[dict]) -> int:
    total = 0
    for layer in layers:
        blocks = layer.get("blocks") if isinstance(layer, dict) else None
        if isinstance(blocks, list):
            total += len(blocks)
    return total


def _select_layers(layers: list[dict], non_air_blocks: int) -> tuple[list[dict], dict]:
    non_empty = [
        layer
        for layer in layers
        if isinstance(layer, dict) and isinstance(layer.get("blocks"), list) and len(layer["blocks"]) > 0
    ]
    non_empty.sort(key=lambda layer: int(layer.get("y", 0) or 0))
    cap = len(non_empty)
    degraded = False
    reason = "full"
    if non_air_blocks >= HUGE_MODEL_NON_AIR_BLOCKS:
        cap = min(cap, HUGE_MODEL_LAYER_CAP)
        degraded = cap < len(non_empty)
        reason = "huge_model_budget"
    elif non_air_blocks >= LARGE_MODEL_NON_AIR_BLOCKS:
        cap = min(cap, LARGE_MODEL_LAYER_CAP)
        degraded = cap < len(non_empty)
        reason = "large_model_budget"
    elif non_air_blocks >= SMALL_MODEL_NON_AIR_BLOCKS:
        reason = "medium_full_budgeted"
    return non_empty[:cap], {
        "strategy": "budgeted_sidecar_precache",
        "degraded": degraded,
        "reason": reason,
        "non_air_blocks": non_air_blocks,
        "non_empty_layers": len(non_empty),
        "selected_layers": cap,
    }


def _activity_is_recent(activity_file: Path | None) -> bool:
    if activity_file is None:
        return False
    try:
        return time.time() - activity_file.stat().st_mtime <= VIEW_ACTIVITY_WINDOW_SECONDS
    except OSError:
        return False


def _sleep_for_budget(activity_file: Path | None, non_air_blocks: int) -> bool:
    if _activity_is_recent(activity_file):
        time.sleep(VIEW_ACTIVITY_SLEEP_SECONDS)
        return True
    if non_air_blocks >= HUGE_MODEL_NON_AIR_BLOCKS:
        time.sleep(0.035)
    elif non_air_blocks >= LARGE_MODEL_NON_AIR_BLOCKS:
        time.sleep(0.015)
    return False


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if not args:
        print("usage: layer_precache_worker <cache_manifest> [status_file] [timeout_seconds] [activity_file]", file=sys.stderr)
        return 2
    cache_file = Path(args[0]).resolve()
    status_file = Path(args[1]).resolve() if len(args) >= 2 else layer_precache_dir(cache_file) / "status.json"
    timeout_seconds = float(args[2]) if len(args) >= 3 else 600.0
    activity_file = Path(args[3]).resolve() if len(args) >= 4 and args[3] else None
    started_at = time.monotonic()
    output_dir = layer_precache_dir(cache_file)

    try:
        _write_json(status_file, {"phase": "meta", "done": 0, "total": 0, "ready": False})
        layer_index = _layer_index_path(cache_file)
        sidecar = _read_json(layer_index)
        meta = _visual_meta_from_sidecar(sidecar)
        visual = meta.get("visual") if isinstance(meta.get("visual"), dict) else {}
        sidecar_visual = sidecar.get("visual") if isinstance(sidecar.get("visual"), dict) else {}
        layers = sidecar_visual.get("layers") if isinstance(sidecar_visual.get("layers"), list) else []
        non_air_blocks = _non_air_block_count(layers)
        selected_layers, strategy = _select_layers(layers, non_air_blocks)
        total = len(selected_layers)
        status_base = {
            "phase": "pack",
            "done": 0,
            "total": total,
            "total_layers": max(0, int(visual.get("size_y", 0) or 0)),
            "ready": False,
            **strategy,
        }
        _write_json(status_file, status_base)
        last_status_at = time.monotonic()
        yielded = False
        pack_layers: list[dict] = []
        for index, layer in enumerate(selected_layers, start=1):
            if time.monotonic() - started_at > timeout_seconds:
                _write_json(status_file, {**status_base, "phase": "timeout", "done": index - 1, "ready": False})
                return 124
            yielded = _sleep_for_budget(activity_file, non_air_blocks)
            y = int(layer.get("y", index - 1) or 0)
            layer = {
                "metadata": sidecar.get("metadata", {}),
                "chunk_size": visual.get("chunk_size"),
                "y": y,
                "blocks": layer.get("blocks", []) if isinstance(layer, dict) else [],
            }
            pack_layers.append(layer)
            now = time.monotonic()
            if yielded or index == total or index % STATUS_WRITE_INTERVAL_LAYERS == 0 or now - last_status_at >= STATUS_WRITE_INTERVAL_SECONDS:
                _write_json(
                    status_file,
                    {
                        **status_base,
                        "phase": "yielding" if yielded else "layers",
                        "done": index,
                        "ready": False,
                    },
                )
                last_status_at = now
        _write_json(status_file, {**status_base, "phase": "commit", "done": total, "ready": False})
        pack_path = write_layer_pack_atomic(cache_file, meta, pack_layers, {**strategy, "source": "cache_layer_sidecar"})
        _cleanup_legacy_layer_files(output_dir)
        _write_json(
            status_file,
            {
                **status_base,
                "phase": "ready",
                "done": total,
                "ready": True,
                "pack_file": str(pack_path),
                "pack_bytes": pack_path.stat().st_size if pack_path.is_file() else 0,
            },
        )
        return 0
    except Exception as exc:
        cleanup_layer_pack_temp(cache_file)
        _write_json(status_file, {"phase": "error", "error": str(exc), "ready": False})
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
