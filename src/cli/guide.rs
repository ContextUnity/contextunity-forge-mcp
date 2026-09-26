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
        "validate" => {
            let adapter = scanner::load_adapter(&root, None)?;
            let scan = scanner::scan_with_adapter(&root, &adapter)?;
            Ok(
                json!({"valid":true,"root":root,"roots":adapter.roots,"files":scan.files,"bytes":scan.bytes}),
            )
        }
        _ => bail!("unknown guide topic; expected init,adapter,docs,validate"),
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
