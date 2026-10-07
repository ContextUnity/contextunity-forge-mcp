use crate::{
    core::tasks::{Milestone, Receipt},
    db::tasks_store::{Task, TasksStore},
    engine::tasks,
};
use anyhow::{bail, Context, Result};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// Inputs for creating a milestone document.
pub struct Init {
    /// Explicit milestone number.
    pub num: Option<String>,
    /// Stable milestone slug.
    pub slug: Option<String>,
    /// Human-readable milestone title.
    pub title: Option<String>,
    /// Source plan path.
    pub plan: Option<PathBuf>,
    /// Target directory for the milestone document.
    pub dir: Option<PathBuf>,
    /// Description appended to the milestone body.
    pub desc: Option<String>,
    /// Milestone IDs required before this work.
    pub depends_on: Vec<String>,
    /// Starts the active development clock at creation.
    pub active: bool,
    /// Description and task blocks supplied through standard input.
    pub stdin: String,
}

/// Returns the list of milestone directories configured for the workspace.
pub fn configured_milestone_dirs(root: &Path) -> Vec<PathBuf> {
    if let Ok(config) = std::fs::read_to_string(root.join("forge-mcp.yaml")) {
        #[derive(serde::Deserialize)]
        struct Cfg {
            milestones: Option<Vec<String>>,
        }
        if let Ok(cfg) = serde_yaml::from_str::<Cfg>(&config) {
            if let Some(dirs) = cfg.milestones {
                let resolved = resolve_dirs_with_wildcards(root, &dirs);
                if !resolved.is_empty() {
                    return resolved;
                }
            }
        }
    }
    vec![root.join("docs/milestones")]
}

fn resolve_dirs_with_wildcards(root: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let mut resolved = Vec::new();
    for pat in patterns {
        let norm = pat.trim_matches('/').replace('\\', "/");
        if norm.contains("..") {
            continue;
        }
        if norm.contains('*') {
            let parts: Vec<&str> = norm.split('/').collect();
            expand_parts(root, root, &parts, &mut resolved);
        } else {
            let candidate = root.join(&norm);
            if candidate.exists() {
                if let (Ok(canon), Ok(root_canon)) = (candidate.canonicalize(), root.canonicalize())
                {
                    if canon.starts_with(&root_canon) {
                        resolved.push(candidate);
                    }
                }
            } else {
                resolved.push(candidate);
            }
        }
    }
    resolved.sort();
    resolved.dedup();
    resolved
}

fn expand_parts(root: &Path, current: &Path, parts: &[&str], out: &mut Vec<PathBuf>) {
    if parts.is_empty() {
        if current.exists() && current.is_dir() {
            if let (Ok(canon), Ok(root_canon)) = (current.canonicalize(), root.canonicalize()) {
                if canon.starts_with(&root_canon) {
                    out.push(current.to_path_buf());
                }
            }
        }
        return;
    }
    let first = parts[0];
    let remaining = &parts[1..];
    if first == "*" {
        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    expand_parts(root, &entry.path(), remaining, out);
                }
            }
        }
    } else {
        let next = current.join(first);
        if next.exists() {
            expand_parts(root, &next, remaining, out);
        }
    }
}

fn target_milestone_paths(target_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for dir in [target_dir.to_path_buf(), target_dir.join("archive")] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().is_some_and(|e| e == "md")
                && entry.file_name().to_str().is_some_and(|name| {
                    name.split_once('-').is_some_and(|(number, _)| {
                        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
                    })
                })
            {
                paths.push(entry.path());
            }
        }
    }
    Ok(paths)
}

