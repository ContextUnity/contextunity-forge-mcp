"""Version-independent code identities and coverage evidence for cold builds.

Read SQLite after timing ends. Documentary edges never count as code coverage.
Identity includes source location, so overloads are not silently collapsed.
"""

from __future__ import annotations

from collections import Counter, defaultdict
import hashlib
import json
import pathlib
import sqlite3
from typing import Any


def workspace(path: str) -> str:
    return path.split("]/", 1)[0][1:] if path.startswith("[") and "]/" in path else "primary"


def digest(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def evidence(db: pathlib.Path) -> dict[str, Any]:
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as conn:
        nodes = {}
        test_nodes, test_cases = set(), set()
        for node_hash, path, kind, qualname, line, language, is_test, details in conn.execute(
            "SELECT n.node_hash,p.path,n.kind,n.qualname,n.line,n.language,n.is_test,n.details "
            "FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id"
        ):
            if language in ("markdown", "mdx") or kind in ("component", "doc_section", "invariant"):
                continue
            key = json.dumps([path, kind, qualname, line, language], ensure_ascii=False)
            nodes[node_hash] = key
            if is_test:
                test_nodes.add(key)
            decoded_details = json.loads(details) if details is not None else None
            is_test_case = (
                isinstance(decoded_details, dict)
                and decoded_details.get("test_role") == "test_case"
            )
            if is_test_case:
                test_cases.add(key)
        if len(set(nodes.values())) != len(nodes):
            raise ValueError("canonical node identity collision; refine identity before comparing coverage")
        occurrences, code_edges = [], set()
        edge_counts = dict(conn.execute("SELECT kind,count(*) FROM edges GROUP BY kind"))
        for src, dst, kind in conn.execute(
            "SELECT src_hash,dst_hash,kind FROM edges"
        ):
            if src in nodes and dst in nodes:
                if kind != "possible_calls":
                    code_edges.add((nodes[src], nodes[dst], kind))
        # Aggregate edges keep only one location per pair. Read all occurrences
        # so losing a second call site is visible even when the pair survives.
        for src, dst, path, line in conn.execute(
            "SELECT e.src_hash,e.dst_hash,p.path,e.line FROM edge_occurrences e "
            "JOIN path_dictionary p ON p.path_id=e.owner_id WHERE e.kind='calls'"
        ):
            if src in nodes and dst in nodes:
                occurrences.append((nodes[src], nodes[dst], path, line))
        resolved_references = sorted(set(conn.execute(
            "SELECT p.path,c.line,x.expression FROM resolution_coverage c "
            "JOIN path_dictionary p ON p.path_id=c.path_id "
            "JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE c.status='resolved'"
        )))
        code_files = sorted(conn.execute(
            "SELECT path,digest FROM files WHERE language NOT IN ('markdown','mdx') ORDER BY path"
        ))
        resolution: dict[str, Counter[str]] = defaultdict(Counter)
        for path, language, status, records in conn.execute(
            "SELECT p.path,c.language,c.status,c.records FROM coverage_language_counts c "
            "JOIN path_dictionary p ON p.path_id=c.path_id"
        ):
            resolution[f"{workspace(path)}:{language}"][status] += records
        resolutions = {}
        for group, counts in sorted(resolution.items()):
            denominator = sum(counts[k] for k in ("resolved", "ambiguous", "unresolved"))
            resolutions[group] = {
                **dict(counts),
                "resolved_percent_excluding_external": 100 * counts["resolved"] / denominator
                if denominator else None,
            }
        return {
            "code_files": code_files,
            "code_nodes": sorted(nodes.values()),
            "test_nodes": sorted(test_nodes),
            "test_cases": sorted(test_cases),
            "exact_call_occurrences": sorted(occurrences),
            "code_edges": sorted(code_edges),
            "resolved_references": resolved_references,
            "edge_counts_by_kind": edge_counts,
            "resolution_by_workspace_language": resolutions,
        }


def metrics(graph: dict[str, Any], db_bytes: int, wall_seconds: float) -> dict[str, Any]:
    nodes = graph["code_nodes"]
    calls = graph["exact_call_occurrences"]
    connected = {node for call in calls for node in call[:2]}
    pairs = {(call[0], call[1]) for call in calls}
    groups: dict[str, Counter[str]] = defaultdict(Counter)
    for key in nodes:
        path, _, _, _, language = json.loads(key)
        groups[f"{workspace(path)}:{language}"]["code_nodes"] += 1
        groups[f"{workspace(path)}:{language}"]["connected_nodes"] += key in connected
    for src, _ in pairs:
        path, _, _, _, language = json.loads(src)
        groups[f"{workspace(path)}:{language}"]["exact_call_pairs"] += 1
    return {
        "code_files": len(graph["code_files"]),
        "code_nodes": len(nodes),
        "exact_call_pairs": len(pairs),
        "exact_call_occurrences": len(calls),
        "code_edges": len(graph["code_edges"]),
        "resolved_references": len(graph["resolved_references"]),
        "exact_calls_per_code_node": len(pairs) / len(nodes) if nodes else None,
        "connected_code_nodes": len(connected),
        "connected_code_percent": 100 * len(connected) / len(nodes) if nodes else None,
        "code_files_per_second": len(graph["code_files"]) / wall_seconds,
        "database_bytes_per_code_node": db_bytes / len(nodes) if nodes else None,
        "code_by_workspace_language": {k: dict(v) for k, v in sorted(groups.items())},
        "resolution_by_workspace_language": graph["resolution_by_workspace_language"],
        "edge_counts_by_kind": graph["edge_counts_by_kind"],
        "evidence_sha256": digest(graph),
    }


def reachable(calls: set[tuple[Any, ...]], roots: set[str]) -> set[str]:
    outgoing: dict[str, set[str]] = defaultdict(set)
    for src, dst, *_ in calls:
        outgoing[src].add(dst)
    seen, todo = set(roots), list(roots)
    while todo:
        for dst in outgoing[todo.pop()] - seen:
            seen.add(dst)
            todo.append(dst)
    return seen


def compare_evidence(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    old_nodes, new_nodes = set(before["code_nodes"]), set(after["code_nodes"])
    old_calls = {tuple(row) for row in before["exact_call_occurrences"]}
    new_calls = {tuple(row) for row in after["exact_call_occurrences"]}
    old_occurrences = Counter(map(tuple, before["exact_call_occurrences"]))
    new_occurrences = Counter(map(tuple, after["exact_call_occurrences"]))
    old_pairs, new_pairs = {row[:2] for row in old_calls}, {row[:2] for row in new_calls}
    roots = set(before["test_cases"]) | set(after["test_cases"])
    root_policy = "union of explicitly classified test_case identities, applied to BOTH graphs"
    if not roots:
        roots = set(before["test_nodes"]) | set(after["test_nodes"])
        root_policy = "legacy test-node union (neither binary classified test cases); not verified test coverage"
    test_nodes = set(before["test_nodes"]) | set(after["test_nodes"])
    production = (old_nodes | new_nodes) - test_nodes
    old_reached = reachable(old_calls, roots & old_nodes) & production
    new_reached = reachable(new_calls, roots & new_nodes) & production
    return {
        "lost_code_edges": sorted(set(map(tuple, before["code_edges"])) - set(map(tuple, after["code_edges"]))),
        "added_code_edges": sorted(set(map(tuple, after["code_edges"])) - set(map(tuple, before["code_edges"]))),
        "lost_resolved_references": sorted(set(map(tuple, before["resolved_references"])) - set(map(tuple, after["resolved_references"]))),
        "added_resolved_references": sorted(set(map(tuple, after["resolved_references"])) - set(map(tuple, before["resolved_references"]))),
        "lost_code_files": sorted(set(map(tuple, before["code_files"])) - set(map(tuple, after["code_files"]))),
        "added_code_files": sorted(set(map(tuple, after["code_files"])) - set(map(tuple, before["code_files"]))),
        "lost_code_nodes": sorted(old_nodes - new_nodes),
        "added_code_nodes": sorted(new_nodes - old_nodes),
        "lost_exact_call_pairs": sorted(old_pairs - new_pairs),
        "added_exact_call_pairs": sorted(new_pairs - old_pairs),
        "lost_exact_call_occurrences": sorted((old_occurrences - new_occurrences).elements()),
        "added_exact_call_occurrences": sorted((new_occurrences - old_occurrences).elements()),
        "aligned_test_reachability": {
            "root_policy": root_policy,
            "roots": len(roots),
            "roots_missing_from_baseline": sorted(roots - old_nodes),
            "roots_missing_from_candidate": sorted(roots - new_nodes),
            "production_union_nodes": len(production),
            "baseline_reached": len(old_reached),
            "candidate_reached": len(new_reached),
            "lost_production_nodes": sorted(old_reached - new_reached),
            "added_production_nodes": sorted(new_reached - old_reached),
        },
    }
