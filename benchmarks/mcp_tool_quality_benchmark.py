#!/usr/bin/env python3
"""Capture tool responses for a quality-only Forge/Codebase study."""

from __future__ import annotations

import argparse
import datetime
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
from typing import Any

from benchmark_profile import BenchmarkProfile, copy_workspace_snapshot, load_profile
from mcp_tool_comparison_benchmark import (
    PROTOCOL_VERSION,
    stop_isolated_daemon,
)


ROOT = pathlib.Path(__file__).resolve().parents[1]


class QualityMcpClient:
    def __init__(
        self,
        backend: str,
        *,
        db_path: pathlib.Path | None = None,
        workspace_root: pathlib.Path | None = None,
        env: dict[str, str] | None = None,
        binary_path: str,
    ) -> None:
        if backend == "forge":
            command = [binary_path, "--root", str(workspace_root), "--db", str(db_path), "serve"]
        elif backend == "codebase":
            command = [binary_path]
        else:
            raise ValueError(backend)
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
            env=env or os.environ.copy(),
        )
        self.next_id = 0
        self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "forge-codebase-quality-study", "version": "1"},
            },
        )
        self.notify("notifications/initialized")

    def notify(self, method: str) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.process.stdin.flush()

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self.next_id += 1
        request_id = self.next_id
        message = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        assert self.process.stdin is not None and self.process.stdout is not None
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        while True:
            line = self.process.stdout.readline()
            if not line:
                raise RuntimeError(f"{self.process.args} exited before responding")
            response = json.loads(line)
            if response.get("id") == request_id:
                return response

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)

    def __enter__(self) -> QualityMcpClient:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


def response_result(response: dict[str, Any]) -> dict[str, Any]:
    if response.get("error"):
        raise RuntimeError(json.dumps(response["error"], ensure_ascii=False))
    result = response.get("result", {})
    if result.get("isError"):
        raise RuntimeError(json.dumps(result, ensure_ascii=False))
    return strip_performance_fields(result)


def strip_performance_fields(value: Any) -> Any:
    excluded = {"elapsed_ms", "duration_ms", "latency_ms", "wall_ms", "cpu_ms", "rss_kib"}
    if isinstance(value, dict):
        return {
            key: strip_performance_fields(item)
            for key, item in value.items()
            if key.casefold() not in excluded
        }
    if isinstance(value, list):
        return [strip_performance_fields(item) for item in value]
    if isinstance(value, str) and value.lstrip().startswith(("{", "[")):
        try:
            parsed = json.loads(value)
        except json.JSONDecodeError:
            return value
        cleaned = strip_performance_fields(parsed)
        return json.dumps(cleaned, ensure_ascii=False, separators=(",", ":"))
    return value