fn milestone_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let dirs = configured_milestone_dirs(root);
    for base_dir in dirs {
        for dir in [&base_dir, &base_dir.join("archive")] {
            if !dir.exists() {
                continue;
            }
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && entry.path().extension().is_some_and(|e| e == "md")
                    && entry.file_name().to_str().is_some_and(|name| {
                        name.split_once('-').is_some_and(|(number, _)| {
                            !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
                        })
                    })
                {
                    paths.push(entry.path());
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn prefix(path: &Path) -> Result<Option<u64>> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };
    let Some((digits, _)) = name.split_once('-') else {
        return Ok(None);
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    Ok(Some(digits.parse().with_context(|| {
        format!("milestone number overflow: {}", path.display())
    })?))
}

pub(crate) fn file_id(path: &Path) -> Result<Option<String>> {
    let text = fs::read_to_string(path)?;
    let text = text.replace("\r\n", "\n");
    let Some(body) = text.strip_prefix("---\n") else {
        return Ok(None);
    };
    let Some((header, _)) = body.split_once("\n---\n") else {
        return Ok(None);
    };
    let meta: serde_yaml::Value = serde_yaml::from_str(header)?;
    Ok(meta["id"].as_str().map(str::to_owned))
}

fn quote(value: &str) -> Result<String> {
    Ok(serde_yaml::to_string(value)?.trim().to_string())
}

/// Creates the next numbered milestone contract from explicit or plan inputs.
pub fn init(root: &Path, options: Init) -> Result<Value> {
    let target_dir = if let Some(d) = &options.dir {
        let path = if d.is_absolute() {
            if !d.starts_with(root) {
                bail!(
                    "milestone directory must be within workspace root: {}",
                    d.display()
                );
            }
            d.clone()
        } else {
            crate::core::tasks::confined_path(root, &d.to_string_lossy())?
        };
        let rel_target = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let matches = configured_milestone_dirs(root).iter().any(|c| {
            let rel_c = c
                .strip_prefix(root)
                .unwrap_or(c)
                .to_string_lossy()
                .replace('\\', "/");
            crate::engine::scanner::matches_path_pattern(&rel_target, &rel_c)
        });
        if !matches {
            bail!("target directory '{}' does not match any configured milestone directory in forge-mcp.yaml", rel_target);
        }
        path
    } else if let Some(plan) = &options.plan {
        let plan_full = if plan.is_absolute() {
            plan.clone()
        } else {
            root.join(plan)
        };
        let candidate = plan_full
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("milestones"));
        if candidate.as_ref().is_some_and(|p| p.exists()) {
            candidate.unwrap()
        } else {
            configured_milestone_dirs(root)
                .into_iter()
                .next()
                .unwrap_or_else(|| root.join("docs/milestones"))
        }
    } else {
        configured_milestone_dirs(root)
            .into_iter()
            .next()
            .unwrap_or_else(|| root.join("docs/milestones"))
    };
    let paths = target_milestone_paths(&target_dir)?;
    let numbered = paths
        .iter()
        .map(|p| prefix(p))
        .collect::<Result<Vec<_>>>()?;
    let number = if let Some(num) = &options.num {
        if num.len() < 3 || !num.bytes().all(|b| b.is_ascii_digit()) {
            bail!("milestone number must contain at least three digits");
        }
        num.clone()
    } else {
        let next = numbered
            .iter()
            .filter_map(|n| *n)
            .max()
            .unwrap_or(0)
            .checked_add(10)
            .context("milestone number overflow")?;
        format!("{next:03}")
    };
    let number_value: u64 = number.parse()?;
    if numbered.contains(&Some(number_value)) {
        bail!("milestone number already exists: {number}");
    }
    let (plan_meta, plan_body) = if let Some(plan) = &options.plan {
        let plan = crate::core::tasks::confined_path(root, &plan.to_string_lossy())?;
        let text = fs::read_to_string(plan)?;
        let body = text
            .strip_prefix("---\n")
            .context("plan requires YAML frontmatter")?;
        let (header, body) = body
            .split_once("\n---\n")
            .context("unterminated plan frontmatter")?;
        let meta: serde_yaml::Value = serde_yaml::from_str(header)?;
        if meta["doc_type"].as_str() != Some("plan") {
            bail!("source document must have doc_type: plan");
        }
        (Some(meta), body.to_string())
    } else {
        (None, String::new())
    };
    let title = options
        .title
        .or_else(|| {
            plan_meta
                .as_ref()
                .and_then(|m| m["title"].as_str().map(str::to_owned))
        })
        .context("milestone title required (--title or plan title)")?;
    let slug = options.slug.unwrap_or_else(|| {
        title
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
    });
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
    {
        bail!("milestone slug must use lowercase letters, digits, and single hyphens");
    }
    let all_paths = milestone_paths(root)?;
    for path in &all_paths {
        if file_id(path)?.as_deref() == Some(&format!("m-{slug}")) {
            bail!("milestone ID already exists: m-{slug}");
        }
    }
    let mut dependencies = options.depends_on;
    if let Some(meta) = &plan_meta {
        if let Some(value) = meta.get("depends_on") {
            let from_plan: Vec<String> = serde_yaml::from_value(value.clone())?;
            dependencies.extend(from_plan);
        }
    }
    dependencies.sort();
    dependencies.dedup();
    let status = if options.active { "active" } else { "planned" };
    let target_dir_rel = target_dir.strip_prefix(root).unwrap_or(&target_dir);
    let inferred_project = crate::core::tasks::infer_project_from_path(target_dir_rel);
    let mut header = format!(
        "---\nid: m-{slug}\ntitle: {}\ndoc_type: contract\nstatus: {status}\n",
        quote(&title)?
    );
    if let Some(proj) = &inferred_project {
        header.push_str(&format!("project: {proj}\n"));
    }
    if options.active {
        header.push_str(&format!("started_at: {}\n", Utc::now().to_rfc3339()));
    }
    if !dependencies.is_empty() {
        header.push_str("depends_on:\n");
        for dep in dependencies {
            header.push_str(&format!("  - {}\n", quote(&dep)?));
        }
    }
    header.push_str("---\n\n");
    let plan_purpose = plan_body
        .lines()
        .skip_while(|line| line.starts_with("# "))
        .take_while(|line| !line.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n");
    let mut description = plan_meta
        .as_ref()
        .and_then(|m| m["purpose"].as_str())
        .unwrap_or(plan_purpose.trim())
        .to_string();
    if let Some(desc) = options.desc {
        if !description.is_empty() {
            description.push('\n');
        }
        description.push_str(&desc);
    }
    let task_start = options.stdin.find("### task:");
    let (stdin_description, stdin_tasks) = task_start.map_or((options.stdin.as_str(), ""), |at| {
        options.stdin.split_at(at)
    });
    if !stdin_description.trim().is_empty() {
        if !description.is_empty() {
            description.push('\n');
        }
        description.push_str(stdin_description);
    }
    let notes = if plan_body.trim().is_empty() {
        String::new()
    } else {
        format!(
            "## Source plan notes\n\n{}\n\n",
            plan_body
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let tasks = if stdin_tasks.is_empty() {
        format!("### task: {slug}\n\n```yaml\ntask_ref: {slug}\ntarget: {}\nproof_policy: seam-test-first\ncontract_revision: 1\nscope:\n  - src/\nstatus: planned\n```\n", quote(&title)?)
    } else {
        stdin_tasks.to_string()
    };
    let body = format!(
        "# {title}\n\n## Outcome and purpose\n\n{}\n\n{}## Tasks in this milestone\n\n{}",
        description.trim(),
        notes,
        tasks
    );
    crate::core::tasks::Milestone::parse(&format!("{header}{body}"), "forge-mcp")?;
    let target_dir_rel = target_dir.strip_prefix(root).unwrap_or(&target_dir);
    let relative = target_dir_rel.join(format!("{number}-{slug}.md"));
    let path = root.join(&relative);
    fs::create_dir_all(path.parent().context("milestone parent")?)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(format!("{header}{body}").as_bytes())?;
    Ok(
        json!({"path": relative, "id": format!("m-{slug}"), "status": status,
        "sync": format!("contextunity-forge-mcp task sync {}", relative.display()),
        "claim": format!("contextunity-forge-mcp task claim <task-id> --stage contract --worker <worker-id> --worktree {}", root.display())}),
    )
}

struct Snapshot {
    path: PathBuf,
    text: String,
    frontmatter: serde_yaml::Value,
    id: String,
    repository: String,
    project: String,
    status: String,
    depends_on: Vec<String>,
    invariants: Vec<String>,
    tasks: Vec<serde_yaml::Value>,
}

fn snapshot(root: &Path, path: PathBuf, repository: &str, project: &str) -> Result<Snapshot> {
    let text = fs::read_to_string(&path)?;
    let status = Milestone::status_from_frontmatter(&text, is_archived_path(&path))?.to_owned();
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

fn is_archived_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "archive")
}

fn snapshots(root: &Path, include_archive: bool, selector: Option<&str>) -> Result<Vec<Snapshot>> {
    let (repository, project) = tasks::project_identity(root)?;
    let mut files = milestone_paths(root)?;
    if !include_archive {
        files.retain(|path| !is_archived_path(path));
    }
    if let Some(selector) = selector {
        let mut selected = Vec::new();
        let mut components = Path::new(selector).components();
        let is_path_reference = !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some();
        if is_path_reference {
            selected.extend(files.into_iter().filter(|path| path.ends_with(selector)));
        } else {
            let numeric = !selector.is_empty() && selector.bytes().all(|b| b.is_ascii_digit());
            let selector_u64 = if numeric {
                selector.parse::<u64>().ok()
            } else {
                None
            };
            for path in files {
                let number = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.split_once('-'))
                    .map(|(number, _)| number);
                let number_u64 = number.and_then(|n| n.parse::<u64>().ok());
                let stem = path.file_stem().and_then(|s| s.to_str());
                let file_name = path.file_name().and_then(|s| s.to_str());
                let id = file_id(&path).with_context(|| {
                    format!("failed to read milestone id from '{}'", path.display())
                })?;

                if (numeric && (number == Some(selector) || number_u64 == selector_u64))
                    || id.as_deref() == Some(selector)
                    || stem == Some(selector)
                    || file_name == Some(selector)
                {
                    selected.push(path);
                }
            }
            let active: Vec<_> = selected
                .iter()
                .filter(|path| !is_archived_path(path))
                .cloned()
                .collect();
            if !active.is_empty() {
                selected = active;
            }
        }
        files = selected;
    }
    files.sort();
    files
        .into_iter()
        .map(|path| snapshot(root, path, &repository, &project))
        .collect()
}

fn task_rows(snapshot: &Snapshot, stored: &[Task]) -> Vec<Value> {
    snapshot.tasks.iter().map(|spec| {
        let task_ref = spec["task_ref"].as_str().unwrap_or("");
        let id = format!("{}/{}/{}:{task_ref}", snapshot.repository, snapshot.project, snapshot.id);
        let status = stored.iter().find(|task| task.task_id == id)
            .map(|task| task.status.as_str()).unwrap_or("unsynced");
        json!({"task_ref":task_ref,"target":spec["target"].as_str().unwrap_or(""),"status":status,
            "proof_policy":spec["proof_policy"].as_str().unwrap_or(""),
            "contract_revision":spec["contract_revision"].as_u64().unwrap_or(1),
            "scope":spec["scope"]})
    }).collect()
}

fn stored_tasks(root: &Path, snapshot: &Snapshot) -> Result<Vec<Task>> {
    let path = tasks::database_path(root)?;
    TasksStore::open_project(&path, &snapshot.repository, &snapshot.project)?
        .list(None, "all", None)
}

fn view(snapshot: &Snapshot, stored: &[Task]) -> Result<Value> {
    let meta = &snapshot.frontmatter;
    let tasks = task_rows(snapshot, stored);
    let completed = tasks
        .iter()
        .filter(|task| task["status"] == "completed")
        .count();
    let outcome = snapshot
        .text
        .split_once("## Outcome and purpose\n")
        .map(|(_, tail)| tail.split("\n## ").next().unwrap_or("").trim())
        .unwrap_or("");
    Ok(json!({"id":snapshot.id,
        "title":meta["title"].as_str().unwrap_or(""),
        "status":snapshot.status,
        "started_at":meta["started_at"].as_str().unwrap_or("-"),
        "completion":format!("{completed}/{}", tasks.len()),
        "tasks":tasks,"frontmatter":serde_json::to_value(meta)?,
        "depends_on":snapshot.depends_on,
        "invariants":snapshot.invariants,"outcome":outcome}))
}

/// Lists milestone metadata and completion ratios from the task store.
pub fn list(root: &Path, archive: bool, status: Option<&str>) -> Result<Value> {
    if status
        .is_some_and(|s| !matches!(s, "all" | "planned" | "active" | "completed" | "cancelled"))
    {
        bail!("invalid milestone status");
    }
    let mut rows = Vec::new();
    for item in snapshots(
        root,
        archive || matches!(status, Some("all" | "completed" | "cancelled")),
        None,
    )? {
        let row = view(&item, &stored_tasks(root, &item)?)?;
        if status.is_some_and(|s| s != "all" && row["status"] != s) {
            continue;
        }
        rows.push(row);
    }
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let mut table = String::from("ID | title | status | started_at | completion\n");
    for row in &rows {
        table.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            row["id"].as_str().unwrap_or(""),
            row["title"].as_str().unwrap_or(""),
            row["status"].as_str().unwrap_or(""),
            row["started_at"].as_str().unwrap_or("-"),
            row["completion"].as_str().unwrap_or("")
        ));
    }
    Ok(json!({"table":table,"milestones":rows}))
}

