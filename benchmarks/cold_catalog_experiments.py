"""Compare catalog variants with isolated caches and a quiet interval between builds."""

from __future__ import annotations

import argparse
import collections
import contextlib
import hashlib
import json
import os
import pathlib
import statistics
import sqlite3
import tempfile
import time

import cold_test_coverage_benchmark as cold


def inventory(db: pathlib.Path, repo: pathlib.Path, linked_roots: dict[str, pathlib.Path]) -> dict:
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as connection:
        files = connection.execute("SELECT path,language,size FROM files ORDER BY path").fetchall()
        semantic_paths = {row[0] for row in connection.execute(
            "SELECT DISTINCT p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id "
            "WHERE n.kind!='module'"
        )}
    languages = collections.Counter()
    language_bytes = collections.Counter()
    trivial = []
    module_only = []
    pass_only = []
    source_paths = {}
    unavailable_sources = []
    for path, language, size in files:
        languages[language] += 1
        language_bytes[language] += size
        if path not in semantic_paths:
            module_only.append(path)
        source_path = repo / path
        if path.startswith("[") and "]/" in path:
            name, relative = path[1:].split("]/", 1)
            linked = linked_roots.get(name)
            if linked is None:
                unavailable_sources.append(path)
                continue
            source_path = linked / relative
        source_paths[path] = str(source_path)
        text = source_path.read_text(errors="replace").strip()
        lines = [line.strip() for line in text.splitlines()]
        fast = not text
        if language == "python":
            fast = all(not line or (line.startswith("#") and not line.startswith("# type:")) for line in lines)
        elif language == "rust":
            fast = all(not line or (line.startswith("//") and not line.startswith(("//!", "///"))) for line in lines)
        elif language in ("javascript", "typescript"):
            fast = all(not line or (line.startswith("//") and not line.startswith("///")) for line in lines)
        if fast:
            trivial.append({"path": path, "language": language, "bytes": size})
        if language == "python" and text in ("pass", "..."):
            pass_only.append(path)
    return {"file_count": len(files), "languages": dict(languages), "language_bytes": dict(language_bytes),
            "existing_trivial_fast_path": trivial, "paths_without_non_module_nodes": module_only,
            "python_pass_only_paths": pass_only,
            "source_paths": source_paths, "unavailable_sources": unavailable_sources,
            "indexed_paths": [row[0] for row in files]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=pathlib.Path, required=True)
    parser.add_argument("--variant", action="append", required=True, metavar="NAME=BINARY")
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--pause-seconds", type=float, default=60)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--warm-variant", action="append", default=[], metavar="NAME")
    parser.add_argument("--inventory-output", type=pathlib.Path)
    parser.add_argument("--linked-root", action="append", default=[], metavar="NAME=PATH")
    args = parser.parse_args()
    if args.repeats < 1 or args.pause_seconds < 0:
        parser.error("repeats must be positive and pause-seconds nonnegative")
    variants = {}
    for item in args.variant:
        name, separator, binary = item.partition("=")
        if not separator or not name or name in variants:
            parser.error("each variant must have a unique NAME=BINARY")
        variants[name] = cold.resolve_binary(binary)
    if set(args.warm_variant) - variants.keys():
        parser.error("warm variants must name a registered variant")
    repo = args.repo.resolve()
    linked_roots = {}
    for item in args.linked_root:
        name, separator, path = item.partition("=")
        if not separator or not name:
            parser.error("linked roots must be NAME=PATH")
        linked_roots[name] = pathlib.Path(path).resolve()
    report = {
        "workspace": str(repo),
        "policy": "sequential fresh databases; alternating order; fresh caches except explicitly primed warm variants",
        "pause_seconds": args.pause_seconds,
        "warm_variants": args.warm_variant,
        "available_cpus": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else os.cpu_count(),
        "environment": {
            key: os.environ[key]
            for key in ("RAYON_NUM_THREADS", "MALLOC_ARENA_MAX", "FORGE_MEMORY_BUDGET_MB")
            if key in os.environ
        },
        "binaries": {
            name: {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
            for name, binary in variants.items()
        },
        "runs": {name: [] for name in variants},
    }
    reference = None
    started = False
    resources = contextlib.ExitStack()
    warm_caches = {}
    report["cache_priming"] = {}
    for name in args.warm_variant:
        directory = resources.enter_context(tempfile.TemporaryDirectory(prefix="forge-warm-cache-"))
        work = pathlib.Path(directory)
        warm_caches[name] = work / "extraction-cache"
        if started:
            time.sleep(args.pause_seconds)
        report["cache_priming"][name] = cold.run_build(
            variants[name], repo, work / "prime.sqlite", work,
            extra_env={"FORGE_EXTRACTION_CACHE_DIR": str(warm_caches[name])},
        )
        started = True
    started = False
    time.sleep(args.pause_seconds)
    for repetition in range(args.repeats):
        order = list(variants) if repetition % 2 == 0 else list(reversed(variants))
        for name in order:
            if started:
                time.sleep(args.pause_seconds)
            started = True
            binary = variants[name]
            with tempfile.TemporaryDirectory(prefix="forge-catalog-experiment-") as directory:
                work = pathlib.Path(directory)
                db = work / "index.sqlite"
                timing = cold.run_build(
                    binary, repo, db, work,
                    extra_env={"FORGE_EXTRACTION_CACHE_DIR": str(warm_caches.get(name, work / "extraction-cache"))},
                )
                build = json.loads((work / "build.stdout").read_text())
                run = {
                    "repetition": repetition + 1,
                    **timing,
                    "phases_ms": {
                        key: value for key, value in build.items()
                        if key.endswith("_ms") and isinstance(value, (int, float))
                    },
                    "domain_commitments": cold.domain_commitments(db),
                    "graph": cold.query_overview(binary, repo, db),
                    "root_inputs_digest": cold.root_inputs_digest(db),
                    "pipeline": build.get("pipeline"),
                }
                run["process_cpu_seconds"] = timing["user_seconds"] + timing["system_seconds"]
                run["average_cpu_percent"] = run["process_cpu_seconds"] / timing["wall_seconds"] * 100
                if reference is None:
                    reference = run
                    if args.inventory_output:
                        args.inventory_output.parent.mkdir(parents=True, exist_ok=True)
                        args.inventory_output.write_text(json.dumps(inventory(db, repo, linked_roots), indent=2, sort_keys=True) + "\n")
                run["semantic_parity"] = len(run["domain_commitments"]) == 17 and all(
                    run[key] == reference[key]
                    for key in ("domain_commitments", "graph", "root_inputs_digest")
                )
                run["peak_rss_limit_passed"] = timing["peak_rss_bytes"] < 800_000_000
                report["runs"][name].append(run)
                report["summary"] = {
                    variant: {
                        "median_wall_seconds": statistics.median(r["wall_seconds"] for r in runs),
                        "median_user_seconds": statistics.median(r["user_seconds"] for r in runs),
                        "median_system_seconds": statistics.median(r["system_seconds"] for r in runs),
                        "median_process_cpu_seconds": statistics.median(r["process_cpu_seconds"] for r in runs),
                        "median_average_cpu_percent": statistics.median(r["average_cpu_percent"] for r in runs),
                        "max_peak_rss_bytes": max(r["peak_rss_bytes"] for r in runs),
                        "semantic_parity": all(r["semantic_parity"] for r in runs),
                        "peak_rss_limit_passed": all(r["peak_rss_limit_passed"] for r in runs),
                        "median_phases_ms": {
                            key: statistics.median(r["phases_ms"][key] for r in runs)
                            for key in runs[0]["phases_ms"]
                        },
                    }
                    for variant, runs in report["runs"].items()
                    if runs
                }
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
                print(json.dumps({"variant": name, "repetition": repetition + 1, **timing,
                                  "semantic_parity": run["semantic_parity"]}), flush=True)
    resources.close()
    return 0 if all(
        summary["semantic_parity"] and summary["peak_rss_limit_passed"]
        for summary in report["summary"].values()
    ) else 1


if __name__ == "__main__":
    raise SystemExit(main())
