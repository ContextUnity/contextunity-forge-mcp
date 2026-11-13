"""Protocol substitute for unavailable LLM roles; Forge owns all state and checks."""
import json
from pathlib import Path
import sys

context = json.loads(sys.argv[1].split("\n\nGate context:\n", 1)[1])
root = Path.cwd()
with (root / ".forge/wave-roles.jsonl").open("a") as log:
    log.write(json.dumps(context) + "\n")
if (root / ".forge/no-wave-decision").exists():
    sys.exit(0)
if context["gate"] == "preflight":
    task_slice = {
        "layer": "fixture seam",
        "summary": "Exercise the pre-task proof boundary.",
        "exported_interfaces": [],
        "negative_boundaries": [],
        "incoming_seams": [],
        "outgoing_seams": [],
    }
    tasks = {} if (root / ".forge/invalid-task-proof-map").exists() else {"first": task_slice}
    print(json.dumps({
        "gate": context["gate"],
        "session_id": context["session_id"],
        "decision": "pass",
        "proof": {"scheme_proof": {
            "cited_invariants": [],
            "cited_symbols": [],
            "global_negative_boundaries": [],
            "blackboard_decision_id": "wave-blueprint",
            "tasks": tasks,
        }},
    }))
    sys.exit(0)
if context["gate"] == "milestone_review" and (root / ".forge/stop-wave-review").exists():
    sys.exit(17)
proof = {"none_proof": {}}
response = {"gate": context["gate"], "session_id": context["session_id"], "decision": "pass"}
if context["gate"] == "milestone_review":
    proof = {"scheme_proof": {"decision": "pass", "summary": "Reviewed fixture task scope and receipts"}}
    response["binding"] = context["subject_context"]["binding"]
    response["integration_review"] = {
        "worker_id": context["session_id"], "decision": "pass",
        "summary": "Reviewed the supplied fixture authority, task receipts and implementation binding",
        "scope_findings": [], "contract_findings": [],
    }
    if (root / ".forge/stale-wave-review").exists():
        response["binding"]["implementation_digest"] = "stale"
response["proof"] = proof
print(json.dumps(response))
