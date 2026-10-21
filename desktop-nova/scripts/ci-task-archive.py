"""Private task files only; the caller authenticates the envelope before extraction."""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tarfile
import zipfile


def source(request_file, tar_file, destination):
    request = json.loads(Path(request_file).read_text(encoding="utf8"))
    selected = set(request.pop("files"))
    amice = Path(request.pop("amiceSource"))
    llvm = request.pop("llvmSourceArchive", None)
    records = []
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as output:
        with tarfile.open(tar_file, "r|") as archive:
            for member in archive:
                if member.name not in selected:
                    continue
                if not member.isfile() or member.size > 64 * 1024**2:
                    raise ValueError("Unexpected source entry")
                data = archive.extractfile(member).read()
                records.append({"path": member.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
                output.writestr("source/" + member.name, data)
        if {entry["path"] for entry in records} != selected:
            raise ValueError("The immutable source archive is incomplete")
        manifest = {"schema": "nova.private-source-snapshot.v1", "sourceSha": request["sourceSha"], "target": request["target"], "files": sorted(records, key=lambda entry: entry["path"])}
        output.writestr("source/snapshot.json", json.dumps(manifest, separators=(",", ":")))
        extensions = {".rs", ".toml", ".lock", ".cc", ".cpp", ".c", ".h", ".hh", ".hpp", ".vm", ".txt", ".md", ".def", ".inc"}
        for directory, names, files in os.walk(amice):
            names[:] = [name for name in names if name not in {"target", ".git", ".idea"} and not (Path(directory) / name).is_symlink()]
            for name in files:
                filename = Path(directory) / name
                if filename.is_symlink() or filename.suffix not in extensions or filename.stat().st_size > 16 * 1024**2:
                    continue
                output.write(filename, "tooling/amice/" + filename.relative_to(amice).as_posix())
        wrapper = next(entry for entry in records if entry["path"] == "desktop-nova/scripts/protect-rustc.py")
        # The same exact source bytes are used by bootstrap's target-format probe.
        with tarfile.open(tar_file, "r") as archive:
            output.writestr("tooling/protect-rustc.py", archive.extractfile(wrapper["path"]).read())
        if llvm:
            filename = Path(llvm)
            if hashlib.sha256(filename.read_bytes()).hexdigest() != "62f2f13ff25b1bb28ea507888e858212d19aafb65e8e72b4a65ee0629ec4ae0c":
                raise ValueError("The official LLVM source identity differs")
            output.write(filename, "tooling/llvm-project-22.1.2.src.tar.xz", compress_type=zipfile.ZIP_STORED)
        output.writestr("task.json", json.dumps(request, separators=(",", ":")))


def extract(archive_file, destination, direction):
    root = Path(destination)
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    with zipfile.ZipFile(archive_file) as archive:
        entries = archive.infolist()
        if len(entries) > 40000 or sum(entry.file_size for entry in entries) > 4 * 1024**3:
            raise ValueError("Task archive exceeds its budget")
        names = set()
        for entry in entries:
            name = entry.filename
            pieces = name.split("/")
            if name in names or "\\" in name or ":" in name or name.startswith("/") or any(part in ("", ".", "..") for part in pieces) or entry.is_dir() or stat.S_ISLNK(entry.external_attr >> 16):
                raise ValueError("Unexpected task archive path")
            if direction == "source" and name != "task.json" and not name.startswith(("source/", "tooling/")):
                raise ValueError("Unexpected source task content")
            if direction == "result" and name != "result.json" and not name.startswith("result/"):
                raise ValueError("Unexpected result task content")
            names.add(name)
            output = root.joinpath(*pieces)
            output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            with archive.open(entry) as source_file, output.open("xb") as destination_file:
                while True:
                    data = source_file.read(1024 * 1024)
                    if not data:
                        break
                    destination_file.write(data)


def result(request_file, build_directory, destination, *extra_logs):
    request = json.loads(Path(request_file).read_text(encoding="utf8"))
    root = Path(build_directory)
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
        request["internalSymbolsPresent"] = False
        if root.exists():
            for filename in root.rglob("*"):
                if not filename.is_file() or filename.is_symlink():
                    continue
                relative = filename.relative_to(root).as_posix()
                if filename.name == "private-build.json" or relative == "backend/libnova_protected_module.dylib" or filename.name == "receipt.json" or filename.suffix == ".log":
                    archive.write(filename, "result/" + relative)
            manifest = root / "private-build.json"
            if manifest.is_file():
                build = json.loads(manifest.read_text(encoding="utf8"))
                symbols = Path(build.get("privateDebugDirectory", "")) / "libnova_protected_module.dylib.dSYM"
                if symbols.is_dir() and not symbols.is_symlink():
                    for filename in symbols.rglob("*"):
                        if filename.is_file() and not filename.is_symlink():
                            archive.write(filename, "result/internal-symbols/" + filename.relative_to(symbols.parent).as_posix())
                            request["internalSymbolsPresent"] = True
        for index, item in enumerate(extra_logs):
            path = Path(item)
            files = path.rglob("*.log") if path.is_dir() else [path]
            for filename in files:
                if filename.is_file() and not filename.is_symlink() and filename.suffix == ".log" and filename.stat().st_size <= 256 * 1024**2:
                    relative = filename.relative_to(path).as_posix() if path.is_dir() else filename.name
                    archive.write(filename, f"result/diagnostics/{index}/" + relative)
        archive.writestr("result.json", json.dumps(request, separators=(",", ":")))


if __name__ == "__main__":
    try:
        {"source": source, "extract": extract, "result": result}[sys.argv[1]](*sys.argv[2:])
    except Exception:
        # No private source path, source fragment or subprocess output reaches stdout.
        print("Task archive processing failed.", file=sys.stderr)
        raise SystemExit(1)
