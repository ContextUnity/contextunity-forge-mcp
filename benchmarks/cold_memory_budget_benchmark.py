#!/usr/bin/env python3
"""Compare cold builds, then measure candidate memory budgets on a workspace copy."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import sys
import tempfile
from typing import Any

from benchmark_profile import load_profile
import cold_test_coverage_benchmark as cold


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True, help="isolated workspace copy")
    parser.add_argument("--baseline-binary", required=True)
    parser.add_argument("--candidate-binary", required=True)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--matrix-repeats", type=int, default=3)
    parser.add_argument("--budgets-mb", type=int, nargs="+", default=[50, 200, 500, 1000])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if min(args.repeats, args.matrix_repeats, *args.budgets_mb) < 1:
        parser.error("repeat counts and budgets must be positive")
    profile = load_profile(args.profile, args.workspace)
    workspace = profile.workspace_root.resolve()
    binaries = {
        "baseline": cold.resolve_binary(args.baseline_binary),
        "candidate": cold.resolve_binary(args.candidate_binary),
    }
    results: dict[str, list[dict[str, Any]]] = {"baseline": [], "candidate": []}
    matrix: dict[str, list[dict[str, Any]]] = {str(budget): [] for budget in args.budgets_mb}
    report: dict[str, Any] = {
        "workspace": str(workspace),
        "profile": str(args.profile.resolve()),
        "baseline_binary": str(binaries["baseline"]),
        "candidate_binary": str(binaries["candidate"]),
        "binary_sha256": {key: hashlib.sha256(binary.read_bytes()).hexdigest() for key, binary in binaries.items()},
        "host": {"platform": platform.platform(), "cpu_count": os.cpu_count(),
                 "rayon_num_threads": os.environ.get("RAYON_NUM_THREADS")},
        "policy": "sequential builds; fresh temporary databases; supplied isolated workspace",
        "budget_policy": "comparison uses adaptive budget; matrix sets FORGE_MEMORY_BUDGET_MB",
    }

    def run(label: str, repetition: int, budget: int | None = None) -> dict[str, Any]:
        previous = os.environ.pop("FORGE_MEMORY_BUDGET_MB", None)
        try:
            if budget is not None:
                os.environ["FORGE_MEMORY_BUDGET_MB"] = str(budget)
            with tempfile.TemporaryDirectory(prefix="forge-cold-budget-") as directory:
                work = Path(directory)
                database = work / "index.sqlite"
                timing = cold.run_build(binaries[label], workspace, database, work)
                build = json.loads((work / "build.stdout").read_text())
                effective_budget = build.get("memory_budget_bytes")
                if budget is not None and effective_budget != budget * 1024 * 1024:
                    raise RuntimeError(
                        f"requested {budget} MiB, build reported {effective_budget!r} bytes; "
                        "remove the workspace's explicit YAML budget before the matrix"
                    )
                result = {
                    "repetition": repetition,
                    **timing,
                    "database_bytes": cold.database_bytes(database),
                    "graph": cold.query_overview(binaries[label], workspace, database),
                    "output_root": build["output_root"],
                    "memory_budget_bytes": effective_budget,
                }
                print(json.dumps({"binary": label, "budget_mb": budget, **timing}), file=sys.stderr, flush=True)
                return result
        finally:
            os.environ.pop("FORGE_MEMORY_BUDGET_MB", None)
            if previous is not None:
                os.environ["FORGE_MEMORY_BUDGET_MB"] = previous

    def save() -> None:
        report.update({key: cold.summarize_runs(rows) for key, rows in results.items() if rows})
        report["budget_matrix"] = {key: cold.summarize_runs(rows) for key, rows in matrix.items() if rows}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

    for repetition in range(1, args.repeats + 1):
        order = ("baseline", "candidate") if repetition % 2 else ("candidate", "baseline")
        for label in order:
            results[label].append(run(label, repetition))
            save()
    report["comparison"] = cold.compare(report["baseline"], report["candidate"])
    for repetition in range(1, args.matrix_repeats + 1):
        budgets = args.budgets_mb if repetition % 2 else list(reversed(args.budgets_mb))
        for budget in budgets:
            matrix[str(budget)].append(run("candidate", repetition, budget))
            save()
    all_rows = [row for rows in (*results.values(), *matrix.values()) for row in rows]
    report["bit_identical_output_roots"] = len({row["output_root"] for row in all_rows}) == 1
    save()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
