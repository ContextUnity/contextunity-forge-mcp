#!/usr/bin/env python3
"""Measure Forge extraction-cache reuse between equivalent workspace copies."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import sys
import tempfile
from typing import Any

import cold_test_coverage_benchmark as cold


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def has_cache_entries(directory: Path) -> bool:
    return any(path.is_file() for path in directory.rglob("*"))


def coverage_signature(graph: dict[str, Any]) -> dict[str, Any]:
    return {
        "counts": graph["counts"],
        "resolution": graph["resolution"],
        "components": graph["components"],
    }


def run_build(
    binary: Path,
    workspace: Path,
    directory: Path,
    cache_directory: Path,
) -> dict[str, Any]:
    directory.mkdir(parents=True, exist_ok=True)
    database = directory / "index.sqlite"
    timing = cold.run_build(
        binary,
        workspace,
        database,
        directory,
        extra_env={"FORGE_EXTRACTION_CACHE_DIR": str(cache_directory)},
    )
    build = json.loads((directory / "build.stdout").read_text())
    pipeline = build.get("pipeline", {})
    if not isinstance(pipeline, dict):
        raise RuntimeError("build report has an invalid pipeline metrics object")
    output_root = build.get("output_root")
    if not isinstance(output_root, str) or not output_root:
        raise RuntimeError("build report has no output root")
    return {
        **timing,
        "graph": cold.query_overview(binary, workspace, database),
        "output_root": output_root,
        "phases_ms": {key: value for key, value in build.items() if key.endswith("_ms")},
        "pipeline": pipeline,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace-a", type=Path, required=True, help="cache-seeding workspace copy")
    parser.add_argument("--workspace-b", type=Path, required=True, help="independent cache-consumer copy")
    parser.add_argument("--candidate-binary", required=True, help="Forge executable under measurement")
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    workspace_a = args.workspace_a.expanduser().resolve()
    workspace_b = args.workspace_b.expanduser().resolve()
    if workspace_a == workspace_b:
        parser.error("workspace copies must have different roots")
    for workspace in (workspace_a, workspace_b):
        if not workspace.is_dir() or not (workspace / "forge-mcp.yaml").is_file():
            parser.error(f"workspace must be a directory containing forge-mcp.yaml: {workspace}")
    if file_sha256(workspace_a / "forge-mcp.yaml") != file_sha256(workspace_b / "forge-mcp.yaml"):
        parser.error("workspace copies must use byte-identical forge-mcp.yaml configurations")
    if args.repeats < 1:
        parser.error("--repeats must be at least 1")

    try:
        binary = cold.resolve_binary(args.candidate_binary)
        cold_runs: list[dict[str, Any]] = []
        seed_runs: list[dict[str, Any]] = []
        warm_runs: list[dict[str, Any]] = []
        report: dict[str, Any] = {
            "workspace_a": str(workspace_a),
            "workspace_b": str(workspace_b),
            "config_sha256": file_sha256(workspace_a / "forge-mcp.yaml"),
            "candidate_binary": str(binary),
            "candidate_binary_sha256": file_sha256(binary),
            "runner_sha256": file_sha256(Path(__file__).resolve()),
            "host": {
                "platform": platform.platform(),
                "cpu_count": os.cpu_count(),
                "rayon_num_threads": os.environ.get("RAYON_NUM_THREADS"),
            },
            "policy": (
                "Forge-only sequential builds with a fresh database for every run; "
                "each repetition uses a new cache seeded in workspace A and consumed in workspace B"
            ),
        }

        for repetition in range(1, args.repeats + 1):
            with tempfile.TemporaryDirectory(prefix="forge-cross-worktree-cache-") as temporary:
                root = Path(temporary)
                shared_cache = root / "shared-cache"
                cold_cache = root / "cold-cache"

                def measure_cold() -> dict[str, Any]:
                    return run_build(binary, workspace_b, root / "cold-work", cold_cache)

                def measure_seed_and_warm() -> tuple[dict[str, Any], dict[str, Any]]:
                    seeded = run_build(binary, workspace_a, root / "seed-work", shared_cache)
                    if not has_cache_entries(shared_cache):
                        raise RuntimeError("Forge did not publish extraction-cache entries during the seed build")
                    warmed = run_build(binary, workspace_b, root / "warm-work", shared_cache)
                    return seeded, warmed

                if repetition % 2:
                    cold_run = measure_cold()
                    seed_run, warm_run = measure_seed_and_warm()
                else:
                    seed_run, warm_run = measure_seed_and_warm()
                    cold_run = measure_cold()

                cache_hits = int(warm_run["pipeline"].get("extraction_cache_hits", 0))
                if cache_hits == 0:
                    raise RuntimeError(
                        f"workspace B reused no extraction facts from workspace A in repetition {repetition}"
                    )
                if coverage_signature(cold_run["graph"]) != coverage_signature(warm_run["graph"]):
                    raise RuntimeError("cold and cross-worktree cached Forge builds have different coverage")
                if cold_run["output_root"] != warm_run["output_root"]:
                    raise RuntimeError(f"cold and cross-worktree cached Forge builds have different output roots: cold={cold_run['output_root']} vs warm={warm_run['output_root']}")
                if coverage_signature(seed_run["graph"]) != coverage_signature(warm_run["graph"]):
                    raise RuntimeError("cache-seeding and cache-consuming Forge builds have different coverage")

                for run, destination in (
                    (cold_run, cold_runs),
                    (seed_run, seed_runs),
                    (warm_run, warm_runs),
                ):
                    run["repetition"] = repetition
                    destination.append(run)

            report.update(
                {
                    "cold_workspace_b": cold.summarize_runs(cold_runs),
                    "cache_seed_workspace_a": cold.summarize_runs(seed_runs),
                    "cached_workspace_b": cold.summarize_runs(warm_runs),
                    "cross_worktree_cache_hits": [
                        int(run["pipeline"].get("extraction_cache_hits", 0)) for run in warm_runs
                    ],
                    "cross_worktree_cached_wall_change_percent": round(
                        100
                        * (
                            statistics.median(run["wall_seconds"] for run in warm_runs)
                            / statistics.median(run["wall_seconds"] for run in cold_runs)
                            - 1
                        ),
                        3,
                    ),
                    "output_roots_match": len(
                        {run["output_root"] for run in (*cold_runs, *warm_runs)}
                    )
                    == 1,
                    "coverage_match": all(
                        coverage_signature(run["graph"])
                        == coverage_signature(cold_runs[0]["graph"])
                        for run in (*seed_runs, *warm_runs, *cold_runs)
                    ),
                }
            )
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

        return 0 if report["output_roots_match"] and report["coverage_match"] else 1
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