def run(
    repeats: int,
    output_path: pathlib.Path,
    profile: BenchmarkProfile,
    scenario_id: str | None = None,
) -> None:
    temp_root = pathlib.Path(tempfile.mkdtemp(prefix="forge-codebase-quality-", dir="/tmp/kilo"))
    try:
        forge_db = temp_root / "forge-code-map.sqlite"
        codebase_cache = temp_root / "codebase-cache"
        codebase_source = temp_root / "codebase-source"
        profile_home = temp_root / "codebase-home"
        copy_workspace_snapshot(profile.workspace_root, codebase_source, profile.excluded_directories)
        codebase_env = os.environ.copy()
        codebase_env.update(
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
        for directory in (
            profile_home,
            *(pathlib.Path(codebase_env[key]) for key in (
                "XDG_CONFIG_HOME",
                "XDG_CACHE_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_RUNTIME_DIR",
            )),
        ):
            directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        for key in ("auto_watch", "ui_enabled"):
            configured = subprocess.run(
                [profile.codebase_binary, "config", "set", key, "false"],
                env=codebase_env,
                capture_output=True,
                text=True,
                check=False,
            )
            if configured.returncode:
                raise RuntimeError(f"failed setting isolated Codebase {key}: {configured.stderr}")

        forge_build = subprocess.run(
            [profile.forge_binary, "--root", str(codebase_source), "--db", str(forge_db), "build"],
            capture_output=True,
            text=True,
            check=False,
        )
        if forge_build.returncode:
            raise RuntimeError(f"Forge index build failed: {forge_build.stderr[-2000:]}")

        with QualityMcpClient("codebase", env=codebase_env, binary_path=profile.codebase_binary) as index_client:
            index_result = response_result(
                index_client.request(
                    "tools/call",
                    {
                        "name": "index_repository",
                        "arguments": {
                            "repo_path": str(codebase_source),
                            "mode": "full",
                            "name": profile.project,
                            "persistence": False,
                        },
                    },
                )
            )
            if not index_result:
                raise RuntimeError("Codebase returned an empty index result")
        stop_isolated_daemon(codebase_cache)
        observations: list[dict[str, Any]] = []
        calls = 0

        for backend in ("forge", "codebase"):
            with QualityMcpClient(
                backend,
                db_path=forge_db if backend == "forge" else None,
                workspace_root=str(codebase_source) if backend == "forge" else None,
                env=codebase_env if backend == "codebase" else None,
                binary_path=profile.forge_binary if backend == "forge" else profile.codebase_binary,
            ) as client:
                for scenario in profile.scenarios:
                    if scenario_id and scenario["id"] != scenario_id:
                        continue
                    call_spec = scenario[backend]
                    if call_spec is None:
                        continue
                    tool_name, arguments = call_spec
                    arguments = dict(arguments)
                    for repetition in range(1, repeats + 1):
                        response = client.request(
                            "tools/call", {"name": tool_name, "arguments": arguments}
                        )
                        result = response_result(response)
                        observations.append(
                            {
                                "scenario_id": scenario["id"],
                                "scenario": scenario["scenario"],
                                "backend": backend,
                                "tool": tool_name,
                                "arguments": arguments,
                                "repetition": repetition,
                                "result": result,
                            }
                        )
                        calls += 1

        result = {
            "metadata": {
                "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "study": "response quality only; no latency, throughput, CPU, RSS, or response-size scoring",
                "workspace_root": str(profile.workspace_root),
                "profile": str(profile.path),
                "profile_name": profile.name,
                "source_snapshot": "current working-tree files copied to a temporary corpus; Forge may additionally follow enabled linked_workspaces from its adapter policy",
                "source_scope": "same corpus copy policy as mcp_tool_comparison_benchmark.py; Forge follows forge-mcp.yaml roots, Codebase indexes in full mode",
                "scope_note": profile.scope_note,
                "forge_binary": profile.forge_binary,
                "codebase_binary": profile.codebase_binary,
                "codebase_project": profile.project,
                "repetitions_per_scenario_backend": repeats,
                "response_count": calls,
                "interpretation": "repetitions check response stability, not performance",
                "excluded_metrics": ["latency", "throughput", "CPU", "RSS", "response size"],
                "removed_tool_fields": ["elapsed_ms", "duration_ms", "latency_ms", "wall_ms", "cpu_ms", "rss_kib"],
            },
            "observations": observations,
        }
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
        print(f"saved {calls} quality-only responses: {output_path}")
    finally:
        stop_isolated_daemon(temp_root / "codebase-cache")
        shutil.rmtree(temp_root, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", type=pathlib.Path, required=True)
    parser.add_argument("--workspace", type=pathlib.Path)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--scenario")
    parser.add_argument(
        "--output",
        type=pathlib.Path,
        default=ROOT / "benchmarks/mcp_tool_quality_results.json",
    )
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    try:
        profile = load_profile(args.profile, args.workspace)
    except ValueError as error:
        parser.error(str(error))
    if args.scenario and args.scenario not in {item["id"] for item in profile.scenarios}:
        parser.error(f"unknown scenario {args.scenario!r} in profile")
    run(args.repeats, args.output, profile, args.scenario)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
