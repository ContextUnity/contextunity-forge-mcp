#!/usr/bin/env python3
"""Repeatable stdio-MCP comparison runner for Forge and Codebase Memory."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import platform
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import shutil
from dataclasses import dataclass
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parents[1]
FORGE_BIN = "/home/oleksii/.local/bin/contextunity-forge-mcp"
CODEBASE_BIN = "/home/oleksii/.local/bin/codebase-memory-mcp"
CODEBASE_PROJECT = "home-oleksii-ContextUnity-projects-contextunity-forge-mcp"
PROTOCOL_VERSION = "2025-03-26"
TARGET = "src/mcp/server.rs:admit"
TARGET_QN = f"{CODEBASE_PROJECT}.src.mcp.server.Server.admit"
TEST_TARGET = "src/mcp/server.rs:read"
TEST_TARGET_QN = f"{CODEBASE_PROJECT}.src.mcp.server.Server.read"
CORPUS_ITEMS = (".gitignore", "Cargo.toml", "Cargo.lock", "build.rs", "forge-mcp.yaml", "README.md", "docs", "src", "tests")
BENCHMARK_ARTIFACTS = {
    "mcp_tool_comparison_benchmark.py",
    "mcp_tool_comparison_results.json",
    "mcp_tool_comparison_report.md",
}


SCENARIOS: list[dict[str, Any]] = [
    {
        "id": "workspace_overview",
        "scenario": "Workspace orientation and index inventory",
        "forge": ("code_map_overview", {"detail": "compact", "limit": 30}),
        "codebase": ("get_architecture", {"project": CODEBASE_PROJECT}),
        "match": "close: both report indexed workspace structure and inventory; Forge emphasizes components/coverage, Codebase emphasizes graph/package counts",
    },
    {
        "id": "symbol_search",
        "scenario": "Locate Server::admit in one file",
        "forge": (
            "code_map_search",
            {"pattern": "admit", "kind": "method", "path": "src/mcp/server.rs", "limit": 10, "detail": "compact"},
        ),
        "codebase": (
            "search_graph",
            {"project": CODEBASE_PROJECT, "name_pattern": "^admit$", "label": "Method", "file_pattern": "src/mcp/server.rs", "fields": ["signature", "docstring"], "limit": 10, "format": "json"},
        ),
        "match": "close: Forge supports FTS/prefix symbols; Codebase supports BM25/regex/semantic query and returns qualified-name rows",
    },
    {
        "id": "symbol_inspection",
        "scenario": "Inspect symbol identity, signature, docs, and references",
        "forge": ("code_map_inspect", {"selector": TARGET, "detail": "compact"}),
        "codebase": (
            "search_graph",
            {"project": CODEBASE_PROJECT, "name_pattern": "^admit$", "label": "Method", "file_pattern": "src/mcp/server.rs", "fields": ["signature", "docstring"], "limit": 10, "format": "json"},
        ),
        "match": "approximate: Forge has dedicated selector resolution, docs, reference counts, and evidence; Codebase search fields are a graph row, not an inspect response",
    },
    {
        "id": "source_snippet",
        "scenario": "Read the exact implementation of Server::admit",
        "forge": ("get_code_snippet", {"selector": TARGET}),
        "codebase": ("get_code_snippet", {"project": CODEBASE_PROJECT, "qualified_name": TARGET_QN}),
        "match": "strong: both resolve the same qualified method and return bounded source",
    },
    {
        "id": "test_relationships",
        "scenario": "Find tests connected to Server::read",
        "forge": ("code_map_tests", {"selector": TEST_TARGET, "direction": "inbound", "limit": 30}),
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": f"MATCH (test)-[:TESTS]->(target) WHERE target.qualified_name = '{TEST_TARGET_QN}' RETURN test.qualified_name, target.qualified_name LIMIT 30"},
        ),
        "match": "approximate: Codebase can query TESTS edges, but has no test-focused tool, direction labels, or witness classifications",
    },
    {
        "id": "unresolved_evidence",
        "scenario": "Inspect workspace unresolved references and retained unresolved-call evidence",
        "forge": ("code_map_analyze", {"target": "", "limit": 30}),
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": "MATCH (a)-[r:CALL_REFERENCE]->(b) RETURN a.qualified_name, r.callee, b.qualified_name LIMIT 30"},
        ),
        "match": "not equivalent: Forge exposes explicit unresolved/ambiguous counts and resolver causes; Codebase CALL_REFERENCE edges are a different evidence type and not a complete unresolved count",
    },
    {
        "id": "codebase_low_confidence_summary",
        "scenario": "Codebase-only diagnostic: group CALLS below confidence 0.5",
        "forge": None,
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": "MATCH ()-[r:CALLS]->() WHERE r.confidence < 0.5 RETURN r.confidence AS confidence, r.strategy AS strategy, count(r) AS calls ORDER BY calls DESC LIMIT 50"},
        ),
        "match": "diagnostic only: confidence < 0.5 is an analyst-selected triage threshold, not Codebase's canonical unresolved status",
    },
    {
        "id": "codebase_low_confidence_examples",
        "scenario": "Codebase-only diagnostic: inspect low-confidence call candidates",
        "forge": None,
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": "MATCH (caller)-[r:CALLS]->(target) WHERE r.confidence < 0.5 RETURN caller.qualified_name, r.callee, r.confidence, r.strategy, r.candidates, target.qualified_name LIMIT 30"},
        ),
        "match": "diagnostic only: raw candidate witnesses supplement index_status but do not form a complete unresolved-reference ledger",
    },
    {
        "id": "impact_analysis",
        "scenario": "Trace incoming callers/dependencies at depth one",
        "forge": ("code_map_impact", {"selector": TARGET, "depth": 1, "limit": 30}),
        "codebase": (
            "trace_path",
            {"project": CODEBASE_PROJECT, "function_name": TARGET_QN, "direction": "inbound", "depth": 1, "limit": 30, "include_tests": True, "format": "json"},
        ),
        "match": "close: both trace incoming graph paths; edge sets and transitive-count semantics differ, so compare witnesses rather than totals",
    },
    {
        "id": "symbol_explanation",
        "scenario": "Explain ownership and direct relationships for the target",
        "forge": ("code_map_explain", {"selector": TARGET, "direction": "both", "show_doc": True, "detail": "compact"}),
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": f"MATCH (n) WHERE n.qualified_name = '{TARGET_QN}' OPTIONAL MATCH (n)-[r]-(m) RETURN n.qualified_name, type(r), m.qualified_name LIMIT 30"},
        ),
        "match": "approximate: Forge assembles ownership/docs/edges into an explanation; Codebase exposes raw Cypher rows that the agent must interpret",
    },
    {
        "id": "graph_slice_query",
        "scenario": "Retrieve a bounded subgraph around the target",
        "forge": ("code_map_query", {"operation": "slice", "selector": TARGET, "depth": 1, "limit": 30}),
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": f"MATCH (n) WHERE n.qualified_name = '{TARGET_QN}' OPTIONAL MATCH (n)-[r]-(m) RETURN n.qualified_name, type(r), m.qualified_name LIMIT 30"},
        ),
        "match": "close with different query models: Forge owns a bounded slice operation; Codebase supports Cypher and requires explicit clauses/limit",
    },
    {
        "id": "workspace_diagnostics",
        "scenario": "Check workspace health, index counts, and cycles",
        "forge": ("code_map_analyze", {"target": "", "include_cycles": True, "limit": 30}),
        "codebase": ("index_status", {"project": CODEBASE_PROJECT}),
        "match": "partial: Codebase index_status is strong on indexing coverage/status; it does not provide Forge's graph diagnostics/cycle analysis",
    },
    {
        "id": "path_diagnostics_coverage",
        "scenario": "Check one cited source file and its indexing coverage",
        "forge": ("code_map_analyze", {"target": "src/mcp/server.rs", "limit": 30}),
        "codebase": ("check_index_coverage", {"project": CODEBASE_PROJECT, "paths": ["src/mcp/server.rs"]}),
        "match": "partial: both scope one file, but Codebase has explicit metadata freshness and coverage status; Forge returns its stored diagnostics",
    },
    {
        "id": "removal_safety",
        "scenario": "Assess whether the target can be removed safely",
        "forge": ("code_map_prove_removal", {"selector": TARGET, "detail": "compact", "limit": 30}),
        "codebase": (
            "query_graph",
            {"project": CODEBASE_PROJECT, "query": f"MATCH (caller)-[r]->(target) WHERE target.qualified_name = '{TARGET_QN}' RETURN type(r), caller.qualified_name LIMIT 30"},
        ),
        "match": "not equivalent: Codebase can list incoming graph edges, but has no candidate-scoped removal proof or unresolved-reference analysis",
    },
    {
        "id": "ast_pattern_search",
        "scenario": "Find Rust function syntax with a Tree-sitter pattern",
        "forge": ("ast_grep_search", {"pattern": "fn admit(&self, slot: &mut ConnectionSlot) -> Result<Freshness> { $$$BODY }", "language": "rust", "path": "src/mcp/server.rs", "limit": 30}),
        "codebase": ("search_code", {"project": CODEBASE_PROJECT, "pattern": "fn admit", "path_filter": "src/mcp/server\\.rs", "regex": False, "mode": "full", "limit": 10}),
        "match": "not equivalent: Forge matches parsed AST structure; Codebase search_code is text regex/literal search enriched with graph symbols",
    },
    {
        "id": "documentation_search",
        "scenario": "Search documentation for indexed response paging limits",
        "forge": ("search_docs", {"query": "MCP pages default to 30 items", "include_excerpt": True, "limit": 10}),
        "codebase": ("search_code", {"project": CODEBASE_PROJECT, "pattern": "MCP pages default to 30 items", "regex": False, "mode": "full", "limit": 10}),
        "match": "approximate: Forge uses indexed Markdown section search and anchors; Codebase literal-searches the Markdown file and returns a graph-enriched source window, without doc-type/section semantics",
    },
    {
        "id": "documentation_read",
        "scenario": "Read documentation about the graph/MCP workflow",
        "forge": ("get_doc", {"path_or_id": "README.md", "detail": "full"}),
        "codebase": ("search_code", {"project": CODEBASE_PROJECT, "pattern": "Native code graph", "regex": False, "mode": "full", "limit": 5}),
        "match": "not equivalent: Forge retrieves indexed Markdown by path/section; Codebase search_code gives matching snippets, not a full document read",
    },
    {
        "id": "tool_guide",
        "scenario": "Ask for agent guidance on using the MCP tools",
        "forge": ("forge_guide", {"topic": "query"}),
        "codebase": ("get_graph_schema", {"project": CODEBASE_PROJECT}),
        "match": "not equivalent: Codebase schema describes labels/edges but does not provide an interactive tool-selection or budget guide",
    },
    {
        "id": "session_checkpoint",
        "scenario": "List saved session checkpoints without changing state",
        "forge": ("session_checkpoint", {"action": "list"}),
        "codebase": None,
        "match": "no counterpart: Codebase exposes no session checkpoint persistence tool",
    },
]


def process_tree(root_pids: int | set[int]) -> set[int]:
    pending = [root_pids] if isinstance(root_pids, int) else list(root_pids)
    pids = set(pending)
    while pending:
        parent = pending.pop()
        try:
            task_dir = pathlib.Path(f"/proc/{parent}/task/{parent}")
            children = [int(pid) for pid in (task_dir / "children").read_text().split()]
        except (FileNotFoundError, PermissionError, ProcessLookupError, ValueError):
            continue
        for child in children:
            if child not in pids:
                pids.add(child)
                pending.append(child)
    return pids


def process_pids_with_cache(cache_dir: str | None) -> set[int]:
    if not cache_dir:
        return set()
    needle = f"CBM_CACHE_DIR={cache_dir}".encode()
    matches: set[int] = set()
    for entry in pathlib.Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if needle in (entry / "environ").read_bytes().split(b"\0"):
                matches.add(int(entry.name))
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
    return matches


def process_cpu_seconds(pid: int) -> float:
    total_ns = 0
    task_dir = pathlib.Path(f"/proc/{pid}/task")
    try:
        tasks = list(task_dir.iterdir())
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        tasks = []
    for task in tasks:
        try:
            total_ns += int((task / "schedstat").read_text().split()[0])
        except (FileNotFoundError, PermissionError, ProcessLookupError, IndexError, ValueError):
            continue
    if not tasks:
        try:
            total_ns = int(pathlib.Path(f"/proc/{pid}/schedstat").read_text().split()[0])
        except (FileNotFoundError, PermissionError, ProcessLookupError, IndexError, ValueError):
            pass
    return total_ns / 1e9


def proc_sample(root_pids: int | set[int]) -> tuple[float, int, int, int]:
    """Return process-tree CPU seconds, RSS KiB, HWM KiB, and process count."""
    cpu_seconds = 0.0
    rss_kib = 0
    hwm_kib = 0
    count = 0
    for member in process_tree(root_pids):
        try:
            cpu_seconds += process_cpu_seconds(member)
            values = pathlib.Path(f"/proc/{member}/status").read_text().splitlines()
            memory = {
                line.split(":", 1)[0]: int(line.split()[1])
                for line in values
                if line.startswith(("VmRSS:", "VmHWM:"))
            }
            rss_kib += memory.get("VmRSS", 0)
            hwm_kib += memory.get("VmHWM", 0)
            count += 1
        except (FileNotFoundError, PermissionError, ProcessLookupError, IndexError, ValueError):
            continue
    return cpu_seconds, rss_kib, hwm_kib, count


@dataclass
class Sample:
    response: dict[str, Any]
    wall_ms: float
    cpu_ms: float
    rss_before_kib: int
    rss_after_kib: int
    rss_peak_kib: int
    rss_hwm_kib: int
    process_count: int
    wire_bytes: int
    content_bytes: int
    error: str | None


class McpClient:
    def __init__(
        self,
        backend: str,
        db_path: str | None = None,
        env_overrides: dict[str, str] | None = None,
        workspace_root: str | None = None,
    ):
        startup_started = time.perf_counter_ns()
        if backend == "forge":
            command = [FORGE_BIN, "--root", workspace_root or str(ROOT)]
            if db_path:
                command += ["--db", db_path]
            command += ["serve"]
            env = os.environ.copy()
        elif backend == "codebase":
            command = [CODEBASE_BIN]
            env = os.environ.copy()
            env.update(env_overrides or {})
        else:
            raise ValueError(backend)

        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            env=env,
        )
        self.next_id = 0
        self.codebase_cache_dir = env.get("CBM_CACHE_DIR") if backend == "codebase" else None
        self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "forge-codebase-benchmark", "version": "1"},
            },
        )
        self.notify("notifications/initialized")
        self.startup_ms = (time.perf_counter_ns() - startup_started) / 1e6

    def notify(self, method: str) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.process.stdin.flush()

    def request(self, method: str, params: dict[str, Any]) -> tuple[dict[str, Any], float, int]:
        self.next_id += 1
        request_id = self.next_id
        request = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
        assert self.process.stdin is not None and self.process.stdout is not None
        started = time.perf_counter_ns()
        self.process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        wire = ""
        while True:
            line = self.process.stdout.readline()
            if not line:
                stderr = self.process.stderr.read() if self.process.stderr else ""
                raise RuntimeError(f"{self.process.args} exited; stderr={stderr[-2000:]}")
            wire += line
            try:
                response = json.loads(line)
            except json.JSONDecodeError:
                continue
            if response.get("id") == request_id:
                return response, (time.perf_counter_ns() - started) / 1e6, len(wire.encode())

    def call(self, name: str, arguments: dict[str, Any]) -> Sample:
        before_cpu, before_rss, _, before_count = proc_sample(self.process_pids())
        peak_rss = [before_rss]
        peak_count = [before_count]
        peak_cpu = [before_cpu]
        stop = threading.Event()

        def sample_memory() -> None:
            while not stop.is_set():
                cpu, rss, _, count = proc_sample(self.process_pids())
                peak_rss[0] = max(peak_rss[0], rss)
                peak_count[0] = max(peak_count[0], count)
                peak_cpu[0] = max(peak_cpu[0], cpu)
                stop.wait(0.005)

        sampler = threading.Thread(target=sample_memory, daemon=True)
        sampler.start()
        response, wall_ms, wire_bytes = self.request(
            "tools/call", {"name": name, "arguments": arguments}
        )
        stop.set()
        sampler.join()
        after_cpu, after_rss, hwm, after_count = proc_sample(self.process_pids())
        result = response.get("result", {})
        text = "\n".join(
            part.get("text", "") for part in result.get("content", []) if isinstance(part, dict)
        )
        return Sample(
            response=response,
            wall_ms=wall_ms,
            cpu_ms=max(0.0, (max(after_cpu, peak_cpu[0]) - before_cpu) * 1000),
            rss_before_kib=before_rss,
            rss_after_kib=after_rss,
            rss_peak_kib=max(peak_rss[0], after_rss),
            rss_hwm_kib=hwm,
            process_count=max(peak_count[0], after_count),
            wire_bytes=wire_bytes,
            content_bytes=len(text.encode()),
            error=(text[:1000] if result.get("isError") else None)
            or (json.dumps(response.get("error"))[:1000] if response.get("error") else None),
        )

    def list_tools(self) -> dict[str, Any]:
        response, _, _ = self.request("tools/list", {})
        return response

    def process_pids(self) -> set[int]:
        roots = {self.process.pid} | process_pids_with_cache(self.codebase_cache_dir)
        return process_tree(roots)

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)

    def __enter__(self) -> McpClient:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


def discover() -> None:
    for backend in ("forge", "codebase"):
        with McpClient(backend) as client:
            tools = client.list_tools().get("result", {}).get("tools", [])
            compact = [
                {
                    "name": tool.get("name"),
                    "description": tool.get("description", ""),
                    "required": tool.get("inputSchema", {}).get("required", []),
                    "properties": list(tool.get("inputSchema", {}).get("properties", {})),
                }
                for tool in tools
            ]
            print(json.dumps({"backend": backend, "tools": compact}, indent=2))


def summarize(samples: list[Sample]) -> dict[str, float]:
    fields = ("wall_ms", "cpu_ms", "rss_before_kib", "rss_after_kib", "rss_peak_kib", "rss_hwm_kib", "process_count", "wire_bytes", "content_bytes")
    return {
        f"median_{field}": statistics.median(getattr(sample, field) for sample in samples)
        for field in fields
    }


def measure_process(command: list[str], env: dict[str, str]) -> dict[str, Any]:
    started = time.perf_counter_ns()
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True)
    before_cpu, before_rss, _, before_count = proc_sample(process.pid)
    peak_cpu = [before_cpu]
    peak_rss = [before_rss]
    peak_hwm = [0]
    peak_count = [before_count]
    stop = threading.Event()

    def sample_process() -> None:
        while not stop.is_set():
            cpu, rss, hwm, count = proc_sample(process.pid)
            peak_cpu[0] = max(peak_cpu[0], cpu)
            peak_rss[0] = max(peak_rss[0], rss)
            peak_hwm[0] = max(peak_hwm[0], hwm)
            peak_count[0] = max(peak_count[0], count)
            stop.wait(0.005)

    sampler = threading.Thread(target=sample_process, daemon=True)
    sampler.start()
    stdout, stderr = process.communicate()
    stop.set()
    sampler.join()
    return {
        "command": command,
        "exit_code": process.returncode,
        "wall_ms": (time.perf_counter_ns() - started) / 1e6,
        "cpu_ms": max(0.0, (peak_cpu[0] - before_cpu) * 1000),
        "rss_peak_kib": peak_rss[0],
        "rss_hwm_kib": peak_hwm[0],
        "process_count": peak_count[0],
        "stdout_tail": stdout[-2000:],
        "stderr_tail": stderr[-2000:],
    }


def directory_bytes(path: pathlib.Path) -> int:
    return sum(file.stat().st_size for file in path.rglob("*") if file.is_file())


def copy_benchmark_corpus(destination: pathlib.Path) -> None:
    def ignore(directory: str, names: list[str]) -> set[str]:
        excluded = {".git", ".forge", ".codebase-memory", ".pytest_cache", ".ruff_cache", "__pycache__", "target", "vendor"}
        return excluded.intersection(names) | BENCHMARK_ARTIFACTS.intersection(names)

    destination.mkdir(parents=True)
    for name in CORPUS_ITEMS:
        source = ROOT / name
        target = destination / name
        if source.is_dir():
            shutil.copytree(source, target, ignore=ignore, symlinks=True)
        elif source.is_file():
            shutil.copy2(source, target)


def prepare_indexes(
    temp_root: pathlib.Path,
) -> tuple[pathlib.Path, pathlib.Path, dict[str, str], pathlib.Path, dict[str, Any]]:
    forge_db = temp_root / "forge-code-map.sqlite"
    codebase_cache = temp_root / "codebase-cache"
    codebase_source = temp_root / "codebase-source"
    copy_benchmark_corpus(codebase_source)
    profile_home = temp_root / "codebase-home"
    cbm_env = os.environ.copy()
    cbm_env.update(
        {
            "HOME": str(profile_home),
            "XDG_CONFIG_HOME": str(profile_home / "config"),
            "XDG_CACHE_HOME": str(profile_home / "cache"),
            "XDG_DATA_HOME": str(profile_home / "data"),
            "XDG_STATE_HOME": str(profile_home / "state"),
            "XDG_RUNTIME_DIR": str(profile_home / "runtime"),
            "CBM_RUNTIME_DIR": str(profile_home / "runtime"),
            "CBM_CACHE_DIR": str(codebase_cache),
        }
    )
    for directory in (profile_home, *(pathlib.Path(cbm_env[key]) for key in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"))):
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    for key in ("auto_watch", "ui_enabled"):
        configured = subprocess.run(
            [CODEBASE_BIN, "config", "set", key, "false"],
            env=cbm_env,
            capture_output=True,
            text=True,
            check=False,
        )
        if configured.returncode:
            raise RuntimeError(f"failed setting isolated Codebase {key}: {configured.stderr}")

    forge_build = measure_process(
        [FORGE_BIN, "--root", str(codebase_source), "--db", str(forge_db), "build"], os.environ.copy()
    )
    if forge_build["exit_code"]:
        raise RuntimeError(f"Forge index build failed: {forge_build['stderr_tail']}")
    forge_build["db_bytes"] = forge_db.stat().st_size

    with McpClient("codebase", env_overrides=cbm_env) as client:
        codebase_index = client.call(
            "index_repository",
            {"repo_path": str(codebase_source), "mode": "full", "name": CODEBASE_PROJECT, "persistence": False},
        )
        if codebase_index.error:
            raise RuntimeError(f"Codebase index build failed: {codebase_index.error}")
        codebase_build = {
            "startup_ms": client.startup_ms,
            "wall_ms": codebase_index.wall_ms,
            "cpu_ms": codebase_index.cpu_ms,
            "rss_before_kib": codebase_index.rss_before_kib,
            "rss_peak_kib": codebase_index.rss_peak_kib,
            "rss_hwm_kib": codebase_index.rss_hwm_kib,
            "process_count": codebase_index.process_count,
            "content_bytes": codebase_index.content_bytes,
            "response": codebase_index.response,
        }
    codebase_build["cache_bytes"] = directory_bytes(codebase_cache)
    return forge_db, codebase_cache, cbm_env, codebase_source, {
        "forge": forge_build,
        "codebase": codebase_build,
        "codebase_source_copy": {
            "root": str(codebase_source),
            "included_roots": list(CORPUS_ITEMS),
            "excluded": [".git", ".forge", ".codebase-memory", "target", "vendor", "AGENTS.md", ".agents", "skills", *sorted(BENCHMARK_ARTIFACTS)],
        },
    }


def stop_isolated_daemon(codebase_cache: pathlib.Path) -> None:
    pids = process_pids_with_cache(str(codebase_cache))
    for pid in pids:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 3
    while pids and time.monotonic() < deadline:
        pids = {pid for pid in pids if pathlib.Path(f"/proc/{pid}").exists()}
        if pids:
            time.sleep(0.05)
    for pid in pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def benchmark(repeats: int, output_path: pathlib.Path, scenario_id: str | None = None) -> None:
    rows: list[dict[str, Any]] = []
    examples: dict[str, Any] = {}
    temp_root = pathlib.Path(tempfile.mkdtemp(prefix="forge-codebase-mcp-bench-", dir="/tmp/kilo"))
    try:
        forge_db, codebase_cache, codebase_env, codebase_source, index_builds = prepare_indexes(temp_root)
    except Exception:
        import shutil

        shutil.rmtree(temp_root, ignore_errors=True)
        raise

    try:
        for scenario in SCENARIOS:
            if scenario_id and scenario["id"] != scenario_id:
                continue
            for backend, call_spec in (("forge", scenario["forge"]), ("codebase", scenario["codebase"])):
                if call_spec is None:
                    continue
                tool_name, arguments = call_spec
                samples_by_state: dict[str, list[Sample]] = {"cold_process": [], "warm_process": []}
                startup_times: dict[str, list[float]] = {"cold_process": [], "warm_process": []}
                if backend == "codebase":
                    stop_isolated_daemon(codebase_cache)

                for _ in range(repeats):
                    with McpClient(
                        backend,
                        db_path=str(forge_db) if backend == "forge" else None,
                        env_overrides=codebase_env if backend == "codebase" else None,
                        workspace_root=str(codebase_source) if backend == "forge" else None,
                    ) as client:
                        startup_times["cold_process"].append(client.startup_ms)
                        sample = client.call(tool_name, arguments)
                        samples_by_state["cold_process"].append(sample)
                        if sample.error:
                            raise RuntimeError(f"{backend}.{tool_name} failed cold: {sample.error}")
                    if backend == "codebase":
                        stop_isolated_daemon(codebase_cache)

                with McpClient(
                    backend,
                    db_path=str(forge_db) if backend == "forge" else None,
                    env_overrides=codebase_env if backend == "codebase" else None,
                    workspace_root=str(codebase_source) if backend == "forge" else None,
                ) as client:
                    startup_times["warm_process"].append(client.startup_ms)
                    warmup = client.call(tool_name, arguments)
                    if warmup.error:
                        raise RuntimeError(f"{backend}.{tool_name} failed warm-up: {warmup.error}")
                    for _ in range(repeats):
                        sample = client.call(tool_name, arguments)
                        samples_by_state["warm_process"].append(sample)
                        if sample.error:
                            raise RuntimeError(f"{backend}.{tool_name} failed warm: {sample.error}")

                for state, samples in samples_by_state.items():
                    key = f"{scenario['id']}:{backend}:{state}"
                    examples[key] = samples[0].response
                    rows.append(
                        {
                            "scenario_id": scenario["id"],
                            "scenario": scenario["scenario"],
                            "backend": backend,
                            "tool": tool_name,
                            "arguments": arguments,
                            "state": state,
                            "repeats": len(samples),
                            "startup_median_ms": statistics.median(startup_times[state]),
                            "summary": summarize(samples),
                            "runs": [
                                {
                                    "wall_ms": sample.wall_ms,
                                    "cpu_ms": sample.cpu_ms,
                                    "rss_before_kib": sample.rss_before_kib,
                                    "rss_after_kib": sample.rss_after_kib,
                                    "rss_peak_kib": sample.rss_peak_kib,
                                    "rss_hwm_kib": sample.rss_hwm_kib,
                                    "process_count": sample.process_count,
                                    "wire_bytes": sample.wire_bytes,
                                    "content_bytes": sample.content_bytes,
                                    "response_sha256": hashlib.sha256(
                                        json.dumps(sample.response, sort_keys=True, ensure_ascii=False).encode()
                                    ).hexdigest(),
                                }
                                for sample in samples
                            ],
                        }
                    )
                print(
                    f"{scenario['id']:<30} {backend:<8} "
                    f"cold={statistics.median(s.wall_ms for s in samples_by_state['cold_process']):8.2f}ms "
                    f"warm={statistics.median(s.wall_ms for s in samples_by_state['warm_process']):8.2f}ms "
                    f"coldStart={statistics.median(startup_times['cold_process']):8.2f}ms "
                    f"cpu={statistics.median(s.cpu_ms for s in samples_by_state['warm_process']):6.2f}ms "
                    f"rssPeak={statistics.median(s.rss_peak_kib for s in samples_by_state['warm_process']):8.0f}KiB"
                )

        result = {
        "metadata": {
            "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "workspace_root": str(ROOT),
            "source_snapshot": "both backends read the same temporary copy; Forge's indexed file count and resolver output_root are retained in index_builds and response examples",
            "codebase_project": CODEBASE_PROJECT,
            "platform": platform.platform(),
            "python": sys.version,
            "cpu_count": os.cpu_count(),
            "cpu_measurement": "sum of Linux /proc/<pid>/task/<tid>/schedstat runtime nanoseconds across each tracked process tree",
            "repeats_per_state": repeats,
            "cold_definition": "first request in a fresh Forge server/SQLite connection; Codebase daemon and graph connection are restarted before each cold repetition; both use freshly built isolated index files; Linux page cache is not flushed; startup time is reported separately",
            "warm_definition": "same MCP process and SQLite connection after one discarded warm-up call",
            "rss_definition": "summed process-tree VmRSS sampled every 5ms during the request; VmHWM is summed process-lifetime high water",
            "cpu_definition": "summed process-tree scheduled runtime delta from /proc/<pid>/schedstat",
            "index_mutations": "only disposable Forge DB and CBM_CACHE_DIR under /tmp/kilo were built; existing .forge and Codebase indexes/config were not modified",
            "index_builds": index_builds,
            "forge_index_bytes": forge_db.stat().st_size,
            "codebase_index_cache_bytes": directory_bytes(codebase_cache),
            "codebase_cache_dir_isolated": True,
            "scenario_match": {
                scenario["id"]: scenario["match"]
                for scenario in SCENARIOS
                if scenario_id is None or scenario["id"] == scenario_id
            },
        },
        "rows": rows,
        "response_examples": examples,
        }
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
        print(f"saved: {output_path} ({output_path.stat().st_size} bytes; {len(rows)} metric rows)")
    finally:
        import shutil

        stop_isolated_daemon(temp_root / "codebase-cache")
        shutil.rmtree(temp_root, ignore_errors=True)


def redact_sensitive_results(path: pathlib.Path) -> None:
    result = json.loads(path.read_text())
    examples = result.get("response_examples", {})
    for key in list(examples):
        if key.startswith("session_checkpoint:"):
            del examples[key]
    metadata = result.setdefault("metadata", {})
    metadata["corpus_parity_caveat"] = (
        "Both tools read one immutable temporary source copy and the benchmark targets the same relative symbols, "
        "but Forge follows forge-mcp.yaml roots while Codebase mode=full indexes all included root files. "
        "Node/edge counts, overview, and disk/cache sizes are therefore not normalized to identical indexed files."
    )
    metadata["source_snapshot"] = (
        "Each run builds a temporary source copy; both backends for a scenario read that same copy. "
        "Single-scenario replacements use a fresh copy from the same include/exclude policy."
    )
    metadata["warm_definition"] = (
        "One discarded same-tool warm-up followed by five requests in the same MCP stdio client. "
        "Forge reuses its server/SQLite connection; Codebase reuses its separate isolated daemon/graph connection behind the stdio client."
    )
    metadata["latency_boundary_caveat"] = (
        "wall_ms measures tools/call only, after initialize; fresh-server startup_ms is separate. "
        "Cold means a fresh Forge server/SQLite connection or restarted isolated Codebase daemon/graph connection, "
        "not an OS page-cache flush."
    )
    metadata["resource_caveat"] = (
        "CPU sums schedstat for observed process-tree threads; RSS is simultaneous VmRSS sampled every 5ms. "
        "Short-lived children between samples and children forked by non-leader threads can be missed; "
        "VmHWM is a sum of per-process lifetime peaks, not a simultaneous peak. Codebase PID discovery scans /proc "
        "during sampling and its overhead is included in wall_ms."
    )
    metadata["build_metric_caveat"] = (
        "Forge build wall_ms covers its build process; Codebase index_repository wall_ms starts after daemon initialization, "
        "which is recorded separately. The isolated config setup phase is not included."
    )
    metadata["response_size_caveat"] = (
        "content_bytes counts MCP text blocks; wire_bytes counts stdout bytes consumed through the matched response. "
        "Structured content may duplicate text. Per-run response hashes include JSON-RPC request IDs and are not a content-stability test."
    )
    metadata["rpc_runner_caveat"] = (
        "The local MCP runner has no per-request timeout, does not drain stderr while a process is live, "
        "and does not answer server-initiated JSON-RPC requests. All tested local requests completed; "
        "do not use this runner against an untrusted or unstable remote server without hardening it."
    )
    metadata["quality_scoring_caveat"] = (
        "Quality and agent-clarity assessments in the companion report are manual, evidence-based rubric scores, "
        "not an automated LLM judge or formal correctness proof."
    )
    metadata["session_checkpoint_scope"] = (
        "Read-only list was remeasured five times against the isolated snapshot, which has no .forge/checkpoints.json; examples are omitted and no checkpoint state was written."
    )
    metadata["redactions"] = [
        "session_checkpoint response examples omitted; byte counts and hashes remain"
    ]
    path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")


def replace_scenario_results(target_path: pathlib.Path, source_path: pathlib.Path, scenario_id: str) -> None:
    target = json.loads(target_path.read_text())
    source = json.loads(source_path.read_text())
    replacement_rows = [row for row in source["rows"] if row["scenario_id"] == scenario_id]
    if len(replacement_rows) not in (2, 4) or any(row["repeats"] != 5 for row in replacement_rows):
        raise ValueError(f"expected two or four five-repeat rows for {scenario_id}; got {len(replacement_rows)}")
    target["rows"] = [row for row in target["rows"] if row["scenario_id"] != scenario_id]
    target["rows"].extend(replacement_rows)
    examples = target.get("response_examples", {})
    for key in list(examples):
        if key.startswith(f"{scenario_id}:"):
            del examples[key]
    examples.update(source.get("response_examples", {}))
    target.setdefault("metadata", {}).setdefault("scenario_match", {}).update(
        {
            scenario_id: source.get("metadata", {}).get("scenario_match", {}).get(scenario_id)
        }
    )
    reasons = {
        "ast_pattern_search": "replacement run used a validated positive AST pattern and same-file text-search scope",
        "session_checkpoint": "replacement run verified read-only list against the isolated snapshot without checkpoint state",
        "codebase_low_confidence_summary": "supplemental five-repeat Codebase-only triage query using an explicit confidence threshold",
        "codebase_low_confidence_examples": "supplemental five-repeat Codebase-only candidate query using the same confidence threshold",
    }
    target.setdefault("metadata", {}).setdefault("scenario_overrides", {})[scenario_id] = {
        "source_metadata": source.get("metadata", {}),
        "reason": reasons.get(scenario_id, "replacement run used a validated scenario-specific input"),
    }
    target_path.write_text(json.dumps(target, indent=2, ensure_ascii=False) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--discover", action="store_true")
    parser.add_argument("--bench", action="store_true")
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--output", type=pathlib.Path, default=ROOT / "tests/mcp_tool_comparison_results.json")
    parser.add_argument("--redact-sensitive-results", type=pathlib.Path)
    parser.add_argument("--scenario", choices=[scenario["id"] for scenario in SCENARIOS])
    parser.add_argument("--replace-scenario", nargs=3, metavar=("TARGET", "SOURCE", "SCENARIO_ID"))
    args = parser.parse_args()
    if args.redact_sensitive_results:
        redact_sensitive_results(args.redact_sensitive_results)
        return 0
    if args.replace_scenario:
        target, source, scenario_id = args.replace_scenario
        replace_scenario_results(pathlib.Path(target), pathlib.Path(source), scenario_id)
        return 0
    if args.discover:
        discover()
        return 0
    if args.bench:
        if args.repeats < 1:
            parser.error("--repeats must be positive")
        benchmark(args.repeats, args.output, args.scenario)
        return 0
    parser.error("select --discover or --bench")
    return 2


if __name__ == "__main__":
    sys.exit(main())
