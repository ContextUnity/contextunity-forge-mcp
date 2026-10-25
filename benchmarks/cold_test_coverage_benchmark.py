#!/usr/bin/env python3
"""Compare cold Forge builds and graph coverage on one configured workspace."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Any

from cold_coverage_metrics import compare_evidence, digest, evidence, metrics


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
) -> dict[str, Any]:
    stdout_path = work / "build.stdout"
    stderr_path = work / "build.stderr"
    command = [str(binary), "--root", str(repo), "--db", str(db), "--format", "json", "build"]
    environment = os.environ.copy()
    if extra_env:
        environment.update(extra_env)
    if "FORGE_EXTRACTION_CACHE_DIR" not in environment:
        environment["FORGE_EXTRACTION_CACHE_DIR"] = str(work / "extraction-cache")
    host_load_before = list(os.getloadavg()) if hasattr(os, "getloadavg") else None
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

    result: dict[str, Any] = {"wall_seconds": elapsed, "host_load_before_build": host_load_before}
    if usage is not None:
        result["user_seconds"] = usage.ru_utime
        result["system_seconds"] = usage.ru_stime
        peak_rss = usage.ru_maxrss
        result["peak_rss_bytes"] = peak_rss if os.uname().sysname == "Darwin" else peak_rss * 1024
    return result


def query_overview(binary: pathlib.Path, repo: pathlib.Path, db: pathlib.Path) -> dict[str, Any]:
    command = [str(binary), "--root", str(repo), "--db", str(db), "--format", "json", "query", "overview"]
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

    # Overview collections are paged; aggregate the complete persisted snapshot.
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as connection:
        persisted_metadata = dict(connection.execute(
            "SELECT key,value FROM metadata WHERE key IN ('index_semantics_version','indexer_engine')"
        ))
        statuses = dict(connection.execute(
            "SELECT status,sum(records) FROM coverage_language_counts GROUP BY status"
        ))
        components = [row[0] for row in connection.execute(
            "SELECT DISTINCT p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id "
            "WHERE n.kind='component' ORDER BY p.path"
        )]
    resolved = int(statuses.get("resolved", 0))
    ambiguous = int(statuses.get("ambiguous", 0))
    unresolved = int(statuses.get("unresolved", 0))
    external = int(statuses.get("external", 0))
    resolvable = resolved + ambiguous + unresolved

    return {
        "workspace_root": metadata["workspace_root"],
        "index_semantics_version": persisted_metadata.get("index_semantics_version"),
        "indexer_engine": persisted_metadata.get("indexer_engine"),
        "compiled_profiles": overview.get("compiled_profiles"),
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


def domain_commitments(db: pathlib.Path) -> dict[str, str]:
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as connection:
        return dict(connection.execute(
            "SELECT domain, CASE WHEN typeof(digest)='blob' THEN lower(hex(digest)) ELSE digest END "
            "FROM domain_commitments WHERE owner_id=0 ORDER BY domain"
        ))


def root_inputs_digest(db: pathlib.Path) -> str:
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as connection:
        metadata = dict(connection.execute(
            "SELECT key,value FROM metadata WHERE key IN "
            "('schema_version','indexer_engine','workspace_root','adapter','corpus_hash',"
            "'commitment_algorithm','inventory_snapshot','manifest_digest')"
        ))
    if len(metadata) != 8:
        raise RuntimeError("missing commitment root input metadata")
    return hashlib.sha256(json.dumps(metadata, sort_keys=True).encode()).hexdigest()


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
        "resolution_match": baseline["graph"]["resolution"] == candidate["graph"]["resolution"],
        "domain_commitments_match": len(baseline["runs"][0]["domain_commitments"]) == 17
        and all(run["domain_commitments"] == baseline["runs"][0]["domain_commitments"]
                for run in baseline["runs"] + candidate["runs"]),
        "root_inputs_match": len({run["root_inputs_digest"]
                                  for run in baseline["runs"] + candidate["runs"]}) == 1,
        "output_roots_match": len({run["output_root"] for run in baseline["runs"] + candidate["runs"]}) == 1
        and baseline["runs"][0]["output_root"] is not None,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=pathlib.Path, required=True, help="workspace containing forge-mcp.yaml")
    parser.add_argument("--baseline-binary", required=True, help="older Forge executable or PATH command")
    parser.add_argument("--candidate-binary", required=True, help="newer Forge executable or PATH command")
    parser.add_argument("--repeats", type=int, default=5, help="fresh-database builds per binary (default: 5)")
    parser.add_argument("--max-peak-rss-mb", type=float, help="require every candidate run to stay below this RSS limit in decimal MB")
    parser.add_argument("--output", type=pathlib.Path, help="write the JSON report to this file")
    parser.add_argument("--artifacts-dir", type=pathlib.Path, help="retain logs, canonical evidence, and the first database per binary outside measured build time")
    parser.add_argument("--comparison-policy", choices=("strict", "coverage"), default="strict",
                        help="strict requires identical versioned commitments; coverage gates canonical code losses and per-binary determinism instead")
    args = parser.parse_args()

    repo = args.repo.expanduser().resolve()
    if not repo.is_dir():
        parser.error(f"workspace is not a directory: {repo}")
    if not (repo / "forge-mcp.yaml").is_file():
        parser.error(f"workspace must contain forge-mcp.yaml: {repo}")
    if args.repeats < 1:
        parser.error("--repeats must be at least 1")
    if args.max_peak_rss_mb is not None and args.max_peak_rss_mb <= 0:
        parser.error("--max-peak-rss-mb must be positive")
    try:
        baseline_binary = resolve_binary(args.baseline_binary)
        candidate_binary = resolve_binary(args.candidate_binary)
        results: dict[str, list[dict[str, Any]]] = {"baseline": [], "candidate": []}
        binaries = {"baseline": baseline_binary, "candidate": candidate_binary}
        first_evidence = {}
        binary_hashes = {label: hashlib.sha256(binary.read_bytes()).hexdigest()
                         for label, binary in binaries.items()}
        if args.artifacts_dir:
            args.artifacts_dir.mkdir(parents=True, exist_ok=True)

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
                        "domain_commitments": domain_commitments(db),
                        "root_inputs_digest": root_inputs_digest(db),
                        "phases_ms": {
                            key: value for key, value in build.items() if key.endswith("_ms")
                        },
                    }
                    if isinstance(build.get("pipeline"), dict):
                        run["pipeline"] = build["pipeline"]
                    graph_evidence = evidence(db)
                    run["code_coverage"] = metrics(graph_evidence, run["database_bytes"], run["wall_seconds"])
                    if label not in first_evidence:
                        first_evidence[label] = graph_evidence
                        if args.artifacts_dir:
                            (args.artifacts_dir / f"{label}-evidence.json").write_text(
                                json.dumps(graph_evidence, sort_keys=True) + "\n"
                            )
                            with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as source:
                                with sqlite3.connect(args.artifacts_dir / f"{label}.sqlite") as target:
                                    source.backup(target)
                    run["host_load_after_build"] = list(os.getloadavg()) if hasattr(os, "getloadavg") else None
                    if args.artifacts_dir:
                        for name in ("build.stdout", "build.stderr"):
                            shutil.copyfile(work / name, args.artifacts_dir / f"{label}-{repetition + 1}-{name}")
                    results[label].append(run)
                    print(f"{label} repetition {repetition + 1}/{args.repeats}: "
                          f"{timing['wall_seconds']:.3f}s", file=sys.stderr, flush=True)

        report = {
            "workspace": str(repo),
            "config": str(repo / "forge-mcp.yaml"),
            "database_policy": "fresh temporary database per run; removed on completion",
            "extraction_cache_policy": "fresh temporary extraction cache per run; removed on completion",
            "build_policy": "sequential cold builds; forge-mcp.yaml and enabled linked_workspaces are read from the supplied workspace",
            "baseline_binary": str(baseline_binary),
            "candidate_binary": str(candidate_binary),
            "binary_sha256": binary_hashes,
            "comparison_policy": args.comparison_policy,
            "os_page_cache_policy": "not flushed; only index and extraction caches are cold",
            "environment": {
                key: os.environ[key]
                for key in ("RAYON_NUM_THREADS", "MALLOC_ARENA_MAX", "FORGE_MEMORY_BUDGET_MB")
                if key in os.environ
            },
            "baseline": summarize_runs(results["baseline"]),
            "candidate": summarize_runs(results["candidate"]),
        }
        report["comparison"] = compare(report["baseline"], report["candidate"])
        evidence_comparison = compare_evidence(first_evidence["baseline"], first_evidence["candidate"])
        report["code_coverage_comparison"] = evidence_comparison
        if args.artifacts_dir:
            (args.artifacts_dir / "code-coverage-diff.json").write_text(
                json.dumps(evidence_comparison, indent=2, sort_keys=True) + "\n"
            )
        report["deterministic_by_binary"] = {
            label: len({digest([run["output_root"], run["domain_commitments"],
                               run["code_coverage"]["evidence_sha256"]]) for run in runs}) == 1
            for label, runs in results.items()
        }
        report["code_coverage_preserved"] = not any(evidence_comparison[key] for key in (
            "lost_code_files", "lost_code_nodes", "lost_exact_call_pairs", "lost_exact_call_occurrences",
            "lost_code_edges", "lost_resolved_references"
        )) and not evidence_comparison["aligned_test_reachability"]["lost_production_nodes"]
        report["candidate_median_not_slower"] = (
            report["candidate"]["median"]["wall_seconds"] <= report["baseline"]["median"]["wall_seconds"]
        )
        comparison = report["comparison"]
        report["semantic_parity"] = (
            all(delta == 0 for delta in comparison["graph_count_deltas"].values())
            and comparison["components_match"]
            and comparison["resolution_match"]
            and comparison["domain_commitments_match"]
            and comparison["root_inputs_match"]
        )
        if args.max_peak_rss_mb is not None:
            report["peak_rss_limit_bytes"] = args.max_peak_rss_mb * 1_000_000
            report["peak_rss_limit_passed"] = all(
                "peak_rss_bytes" in run
                and run["peak_rss_bytes"] < report["peak_rss_limit_bytes"]
                for run in results["candidate"]
            )
        rendered = json.dumps(report, indent=2, sort_keys=True)
        if args.output is not None:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(rendered + "\n")
        print(rendered)
        semantic_gate = report["semantic_parity"] if args.comparison_policy == "strict" else (
            report["code_coverage_preserved"] and all(report["deterministic_by_binary"].values())
        )
        # Timing is reported independently; exit 0 never implies a speed pass.
        return 0 if semantic_gate and report.get("peak_rss_limit_passed", True) else 1
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
