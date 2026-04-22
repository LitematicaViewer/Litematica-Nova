from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load_summary(path: Path) -> dict[str, dict[str, str]]:
    rows = path.read_text(encoding="utf-8").splitlines()
    if not rows:
        raise RuntimeError(f"empty summary file: {path}")
    header = rows[0].split("\t")
    table: dict[str, dict[str, str]] = {}
    for line in rows[1:]:
        if not line.strip():
            continue
        values = line.split("\t")
        row = dict(zip(header, values))
        case = row.get("case", "").strip()
        mode = row.get("mode", "").strip()
        if case and mode:
            table[f"{case}:{mode}"] = row
    return table


def parse_int(row: dict[str, str], key: str) -> int:
    value = (row.get(key) or "").strip()
    if not value:
        return 0
    return int(float(value))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("summary", type=Path)
    parser.add_argument(
        "--baseline",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "baselines" / "render_build_baseline.json",
    )
    args = parser.parse_args()

    baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
    summary = load_summary(args.summary)
    pipeline = str(baseline["formal_default_pipeline"])
    failed: list[str] = []

    for case_name, case_baseline in baseline["cases"].items():
        key = f"{case_name}:{pipeline}"
        row = summary.get(key)
        if row is None:
            failed.append(f"missing summary row: {key}")
            continue

        for metric, expected in case_baseline["expected_counts"].items():
            actual = parse_int(row, metric)
            if actual != int(expected):
                failed.append(
                    f"{key} count mismatch for {metric}: expected {expected}, got {actual}"
                )

        for metric, maximum in case_baseline["max_wall_ms"].items():
            actual = parse_int(row, metric)
            if actual > int(maximum):
                failed.append(
                    f"{key} wall regression for {metric}: limit {maximum}, got {actual}"
                )

    if failed:
        print("render build baseline check failed:", file=sys.stderr)
        for item in failed:
            print(f"  - {item}", file=sys.stderr)
        return 1

    print("render build baseline check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