/// Shows one milestone by ID, prefix, stem, or path.
pub fn show(root: &Path, selector: &str, full: bool) -> Result<Value> {
    let mut found = snapshots(root, true, Some(selector))?;
    if found.len() != 1 {
        bail!("milestone selector must resolve to exactly one document: {selector}");
    }
    let item = found.pop().context("milestone missing")?;
    let mut result = view(&item, &stored_tasks(root, &item)?)?;
    if full {
        result["full_document"] = json!(item.text);
    }
    Ok(result)
}

/// Records a milestone's active start time when its task is first claimed.
pub fn activate_on_claim(worktree: &Path, reference: &str, claimed_at: i64) -> Result<()> {
    let path = crate::core::tasks::confined_path(worktree, reference)?;
    let text = fs::read_to_string(&path)?;
    let status = Milestone::status_from_frontmatter(&text, is_archived_path(Path::new(reference)))?;
    let frontmatter = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))
        .context("milestone requires YAML frontmatter")?;
    let (header, body) = frontmatter
        .split_once("\r\n---\r\n")
        .or_else(|| frontmatter.split_once("\n---\n"))
        .context("unterminated milestone frontmatter")?;
    let mut meta: serde_yaml::Value = serde_yaml::from_str(header)?;
    if meta["started_at"].as_str().is_some() {
        return Ok(());
    }
    if !matches!(status, "planned" | "active") {
        return Ok(());
    }
    let started = Utc
        .timestamp_opt(claimed_at, 0)
        .single()
        .context("invalid claim timestamp")?
        .to_rfc3339();
    let map = meta
        .as_mapping_mut()
        .context("milestone frontmatter must be a mapping")?;
    map.insert("status".into(), "active".into());
    map.insert("started_at".into(), started.into());
    atomic_replace(
        &path,
        format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?).as_bytes(),
    )?;
    Ok(())
}

