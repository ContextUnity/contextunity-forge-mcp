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
            "gates": ["contract/v1", "build/v1", "review/v1", "deliver/v1"],
            "proof_policies": {
                "seam-test-first": "Greenfield: failing red seam test at contract/v1 (nonzero exit), passing green at build/v1.",
                "direct-proof": "Pre-existing code: validate existing seam directly, exit code 0 accepted at contract/v1.",
                "deferred-final-test": "Task-level policy: exit code 0 accepted at contract/v1; milestone-level final test runs before milestone handoff."
            },
            "blackboard_topics": {
                "contract_draft": "Red test path, command, observed failure, proposed seam.",
                "contract_findings": "Unsupported assumptions and required contract repairs.",
                "build_proof": "Candidate SHA, focused test result, Clippy result.",
                "architectural_notes": "Decisions or trade-offs surviving delivery; auto-copied to receipt at deliver/v1."
            },
            "review_contours": ["paths", "claims", "concurrency", "project_isolation", "administration"],
            "subtask_rules": "Subtask must name concrete target, expected status, and verification command. Not completed with partial implementation. Use subtask_add to deepen tasks, not root task proliferation.",
            "commit_authority": "Task commits allowed in milestone worktrees. Merge to main, push, and force require user permission.",
            "reference": "Read docs/reference/acdd.md for the contract, docs/runbooks/acdd.md for the execution sequence, docs/reference/tasks.md for API and evidence schemas."
        })),
        "query" => Ok(json!({
            "start": "Call code_map_overview to identify indexed paths and coverage.",
            "symbols": "Use code_map_search with FTS terms or prefix* pattern. Selectors support canonical ID, file path (resolves to module), path:symbol (resolves inner symbol), and path:line/path#Lline (resolves innermost AST node). Bare names never get hijacked into modules. Compact inspect and explain include signature, docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees; set include_coverage=true when resolution evidence is needed.",
            "snippets": "Use get_code_snippet for a fast, bounded AST preview (default 5 leading + 35 body lines) before reading entire files via ctx_read.",
            "tests": "Call code_map_tests on a narrow symbol or module; broad scopes are rejected before unbounded traversal.",
            "queries": "Use code_map_inspect and code_map_explain for symbols, code_map_analyze for diagnostics, and code_map_impact for traversal. The legacy code_map_query tool supports overview, impact, slice, unwired, and read-only SQL; its inspect and explain operations are deprecated. For SQL, put one SELECT/WITH statement in selector; pages use limit, offset, and generation.",
            "diagnostics": "Call code_map_analyze with target='' for workspace totals; use an indexed path for a smaller scope or an exact file for paged rows. Cycles require include_cycles=true.",
            "documents": "Use search_docs to find sections and get_doc to read the selected section.",
            "pages": "Start with compact detail and a small limit. Continue with the returned next_offset and generation for the same selector and filters. For tighter agent context, set adapter response.page_size=10 and response.max_output_bytes=16384.",
            "recovery": "On a computation budget error, narrow the selector or path and reduce depth; lowering limit alone may not reduce count or traversal work. On a byte limit error, use compact detail, a smaller limit, or fewer SQL columns."
        })),
        "validate" => {
            let adapter = scanner::load_adapter(&root, None)?;
            let scan = scanner::scan_with_adapter(&root, &adapter)?;
            Ok(
                json!({"valid":true,"root":root,"roots":adapter.roots,"files":scan.files,"bytes":scan.bytes}),
            )
        }
        _ => bail!("unknown guide topic; expected init,adapter,docs,acdd,query,validate"),
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
                    Ok((name.clone(), json!({"bytes":serde_json::to_vec(value)?.len()})))
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
