#!/usr/bin/env python3
"""Compare cold Forge builds and graph coverage on one configured workspace."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import statistics
import subprocess
import tempfile
import time
from typing import Any


def resolve_binary(value: str) -> pathlib.Path:
    candidate = pathlib.Path(value).expanduser()
    if candidate.is_absolute() or "/" in value:
        binary = candidate.resolve()
    else:
        found = shutil.which(value)
        if found is None:
            raise ValueError(f"binary is not on PATH: {value}")
        binary = pathlib.Path(found).resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"binary is not an executable file: {binary}")
    return binary


def run_build(
    binary: pathlib.Path,
    repo: pathlib.Path,
    db: pathlib.Path,
    work: pathlib.Path,
    extra_env: dict[str, str] | None = None,
) -> dict[str, float | int]:
    stdout_path = work / "build.stdout"
    stderr_path = work / "build.stderr"
    command = [str(binary), "--root", str(repo), "--db", str(db), "build"]
    environment = os.environ.copy()
    if extra_env:
        environment.update(extra_env)
    if "FORGE_EXTRACTION_CACHE_DIR" not in environment:
        environment["FORGE_EXTRACTION_CACHE_DIR"] = str(work / "extraction-cache")
    start = time.perf_counter()
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        process = subprocess.Popen(command, cwd=repo, env=environment, stdout=stdout, stderr=stderr)
        usage = None
        if hasattr(os, "wait4"):
            while True:
                try:
                    _, status, usage = os.wait4(process.pid, 0)
                    break
                except InterruptedError:
                    continue
            process.returncode = os.waitstatus_to_exitcode(status)
        else:
            process.wait()
    elapsed = time.perf_counter() - start
    if process.returncode:
        detail = stderr_path.read_text(errors="replace").strip()
        raise RuntimeError(
            f"cold build failed ({process.returncode}) for {binary}: {detail or stdout_path.read_text(errors='replace')}"
        )
    if not db.is_file():
        raise RuntimeError(f"build succeeded without creating its database: {db}")

    result: dict[str, float | int] = {"wall_seconds": elapsed}
    if usage is not None:
        result["user_seconds"] = usage.ru_utime
        result["system_seconds"] = usage.ru_stime
        peak_rss = usage.ru_maxrss
        result["peak_rss_bytes"] = peak_rss if os.uname().sysname == "Darwin" else peak_rss * 1024
    return result


def query_overview(binary: pathlib.Path, repo: pathlib.Path, db: pathlib.Path) -> dict[str, Any]:
    command = [str(binary), "--root", str(repo), "--db", str(db), "query", "overview"]
    result = subprocess.run(command, cwd=repo, check=False, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(
            f"query overview failed ({result.returncode}) for {binary}: {result.stderr.strip()}"
        )
    try:
        overview = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"query overview returned invalid JSON for {binary}: {error}") from error

    metadata = {row["key"]: row["value"] for row in overview.get("metadata", [])}
    if pathlib.Path(metadata.get("workspace_root", "")).resolve() != repo:
        raise RuntimeError(
            f"index root mismatch: expected {repo}, got {metadata.get('workspace_root')!r}"
        )
    counts = overview.get("counts")
    languages = overview.get("languages", {}).get("items", [])
    if not isinstance(counts, dict) or not isinstance(languages, list):
        raise RuntimeError("query overview is missing graph counts or language coverage")

    resolved = sum(int(row.get("resolved", 0)) for row in languages)
    ambiguous = sum(int(row.get("ambiguous", 0)) for row in languages)
    unresolved = sum(int(row.get("unresolved", 0)) for row in languages)
    external = sum(int(row.get("external_imports", 0)) for row in languages)
    resolvable = resolved + ambiguous + unresolved
    components_page = overview.get("components", {})
    components = [item.get("path", item.get("name", "")) for item in components_page.get("items", [])]

    return {
        "workspace_root": metadata["workspace_root"],
        "counts": counts,
        "resolution": {
            "resolved": resolved,
            "ambiguous": ambiguous,
            "unresolved": unresolved,
            "external_imports": external,
            "resolved_percent_excluding_external": round(100 * resolved / resolvable, 4)
            if resolvable
            else 0.0,
        },
        "components": components,
    }


def database_bytes(db: pathlib.Path) -> int:
    return sum(
        path.stat().st_size
        for path in (db, pathlib.Path(f"{db}-wal"), pathlib.Path(f"{db}-shm"))
        if path.exists()
    )


def summarize_runs(runs: list[dict[str, Any]]) -> dict[str, Any]:
    keys = (
        "wall_seconds",
        "user_seconds",
        "system_seconds",
        "peak_rss_bytes",
        "database_bytes",
        "index_and_seal_ms",
        "seal_ms",
    )
    medians: dict[str, float | int] = {}
    for key in keys:
        values = [float(run[key]) for run in runs if key in run]
        if values:
            medians[key] = round(statistics.median(values), 3 if key.endswith("seconds") else 0)
    for key in ("extraction_cache_hits", "extracted_files", "ingested_files"):
        values = [
            float(run["pipeline"][key])
            for run in runs
            if isinstance(run.get("pipeline"), dict) and key in run["pipeline"]
        ]
        if values:
            medians[key] = round(statistics.median(values), 3)
    for key in ("index_and_seal_ms", "seal_ms"):
        values = [
            float(run["phases_ms"][key])
            for run in runs
            if isinstance(run.get("phases_ms"), dict) and key in run["phases_ms"]
        ]
        if values:
            medians[key] = round(statistics.median(values), 3)
    return {"runs": runs, "median": medians, "graph": runs[-1]["graph"]}


def compare(baseline: dict[str, Any], candidate: dict[str, Any]) -> dict[str, Any]:
    before = baseline["median"]
    after = candidate["median"]
    before_wall = before["wall_seconds"]
    after_wall = after["wall_seconds"]
    baseline_resolution = baseline["graph"]["resolution"]["resolved_percent_excluding_external"]
    candidate_resolution = candidate["graph"]["resolution"]["resolved_percent_excluding_external"]
    count_keys = ("files", "nodes", "edges", "doc_sections", "parse_errors", "unresolved", "external_imports")
    before_counts = baseline["graph"]["counts"]
    after_counts = candidate["graph"]["counts"]
    paired_wall_changes = [
        100 * (new["wall_seconds"] / old["wall_seconds"] - 1)
        for old, new in zip(baseline["runs"], candidate["runs"], strict=True)
        if old["wall_seconds"]
    ]
    return {
        "candidate_wall_change_percent": round(100 * (after_wall / before_wall - 1), 3)
        if before_wall
        else None,
        "resolved_coverage_change_percentage_points": round(candidate_resolution - baseline_resolution, 4),
        "median_paired_wall_change_percent": round(statistics.median(paired_wall_changes), 3)
        if paired_wall_changes
        else None,
        "graph_count_deltas": {
            key: int(after_counts.get(key, 0)) - int(before_counts.get(key, 0)) for key in count_keys
        },
        "components_match": baseline["graph"]["components"] == candidate["graph"]["components"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=pathlib.Path, required=True, help="workspace containing forge-mcp.yaml")
    parser.add_argument("--baseline-binary", required=True, help="older Forge executable or PATH command")
    parser.add_argument("--candidate-binary", required=True, help="newer Forge executable or PATH command")
    parser.add_argument("--repeats", type=int, default=5, help="fresh-database builds per binary (default: 5)")
    args = parser.parse_args()

    repo = args.repo.expanduser().resolve()
    if not repo.is_dir():
        parser.error(f"workspace is not a directory: {repo}")
    if not (repo / "forge-mcp.yaml").is_file():
        parser.error(f"workspace must contain forge-mcp.yaml: {repo}")
    if args.repeats < 1:
        parser.error("--repeats must be at least 1")
    try:
        baseline_binary = resolve_binary(args.baseline_binary)
        candidate_binary = resolve_binary(args.candidate_binary)
        results: dict[str, list[dict[str, Any]]] = {"baseline": [], "candidate": []}
        binaries = {"baseline": baseline_binary, "candidate": candidate_binary}

        # Alternate order to reduce systematic drift while keeping builds strictly sequential.
        for repetition in range(args.repeats):
            order = ("baseline", "candidate") if repetition % 2 == 0 else ("candidate", "baseline")
            for label in order:
                with tempfile.TemporaryDirectory(prefix="forge-cold-coverage-") as directory:
                    work = pathlib.Path(directory)
                    db = work / "index.sqlite"
                    timing = run_build(
                        binaries[label],
                        repo,
                        db,
                        work,
                        extra_env={"FORGE_EXTRACTION_CACHE_DIR": str(work / "extraction-cache")},
                    )
                    build = json.loads((work / "build.stdout").read_text())
                    graph = query_overview(binaries[label], repo, db)
                    run = {
                        "repetition": repetition + 1,
                        **timing,
                        "database_bytes": database_bytes(db),
                        "graph": graph,
                        "output_root": build.get("output_root"),
                        "phases_ms": {
                            key: value for key, value in build.items() if key.endswith("_ms")
                        },
                    }
                    if isinstance(build.get("pipeline"), dict):
                        run["pipeline"] = build["pipeline"]
                    results[label].append(run)

        report = {
            "workspace": str(repo),
            "config": str(repo / "forge-mcp.yaml"),
            "database_policy": "fresh temporary database per run; removed on completion",
            "extraction_cache_policy": "fresh temporary extraction cache per run; removed on completion",
            "build_policy": "sequential cold builds; forge-mcp.yaml and enabled linked_workspaces are read from the supplied workspace",
            "baseline_binary": str(baseline_binary),
            "candidate_binary": str(candidate_binary),
            "baseline": summarize_runs(results["baseline"]),
            "candidate": summarize_runs(results["candidate"]),
        }
        report["comparison"] = compare(report["baseline"], report["candidate"])
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