/// Replaces the YAML block for one task while preserving the surrounding milestone text.
pub(crate) fn render_task_receipt(
    text: &str,
    task_ref: &str,
    receipt: &Receipt,
    subtasks: &[crate::core::tasks::SubtaskSpec],
) -> Result<String> {
    let mut block_start = None;
    let mut cursor = 0;
    for line in text.split_inclusive('\n') {
        if line.trim() == "```yaml" {
            block_start = Some(cursor + line.len());
        } else if line.trim() == "```" {
            if let Some(start) = block_start.take() {
                let mut block: serde_yaml::Value = serde_yaml::from_str(&text[start..cursor])?;
                if block["task_ref"].as_str() == Some(task_ref) {
                    let map = block
                        .as_mapping_mut()
                        .context("task block must be a mapping")?;
                    map.insert("status".into(), "completed".into());
                    map.insert("receipt".into(), serde_yaml::to_value(receipt)?);
                    if !subtasks.is_empty() {
                        map.insert("subtasks".into(), serde_yaml::to_value(subtasks)?);
                    }
                    let yaml = serde_yaml::to_string(&block)?;
                    return Ok(format!("{}{}{}", &text[..start], yaml, &text[cursor..]));
                }
            }
        }
        cursor += line.len();
    }
    bail!("TASK_RECEIPT_INVALID: task block not found")
}

