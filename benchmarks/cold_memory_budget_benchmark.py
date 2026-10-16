#!/usr/bin/env python3
"""Compare cold builds, then measure candidate memory budgets on a workspace copy."""

from __future__ import annotations

import argparse
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import sqlite3
from statistics import median
import sys
import tempfile
from typing import Any

from benchmark_profile import load_profile
import cold_test_coverage_benchmark as cold


def fixture_commitments(database: Path) -> dict[str, Any]:
    """Fingerprint persisted domain Merkle rows separately from engine-version metadata."""
    digest = hashlib.sha256()
    count = 0
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as connection:
        for domain, owner_id, value in connection.execute(
            "SELECT domain, owner_id, digest FROM domain_commitments ORDER BY domain, owner_id"
        ):
            row = json.dumps([domain, owner_id, value.hex()], separators=(",", ":")).encode()
            digest.update(len(row).to_bytes(8, "big"))
            digest.update(row)
            count += 1
        metadata = dict(connection.execute(
            "SELECT key, value FROM metadata "
            "WHERE key IN ('index_semantics_version', 'manifest_digest', 'adapter')"
        ))
    if not count:
        raise RuntimeError("build produced no domain commitments")
    adapter = metadata.pop("adapter")
    return {
        "fixture_merkle_sha256": digest.hexdigest(),
        "fixture_merkle_rows": count,
        "commitment_metadata": {**metadata, "adapter_sha256": hashlib.sha256(adapter.encode()).hexdigest()},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True, help="isolated workspace copy")
    parser.add_argument("--baseline-binary", required=True)
    parser.add_argument("--candidate-binary", required=True)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--matrix-repeats", type=int, default=3,
                        help="repeats per candidate budget; 0 runs only paired builds")
    parser.add_argument("--budgets-mb", type=int, nargs="+", default=[50, 200, 500, 1000])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if min(args.repeats, *args.budgets_mb) < 1 or args.matrix_repeats < 0:
        parser.error("paired repeats and budgets must be positive; matrix repeats must be nonnegative")
    profile = load_profile(args.profile, args.workspace)
    workspace = profile.workspace_root.resolve()
    binaries = {
        "baseline": cold.resolve_binary(args.baseline_binary),
        "candidate": cold.resolve_binary(args.candidate_binary),
    }
    results: dict[str, list[dict[str, Any]]] = {"baseline": [], "candidate": []}
    matrix: dict[str, list[dict[str, Any]]] = {str(budget): [] for budget in args.budgets_mb}
    report: dict[str, Any] = {
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "command": [sys.executable, *sys.argv],
        "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
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
                    "phases_ms": {key: value for key, value in build.items() if key.endswith("_ms")},
                    **fixture_commitments(database),
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
    baseline_wall = median(row["wall_seconds"] for row in results["baseline"])
    candidate_wall = median(row["wall_seconds"] for row in results["candidate"])
    paired_change = median(
        candidate["wall_seconds"] / baseline["wall_seconds"] - 1
        for baseline, candidate in zip(results["baseline"], results["candidate"], strict=True)
    )
    report["performance_gate_passed"] = candidate_wall <= baseline_wall and paired_change <= 0
    report["performance_gate_policy"] = "candidate wall median and median paired change must both be no worse than baseline; evaluate unrounded runs"
    for repetition in range(1, args.matrix_repeats + 1):
        budgets = args.budgets_mb if repetition % 2 else list(reversed(args.budgets_mb))
        for budget in budgets:
            matrix[str(budget)].append(run("candidate", repetition, budget))
            save()
    candidate_rows = [row for rows in (results["candidate"], *matrix.values()) for row in rows]
    report["candidate_matrix_bit_identical"] = len({row["output_root"] for row in candidate_rows}) == 1
    all_rows = [*results["baseline"], *candidate_rows]
    report["bit_identical_output_roots"] = len({row["output_root"] for row in all_rows}) == 1
    report["bit_identical_fixture_merkle_rows"] = len({
        (row["fixture_merkle_sha256"], row["fixture_merkle_rows"]) for row in all_rows
    }) == 1
    report["deterministic_output_roots_by_binary"] = {
        "baseline": len({row["output_root"] for row in results["baseline"]}) == 1,
        "candidate": report["candidate_matrix_bit_identical"],
    }
    medians = {budget: cold.summarize_runs(rows)["median"]
               for budget, rows in matrix.items() if rows}
    report["recommended_budget_mb"] = None
    if medians:
        fastest = min(row["wall_seconds"] for row in medians.values())
        eligible = [budget for budget, row in medians.items()
                    if row["wall_seconds"] <= fastest * 1.05]
        report["recommended_budget_mb"] = int(min(
            eligible,
            key=lambda budget: (medians[budget].get("peak_rss_bytes", float("inf")),
                                medians[budget]["wall_seconds"], int(budget)),
        ))
    report["recommendation_policy"] = "lowest median peak RSS within 5% of fastest median wall time; ties prefer latency then smaller budget"
    save()
    return 0 if (
        report["performance_gate_passed"]
        and report["bit_identical_fixture_merkle_rows"]
        and all(report["deterministic_output_roots_by_binary"].values())
    ) else 1


if __name__ == "__main__":
    raise SystemExit(main())
