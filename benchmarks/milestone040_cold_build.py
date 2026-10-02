"""Compare two release binaries with paired fresh cold builds."""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path


def run_build(binary: Path, workspace: Path, database: Path) -> dict[str, object]:
    completed = subprocess.run(
        [str(binary), "--root", str(workspace), "--db", str(database), "build"],
        check=True,
        capture_output=True,
        text=True,
    )
    report = json.loads(completed.stdout)
    return {
        "elapsed_ms": report["elapsed_ms"],
        "persist_graph_ms": report["persist_graph_ms"],
        "indexes_ms": report["indexes_ms"],
        "seal_ms": report["seal_ms"],
        "files": report["files"],
        "nodes": report["nodes"],
        "edges": report["edges"],
        "output_root": report["output_root"],
        "database_bytes": database.stat().st_size,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    rounds: list[dict[str, object]] = []
    with tempfile.TemporaryDirectory(prefix="milestone040-cold-") as temporary:
        for order in (("baseline", "candidate"), ("candidate", "baseline")):
            measured: dict[str, object] = {"order": list(order)}
            for label in order:
                binary = args.baseline if label == "baseline" else args.candidate
                database = Path(temporary) / f"{len(rounds)}-{label}.sqlite"
                measured[label] = run_build(binary, args.workspace, database)
            rounds.append(measured)

    roots_repeatable = all(
        rounds[0][label]["output_root"] == rounds[1][label]["output_root"]
        for label in ("baseline", "candidate")
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(
            {
                "measured_at": datetime.now(timezone.utc).isoformat(),
                "workspace": str(args.workspace),
                "baseline": str(args.baseline),
                "candidate": str(args.candidate),
                "roots_repeatable": roots_repeatable,
                "rounds": rounds,
            },
            indent=2,
        )
        + "\n"
    )
    if not roots_repeatable:
        raise SystemExit("Merkle output root varied within one binary; inspect output artifact")


if __name__ == "__main__":
    main()
