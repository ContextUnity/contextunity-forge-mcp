use crate::engine::scanner;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};
/// The template value.
pub const TEMPLATE: &str =
    "roots:\n  - .\nignore:\n  - target\n  - node_modules\n  - .venv\n  - .git\n";
/// Performs run.
pub fn run(root: &Path, topic: &str, force: bool) -> Result<Value> {
    let root = scanner::canonical_root(root)?;
    match topic {
        "init" => {
            let path = root.join("forge-mcp.yaml");
            if path.exists() && !force {
                bail!("forge-mcp.yaml already exists; pass force to replace");
            }
            crate::core::fs::atomic_write(&path, TEMPLATE.as_bytes(), false)?;
            Ok(json!({"path":path,"created":true}))
        }
        "adapter" => Ok(
            json!({"filename":"forge-mcp.yaml","template":TEMPLATE,"keys":["roots","eligible_roots","docs","milestones","plans","ignore","excluded_directory_names","excluded_file_names","linked_workspaces","workspaces"],"roots":"relative plain paths or .; symlinks and parent traversal rejected","ignore":"exact file or directory basenames","linked_workspaces":"array of {name, path, enabled?, roots, docs, ignore}; indexes external/sibling worktrees into unified graph; enabled (default true) allows toggling workspaces; skips unavailable worktrees and auto-rebuilds","default":"workspace root; gitignore and common build directories excluded"}),
        ),
        "docs" => Ok(
            json!({"frontmatter":"---\ndoc_type: architecture\ntitle: Module contract\n---","invariant":"> [!IMPORTANT] Invariant: Document the rule and reference `symbol.name`.","types":["architecture","adr","guide","api","plan"]}),
        ),
        "acdd" => Ok(json!({
            "gates": ["contract", "build", "review", "deliver"],
            "proof_policies": {
                "seam-test-first": "Greenfield: failing red seam test at contract (nonzero exit), passing green at build.",
                "direct-proof": "Pre-existing code: validate existing seam directly, exit code 0 accepted at contract.",
                "deferred-final-test": "Task-level policy: exit code 0 accepted at contract; milestone-level final test runs before milestone handoff."
            },
            "blackboard_topics": {
                "draft": "Contract drafts and proposed changes.",
                "notes": "Build notes, proof observations, and general context.",
                "findings": "Review findings and verified defects.",
                "blockers": "Issues preventing the active work from proceeding.",
                "decisions": "Decisions and trade-offs retained in the delivery receipt.",
                "deferred": "Deferred findings retained in the delivery receipt."
            },
            "blackboard_gate_targeting": "An optional gate field targets a message to one gate id from the active task profile. Untargeted messages remain generally visible; claim context also includes messages targeted to its active gate.",
            "blackboard_schema_version": 5,
            "review_contours": ["paths", "claims", "concurrency", "project_isolation", "administration"],
            "subtask_rules": "Subtask must name concrete target, expected status, and verification command. Not completed with partial implementation. Use subtask_add to deepen tasks, not root task proliferation.",
            "commit_authority": "Task commits, task-branch merges/cleanups, and merging the completed milestone branch into its target branch after handoff are authorized under ACDD. Push, force-push, and publication require user permission.",
            "reference": "Load the contextunity-forge skill for the procedure, docs/runbooks/acdd.md for the execution sequence, docs/reference/tasks.md for API and evidence schemas."
        })),
        "query" => Ok(json!({
            "start": "Call code_map_overview to identify indexed paths and coverage.",
            "symbols": "Use code_map_search with FTS terms or prefix* pattern. Hits include their indexed signature and inspect_selector arguments for code_map_inspect with show_source=true. Previews appear automatically for at most three total hits; set include_preview=true to include up to three previews on larger result sets. Selectors accept an exact ID, path:name, path:known_kind:name, path:line, or path#Lline. Bare names never get hijacked into modules. Compact inspect and explain include signature, docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees; set include_coverage=true when resolution evidence is needed.",
            "snippets": "Use get_code_snippet for a fast, bounded AST preview (default 5 leading + 35 body lines) before reading entire files via ctx_read.",
            "tests": "Call code_map_tests on a narrow symbol or module; broad scopes are rejected before unbounded traversal.",
            "queries": "Use code_map_inspect and code_map_explain for symbols, code_map_analyze for diagnostics, and code_map_impact for traversal. The legacy code_map_query tool supports overview, impact, slice, unwired, and read-only SQL; its inspect and explain operations are deprecated. For SQL, put one SELECT/WITH statement in selector; pages use limit, offset, and generation.",
            "diagnostics": "Call code_map_analyze with target='' for workspace totals; use an indexed path for a smaller scope or an exact file for paged rows. Cycles require include_cycles=true.",
            "documents": "Use search_docs to find sections and get_doc to read the selected section.",
            "pages": "Start with compact detail and a small limit. Continue with the returned next_offset and generation for the same selector and filters. For tighter agent context, set adapter response.page_size=10 and response.max_output_bytes=16384.",
            "recovery": "On a computation budget error, narrow the selector or path and reduce depth; lowering limit alone may not reduce count or traversal work. On a byte limit error, use compact detail, a smaller limit, or fewer SQL columns."
        })),
        "ast" => Ok(json!({
            "capabilities": {
                "complete_syntax": "Patterns that form valid standalone syntax at the grammar root; every profile supports this capability.",
                "declaration_without_body": "Declaration patterns omit profile-declared body nodes, including function, type, class, interface, and implementation bodies where supported.",
                "fragment_probe": "A profile may wrap root-rejected expressions, calls, macros, or assignments in a probe container, then unwrap only the matched target node.",
                "attribute": "A profile may match its declared decorators or annotations layered over declarations."
            },
            "profile_matrix": {
                "rust": ["complete_syntax", "declaration_without_body", "fragment_probe", "attribute"],
                "python": ["complete_syntax", "declaration_without_body", "attribute"],
                "typescript_javascript": ["complete_syntax", "declaration_without_body", "attribute"],
                "html_vue": ["complete_syntax"]
            },
            "wildcards": {
                "$NAME": "Capture one syntax node.",
                "$$$SEQ": "Capture a sequence of zero or more syntax nodes."
            },
            "examples": ["fn $NAME($$$ARGS) {}", "client.fetch($ARG)", "@$DEC\\ndef $NAME($$$ARGS):", "<div id=\"$ID\">$$$CHILDREN</div>"],
            "diagnostics": "Unsupported fragments and invalid syntax return a structured diagnostic with language, error span, actionable hint, and profile examples."
        })),
        "validate" => {
            let adapter = scanner::load_adapter(&root, None)?;
            let scan = scanner::scan_with_adapter(&root, &adapter)?;
            Ok(
                json!({"valid":true,"root":root,"roots":adapter.roots,"files":scan.files,"bytes":scan.bytes}),
            )
        }
        _ => bail!("unknown guide topic; expected init,adapter,docs,acdd,query,ast,validate"),
    }
}
#[derive(serde::Deserialize, serde::Serialize, Default)]
struct Checkpoints {
    entries: std::collections::BTreeMap<String, Value>,
}
fn checkpoint_name(name: Option<&str>) -> Result<&str> {
    let name = name.ok_or_else(|| anyhow::anyhow!("checkpoint name required"))?;
    if name.is_empty() || name.len() > 200 {
        bail!("checkpoint name must be 1..=200 characters");
    }
    if name.contains("..") || name.contains(['/', '\\', '\0']) {
        bail!("checkpoint name must not contain path traversal characters");
    }
    Ok(name)
}
/// Performs checkpoint.
pub fn checkpoint(
    root: &Path,
    action: &str,
    name: Option<&str>,
    content: Option<Value>,
) -> Result<Value> {
    let dir = root.join(".forge");
    let path = dir.join("checkpoints.json");
    if path.exists() {
        scanner::checked_child(root, Path::new(".forge/checkpoints.json"))?;
    }
    let mut data: Checkpoints = if path.exists() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        Checkpoints::default()
    };
    match action {
        "list" => {
            let entries = data
                .entries
                .iter()
                .map(|(name, value)| {
                    Ok((
                        name.clone(),
                        json!({"bytes":serde_json::to_vec(value)?.len()}),
                    ))
                })
                .collect::<Result<serde_json::Map<String, Value>>>()?;
            Ok(Value::Object(entries))
        }
        "get" => {
            let name = checkpoint_name(name)?;
            data.entries
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("checkpoint not found"))
        }
        "save" | "delete" => {
            let name = checkpoint_name(name)?;
            if action == "save" {
                let value =
                    content.ok_or_else(|| anyhow::anyhow!("checkpoint content required"))?;
                if serde_json::to_vec(&value)?.len() > 1024 * 1024 {
                    bail!("checkpoint exceeds1MiB");
                }
                data.entries.insert(name.into(), value);
            } else if data.entries.remove(name).is_none() {
                bail!("checkpoint not found");
            }
            crate::core::fs::atomic_write(&path, &serde_json::to_vec_pretty(&data)?, false)?;
            Ok(json!({"action":action,"name":name}))
        }
        _ => bail!("checkpoint action must be list,get,save,delete"),
    }
}
