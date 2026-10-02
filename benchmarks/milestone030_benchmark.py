#!/usr/bin/env python3
"""Measure milestone 030 through the real stdio MCP transport."""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import math
import pathlib
import statistics
import sqlite3

from mcp_tool_comparison_benchmark import McpClient


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--db", required=True)
    parser.add_argument("--binary", default="target/release/contextunity-forge-mcp")
    parser.add_argument("--repeats", type=int, default=30)
    parser.add_argument("--output", default="/tmp/bench-tools.json")
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("repeats must be positive")
    target = "extensions/commerce/src/contextunity/commerce/modules/matcher/pipeline.py:run_matcher_pipeline"
    scenarios = [
        ("search_symbol", "code_map_search", {"pattern": "run_matcher_pipeline"}),
        ("search_text", "code_map_search", {"pattern": "matcher pipeline"}),
        ("search_prefix", "code_map_search", {"pattern": "run*"}),
        ("tests", "code_map_tests", {"selector": target, "direction": "inbound"}),
        ("ast_workspace", "ast_grep_search", {"pattern": "def run_matcher_pipeline($$$ARGS) -> MatcherPipelineResult: $$$BODY", "language": "python"}),
    ]
    report = {"measured_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "root": args.root, "db": args.db,
              "binary": str(pathlib.Path(args.binary).resolve()), "repeats": args.repeats,
              "scenarios": {}}
    report["binary_sha256"] = hashlib.sha256(pathlib.Path(args.binary).read_bytes()).hexdigest()
    report["database_bytes"] = pathlib.Path(args.db).stat().st_size
    with sqlite3.connect(f"file:{args.db}?mode=ro", uri=True) as conn:
        report["metadata"] = dict(conn.execute(
            "SELECT key,value FROM metadata WHERE key IN('corpus_hash','schema_version','index_semantics_version','output_root')"))
    with McpClient("forge", db_path=args.db, workspace_root=args.root,
                   binary_path=report["binary"]) as client:
        for label, tool, arguments in scenarios:
            samples = []
            first_ms = None
            inventory_scans = []
            for index in range(args.repeats + 1):
                response, elapsed, _ = client.request("tools/call", {
                    "name": tool, "arguments": {**arguments, "limit": 30, "detail": "compact"}})
                result = response.get("result", {})
                if response.get("error") or result.get("isError"):
                    raise RuntimeError(f"{label}: {response}")
                payload = json.loads(result["content"][0]["text"])
                collection = payload.get("nodes", payload.get("matches", {}))
                if index == 0:
                    first_ms = elapsed
                else:
                    samples.append(elapsed)
                    inventory_scans.append(payload.get("freshness", {}).get("inventory_scan_ms"))
            report["scenarios"][label] = {
                "first_ms": first_ms, "median_ms": statistics.median(samples),
                "p95_ms": sorted(samples)[math.ceil(len(samples) * .95) - 1],
                "max_ms": max(samples), "result_total": collection.get("total"),
                "inventory_scan_ms": inventory_scans,
                "samples_ms": samples}
    pathlib.Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({**report, "scenarios": {
        name: {key: value for key, value in result.items() if key not in {"samples_ms", "inventory_scan_ms"}}
        for name, result in report["scenarios"].items()}}, indent=2))


if __name__ == "__main__":
    main()
