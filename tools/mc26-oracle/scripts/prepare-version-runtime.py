#!/usr/bin/env python3
"""Prepare exact vanilla inputs for the existing headless data exporters.

Only official manifest objects are admitted. Game code/dependencies stay in the
local ignored cache; no renderer, server world or GUI is started here.
"""
import argparse
import hashlib
import json
import os
import pathlib
import urllib.request


MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"


def digest(path, algorithm="sha1"):
    h = hashlib.new(algorithm)
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch(entry, destination, candidates=()):
    expected = entry["sha1"]
    for path in [destination, *candidates]:
        if path.is_file() and path.stat().st_size == entry["size"] and digest(path) == expected:
            return path
    if not entry["url"].startswith("https://"):
        raise ValueError("Official runtime object is not HTTPS")
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + f".{os.getpid()}.partial")
    with urllib.request.urlopen(entry["url"], timeout=60) as source, temporary.open("wb") as output:
        while chunk := source.read(1024 * 1024):
            output.write(chunk)
    if temporary.stat().st_size != entry["size"] or digest(temporary) != expected:
        raise ValueError(f"Official runtime object content mismatch: {destination.name}")
    temporary.replace(destination)
    return destination


def prepare(version, manifest, cache, installs, libraries):
    record = next((entry for entry in manifest["versions"] if entry["id"] == version), None)
    if record is None:
        raise ValueError(f"No official source metadata for {version}")
    directory = cache / version
    directory.mkdir(parents=True, exist_ok=True)
    metadata = directory / "version.json"
    if not metadata.is_file() or digest(metadata) != record["sha1"]:
        data = urllib.request.urlopen(record["url"], timeout=30).read()
        if hashlib.sha1(data).hexdigest() != record["sha1"]:
            raise ValueError(f"Official version metadata mismatch: {version}")
        metadata.write_bytes(data)
    config = json.loads(metadata.read_bytes())
    if config["id"] != version:
        raise ValueError("Version identity differs from requested input")
    source = fetch(config["downloads"]["client"], directory / "client.jar",
                   [root / "versions" / version / f"{version}.jar" for root in installs])
    jars = []
    for lib in config["libraries"]:
        artifact = lib.get("downloads", {}).get("artifact")
        if artifact is None or "natives-" in lib["name"]:
            continue
        relative = pathlib.PurePosixPath(artifact["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("Invalid official library path")
        jars.append(fetch(artifact, cache / "libraries" / relative,
                          [root / relative for root in libraries]))
    mappings = config["downloads"].get("client_mappings")
    mappings_path = fetch(mappings, directory / "client-mappings.txt") if mappings else None
    descriptor = {
        "schema": "nova.version-runtime-input.v1",
        "minecraft_version": version,
        "metadata_sha1": record["sha1"],
        "source_url": config["downloads"]["client"]["url"],
        "client_jar": str(source.resolve()),
        "client_sha256": digest(source, "sha256"),
        "client_bytes": source.stat().st_size,
        "java_major": config.get("javaVersion", {}).get("majorVersion", 8),
        "mappings": str(mappings_path.resolve()) if mappings_path else None,
        "classpath": os.pathsep.join(str(p.resolve()) for p in [source, *jars]),
    }
    output = directory / "runtime.json"
    output.write_text(json.dumps(descriptor, indent=2) + "\n", encoding="utf-8")
    return {"version": version, "descriptor": str(output), "official_mappings": bool(mappings_path)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("versions", nargs="+")
    parser.add_argument("--cache", type=pathlib.Path, default=pathlib.Path(".tmp/game-data/version-runtime"))
    parser.add_argument("--minecraft-root", action="append", type=pathlib.Path, default=[])
    parser.add_argument("--libraries", action="append", type=pathlib.Path, default=[])
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    cached = args.cache / "version_manifest_v2.json"
    if not cached.is_file():
        cached.write_bytes(urllib.request.urlopen(MANIFEST, timeout=30).read())
    manifest = json.loads(cached.read_bytes())
    # Metadata and dependencies are reused by verified content, never merely by
    # filename/version. Keep downloads bounded and aggregate independent errors.
    failures = []
    for version in dict.fromkeys(args.versions):
        try:
            print(json.dumps(prepare(version, manifest, args.cache, args.minecraft_root,
                                     [*args.libraries, *(p / "libraries" for p in args.minecraft_root)])), flush=True)
        except Exception as error:
            failures.append({"version": version, "error": str(error)})
    if failures:
        print(json.dumps({"failed": failures}, ensure_ascii=False))
        raise SystemExit(1)


if __name__ == "__main__":
    main()
