use crate::engine::scanner;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::Path;
/// The template value.
pub const TEMPLATE: &str =
    "roots:\n  - .\nignore:\n  - target\n  - node_modules\n  - .venv\n  - .git\n";
const TOPICS: &[(&str, Option<&str>)] = &[
    ("query", None),
    ("init", None),
    ("adapter", None),
    ("validate", None),
    ("code", Some("codemap")),
    ("ast", Some("codemap")),
    ("docs", Some("docs")),
    ("lifecycle", Some("acdd")),
    ("acdd", Some("acdd")),
    ("gates", Some("acdd")),
    ("subtasks", Some("acdd")),
];

/// Produce guide content using the invocation's selected configuration.
pub fn run(root: &Path, topic: &str, force: bool) -> Result<Value> {
    let root = scanner::canonical_root(root)?;
    if topic.trim() == "init" {
        return initialize(&root, force);
    }
    let settings = forge_tasks::tasks::configuration::current(&root);
    let adapter =
        scanner::load_adapter(&root, settings.as_ref().and_then(|s| s.config.as_deref()))?;
    run_with_adapter(&root, topic, force, &adapter)
}

/// Produce the same guide from an already resolved CLI or MCP adapter.
pub fn run_with_adapter(
    root: &Path,
    topic: &str,
    force: bool,
    adapter: &scanner::Adapter,
) -> Result<Value> {
    let root = scanner::canonical_root(root)?;
    let topic = topic.trim();
    let topic = if topic.is_empty() { "query" } else { topic };
    if topic == "init" {
        return initialize(&root, force);
    }
    let enabled = |module: Option<&str>| module.is_none_or(|m| adapter.modules.is_enabled(m));
    if TOPICS
        .iter()
        .any(|(name, module)| *name == topic && !enabled(*module))
    {
        return Ok(json!({}));
    }
    let mut value = topic_value(&root, topic, adapter)?;
    if let Some(fields) = value.as_object_mut() {
        if topic == "query" {
            fields.insert(
                "topics".into(),
                json!(TOPICS
                    .iter()
                    .filter(|(_, module)| enabled(*module))
                    .map(|(topic, _)| *topic)
                    .collect::<Vec<_>>()),
            );
            if !adapter.modules.is_enabled("codemap") {
                for key in [
                    "start",
                    "sql_schema",
                    "symbols",
                    "snippets",
                    "tests",
                    "queries",
                    "diagnostics",
                    "recovery",
                ] {
                    fields.remove(key);
                }
            }
            if !adapter.modules.is_enabled("docs") {
                fields.remove("documents");
            }
            if !adapter.modules.is_enabled("codemap") && !adapter.modules.is_enabled("docs") {
                fields.remove("pages");
            }
        }
        if !adapter.modules.is_enabled("blackboard") {
            for key in [
                "blackboard_topics",
                "blackboard_gate_targeting",
                "blackboard_schema_version",
                "blackboard",
            ] {
                fields.remove(key);
            }
        }
    }
    Ok(value)
}

fn initialize(root: &Path, force: bool) -> Result<Value> {
    let path = root.join("forge-mcp.yaml");
    if path.exists() && !force {
        bail!("forge-mcp.yaml already exists; pass force to replace");
    }
    crate::core::fs::atomic_write(&path, TEMPLATE.as_bytes(), false)?;
    Ok(json!({"path":path,"created":true}))
}

