import json
import os
from pathlib import Path
import sys

context = json.loads(sys.argv[1].split("\n\nGate context:\n", 1)[1])
gate = context["gate"]
session = os.environ["FORGE_AGENT_SESSION_ID"]
with Path("workflow-role-sessions.jsonl").open("a") as output:
    output.write(json.dumps({"gate": gate, "session": session, "findings": context["findings"], "config": os.environ.get("FORGE_CONFIG"), "profile": os.environ.get("FORGE_PROFILE"), "db": os.environ.get("FORGE_DB")}) + "\n")
if Path("missing-decision").exists():
    sys.exit(0)
decision = "pass"
findings = None
proof = {"none_proof": {}}
if gate == "review":
    if not Path("review-rejected").exists():
        Path("review-rejected").touch()
        decision = "reject"
        findings = {"scope_findings": ["Apply the required correction"]}
    proof = {"scheme_proof": {"decision": decision, "summary": "Inspected this workflow run"}}
print(json.dumps({"gate": gate, "session_id": session, "decision": decision, "proof": proof, "findings": findings}))
