"""Windows 打包：从源码读取版本、计算 exe 基名、读写打包计数（供 pac-win.bat 调用）。"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

STATE_FILE = "pack_state.json"


def read_version(root: Path) -> str:
    init_path = root / "src" / "litematicaba" / "__init__.py"
    text = init_path.read_text(encoding="utf-8")
    m = re.search(r'__version__\s*=\s*["\']([^"\']+)["\']', text)
    if not m:
        return "0.1.0"
    return m.group(1).strip()


def state_path(root: Path) -> Path:
    return root / "bin" / STATE_FILE


def load_state(path: Path) -> dict:
    if not path.is_file():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {}


def compute(root: Path) -> tuple[str, str, int]:
    """返回 (PyInstaller --name 基名不含 .exe, 版本, 本次打包计数)。"""
    version = read_version(root)
    sp = state_path(root)
    state = load_state(sp)
    last_v = state.get("version")
    raw_c = state.get("pack_count", 0)
    try:
        last_c = int(raw_c)
    except (TypeError, ValueError):
        last_c = 0

    if last_v != version:
        pack_count = 1
    else:
        pack_count = last_c + 1

    basename = f"LitematicaBA-v.{version}-windows-pck.{pack_count:02d}"
    return basename, version, pack_count


def cmd_preview(root: Path) -> None:
    name, version, pack_count = compute(root)
    print(f"{name}|{version}|{pack_count}", end="")


def cmd_finalize(root: Path, version: str, pack_count: int) -> None:
    sp = state_path(root)
    sp.parent.mkdir(parents=True, exist_ok=True)
    data = {"version": version, "pack_count": pack_count}
    sp.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    argv = argv if argv is not None else sys.argv[1:]
    p = argparse.ArgumentParser(description="Pack name / version resolver")
    sub = p.add_subparsers(dest="cmd", required=True)
    prev = sub.add_parser("preview", help="print NAME|VERSION|PACKCOUNT for PyInstaller --name")
    prev.add_argument("root", type=Path)
    fin = sub.add_parser("finalize", help="write pack_state.json after successful build")
    fin.add_argument("root", type=Path)
    fin.add_argument("version")
    fin.add_argument("pack_count", type=int)
    args = p.parse_args(argv)
    root = args.root.resolve()
    if args.cmd == "preview":
        cmd_preview(root)
        return 0
    if args.cmd == "finalize":
        cmd_finalize(root, args.version, args.pack_count)
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
