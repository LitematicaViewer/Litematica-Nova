from __future__ import annotations

import json
import sys
from pathlib import Path

from litematicaba.core.native_backend_bridge import _analyze_litematic_direct
from litematicaba.core.snbt_properties import load_snbt_properties, snbt_properties_to_dict


def _emit(payload: dict) -> int:
    sys.stdout.write(json.dumps(payload, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()
    return 0


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) < 2:
        raise SystemExit("usage: viewer_backend_worker <analyze|load-and-analyze> <file> [--include-entities]")
    command = args.pop(0)
    if command not in {"analyze", "load-and-analyze"}:
        raise SystemExit(f"unknown command: {command}")
    file_path = Path(args.pop(0)).resolve()
    include_entities = "--include-entities" in args
    result = _analyze_litematic_direct(file_path, include_entities=include_entities)
    if command == "analyze":
        return _emit({"file_path": result.file_path, "output": result.output})
    snbt = load_snbt_properties(file_path)
    return _emit(
        {
            "file_path": result.file_path,
            "output": result.output,
            "snbt": snbt_properties_to_dict(snbt),
        }
    )


if __name__ == "__main__":
    raise SystemExit(main())
