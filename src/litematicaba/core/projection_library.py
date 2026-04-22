from __future__ import annotations

import json
import shutil
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from tempfile import NamedTemporaryFile
from typing import Any
from uuid import uuid4

from litematicaba.core.config import user_data_dir
from litematicaba.core.snbt_properties import load_snbt_properties, save_snbt_properties

INDEX_VERSION = 1
VALID_SORT_MODES = ("manual", "recent", "name")


class ProjectionLibraryError(RuntimeError):
    pass


@dataclass(slots=True)
class ProjectionValidation:
    backup_missing: bool = False
    original_missing: bool = False
    original_size_mismatch: bool = False
    details: list[str] | None = None

    @property
    def messages(self) -> list[str]:
        return list(self.details or [])

    @property
    def has_issue(self) -> bool:
        return self.backup_missing or self.original_missing or self.original_size_mismatch


@dataclass(slots=True)
class ProjectionLibraryEntry:
    entry_id: str
    display_name: str
    backup_file_path: str
    original_file_path: str
    file_size: int
    imported_at: str
    last_used_at: str
    original_file_name: str
    note: str = ""
    tags: list[str] | None = None
    structure_type: str = ""
    preview_image_path: str = ""
    sort_index: int = 0
    internal_name: str = ""
    author: str = ""
    description: str = ""
    litematic_version: int = 0
    minecraft_data_version: int = 0

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> ProjectionLibraryEntry:
        return cls(
            entry_id=str(raw.get("entry_id", "")),
            display_name=str(raw.get("display_name", "")).strip(),
            backup_file_path=str(raw.get("backup_file_path", "")),
            original_file_path=str(raw.get("original_file_path", "")),
            file_size=max(0, int(raw.get("file_size", 0))),
            imported_at=str(raw.get("imported_at", "")),
            last_used_at=str(raw.get("last_used_at", "")),
            original_file_name=str(raw.get("original_file_name", "")),
            note=str(raw.get("note", "")),
            tags=[str(item) for item in raw.get("tags", []) if str(item).strip()],
            structure_type=str(raw.get("structure_type", "")),
            preview_image_path=str(raw.get("preview_image_path", "")),
            sort_index=max(0, int(raw.get("sort_index", 0))),
            internal_name=str(raw.get("internal_name", "")),
            author=str(raw.get("author", "")),
            description=str(raw.get("description", "")),
            litematic_version=max(0, int(raw.get("litematic_version", 0) or 0)),
            minecraft_data_version=max(0, int(raw.get("minecraft_data_version", 0) or 0)),
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @property
    def backup_path(self) -> Path:
        return Path(self.backup_file_path)

    @property
    def original_path(self) -> Path | None:
        value = self.original_file_path.strip()
        if not value:
            return None
        return Path(value)

    @property
    def preview_path(self) -> Path | None:
        value = self.preview_image_path.strip()
        if not value:
            return None
        return Path(value)

    def normalized_tags(self) -> list[str]:
        seen: set[str] = set()
        out: list[str] = []
        for raw in self.tags or []:
            tag = str(raw).strip()
            if not tag or tag in seen:
                continue
            seen.add(tag)
            out.append(tag)
        return out


def _now_iso() -> str:
    return datetime.now().isoformat(timespec="seconds")


def _sanitize_backup_name(name: str) -> str:
    path = Path(name)
    suffix = path.suffix if path.suffix and path.suffix.isascii() else ".litematic"
    stem = path.stem or "projection"
    safe_stem = "".join(
        ch if (ch.isascii() and (ch.isalnum() or ch in ("-", "_", "."))) else "_"
        for ch in stem
    )
    safe_stem = safe_stem.strip("._")
    return f"{safe_stem or 'projection'}{suffix}"


def _apply_file_metadata_to_entry(entry: ProjectionLibraryEntry, file_path: str | Path) -> None:
    try:
        data = load_snbt_properties(file_path)
    except Exception:
        return
    entry.internal_name = data.internal_name
    entry.author = data.author
    entry.description = data.description
    entry.litematic_version = int(data.litematic_version)
    entry.minecraft_data_version = int(data.minecraft_data_version)


class ProjectionLibraryStore:
    def __init__(self, *, default_limit: int = 20) -> None:
        self._default_limit = int(default_limit)

    @property
    def root_dir(self) -> Path:
        return user_data_dir() / "projection-library"

    @property
    def backups_dir(self) -> Path:
        return self.root_dir / "backups"

    @property
    def previews_dir(self) -> Path:
        return self.root_dir / "previews"

    @property
    def index_path(self) -> Path:
        return self.root_dir / "index.json"

    def ensure_layout(self) -> None:
        self.backups_dir.mkdir(parents=True, exist_ok=True)
        self.previews_dir.mkdir(parents=True, exist_ok=True)

    def preview_path_for(self, entry_id: str) -> Path:
        self.ensure_layout()
        return self.previews_dir / f"{entry_id}.png"

    def load_entries(self) -> list[ProjectionLibraryEntry]:
        self.ensure_layout()
        path = self.index_path
        if not path.is_file():
            return []
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise ProjectionLibraryError(f"投影库索引读取失败：{exc}") from exc
        items = payload.get("entries", [])
        if not isinstance(items, list):
            raise ProjectionLibraryError("投影库索引格式无效：entries 不是数组")
        entries = [ProjectionLibraryEntry.from_dict(item) for item in items if isinstance(item, dict)]
        for index, entry in enumerate(entries):
            if entry.sort_index <= 0 and index > 0:
                entry.sort_index = index
        entries.sort(key=lambda item: (int(item.sort_index), item.display_name.lower()))
        return entries

    def save_entries(self, entries: list[ProjectionLibraryEntry]) -> None:
        self.ensure_layout()
        payload = {
            "version": INDEX_VERSION,
            "entries": [entry.to_dict() for entry in entries],
        }
        try:
            self.index_path.write_text(
                json.dumps(payload, ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
        except OSError as exc:
            raise ProjectionLibraryError(f"投影库索引写入失败：{exc}") from exc

    def enforce_limit(self, limit: int) -> None:
        entries = self.load_entries()
        kept = self._trim_entries(entries, limit)
        if len(kept) != len(entries):
            self.save_entries(kept)

    def import_projection(
        self,
        source_path: str | Path,
        *,
        display_name: str | None = None,
        limit: int | None = None,
    ) -> ProjectionLibraryEntry:
        source = Path(source_path)
        if not source.is_file():
            raise ProjectionLibraryError(f"源文件不存在：{source}")
        self.ensure_layout()
        source = source.resolve()
        entries = self.load_entries()
        existing = self.find_entry_by_original_path(source, entries=entries)
        entry_id = existing.entry_id if existing is not None else uuid4().hex
        backup_name = f"{entry_id}_{_sanitize_backup_name(source.name)}"
        backup_path = self.backups_dir / backup_name
        try:
            shutil.copy2(source, backup_path)
        except OSError as exc:
            raise ProjectionLibraryError(f"备份投影文件失败：{exc}") from exc
        stat = source.stat()
        now = _now_iso()
        entry = ProjectionLibraryEntry(
            entry_id=entry_id,
            display_name=(display_name or source.stem or source.name).strip() or source.name,
            backup_file_path=str(backup_path),
            original_file_path=str(source),
            file_size=int(stat.st_size),
            imported_at=existing.imported_at if existing is not None else now,
            last_used_at=now,
            original_file_name=source.name,
            note=existing.note if existing is not None else "",
            tags=existing.normalized_tags() if existing is not None else [],
            structure_type=existing.structure_type if existing is not None else "",
            preview_image_path=existing.preview_image_path if existing is not None else "",
            sort_index=0,
            internal_name=existing.internal_name if existing is not None else "",
            author=existing.author if existing is not None else "",
            description=existing.description if existing is not None else "",
            litematic_version=existing.litematic_version if existing is not None else 0,
            minecraft_data_version=existing.minecraft_data_version if existing is not None else 0,
        )
        _apply_file_metadata_to_entry(entry, backup_path)
        updated_entries = [item for item in entries if item.entry_id != entry.entry_id]
        updated_entries.insert(0, entry)
        self._assign_sort_indices(updated_entries)
        updated_entries = self._trim_entries(updated_entries, limit)
        self.save_entries(updated_entries)
        if existing is not None:
            old_backup = existing.backup_path
            if old_backup != backup_path:
                old_backup.unlink(missing_ok=True)
        return entry

    def find_entry_by_original_path(
        self,
        source_path: str | Path,
        *,
        entries: list[ProjectionLibraryEntry] | None = None,
    ) -> ProjectionLibraryEntry | None:
        source = Path(source_path)
        try:
            target = str(source.resolve())
        except OSError:
            target = str(source)
        for entry in entries if entries is not None else self.load_entries():
            if entry.original_file_path == target:
                return entry
        return None

    def find_entry_by_backup_path(
        self,
        backup_path: str | Path,
        *,
        entries: list[ProjectionLibraryEntry] | None = None,
    ) -> ProjectionLibraryEntry | None:
        source = Path(backup_path)
        try:
            target = str(source.resolve())
        except OSError:
            target = str(source)
        for entry in entries if entries is not None else self.load_entries():
            try:
                candidate = str(entry.backup_path.resolve())
            except OSError:
                candidate = entry.backup_file_path
            if candidate == target:
                return entry
        return None

    def get_entry(self, entry_id: str) -> ProjectionLibraryEntry:
        entries = self.load_entries()
        for index, entry in enumerate(entries):
            if entry.entry_id == entry_id:
                if self._entry_needs_file_metadata(entry):
                    _apply_file_metadata_to_entry(entry, entry.backup_path)
                    entries[index] = entry
                    self.save_entries(entries)
                return entry
        raise ProjectionLibraryError("未找到指定投影记录")

    def list_entries_with_validation(self) -> list[tuple[ProjectionLibraryEntry, ProjectionValidation]]:
        entries = self.load_entries()
        changed = False
        for entry in entries:
            if self._entry_needs_file_metadata(entry):
                _apply_file_metadata_to_entry(entry, entry.backup_path)
                changed = True
        if changed:
            self.save_entries(entries)
        return [(entry, self.validate_entry(entry)) for entry in entries]

    def validate_entry(self, entry: ProjectionLibraryEntry) -> ProjectionValidation:
        messages: list[str] = []
        backup_missing = not entry.backup_path.is_file()
        if backup_missing:
            messages.append("备份文件缺失")
        original_missing = False
        original_size_mismatch = False
        original = entry.original_path
        if original is None:
            original_missing = True
            messages.append("原地址未设置")
        elif not original.exists():
            original_missing = True
            messages.append("原地址不存在")
        else:
            try:
                current_size = original.stat().st_size
            except OSError:
                original_missing = True
                messages.append("原地址无法访问")
            else:
                if int(current_size) != int(entry.file_size):
                    original_size_mismatch = True
                    messages.append(
                        f"原地址文件大小不一致：记录 {entry.file_size} B，当前 {current_size} B"
                    )
        return ProjectionValidation(
            backup_missing=backup_missing,
            original_missing=original_missing,
            original_size_mismatch=original_size_mismatch,
            details=messages,
        )

    def touch_entry(self, entry_id: str) -> ProjectionLibraryEntry:
        entries = self.load_entries()
        for index, entry in enumerate(entries):
            if entry.entry_id != entry_id:
                continue
            entry.last_used_at = _now_iso()
            entries.pop(index)
            entries.insert(0, entry)
            self._assign_sort_indices(entries)
            self.save_entries(entries)
            return entry
        raise ProjectionLibraryError("未找到指定投影记录")

    def remove_entry(self, entry_id: str) -> None:
        entries = self.load_entries()
        target: ProjectionLibraryEntry | None = None
        remaining: list[ProjectionLibraryEntry] = []
        for entry in entries:
            if entry.entry_id == entry_id:
                target = entry
            else:
                remaining.append(entry)
        if target is None:
            raise ProjectionLibraryError("未找到指定投影记录")
        try:
            target.backup_path.unlink(missing_ok=True)
            preview = target.preview_path
            if preview is not None:
                preview.unlink(missing_ok=True)
        except OSError as exc:
            raise ProjectionLibraryError(f"删除备份文件失败：{exc}") from exc
        self.save_entries(remaining)

    def save_backup_as(self, entry_id: str, target_path: str | Path) -> Path:
        entry = self.get_entry(entry_id)
        source = entry.backup_path
        if not source.is_file():
            raise ProjectionLibraryError("备份文件不存在，无法另存为")
        target = Path(target_path)
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            shutil.copy2(source, target)
        except OSError as exc:
            raise ProjectionLibraryError(f"另存为失败：{exc}") from exc
        return target

    def update_entry(
        self,
        entry_id: str,
        *,
        display_name: str,
        original_file_path: str,
        note: str = "",
        internal_name: str | None = None,
        author: str | None = None,
        description: str | None = None,
    ) -> ProjectionLibraryEntry:
        entries = self.load_entries()
        for index, entry in enumerate(entries):
            if entry.entry_id != entry_id:
                continue
            updated = ProjectionLibraryEntry(
                entry_id=entry.entry_id,
                display_name=display_name.strip() or entry.display_name,
                backup_file_path=entry.backup_file_path,
                original_file_path=original_file_path.strip(),
                file_size=int(entry.file_size),
                imported_at=entry.imported_at,
                last_used_at=_now_iso(),
                original_file_name=Path(original_file_path.strip() or entry.original_file_name).name
                or entry.original_file_name,
                note=note.strip(),
                tags=entry.normalized_tags(),
                structure_type=entry.structure_type,
                preview_image_path=entry.preview_image_path,
                sort_index=entry.sort_index,
                internal_name=entry.internal_name if internal_name is None else internal_name.strip(),
                author=entry.author if author is None else author.strip(),
                description=entry.description if description is None else description.strip(),
                litematic_version=entry.litematic_version,
                minecraft_data_version=entry.minecraft_data_version,
            )
            self._write_entry_metadata_to_backup(updated)
            self._overwrite_target_from_backup(updated)
            try:
                updated.file_size = int(updated.backup_path.stat().st_size)
            except OSError:
                pass
            entries.pop(index)
            entries.insert(0, updated)
            self._assign_sort_indices(entries)
            self.save_entries(entries)
            return updated
        raise ProjectionLibraryError("未找到指定投影记录")

    def update_analysis_metadata(
        self,
        file_path: str | Path,
        analysis_output: dict[str, Any],
    ) -> ProjectionLibraryEntry | None:
        entries = self.load_entries()
        entry = self.find_entry_by_backup_path(file_path, entries=entries)
        if entry is None:
            return None
        structure_type = _extract_structure_type(analysis_output)
        tags = _tags_from_structure_type(structure_type)
        for index, candidate in enumerate(entries):
            if candidate.entry_id != entry.entry_id:
                continue
            candidate.structure_type = structure_type
            candidate.tags = tags
            _apply_file_metadata_to_entry(candidate, candidate.backup_path)
            entries[index] = candidate
            self.save_entries(entries)
            return candidate
        return None

    def update_preview_path(self, entry_id: str, preview_path: str | Path) -> ProjectionLibraryEntry:
        entries = self.load_entries()
        for index, entry in enumerate(entries):
            if entry.entry_id != entry_id:
                continue
            entry.preview_image_path = str(Path(preview_path))
            entries[index] = entry
            self.save_entries(entries)
            return entry
        raise ProjectionLibraryError("Projection library entry was not found")

    def reorder_entries(self, ordered_entry_ids: list[str]) -> None:
        entries = self.load_entries()
        by_id = {entry.entry_id: entry for entry in entries}
        ordered: list[ProjectionLibraryEntry] = []
        seen: set[str] = set()
        for entry_id in ordered_entry_ids:
            entry = by_id.get(entry_id)
            if entry is None or entry_id in seen:
                continue
            ordered.append(entry)
            seen.add(entry_id)
        ordered.extend(entry for entry in entries if entry.entry_id not in seen)
        self._assign_sort_indices(ordered)
        self.save_entries(ordered)

    def sort_entries(self, mode: str) -> None:
        entries = self.load_entries()
        if mode == "recent":
            entries.sort(key=lambda entry: entry.last_used_at or entry.imported_at, reverse=True)
        elif mode == "name":
            entries.sort(key=lambda entry: (entry.display_name.lower(), entry.original_file_name.lower()))
        else:
            entries.sort(key=lambda entry: (entry.sort_index, entry.display_name.lower()))
        self._assign_sort_indices(entries)
        self.save_entries(entries)

    def open_entry_backup(self, entry_id: str) -> Path:
        entry = self.touch_entry(entry_id)
        path = entry.backup_path
        if not path.is_file():
            raise ProjectionLibraryError("备份文件不存在，无法打开")
        return path

    def _write_entry_metadata_to_backup(self, entry: ProjectionLibraryEntry) -> None:
        backup = entry.backup_path
        if not backup.is_file():
            raise ProjectionLibraryError("Backup projection file is missing; cannot save metadata.")
        try:
            data = load_snbt_properties(backup)
            data.internal_name = entry.internal_name
            data.author = entry.author
            data.description = entry.description
            save_snbt_properties(data)
        except Exception as exc:
            raise ProjectionLibraryError(f"Failed to write projection metadata: {exc}") from exc
        entry.litematic_version = int(data.litematic_version)
        entry.minecraft_data_version = int(data.minecraft_data_version)

    def _entry_needs_file_metadata(self, entry: ProjectionLibraryEntry) -> bool:
        return not any(
            (
                entry.internal_name.strip(),
                entry.author.strip(),
                entry.description.strip(),
                entry.litematic_version,
                entry.minecraft_data_version,
            )
        )

    def _overwrite_target_from_backup(self, entry: ProjectionLibraryEntry) -> Path:
        backup = entry.backup_path
        if not backup.is_file():
            raise ProjectionLibraryError("备份文件不存在，无法保存属性")
        target = entry.original_path
        if target is not None and target.exists():
            write_target = target
        else:
            write_target = backup
        self._replace_file(backup, write_target)
        return write_target

    def _replace_file(self, source: Path, target: Path) -> None:
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            if source.resolve() != target.resolve():
                shutil.copy2(source, target)
                return
        except OSError:
            pass
        try:
            data = source.read_bytes()
        except OSError as exc:
            raise ProjectionLibraryError(f"读取备份文件失败：{exc}") from exc
        try:
            with NamedTemporaryFile(
                delete=False,
                dir=str(target.parent),
                prefix=f"{target.stem}_",
                suffix=target.suffix or ".tmp",
            ) as handle:
                handle.write(data)
                temp_path = Path(handle.name)
            temp_path.replace(target)
        except OSError as exc:
            raise ProjectionLibraryError(f"覆盖写入失败：{exc}") from exc

    def _trim_entries(
        self,
        entries: list[ProjectionLibraryEntry],
        limit: int | None = None,
    ) -> list[ProjectionLibraryEntry]:
        cap = max(1, int(limit or self._default_limit))
        if len(entries) <= cap:
            return entries
        kept = entries[:cap]
        for stale in entries[cap:]:
            try:
                stale.backup_path.unlink(missing_ok=True)
                preview = stale.preview_path
                if preview is not None:
                    preview.unlink(missing_ok=True)
            except OSError:
                pass
        return kept

    def _assign_sort_indices(self, entries: list[ProjectionLibraryEntry]) -> None:
        for index, entry in enumerate(entries):
            entry.sort_index = index


def _extract_structure_type(output: dict[str, Any]) -> str:
    derived = output.get("derived") if isinstance(output, dict) else None
    building = derived.get("building") if isinstance(derived, dict) else None
    if isinstance(building, dict):
        value = building.get("building_type")
        if value:
            return str(value)
    metadata = output.get("metadata") if isinstance(output, dict) else None
    if isinstance(metadata, dict):
        value = metadata.get("structure_type")
        if value:
            return str(value)
    return ""


def _tags_from_structure_type(structure_type: str) -> list[str]:
    value = structure_type.strip()
    if not value:
        return ["未分类"]
    labels = {
        "building": "建筑",
        "redstone": "红石",
        "farm": "农场",
        "terrain": "地形",
        "decoration": "装饰",
        "machine": "机器",
        "unknown": "未分类",
    }
    return [labels.get(value, value)]
