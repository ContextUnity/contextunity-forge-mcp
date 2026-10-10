use crate::core::tasks::Milestone;
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) struct Snapshot {
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) frontmatter: serde_yaml::Value,
    pub(super) id: String,
    pub(super) repository: String,
    pub(super) project: String,
    pub(super) status: String,
    pub(super) depends_on: Vec<String>,
    pub(super) invariants: Vec<String>,
    pub(super) tasks: Vec<serde_yaml::Value>,
}

pub(super) fn snapshot(
    root: &Path,
    path: PathBuf,
    repository: &str,
    project: &str,
) -> Result<Snapshot> {
    let text = fs::read_to_string(&path)?;
    let text = text.replace("\r\n", "\n");
    let status =
        Milestone::status_from_frontmatter(&text, super::is_archived_path(&path))?.to_owned();
    let header = text
        .strip_prefix("---\n")
        .context("milestone requires YAML frontmatter")?
        .split_once("\n---\n")
        .context("unterminated milestone frontmatter")?
        .0;
    let frontmatter: serde_yaml::Value = serde_yaml::from_str(header)?;
    let id = frontmatter["id"]
        .as_str()
        .context("milestone requires id")?
        .to_string();
    let repository = frontmatter["repository"]
        .as_str()
        .unwrap_or(repository)
        .to_string();
    let inferred_project = path
        .strip_prefix(root)
        .ok()
        .and_then(crate::core::tasks::infer_project_from_path);
    let project = frontmatter["project"]
        .as_str()
        .map(str::to_owned)
        .or(inferred_project)
        .unwrap_or_else(|| project.to_string());
    for value in [&id, &repository, &project] {
        crate::core::tasks::valid_identity(value)?;
    }
    let depends_on = frontmatter
        .get("depends_on")
        .map(|value| serde_yaml::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let invariants = frontmatter
        .get("invariants")
        .map(|value| serde_yaml::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut tasks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "```yaml" {
            continue;
        }
        let yaml = lines
            .by_ref()
            .take_while(|line| line.trim() != "```")
            .collect::<Vec<_>>()
            .join("\n");
        let value: serde_yaml::Value = serde_yaml::from_str(&yaml)?;
        if value["task_ref"].as_str().is_some() {
            tasks.push(value);
        }
    }
    let _ = path.strip_prefix(root)?;
    Ok(Snapshot {
        path,
        text,
        frontmatter,
        id,
        repository,
        project,
        status,
        depends_on,
        invariants,
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn snapshot_parses_frontmatter_identity_and_task_blocks() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("milestone_contract_{nonce}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("045-contract.md");
        fs::write(
            &path,
            "---\nid: m-contract-seam\ntitle: Contract fixture\ndoc_type: contract\nstatus: active\n---\n\n### task: seam\n\n```yaml\ntask_ref: seam\ntarget: parse milestone contract\n```\n",
        )
        .unwrap();

        let snapshot = super::snapshot(&root, path.clone(), "forge-mcp", "forge-mcp").unwrap();

        assert_eq!(snapshot.id, "m-contract-seam");
        assert_eq!(snapshot.repository, "forge-mcp");
        assert_eq!(snapshot.project, "forge-mcp");
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.tasks[0]["task_ref"].as_str(), Some("seam"));
        fs::remove_dir_all(root).unwrap();
    }
}
