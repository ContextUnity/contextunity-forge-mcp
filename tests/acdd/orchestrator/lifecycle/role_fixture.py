"""External role protocol fixture: all Forge state/proof operations stay real."""

import json
import os
from pathlib import Path
import subprocess
import sys
import time


executable, session, prompt = sys.argv[1:]
claim = json.loads(prompt.split("\n\nGate context:\n", 1)[1])
root = Path(claim["worktree"])
control = root / ".forge/lifecycle"
guidance = claim["workflow_guidance"]
record = {
    "stage": claim["stage"],
    "worker_id": claim["worker_id"],
    "session": session,
    "environment_session": os.environ.get("FORGE_AGENT_SESSION_ID"),
    "claim_revision": claim["claim_revision"],
    "rejection": guidance.get("rejection"),
    "prompt": prompt,
}
with (control / "roles.jsonl").open("a") as log:
    log.write(json.dumps(record) + "\n")

mode_path = control / "mode.json"
mode = json.loads(mode_path.read_text()) if mode_path.exists() else "submit"
if mode == "no-submit":
    sys.exit(0)
if mode == "exit-17":
    sys.exit(17)
if mode == "sleep":
    time.sleep(60)
    sys.exit(0)


def forge(*args):
    result = subprocess.run(
        [executable, "--root", str(root), "--format", "json", "task", *args],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


task_id, stage = claim["task_id"], claim["stage"]
kind = guidance["proof_kind"]
if kind in ("contract", "command", "seamtest"):
    observation = forge("verify", task_id)
    if observation.get("error") is not None:
        raise RuntimeError(observation["error"])
    forge("submit", task_id, "--stage", stage, "--action", "pass")
elif kind == "review":
    evidence = guidance["expected_evidence_template"]
    evidence["proof"] = json.loads((control / "review-proof.json").read_text())
    rejection_path = control / "reject.json"
    action = "pass"
    args = []
    if rejection_path.exists():
        findings = json.loads(rejection_path.read_text())
        rejection_path.rename(control / "rejected.json")
        evidence["proof"]["review_proof"]["decision"] = "reject"
        action = "reject"
        args = ["--findings", json.dumps(findings)]
    forge(
        "submit", task_id, "--stage", stage, "--action", action,
        "--evidence", json.dumps(evidence), *args,
    )
else:
    raise RuntimeError(f"unexpected role proof kind: {kind}")