/// Clears completion status and receipt from the task YAML block in the milestone document.
pub(crate) fn clear_task_receipt(text: &str, task_ref: &str) -> Result<String> {
    let mut block_start = None;
    let mut cursor = 0;
    for line in text.split_inclusive('\n') {
        if line.trim() == "```yaml" {
            block_start = Some(cursor + line.len());
        } else if line.trim() == "```" {
            if let Some(start) = block_start.take() {
                let mut block: serde_yaml::Value = serde_yaml::from_str(&text[start..cursor])?;
                if block["task_ref"].as_str() == Some(task_ref) {
                    let map = block
                        .as_mapping_mut()
                        .context("task block must be a mapping")?;
                    map.remove("status");
                    map.remove("receipt");
                    let yaml = serde_yaml::to_string(&block)?;
                    return Ok(format!("{}{}{}", &text[..start], yaml, &text[cursor..]));
                }
            }
        }
        cursor += line.len();
    }
    bail!("TASK_RECEIPT_INVALID: task block not found")
}

pub(crate) fn atomic_replace(path: &Path, contents: &[u8]) -> Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .context("milestone filename missing")?
        .to_string_lossy();
    let temp = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temp, path));
    drop(file);
    if let Err(error) = result {
        fs::remove_file(&temp)?;
        return Err(error.into());
    }
    Ok(())
}

