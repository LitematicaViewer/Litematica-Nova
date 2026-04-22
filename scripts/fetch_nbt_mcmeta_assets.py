#!/usr/bin/env python3
"""CLI：下载 misode/mcmeta 至 ``data/minecraft-assets/nbt-viewer/<版本 id>/mcmeta``。

逻辑在 ``litematicaba.core.nbt_mcmeta_fetch``，与选项页「管理游戏资源」一致。
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "src"))

from litematicaba.core.nbt_mcmeta_fetch import run_mcmeta_fetch  # noqa: E402

DEFAULT_OUT_BASE = REPO_ROOT / "data" / "minecraft-assets" / "nbt-viewer"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", default=None, help="MC 版本 id，如 1.21.4；缺省为最新稳定版")
    ap.add_argument(
        "--output",
        type=Path,
        default=None,
        help="输出根目录（默认：<仓库>/data/minecraft-assets/nbt-viewer）",
    )
    ap.add_argument(
        "--with-ui-extras",
        action="store_true",
        help="在输出根目录写入 codicon、editor.css（整包外置用）",
    )
    ap.add_argument(
        "--copy-packaged-ui",
        action="store_true",
        help="从包内复制 index.html、editor.js（若存在）",
    )
    args = ap.parse_args()
    out_base = (args.output or DEFAULT_OUT_BASE).resolve()
    r = run_mcmeta_fetch(
        out_base=out_base,
        version_spec=args.version,
        with_ui_extras=args.with_ui_extras,
        copy_packaged_ui=args.copy_packaged_ui,
        repo_root=REPO_ROOT,
    )
    if r.warning:
        print(r.warning, file=sys.stderr)
    if not r.ok:
        print(r.message, file=sys.stderr)
        return 1
    print(r.message)
    print(f"目录: {r.mcmeta_dir}")
    print("请在应用中点「重新加载 3D」或重启以生效。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
