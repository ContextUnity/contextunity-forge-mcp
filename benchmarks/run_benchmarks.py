#!/usr/bin/env python3
"""Run Forge/Codebase resource benchmarks and answer-quality capture."""

from __future__ import annotations

import argparse
import datetime
import pathlib
import tempfile

import mcp_tool_comparison_benchmark as comparison
import mcp_tool_quality_benchmark as quality
from benchmark_profile import load_profile


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=pathlib.Path, required=True)
    parser.add_argument("--workspace", type=pathlib.Path)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--scenario")
    parser.add_argument("--mode", choices=("all", "comparison", "quality"), default="all")
    parser.add_argument("--output-dir", type=pathlib.Path)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")

    try:
        profile = load_profile(args.profile, args.workspace)
    except ValueError as error:
        parser.error(str(error))
    if args.scenario and args.scenario not in {item["id"] for item in profile.scenarios}:
        parser.error(f"unknown scenario {args.scenario!r} in profile")

    run_id = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output_dir = args.output_dir or pathlib.Path(tempfile.gettempdir()) / "benchmark-runs" / profile.name / run_id
    output_dir = output_dir.expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    if args.mode in ("all", "comparison"):
        comparison.benchmark(
            args.repeats,
            output_dir / "comparison.json",
            profile,
            args.scenario,
        )
    if args.mode in ("all", "quality"):
        quality.run(
            args.repeats,
            output_dir / "quality.json",
            profile,
            args.scenario,
        )
    print(f"results: {output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
