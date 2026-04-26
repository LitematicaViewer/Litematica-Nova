#!/usr/bin/env python3
"""Export a minimal Litematica-BA diagnostics bundle.

This script is intentionally low-intrusion:
- it only reads existing files and environment data;
- it does not copy user .litematic files;
- it does not copy large cache payloads;
- it redacts common secret-looking values.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


SECRET_KEY_RE = re.compile(
    r"(api[_-]?key|token|authorization|cookie|secret|password|bearer)",
    re.IGNORECASE,
)
SECRET_VALUE_RE = re.compile(
    r"(?i)(api[_-]?key|authorization|cookie|token|secret|password)\s*[:=]\s*([^\s,;]+)"
)
BEARER_RE = re.compile(r"(?i)bearer\s+[a-z0-9._~+/=-]+")
MAX_TEXT_CHARS = 12_000
MAX_JSON_FILE_BYTES = 2_000_000


def find_repo_root(start: Path) -> Path:
    current = start.resolve()
    for candidate in [current, *current.parents]:
        if (candidate / "desktop-js").is_dir() and (candidate / "tools" / "viewer-core").is_dir():
            return candidate
    return current


def now_stamp() -> str:
    return _dt.datetime.now().strftime("%Y%m%d_%H%M%S")


def iso_now() -> str:
    return _dt.datetime.now().astimezone().isoformat(timespec="seconds")


def redact_text(text: str) -> str:
    text = SECRET_VALUE_RE.sub(lambda m: f"{m.group(1)}=<redacted>", text)
    text = BEARER_RE.sub("Bearer <redacted>", text)
    return text


def safe_env_value(key: str, value: str) -> str:
    if SECRET_KEY_RE.search(key):
        return "<redacted>"
    return redact_text(value)


def unavailable(reason: str) -> dict[str, Any]:
    return {"status": "not available yet", "reason": reason}


def path_info(path: Path) -> dict[str, Any]:
    info: dict[str, Any] = {
        "path": str(path),
        "exists": path.exists(),
    }
    if path.exists():
        try:
            stat = path.stat()
            info.update(
                {
                    "is_file": path.is_file(),
                    "is_dir": path.is_dir(),
                    "size_bytes": stat.st_size if path.is_file() else None,
                    "modified_time": _dt.datetime.fromtimestamp(stat.st_mtime)
                    .astimezone()
                    .isoformat(timespec="seconds"),
                }
            )
        except OSError as exc:
            info["stat_error"] = str(exc)
    return info


def run_version(command: list[str], cwd: Path) -> str:
    exe = shutil.which(command[0])
    if not exe:
        return "unavailable: executable not found"
    try:
        completed = subprocess.run(
            command,
            cwd=str(cwd),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=8,
            check=False,
        )
        output = (completed.stdout or "").strip()
        return output.splitlines()[0] if output else f"unavailable: exit={completed.returncode}"
    except Exception as exc:  # noqa: BLE001 - diagnostics should not abort.
        return f"unavailable: {exc}"


def write_text(path: Path, text: str) -> None:
    path.write_text(redact_text(text), encoding="utf-8")


def write_json(path: Path, value: Any) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


def collect_environment(repo: Path) -> str:
    backend_core = repo / "bin" / "viewer-backend" / "litematica_core.exe"
    backend_viewer = repo / "bin" / "viewer-backend" / "litematica_native_viewer.exe"
    lines = [
        "Litematica-BA diagnostics environment",
        f"generated_at={iso_now()}",
        "",
        f"os={platform.platform()}",
        f"system={platform.system()}",
        f"release={platform.release()}",
        f"machine={platform.machine()}",
        f"python={sys.version.splitlines()[0]}",
        f"node={run_version(['node', '--version'], repo)}",
        f"npm={run_version(['npm', '--version'], repo)}",
        f"cargo={run_version(['cargo', '--version'], repo)}",
        "",
        f"cwd={Path.cwd()}",
        f"repo_root={repo}",
        "",
        "key executables:",
        f"- litematica_core.exe exists={backend_core.is_file()} path={backend_core}",
        f"- litematica_native_viewer.exe exists={backend_viewer.is_file()} path={backend_viewer}",
    ]
    return "\n".join(lines) + "\n"


def collect_project_paths(repo: Path) -> dict[str, Any]:
    paths = {
        "project_root": repo,
        "desktop_js": repo / "desktop-js",
        "viewer_core": repo / "tools" / "viewer-core",
        "data": repo / "data",
        "viewer_backend_bin": repo / "bin" / "viewer-backend",
        "projection_library": repo / "data" / "projection-library",
        "cache": repo / ".tmp" / "desktop-js" / "render",
        "docs": repo / "docs",
        "diagnostics": repo / "diagnostics",
        "assistant_index": repo / ".tmp" / "assistant" / "lba_assistant_index.sqlite",
    }
    return {key: path_info(path) for key, path in paths.items()}


def collect_debug_flags() -> str:
    lba_env = {
        key: safe_env_value(key, value)
        for key, value in sorted(os.environ.items())
        if key.startswith("LBA_")
    }
    if not lba_env:
        return "none\n"
    return "\n".join(f"{key}={value}" for key, value in lba_env.items()) + "\n"


def short_hash(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8", errors="ignore")).hexdigest()[:12]


def summarize_path(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, str) or not value:
        return None
    p = Path(value)
    return {
        "basename": p.name,
        "parent_name": p.parent.name if str(p.parent) not in ("", ".") else "",
        "sha256_12": short_hash(value),
        "is_absolute": p.is_absolute(),
    }


def collect_projection_library(repo: Path) -> dict[str, Any]:
    candidates = [
        repo / "data" / "projection-library" / "js_library.json",
        repo / "data" / "projection-library" / "index.json",
    ]
    source = next((p for p in candidates if p.is_file()), None)
    if not source:
        return unavailable("no projection library index found at data/projection-library/js_library.json or index.json")
    try:
        raw = json.loads(source.read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001
        return {"status": "not available yet", "reason": f"failed to parse {source}: {exc}"}

    records = raw.get("records") if isinstance(raw, dict) else None
    if not isinstance(records, list):
        return {
            "source": str(source),
            "status": "not available yet",
            "reason": "index format does not expose a records array",
        }

    safe_records = []
    for record in records[:10]:
        if not isinstance(record, dict):
            continue
        safe_records.append(
            {
                "id": record.get("id") or record.get("entry_id"),
                "displayName": record.get("displayName") or record.get("display_name"),
                "fileName": record.get("fileName"),
                "path_summary": summarize_path(record.get("path") or record.get("original_file_path")),
                "backup_path_summary": summarize_path(record.get("backup_file_path")),
                "preview_path_summary": summarize_path(record.get("previewPath") or record.get("preview_file_path")),
                "cache_path_summary": summarize_path(record.get("cachePath") or record.get("cache_file_path")),
                "status": record.get("status"),
                "lastAnalyzedAt": record.get("lastAnalyzedAt"),
                "importedAt": record.get("imported_at") or record.get("importedAt"),
                "minecraftDataVersion": record.get("minecraftDataVersion")
                or record.get("minecraft_data_version"),
                "totalBlocks": record.get("totalBlocks"),
                "regionCount": record.get("regionCount"),
            }
        )

    return {
        "source": str(source),
        "total_records": len(records),
        "excerpt_count": len(safe_records),
        "records": safe_records,
        "note": "Paths are summarized; full user file contents are not copied.",
    }


def latest_file(paths: list[Path]) -> Path | None:
    existing = [p for p in paths if p.is_file()]
    if not existing:
        return None
    return max(existing, key=lambda p: p.stat().st_mtime)


def collect_recent_cache_manifest(repo: Path) -> dict[str, Any]:
    cache_dir = repo / ".tmp" / "desktop-js" / "render"
    candidates = sorted(cache_dir.glob("lba_native_cache*.json")) if cache_dir.is_dir() else []
    latest = latest_file(candidates)
    if not latest:
        return unavailable("no lba_native_cache*.json manifest found under .tmp/desktop-js/render")
    try:
        stat = latest.stat()
        if stat.st_size > MAX_JSON_FILE_BYTES:
            return {
                "source": str(latest),
                "status": "not available yet",
                "reason": f"manifest is unexpectedly large ({stat.st_size} bytes); refusing full read",
            }
        manifest = json.loads(latest.read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001
        return {"source": str(latest), "status": "not available yet", "reason": f"failed to parse manifest: {exc}"}

    chunks = manifest.get("chunks") if isinstance(manifest, dict) else None
    return {
        "source": str(latest),
        "modified_time": _dt.datetime.fromtimestamp(latest.stat().st_mtime)
        .astimezone()
        .isoformat(timespec="seconds"),
        "size_bytes": latest.stat().st_size,
        "format": manifest.get("format"),
        "color_chain": manifest.get("color_chain"),
        "chunk_size": manifest.get("chunk_size"),
        "scene_size": {
            "x": manifest.get("scene_size_x"),
            "y": manifest.get("scene_size_y"),
            "z": manifest.get("scene_size_z"),
        },
        "chunk_data_dir": manifest.get("chunk_data_dir"),
        "layer_index_file": manifest.get("layer_index_file"),
        "chunks_count": len(chunks) if isinstance(chunks, list) else None,
        "chunks_excerpt": chunks[:5] if isinstance(chunks, list) else [],
        "note": "Large chunk payload files are not copied.",
    }


def read_tail(path: Path, max_chars: int = MAX_TEXT_CHARS) -> str:
    try:
        data = path.read_text(encoding="utf-8", errors="replace")
    except Exception as exc:  # noqa: BLE001
        return f"[unavailable: failed to read {path}: {exc}]\n"
    if len(data) <= max_chars:
        return data
    return data[-max_chars:]


def collect_recent_logs(repo: Path) -> str:
    candidates: list[Path] = []
    for folder in [repo / ".tmp" / "desktop-js" / "render", repo]:
        if folder.is_dir():
            candidates.extend(folder.glob("*.log"))
    candidates = [p for p in candidates if p.is_file()]
    if not candidates:
        return "not available yet: no .log files found under .tmp/desktop-js/render or repo root\n"
    candidates = sorted(candidates, key=lambda p: p.stat().st_mtime, reverse=True)[:8]
    parts = [
        "Recent log excerpts",
        "Only tails are included. Secrets are redacted. Full logs are not copied wholesale.",
        "",
    ]
    for path in candidates:
        stat = path.stat()
        parts.append("=" * 80)
        parts.append(f"path={path}")
        parts.append(
            "modified_time="
            + _dt.datetime.fromtimestamp(stat.st_mtime).astimezone().isoformat(timespec="seconds")
        )
        parts.append(f"size_bytes={stat.st_size}")
        parts.append("-" * 80)
        parts.append(read_tail(path))
        parts.append("")
    return redact_text("\n".join(parts))


def not_available_text(reason: str) -> str:
    return f"not available yet: {reason}\n"


def write_readme(bundle: Path, repo: Path) -> None:
    text = f"""Litematica-BA diagnostics bundle

