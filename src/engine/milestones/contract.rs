use crate::{core::tasks::Milestone, engine::tasks::TaskSettings};
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

pub(super) fn snapshot(root: &Path, path: PathBuf, settings: &TaskSettings) -> Result<Snapshot> {
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
        .unwrap_or(&settings.task_repository)
        .to_string();
    let project = settings.resolve_project(
        path.strip_prefix(root)?,
        frontmatter["project"].as_str(),
        settings
            .task_project
            .as_deref()
            .unwrap_or(&settings.task_repository),
    )?;
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
    use crate::engine::{milestones::contract::snapshot, tasks::TaskSettings};
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

        let settings: TaskSettings = serde_yaml::from_str("").unwrap();
        let parsed = snapshot(&root, path.clone(), &settings).unwrap();

        assert_eq!(parsed.id, "m-contract-seam");
        assert_eq!(parsed.repository, "forge-mcp");
        assert_eq!(parsed.project, "forge-mcp");
        assert_eq!(parsed.tasks.len(), 1);
        assert_eq!(parsed.tasks[0]["task_ref"].as_str(), Some("seam"));

        fs::write(
            root.join("forge-mcp.yaml"),
            "owners:\n  extensions/base: base\n  extensions/base/nested: nested\naliases:\n  legacy: canonical\n",
        )
        .unwrap();
        let settings = TaskSettings::load(&root).unwrap();
        for (relative, declared, expected) in [
            ("docs/milestones/001-default.md", "", "forge-mcp"),
            (
                "extensions/other/docs/milestones/001-inferred.md",
                "",
                "extensions.other",
            ),
            ("extensions/base/docs/milestones/001-owned.md", "", "base"),
            (
                "extensions/base/nested/docs/milestones/001-nested.md",
                "",
                "nested",
            ),
            (
                "extensions/baseball/docs/milestones/001-boundary.md",
                "",
                "extensions.baseball",
            ),
            (
                "extensions/base/docs/milestones/002-declared.md",
                "project: legacy\n",
                "canonical",
            ),
        ] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("---\nid: m-case\ntitle: Case\n{declared}doc_type: contract\n---\n```yaml\ntask_ref: partial\ntarget: Still readable before admission\n```\n")).unwrap();
            let parsed = snapshot(&root, path, &settings).unwrap();
            assert_eq!(parsed.project, expected, "{relative}");
            assert_eq!(parsed.tasks[0]["task_ref"].as_str(), Some("partial"));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