fn topic_value(root: &Path, topic: &str, adapter: &scanner::Adapter) -> Result<Value> {
    match topic {
        "lifecycle" => Ok(json!({
            "purpose":"Advance admitted work through its active profile and observed proof gates.",
            "authority":"The selected active profile and task_claim workflow_guidance are authoritative. Gate names and order in examples are illustrative; follow the returned current gate, role, scope and proof policy.",
            "steps":[
                "Use task_list to discover ready work, then task_claim with task_id, worker_id, stage and worktree to obtain the current claim and complete context.",
                "Implement inside admitted scope. Call task_manage(action=verify, task_id=TASK_ID, test=tests/seam.rs::case) to observe proof, then task_submit for the claimed gate. Submission consumes accepted proof and follows profile transitions.",
                "Inspect structured rejection findings and the configured upstream target; resume the retained builder rather than starting an unrelated replacement.",
                "For configured headless runners use contextunity-forge-mcp task run TASK_ID --in-place or milestone run MILESTONE --in-place --target-base main. Complete required subtasks before delivery; use the existing immutable-candidate and receipt pipeline."
            ],
            "runners":"Configured role runner processes receive actual separate session identities and the gate prompt/context. Independent review currently checks distinct worker identity and persisted session fingerprint. Rejections resume the target builder session with structured findings when reuse is configured. Only an accepted structured submission passes a task gate; process exit zero alone is insufficient.",
            "blackboard":"Use task_blackboard for decisions, findings and blockers; inspect full payloads and follow the current claim's gate targeting.",
            "recovery":{
                "TASK_ALREADY_CLAIMED":"Inspect the active owner and worktree. Resume the owning worker or coordinate release/recovery before claiming; preserve its work and proofs.",
                "TASK_STALE_CLAIM":"Inspect current task and claim revision, obtain a fresh applicable claim, then rerun claim-bound verification; do not reuse stale evidence.",
                "TASK_AUTO_PROOF_UNAVAILABLE":"Run task verify for the admitted seam under the active claim and registered command, inspect the actual observation, then submit again.",
                "TASK_CANDIDATE_MISMATCH":"Preserve both work and accepted snapshot. Inspect drift and follow rejection back to the admitted build/contract owner before editing or regenerating candidate proof."
            },
            "example":{"mcp":{"name":"task_list","arguments":{"status":"ready"}},"cli":"contextunity-forge-mcp task list --status ready"},
            "references":["AGENTS.md","docs/reference/tasks.md","docs/reference/cli.md"]
        })),
        "gates" => Ok(json!({
            "purpose":"Follow the selected profile's conditions, routes, persistence and observed-proof requirements.",
            "steps":["Read the current claim workflow_guidance and active profile before choosing a gate or proof.","Verify the admitted seam without advancing; submit only current claim-bound observations.","Follow configured pass/fail and finding-class loop routes. Refresh verification after candidate changes."],
            "when":{
                "code":"At least one scope path is non-doc code.",
                "doc":"The scope is nonempty and all scope paths are documentation.",
                "db_migration":"Scope includes SQL files or migration paths.",
                "seam":"Task concurrency is Speculative or Parallel; test presence alone does not select this condition.",
                "public_api":"Scope exposes an exported AST callable."
            },
            "transitions":"The profile's pass target selects the next successful gate (default: next applicable gate); fail routes and max_attempts control retries/exhaustion. Classified findings select configured loop/reject_routing targets such as scope or symbols. Gate names and routes are profile data, not a fixed sequence.",
            "persistence":"save.pre and save.post apply configured persistence around execution. Parentless Git snapshot commits retain content without advancing branch history. Branch commits and automatic receipt commits occur only when configured. immutable: true fences the accepted candidate; drift fails TASK_CANDIDATE_MISMATCH and requires the configured rejection/repair path.",
            "verification":"task verify / task_manage(action=verify) observes without advancing; task submit consumes fresh claim-bound proof. gate_execution_traces persist actual command execution; commands.output.tests declares parsing of individual test outcomes and seam_test_refs records individually observed seams. Caller-supplied JSON is not an execution observation and cannot replace registered command proof or immutable candidate bindings.",
            "example":{"mcp":{"name":"task_manage","arguments":{"action":"verify","task_id":"TASK_ID","test":"tests/seam.rs::case"}},"cli":"contextunity-forge-mcp task verify TASK_ID --test tests/seam.rs::case"},
            "references":["docs/reference/tasks.md","docs/reference/acdd.md","docs/reference/cli.md"]
        })),
        "subtasks" => Ok(json!({
            "purpose":"Keep bounded task slices and their evidence visible before delivery.",
            "steps":["Use task_manage(action=subtask_add) to declare a concrete bounded target.","Use subtask_update with status and honest evidence for work actually verified; subtask_list reads the current slices.","Complete or cancel every subtask before delivery. A completed subtask requires nonempty evidence; cancelled is also permitted."],
            "delivery":{
                "terminal_statuses":["completed","cancelled"],"evidence_required_for":["completed"],
                "unfinished":"ready, in_progress, blocked or pending subtasks fail TASK_SUBTASKS_INCOMPLETE.",
                "missing_evidence":"A completed subtask with missing, empty or whitespace-only evidence fails TASK_SUBTASK_EVIDENCE_MISSING.",
                "evidence_boundary":"subtask_update stores an evidence string; it does not independently execute or verify that command. Gate proof still requires observed verification."
            },
            "example":{"mcp":{"name":"task_manage","arguments":{"action":"subtask_list","task_id":"TASK_ID"}},"cli":"contextunity-forge-mcp task subtask list TASK_ID"},
            "references":["docs/reference/tasks.md","docs/reference/cli.md"]
        })),
        "code" => {
            let query = topic_value(root, "query", adapter)?;
            let mut steps = ["start", "symbols", "snippets", "queries", "tests", "recovery"].into_iter().map(|key| query[key].clone()).collect::<Vec<_>>();
            steps.push(json!("Use ast_grep_search for structural patterns; guide ast describes profile capabilities. Treat candidate relationships and lexical test suggestions as navigation, not verified calls or coverage."));
            Ok(json!({"purpose":"Navigate indexed code through the existing query guidance and verified relationships.","steps":steps,"example":{"mcp":{"name":"code_map_overview","arguments":{}},"cli":"contextunity-forge-mcp query overview"},"references":["docs/reference/mcp-tools.md","docs/reference/cli.md"]}))
        }
        "adapter" => Ok(
            json!({"filename":"forge-mcp.yaml","template":TEMPLATE,"keys":["roots","eligible_roots","docs","milestones","plans","ignore","excluded_directory_names","excluded_file_names","linked_workspaces","workspaces"],"roots":"relative plain paths or .; symlinks and parent traversal rejected","ignore":"exact file or directory basenames","linked_workspaces":"array of {name, path, enabled?, roots, docs, ignore}; indexes external/sibling worktrees into unified graph; enabled (default true) allows toggling workspaces; skips unavailable worktrees and auto-rebuilds","default":"workspace root; gitignore and common build directories excluded"}),
        ),
        "docs" => Ok(
            json!({"frontmatter":"---\ndoc_type: architecture\ntitle: Module contract\n---","invariant":"> [!IMPORTANT] INV-MODULE-RULE: Document the rule and reference [`symbol.name`](../src/module.rs).","types":["architecture","adr","guide","api","plan"],
                "purpose":"Find and read documentation, then author valid metadata, links and invariants.",
                "steps":["Use search_docs(query, include_excerpt=true) to find relevant sections; CLI: contextunity-forge-mcp docs search TERM.","Use get_doc(path_or_id, detail=compact) for an outline, then section for the exact content; CLI: contextunity-forge-mcp docs get docs/guide.md.","Use forge_read(path, mode=lines) for source text; preserve frontmatter, relative links and uniquely linked symbol/invariant claims."],
                "example":{"mcp":{"name":"search_docs","arguments":{"query":"guide"}},"cli":"contextunity-forge-mcp docs search guide"},"references":["docs/AGENTS.md","docs/reference/mcp-tools.md","docs/reference/cli.md"]}),
        ),
        "acdd" => Ok(json!({
            "purpose":"Choose Workflow, Plan or Milestone execution under the canonical profile and proof authorities.",
            "steps":["Use workflow list/show/run for standalone root graphs; plan list/show/run for structured documentation steps; milestone run for admitted task waves and bound handoff.","Follow the active profile and claim workflow_guidance. Listed gate names are examples, not a hardcoded lifecycle.","Keep observed command/review evidence current with candidate content before submission or handoff."],
            "subjects":{
                "workflow":"A taskless root-gate graph uses save: none and no task rows or claims. Use workflow list, workflow show SELECTOR and workflow run SELECTOR. Persistent autonomous run/audit logging remains planned.",
                "plan":"Source-owned plan steps execute in the active working tree with snapshots and no automatic branch commit. Use plan list, plan show SELECTOR and plan run SELECTOR --in-place. Explicit execution profiles must be task-only and snapshot-only.",
                "milestone":"An admitted milestone follows synthesis, topological task waves, root verification, independent integration review and content-bound handoff. Use milestone run ID --in-place --target-base main, or owned isolated task worktrees. Existing task lifecycle, receipt, recovery and handoff owners remain authoritative."
            },
            "example":{"mcp":{"name":"task_list","arguments":{}},"cli":"contextunity-forge-mcp task list"},
            "references":["docs/adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md","docs/reference/cli.md","docs/reference/tasks.md"],
            "gates": ["contract_synthesis", "contract", "build", "review", "deliver"],
            "proof_policies": {
                "seam-test-first": "Greenfield: a compiling runtime-red seam at contract and passing green at build. Follow the selected profile's exact expected failure and count policy.",
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
            "commit_authority": "Follow current user authorization and repository AGENTS.md for task commits, integration and cleanup. Guidance alone grants no delivery or history-editing permission. Remote publication requires explicit approval.",
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
            let scan = scanner::scan_with_adapter(root, adapter)?;
            Ok(
                json!({"valid":true,"root":root,"roots":adapter.roots,"files":scan.files,"bytes":scan.bytes}),
            )
        }
        _ => bail!("unknown guide topic; expected init,adapter,docs,acdd,query,ast,validate,lifecycle,gates,subtasks,code"),
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