Generated at: {iso_now()}
Project root: {repo}

This bundle collects low-risk maintenance context for debugging Litematica-BA.
Send the whole bundle directory to the maintainer when reporting a bug.

Security boundaries:
- User .litematic files are not copied.
- Large cache chunk payloads are not copied.
- API keys, tokens, Authorization headers, cookies, and similar secrets are redacted when detected.
- Missing runtime state is written as "not available yet" instead of aborting the export.

Expected files:
- README.txt
- environment.txt
- project_paths.json
- debug_flags.txt
- projection_library_index_excerpt.json
- recent_cache_manifest.json
- recent_viewer_command.txt
- recent_ai_plan_trace.json
- recent_logs.txt

Some files may contain "not available yet" when the current app build does not persist that state.
"""
    write_text(bundle / "README.txt", text)


def export_bundle(repo: Path, output_root: Path) -> Path:
    bundle = output_root / f"bundle_{now_stamp()}"
    bundle.mkdir(parents=True, exist_ok=False)

    write_readme(bundle, repo)
    write_text(bundle / "environment.txt", collect_environment(repo))
    write_json(bundle / "project_paths.json", collect_project_paths(repo))
    write_text(bundle / "debug_flags.txt", collect_debug_flags())
    write_json(bundle / "projection_library_index_excerpt.json", collect_projection_library(repo))
    write_json(bundle / "recent_cache_manifest.json", collect_recent_cache_manifest(repo))
    write_text(
        bundle / "recent_viewer_command.txt",
        not_available_text(
            "no persisted recent native viewer/viewer-core command record was found; use recent_logs.txt for startup lines when available"
        ),
    )
    write_json(
        bundle / "recent_ai_plan_trace.json",
        unavailable(
            "no persisted AI plan trace was found; current AI plan state appears to live in GeneratePage runtime state"
        ),
    )
    write_text(bundle / "recent_logs.txt", collect_recent_logs(repo))
    return bundle


def main() -> int:
    parser = argparse.ArgumentParser(description="Export a minimal Litematica-BA diagnostics bundle.")
    parser.add_argument(
        "--output-dir",
        default="diagnostics",
        help="Directory where bundle_YYYYMMDD_HHMMSS will be created (default: diagnostics)",
    )
    args = parser.parse_args()

    repo = find_repo_root(Path.cwd())
    output_root = Path(args.output_dir)
    if not output_root.is_absolute():
        output_root = repo / output_root
    try:
        bundle = export_bundle(repo, output_root)
    except Exception as exc:  # noqa: BLE001
        print(f"diagnostics export failed: {exc}", file=sys.stderr)
        return 1

    print(str(bundle))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
