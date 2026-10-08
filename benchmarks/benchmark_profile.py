"""Shared JSON profile loading and workspace snapshot policy."""

from __future__ import annotations

import json
import pathlib
import re
import shutil
from dataclasses import dataclass
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parent.parent

DEFAULT_EXCLUDED_DIRECTORIES = {
    ".agents",
    ".benchmarks",
    ".codebase-memory",
    ".codex",
    ".contextunity",
    ".forge",
    ".git",
    ".kilo",
    ".mypy_cache",
    ".next",
    ".pi",
    ".playwright-mcp",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
    "__pycache__",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
    "venv",
}

BENCHMARK_FILES = {
    "benchmark_profile.py",
    "mcp_adapter_single_load.py",
    "mcp_tool_comparison_benchmark.py",
    "mcp_tool_quality_benchmark.py",
    "run_benchmarks.py",
}


@dataclass(frozen=True)
class BenchmarkProfile:
    path: pathlib.Path
    name: str
    project: str
    workspace_root: pathlib.Path
    forge_binary: str
    codebase_binary: str
    scope_note: str
    excluded_directories: frozenset[str]
    scenarios: list[dict[str, Any]]


def _require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"profile field {label!r} must be a non-empty string")
    return value


def _interpolate(value: Any, project: str) -> Any:
    if isinstance(value, dict):
        return {key: _interpolate(item, project) for key, item in value.items()}
    if isinstance(value, list):
        return [_interpolate(item, project) for item in value]
    if isinstance(value, str):
        return value.replace("${project}", project)
    return value


def _parse_call(value: object, label: str, project: str) -> tuple[str, dict[str, Any]] | None:
    if value is None:
        return None
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be null or an object with tool and arguments")
    tool = _require_string(value.get("tool"), f"{label}.tool")
    arguments = value.get("arguments", {})
    if not isinstance(arguments, dict):
        raise ValueError(f"{label}.arguments must be an object")
    return tool, _interpolate(arguments, project)


def load_profile(
    profile_path: pathlib.Path,
    workspace_override: pathlib.Path | None = None,
) -> BenchmarkProfile:
    path = profile_path.expanduser().resolve()
    try:
        raw = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot load benchmark profile {path}: {error}") from error
    if (
        not isinstance(raw, dict)
        or not isinstance(raw.get("schema_version"), int)
        or isinstance(raw.get("schema_version"), bool)
        or raw.get("schema_version") != 1
    ):
        raise ValueError("benchmark profile requires schema_version: 1")

    name = _require_string(raw.get("name"), "name")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,99}", name):
        raise ValueError("profile name must contain only letters, digits, dot, underscore, or hyphen")
    project = _require_string(raw.get("project"), "project")
    configured_workspace = workspace_override if workspace_override is not None else raw.get("workspace_root")
    if configured_workspace is None:
        raise ValueError("set workspace_root in the profile or pass --workspace")
    if not isinstance(configured_workspace, (str, pathlib.Path)):
        raise ValueError("workspace_root must be a path string")
    workspace_root = pathlib.Path(configured_workspace).expanduser()
    if not workspace_root.is_absolute():
        base = pathlib.Path.cwd() if workspace_override is not None else ROOT
        workspace_root = base / workspace_root
    workspace_root = workspace_root.resolve()
    if not workspace_root.is_dir():
        raise ValueError(f"workspace root is not a directory: {workspace_root}")

    raw_scenarios = raw.get("scenarios")
    if not isinstance(raw_scenarios, list) or not raw_scenarios:
        raise ValueError("profile scenarios must be a non-empty array")
    scenarios: list[dict[str, Any]] = []
    identifiers: set[str] = set()
    for index, item in enumerate(raw_scenarios):
        label = f"scenarios[{index}]"
        if not isinstance(item, dict):
            raise ValueError(f"{label} must be an object")
        scenario_id = _require_string(item.get("id"), f"{label}.id")
        if scenario_id in identifiers:
            raise ValueError(f"duplicate scenario id: {scenario_id}")
        identifiers.add(scenario_id)
        scenario_name = _require_string(item.get("scenario"), f"{label}.scenario")
        forge = _parse_call(item.get("forge"), f"{label}.forge", project)
        codebase = _parse_call(item.get("codebase"), f"{label}.codebase", project)
        if forge is None and codebase is None:
            raise ValueError(f"{label} must define a forge or codebase call")
        match = item.get("match", "")
        if not isinstance(match, str):
            raise ValueError(f"{label}.match must be a string")
        scenarios.append(
            {
                "id": scenario_id,
                "scenario": scenario_name,
                "forge": forge,
                "codebase": codebase,
                "match": match,
            }
        )

    exclusions = raw.get("exclude_directories", [])
    if not isinstance(exclusions, list) or not all(
        isinstance(name, str) and name and "/" not in name and "\\" not in name
        for name in exclusions
    ):
        raise ValueError("exclude_directories must be an array of directory names")
    scope_note = raw.get("scope_note", "")
    if not isinstance(scope_note, str):
        raise ValueError("scope_note must be a string")

    return BenchmarkProfile(
        path=path,
        name=name,
        project=project,
        workspace_root=workspace_root,
        forge_binary=_binary_path(raw.get("forge_binary"), "contextunity-forge-mcp", path.parent),
        codebase_binary=_binary_path(raw.get("codebase_binary"), "codebase-memory-mcp", path.parent),
        scope_note=scope_note,
        excluded_directories=frozenset(DEFAULT_EXCLUDED_DIRECTORIES | set(exclusions)),
        scenarios=scenarios,
    )


def _binary_path(value: object, command_name: str, profile_directory: pathlib.Path) -> str:
    if value is not None:
        binary = pathlib.Path(_require_string(value, f"{command_name} binary")).expanduser()
        if binary.is_absolute() or "/" in str(binary):
            if not binary.is_absolute():
                binary = profile_directory / binary
            return str(binary.resolve())
        return str(binary)
    return shutil.which(command_name) or command_name


def copy_workspace_snapshot(
    workspace_root: pathlib.Path,
    destination: pathlib.Path,
    excluded_directories: frozenset[str],
) -> None:
    root = workspace_root.resolve()

    def ignore(_directory: str, names: list[str]) -> set[str]:
        excluded = excluded_directories.intersection(names)
        return excluded | BENCHMARK_FILES.intersection(names)

    shutil.copytree(root, destination, ignore=ignore, symlinks=True)