/// Verifies completion, writes a handoff receipt, and archives the milestone.
pub fn handoff(
    root: &Path,
    selector: &str,
    commit: Option<&str>,
    command: &str,
    passed: u64,
    failed: u64,
) -> Result<Value> {
    if command.trim().is_empty() || failed != 0 {
        bail!("handoff requires a passed verification command and zero failed tests");
    }
    let mut found = snapshots(root, false, Some(selector))?;
    if found.len() != 1 {
        bail!("milestone selector must resolve to exactly one active document: {selector}");
    }
    let item = found.pop().context("milestone missing")?;
    if !matches!(item.status.as_str(), "planned" | "active") {
        bail!("milestone is not ready for handoff");
    }
    let db = tasks::database_path(root)?;
    let mut store = TasksStore::open_project(&db, &item.repository, &item.project)?;
    let stored = store.list(None, "all", None)?;
    let prefix = format!("{}/{}/{}:", item.repository, item.project, item.id);
    let relevant: Vec<_> = stored
        .iter()
        .filter(|task| task.task_id.starts_with(&prefix))
        .collect();
    let mut incomplete = Vec::new();
    for spec in &item.tasks {
        let task_ref = spec["task_ref"].as_str().context("task_ref missing")?;
        let id = format!("{prefix}{task_ref}");
        if !relevant
            .iter()
            .any(|task| task.task_id == id && task.status == "completed")
        {
            incomplete.push(task_ref.to_string());
        }
    }
    for task in &relevant {
        if task.status != "completed"
            && !incomplete
                .iter()
                .any(|name| task.task_id.ends_with(&format!(":{name}")))
        {
            incomplete.push(task.task_id.clone());
        }
    }
    if !incomplete.is_empty() {
        bail!("milestone has incomplete tasks: {}", incomplete.join(", "));
    }
    if relevant.is_empty() {
        bail!("milestone tasks are not synced to SQLite");
    }
    let expected_ids: Vec<String> = relevant.iter().map(|task| task.task_id.clone()).collect();
    let sha = match commit {
        Some(value) => value.to_string(),
        None => {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["rev-parse", "HEAD"])
                .output()?;
            if !output.status.success() {
                bail!("unable to resolve milestone commit");
            }
            String::from_utf8(output.stdout)?.trim().to_string()
        }
    };
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("handoff commit must be a full Git SHA");
    }
    let started = match item.frontmatter["started_at"].as_str() {
        Some(value) => chrono::DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc),
        None => Utc
            .timestamp_opt(
                store
                    .earliest_claim(&prefix)?
                    .context("milestone activation time missing")?,
                0,
            )
            .single()
            .context("invalid claim timestamp")?,
    };
    let completed = Utc::now();
    if completed < started {
        bail!("milestone started_at is in the future");
    }
    let minutes = (completed - started).num_minutes();
    let mut meta = item.frontmatter;
    let map = meta
        .as_mapping_mut()
        .context("milestone frontmatter must be a mapping")?;
    map.insert("status".into(), "completed".into());
    map.insert("started_at".into(), started.to_rfc3339().into());
    map.insert("handoff".into(), serde_yaml::to_value(json!({
        "completed_at": completed.to_rfc3339(), "duration": format!("{}h {}m", minutes / 60, minutes % 60),
        "commit": sha, "verification": {"command": command, "status": "passed", "tests_passed": passed, "tests_failed": failed}
    }))?);
    let relative = item.path.strip_prefix(root)?.to_string_lossy().into_owned();
    let parent = item.path.parent().context("milestone parent missing")?;
    let archive_dir = parent.join("archive");
    let archived = archive_dir.join(
        item.path
            .file_name()
            .context("milestone filename missing")?,
    );
    let archive_ref = archived.strip_prefix(root)?.to_string_lossy().into_owned();
    fs::create_dir_all(&archive_dir)?;
    let (_, body) = item
        .text
        .strip_prefix("---\n")
        .context("milestone frontmatter missing")?
        .split_once("\n---\n")
        .context("unterminated milestone frontmatter")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archived)?;
    let archive_text = format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?);
    if let Err(error) = file.write_all(archive_text.as_bytes()) {
        drop(file);
        fs::remove_file(&archived)?;
        return Err(error.into());
    }
    drop(file);
    if let Err(error) = store.relocate_milestone(&prefix, &relative, &archive_ref, &expected_ids) {
        fs::remove_file(&archived)?;
        return Err(error);
    }
    if let Err(error) = fs::remove_file(&item.path) {
        store.relocate_milestone(&prefix, &archive_ref, &relative, &expected_ids)?;
        fs::remove_file(&archived)?;
        return Err(error.into());
    }
    Ok(
        json!({"id": item.id, "status": "completed", "path": archive_ref, "handoff": meta["handoff"]}),
    )
}
