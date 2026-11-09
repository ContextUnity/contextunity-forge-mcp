use crate::engine::scanner;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::Path;
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
            json!({"frontmatter":"---\ndoc_type: architecture\ntitle: Module contract\n---","invariant":"> [!IMPORTANT] INV-MODULE-RULE: Document the rule and reference [`symbol.name`](../src/module.rs).","types":["architecture","adr","guide","api","plan"]}),
        ),
        "acdd" => Ok(json!({
            "gates": ["contract_synthesis", "contract", "build", "review", "deliver"],
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
            "blackboard_gate_targeting": "An optional gate field targets a message to one gate id from the active task profile. Claim context includes untargeted and active-gate messages; active decisions, findings, and blockers remain visible across gates in context_bundle.",
            "blackboard_schema_version": 5,
            "review_contours": ["paths", "claims", "concurrency", "project_isolation", "administration"],
            "subtask_rules": "Subtask must name concrete target, expected status, and verification command. Not completed with partial implementation. Use subtask_add to deepen tasks, not root task proliferation.",
            "commit_authority": "Task commits, task-branch merges/cleanups, and merging the completed milestone branch into its target branch after handoff are authorized under ACDD. Push, force-push, and publication require user permission.",
            "reference": "Load the contextunity-forge skill for the procedure, AGENTS.md for worktrees and permissions, docs/reference/tasks.md for API and evidence schemas."
        })),
        "query" => Ok(json!({
            "start": "Call code_map_overview to identify indexed paths and coverage.",
            "sql_schema": "nodes(node_id, id, kind, name, qualname, path_id, line, end_line, is_test, language, generated, details, node_hash, owner_path_id); path_dictionary(path_id, path); edges(src_hash, kind, dst_hash, path_id, line, evidence_id, confidence_id, occurrence_count); files(path, status, digest, size, language, is_test, generated, path_hash); doc_sections(doc_id, path, section_title, doc_type, content, invariants, referenced_symbols, is_invariant). Paths: FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id. Edge endpoints: FROM edges e JOIN nodes src ON e.src_hash=src.node_hash JOIN nodes dst ON e.dst_hash=dst.node_hash. nodes and edges have no path text column; edges have no src/dst text columns or compatibility views.",
            "symbols": "Use code_map_search with FTS terms or prefix* pattern. Hits include their indexed signature and inspect_selector arguments for code_map_inspect with show_source=true. Previews appear automatically for at most three total hits; set include_preview=true to include up to three previews on larger result sets. Selectors accept an exact ID, path:name, path:known_kind:name, path:line, or path#Lline. Bare names never get hijacked into modules. Compact inspect and explain include signature, docstring, receiver-aware container, inbound/outbound call counts, and up to five direct callers/callees; set include_coverage=true when resolution evidence is needed.",
            "snippets": "Use forge_read(selector=...) for digest-verified symbol bodies, or forge_read(path=..., mode=lines|raw|search|delta|outline) for layered file reads. CLI: contextunity-forge-mcp read <path> --mode <mode>. Core reads work in minimal workspaces; conditional Hints reflect enabled graph and task signals.",
            "tests": "Call code_map_tests on a narrow symbol or module; broad scopes are rejected before unbounded traversal.",
            "queries": "Use code_map_inspect and code_map_explain for symbols, code_map_analyze for diagnostics, and code_map_impact for traversal. Use code_map_query for sql (pass sql='SELECT ...' directly), untested, implementations, slice, unwired, and duplicates; pages use limit, offset, and generation.",
            "diagnostics": "Call code_map_analyze with target='' for workspace totals; use an indexed path for a smaller scope or an exact file for paged rows. Cycles require include_cycles=true.",
            "documents": "Use search_docs with include_excerpt=true to find sections, get_doc with detail='compact' for section outlines, and get_doc with section='...' to read a specific section.",
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
            let settings = forge_tasks::tasks::configuration::current(&root);
            let adapter = scanner::load_adapter(
                &root,
                settings
                    .as_ref()
                    .and_then(|settings| settings.config.as_deref()),
            )?;
            let scan = scanner::scan_with_adapter(&root, &adapter)?;
            Ok(
                json!({"valid":true,"root":root,"roots":adapter.roots,"files":scan.files,"bytes":scan.bytes}),
            )
        }
        _ => bail!("unknown guide topic; expected init,adapter,docs,acdd,query,ast,validate"),
    }
}
fn checkpoint_name(name: Option<&str>) -> Result<&str> {
    let breadcrumb = name.ok_or_else(|| anyhow::anyhow!("checkpoint breadcrumb required"))?;
    if breadcrumb.is_empty() || breadcrumb.len() > 2048 || breadcrumb.contains('\0') {
        bail!("checkpoint breadcrumb must contain 1..=2048 bytes");
    }
    Ok(breadcrumb)
}

fn checkpoint_git_tree(root: &Path) -> Result<String> {
    if root.join(".git").exists() {
        crate::core::git::git_stdout(
            root,
            &["rev-parse", "HEAD^{tree}"],
            "TASK_CHECKPOINT_GIT_FAILED",
        )
    } else {
        Ok("unversioned".into())
    }
}

/// List, save, get, or delete a state payload addressed by its full gate breadcrumb.
pub fn checkpoint(
    root: &Path,
    action: &str,
    name: Option<&str>,
    content: Option<Value>,
) -> Result<Value> {
    let store = crate::engine::tasks::store(root)?;
    match action {
        "list" => {
            let entries = store
                .list_checkpoints()?
                .iter()
                .map(|checkpoint| {
                    Ok((
                        checkpoint.breadcrumb.clone(),
                        json!({"bytes":serde_json::to_vec(&checkpoint.state_payload)?.len()}),
                    ))
                })
                .collect::<Result<serde_json::Map<String, Value>>>()?;
            Ok(Value::Object(entries))
        }
        "get" => {
            let breadcrumb = checkpoint_name(name)?;
            store
                .get_checkpoint(breadcrumb)?
                .map(|checkpoint| checkpoint.state_payload)
                .ok_or_else(|| anyhow::anyhow!("checkpoint not found"))
        }
        "save" | "delete" => {
            let breadcrumb = checkpoint_name(name)?;
            if action == "save" {
                let value =
                    content.ok_or_else(|| anyhow::anyhow!("checkpoint content required"))?;
                if serde_json::to_vec(&value)?.len() > 1024 * 1024 {
                    bail!("checkpoint exceeds 1 MiB");
                }
                store.save_checkpoint_payload(breadcrumb, &checkpoint_git_tree(root)?, value)?;
            } else if !store.delete_checkpoint(breadcrumb)? {
                bail!("checkpoint not found");
            }
            Ok(json!({"action":action,"name":breadcrumb}))
        }
        _ => bail!("checkpoint action must be list,get,save,delete"),
    }
}
