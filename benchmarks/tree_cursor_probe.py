"""Screen wildcard query captures against balanced Python cursor traversal."""

import argparse
import hashlib
import json
import pathlib
import subprocess
import tempfile


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=pathlib.Path, required=True)
    parser.add_argument("--repo", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    root = pathlib.Path(__file__).resolve().parent.parent
    source = root / "benchmarks/tree_cursor_probe.rs"
    deps = root / "target/release/deps"
    dependencies = {
        name: max(deps.glob(f"lib{name}-*.rlib"), key=lambda path: path.stat().st_mtime)
        for name in ("tree_sitter", "tree_sitter_python")
    }
    with tempfile.TemporaryDirectory(prefix="forge-tree-query-probe-") as directory:
        work = pathlib.Path(directory)
        binary = work / "probe"
        command = ["rustc", "--edition=2021", "-O", "-C", "lto=thin", "-C", "panic=abort",
                   "-L", f"dependency={deps}"]
        for name, path in dependencies.items():
            command.extend(["--extern", f"{name}={path}"])
        command.extend([str(source), "-o", str(binary)])
        subprocess.run(command, check=True)
        paths = work / "paths.txt"
        inventory = json.loads(args.inventory.read_text())
        indexed = inventory["source_paths"]
        trivial = {item["path"] for item in inventory["existing_trivial_fast_path"]}
        paths.write_text("".join(f"{path}\n" for logical, path in indexed.items() if logical not in trivial))
        result = subprocess.run([str(binary), str(paths)], check=False, capture_output=True, text=True)
        if result.returncode:
            report = {"exit_code": result.returncode, "stderr": result.stderr[:4096],
                      "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
            args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
            print(json.dumps(report))
            raise SystemExit(1)
        report = json.loads(result.stdout)
        report.update({
            "workspace": str(args.repo.resolve()),
            "diagnostics": result.stderr[:4096],
            "existing_trivial_files_excluded": len(trivial),
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "compiler_command": command,
            "policy": "Six alternating per-file repeats; parse and order validation excluded; query measures entries only, cursor measures balanced entry/exit events; screening only, not an end-to-end speed claim",
            "query_to_manual_ratio": report["query_entries_only_ns"] / report["manual_balanced_ns"],
            "guarded_to_manual_ratio": report["guarded_balanced_ns"] / report["manual_balanced_ns"],
        })
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
        print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()
