"""Exercise a release candidate's real, headless catalog IPC using its own pack-in."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import socket
import struct
import subprocess
import tempfile


def receive(stream):
    header = stream.read(4)
    if len(header) != 4:
        raise EOFError("worker closed without terminal response")
    size, = struct.unpack("<I", header)
    if size > 64 * 1024 * 1024:
        raise ValueError("frame exceeds wire limit")
    return json.loads(stream.read(size))


def run(args, directory):
    root = args.root.resolve()
    exe = str(args.core.resolve())
    env = {**os.environ, "LBA_DATA_ROOT": str(directory)}
    env.pop("NOVA_GAME_DATA_SNAPSHOT", None)
    ctx = json.loads(subprocess.check_output(
        [exe, "game-data", "context", "--store", str(directory / "game-data"),
         "--default-root", str(root / "pack-in/game-data/default")], cwd=root, env=env))
    if args.dist:
        # Read only compiled browser JS (not sources/maps/copied manifests).
        bindings = {match for js in (args.dist / "assets").glob("*.js")
                    for match in re.findall(rb"nova-catalog-v1:([0-9a-f]{64})", js.read_bytes())}
        assert len(bindings) == 1, f"expected exactly one bundled catalog binding in {args.dist}, got {bindings}"
        expected_id = bindings.pop().decode("ascii")
    else:
        expected_id = args.expected_content_id
    assert re.fullmatch("[0-9a-f]{64}", expected_id) and ctx["content_id"] == expected_id, "worker pack and UI binding differ"
    nonce = secrets.token_hex(24)
    with socket.socket() as server:
        server.bind(("127.0.0.1", 0))
        server.settimeout(120)
        server.listen(1)
        env.update(NOVA_GAME_DATA_SNAPSHOT=json.dumps(ctx), NOVA_DATA_WORKER_CONNECT=json.dumps(
            {"address": "127.0.0.1:%d" % server.getsockname()[1], "nonce": nonce}))
        worker = subprocess.Popen([exe, "data-worker"], cwd=root, env=env)
        try:
            with server.accept()[0] as conn, conn.makefile("rb") as stream:
                conn.settimeout(120)
                assert receive(stream) == nonce
                version = args.version or ctx["game_version"]
                request = {"operationId": "inventory-check", "kind": "layerView", "input": {
                    "action": "creativeInventory", "catalog": True, "version": version,
                    "contentId": ctx["content_id"], "language": "zh-CN", "resourcePacks": []}}
                raw = json.dumps(request).encode()
                conn.sendall(struct.pack("<I", len(raw)) + raw)
                while True:
                    response = receive(stream)
                    assert response["operationId"] == request["operationId"]
                    if response["state"] in ("Pending", "Running"):
                        continue
                    if response["state"] != "Ready":
                        raise RuntimeError(response)
                    data = response["result"]
                    assert data["contentId"] == ctx["content_id"] and data["version"] == version
                    items = data["items"]
                    groups = data["categories"]
                    indices = next(group["indices"] for group in groups if group["id"] == "base:dv-blocks")
                    assert len(indices) > 45 and len(indices) == len(set(indices))
                    bundled_file = Path(__file__).resolve().parents[1] / "src/services/creativeInventoryBundled.json"
                    bundled = json.loads(bundled_file.read_text(encoding="utf-8"))
                    assert data["contentId"] == expected_id, "release pack and cold category binding differ"
                    mask = next(int(mask) for mask, versions in bundled["versionsByMask"].items() if version in versions)
                    expected = {key for i, key in enumerate(bundled["categories"]) if mask & (1 << i)}
                    assert expected == {group["id"][9:] for group in groups if group["id"].startswith("category:")}
                    assert all(0 <= index < len(items) for group in groups for index in group["indices"])
                    assert all(item["blockState"] and item["state"]["id"] == item["id"] and
                               isinstance(item["state"]["properties"], dict) for item in items)
                    print(json.dumps({"version": version, "contentId": data["contentId"], "items": len(items),
                          "groups": len(groups), "last": items[indices[-1]]["blockState"]}, ensure_ascii=True))
                    break
            if worker.wait(timeout=10):
                raise RuntimeError("worker exited with error")
        finally:
            if worker.poll() is None:
                worker.kill()
                worker.wait()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--core", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--version")
    binding = parser.add_mutually_exclusive_group(required=True)
    binding.add_argument("--dist", type=Path, help="actual compiled desktop dist for the final candidate")
    binding.add_argument("--expected-content-id", help="exact Vite build input identity (pre-build preflight only)")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="nova-inventory-") as folder:
        run(args, Path(folder))
