from __future__ import annotations

import time
from dataclasses import dataclass
from pathlib import Path
from tempfile import NamedTemporaryFile

from litematicaba.core.native_backend_bridge import (
    cancel_cache_build,
    poll_cache_build,
    render_cache_preview_to_file,
    start_cache_build,
)
from litematicaba.core.projection_library import ProjectionLibraryEntry, ProjectionLibraryError, ProjectionLibraryStore


PREVIEW_SIZE = (320, 200)
_BUILD_TIMEOUT_SECONDS = 420.0


class ProjectionPreviewError(RuntimeError):
    pass


@dataclass(slots=True)
class ProjectionPreviewRequest:
    entry: ProjectionLibraryEntry
    cache_file: Path | None = None


def preview_output_path(store: ProjectionLibraryStore, entry: ProjectionLibraryEntry) -> Path:
    return store.preview_path_for(entry.entry_id)


def generate_projection_preview(
    store: ProjectionLibraryStore,
    request: ProjectionPreviewRequest,
) -> Path:
    entry = request.entry
    if not entry.backup_path.is_file():
        raise ProjectionPreviewError("投影备份文件不存在，无法生成预览图")

    cache_file = request.cache_file
    if cache_file is None:
        cache_file = _build_cache_for_preview(entry.backup_path)
    elif not cache_file.is_file():
        raise ProjectionPreviewError(f"指定的 3D cache 不存在：{cache_file}")

    output = preview_output_path(store, entry)
    _render_preview_atomically(entry.backup_path, cache_file, output)
    store.update_preview_path(entry.entry_id, output)
    return output


def save_existing_preview_path(
    store: ProjectionLibraryStore,
    entry_id: str,
    preview_path: str | Path,
) -> Path:
    path = Path(preview_path)
    if not path.is_file():
        raise ProjectionLibraryError(f"预览图不存在：{path}")
    store.update_preview_path(entry_id, path)
    return path


def _build_cache_for_preview(file_path: Path) -> Path:
    launch = start_cache_build(file_path)
    deadline = time.monotonic() + _BUILD_TIMEOUT_SECONDS
    try:
        while time.monotonic() < deadline:
            snapshot = poll_cache_build(launch.launch_id)
            progress = snapshot.progress
            if progress is not None and progress.ready:
                cache = Path(launch.cache_file)
                if cache.is_file():
                    return cache
                raise ProjectionPreviewError(f"3D cache 构建完成但文件不存在：{cache}")
            if not snapshot.running and (progress is None or not progress.ready):
                raise ProjectionPreviewError("3D cache 构建进程结束，但没有生成可用 cache")
            time.sleep(0.35)
    except Exception:
        cancel_cache_build(launch.launch_id)
        raise
    cancel_cache_build(launch.launch_id)
    raise ProjectionPreviewError("3D cache 构建超时，未生成预览图")


def _render_preview_atomically(file_path: Path, cache_file: Path, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temp_path: Path | None = None
    try:
        with NamedTemporaryFile(
            delete=False,
            dir=str(output.parent),
            prefix=f"{output.stem}_",
            suffix=output.suffix or ".png",
        ) as handle:
            temp_path = Path(handle.name)
        render_cache_preview_to_file(file_path, cache_file, temp_path)
        if not temp_path.is_file() or temp_path.stat().st_size <= 0:
            raise ProjectionPreviewError("离屏渲染没有生成有效预览图")
        temp_path.replace(output)
    except Exception as exc:
        if temp_path is not None:
            try:
                temp_path.unlink(missing_ok=True)
            except OSError:
                pass
        if isinstance(exc, ProjectionPreviewError):
            raise
        raise ProjectionPreviewError(f"离屏生成预览图失败：{exc}") from exc
