use crate::engine::scanner;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};
pub const TEMPLATE: &str =
    "roots:\n  - .\nignore:\n  - target\n  - node_modules\n  - .venv\n  - .git\n";
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
            json!({"filename":"forge-mcp.yaml","template":TEMPLATE,"keys":["roots","eligible_roots","doc_roots","ignore","excluded_directory_names","excluded_file_names","linked_workspaces","workspaces"],"roots":"relative plain paths or .; symlinks and parent traversal rejected","ignore":"exact file or directory basenames","linked_workspaces":"array of {name, path, roots, doc_roots, ignore}; indexes external/sibling worktrees into unified graph; skips unavailable worktrees and auto-rebuilds","default":"workspace root; gitignore and common build directories excluded"}),
        ),
        "docs" => Ok(
            json!({"frontmatter":"---\ndoc_type: architecture\ntitle: Module contract\n---","invariant":"> [!IMPORTANT] Invariant: Document the rule and reference `symbol.name`.","types":["architecture","adr","guide","api","plan"]}),
        ),
        "query" => Ok(json!({
            "start": "Call code_map_overview to identify indexed paths and coverage.",
            "symbols": "Use code_map_search with FTS terms or a prefix* pattern for names; a leading wildcard can scan the full index. Then use code_map_inspect for a precise selector, code_map_explain for relationships, and code_map_impact with depth=1 before deeper traversal.",
            "tests": "Call code_map_tests on a narrow symbol or module; broad scopes are rejected before unbounded traversal.",
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
        _ => bail!("unknown guide topic; expected init,adapter,docs,query,validate"),
    }
}
#[derive(serde::Deserialize, serde::Serialize, Default)]
struct Checkpoints {
    entries: std::collections::BTreeMap<String, Value>,
}
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
        "list" => Ok(json!(data.entries)),
        "get" => {
            let name = name.ok_or_else(|| anyhow::anyhow!("checkpoint name required"))?;
            data.entries
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("checkpoint not found"))
        }
        "save" | "delete" => {
            let name = name
                .filter(|n| !n.is_empty() && n.len() <= 200)
                .ok_or_else(|| anyhow::anyhow!("checkpoint name must be1..200 characters"))?;
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
