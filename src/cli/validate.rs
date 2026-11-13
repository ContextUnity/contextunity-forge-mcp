//! Fail-fast validation for ACDD profiles and milestone manifests.

use crate::{
    core::tasks::{confined_path, profile, Milestone, TaskSpec},
    engine::{milestones, tasks::TaskSettings},
};
use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::Path,
};

#[derive(Debug, Serialize)]
struct Diagnostic {
    status: &'static str,
    code: String,
    path: String,
    line: usize,
    column: usize,
    message: String,
}

struct ParsedManifest {
    path: String,
    text: String,
    milestone: Milestone,
}

struct ValidationTask {
    milestone_id: String,
    task_ref: String,
    repository: String,
    project: String,
    path: String,
    line: usize,
    scopes: Vec<String>,
    dependencies: Vec<String>,
}

/// Validate one document, or the active profile and every open workspace manifest.
pub fn run(root: &Path, relative: Option<&Path>) -> Result<Value> {
    let mut diagnostics = Vec::new();
    let mut warnings = Vec::new();

    if let Some(relative) = relative {
        let Some(relative_text) = relative.to_str() else {
            diagnostics.push(fail(
                "VALIDATION_PATH_INVALID",
                "<path>",
                1,
                1,
                "validation path must be valid UTF-8",
            ));
            return Ok(report("document", diagnostics, warnings, 1));
        };
        let path = match confined_path(root, relative_text) {
            Ok(path) => path,
            Err(error) => {
                diagnostics.push(fail_from_error(relative_text, &error, 1, 1));
                return Ok(report("document", diagnostics, warnings, 1));
            }
        };
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                diagnostics.push(fail(
                    "VALIDATION_IO",
                    relative_text,
                    1,
                    1,
                    format!("unable to read validation input: {error}"),
                ));
                return Ok(report("document", diagnostics, warnings, 1));
            }
        };
        match path_kind(&path) {
            Some(DocumentKind::Profile) => {
                validate_profile(relative_text, &content, &mut diagnostics);
            }
            Some(DocumentKind::Milestone) => {
                let settings = match TaskSettings::load(root) {
                    Ok(settings) => settings,
                    Err(error) => {
                        diagnostics.push(fail_from_error("forge-mcp.yaml", &error, 1, 1));
                        return Ok(report("document", diagnostics, warnings, 1));
                    }
                };
                let active = match profile::load_for_workspace(root) {
                    Ok(profile) => Some(profile),
                    Err(error) => {
                        diagnostics.push(fail_from_error(&active_profile_path(root), &error, 1, 1));
                        None
                    }
                };
                if let Some(manifest) = validate_manifest(
                    relative_text,
                    &content,
                    &settings,
                    active.as_deref(),
                    &mut diagnostics,
                    &mut warnings,
                ) {
                    validate_task_graph(&[manifest], &mut diagnostics);
                }
            }
            None => diagnostics.push(fail(
                "VALIDATION_KIND_UNSUPPORTED",
                relative_text,
                1,
                1,
                "expected a .md, .yaml, .yml, .yaml.example, or .yml.example document",
            )),
        }
        return Ok(report("document", diagnostics, warnings, 1));
    }

    let active = match profile::load_for_workspace(root) {
        Ok(active) => {
            diagnostics.push(pass(
                &active_profile_path(root),
                "PROFILE_VALID",
                "active ACDD profile and its gate catalogs are valid",
            ));
            Some(active)
        }
        Err(error) => {
            diagnostics.push(fail_from_error(&active_profile_path(root), &error, 1, 1));
            None
        }
    };

    let settings = match TaskSettings::load(root) {
        Ok(settings) => settings,
        Err(error) => {
            diagnostics.push(fail_from_error("forge-mcp.yaml", &error, 1, 1));
            return Ok(report("workspace", diagnostics, warnings, 0));
        }
    };

    let paths = open_manifest_paths(root, &mut diagnostics);
    let mut parsed = Vec::new();
    for (relative_path, content) in paths {
        if let Some(manifest) = validate_manifest(
            &relative_path,
            &content,
            &settings,
            active.as_deref(),
            &mut diagnostics,
            &mut warnings,
        ) {
            parsed.push(manifest);
        }
    }
    validate_task_graph(&parsed, &mut diagnostics);
    Ok(report("workspace", diagnostics, warnings, parsed.len()))
}

#[derive(Clone, Copy)]
enum DocumentKind {
    Milestone,
    Profile,
}

fn path_kind(path: &Path) -> Option<DocumentKind> {
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".yaml.example") || name.ends_with(".yml.example") {
        return Some(DocumentKind::Profile);
    }
    match path.extension()?.to_str()? {
        "md" => Some(DocumentKind::Milestone),
        "yaml" | "yml" => Some(DocumentKind::Profile),
        _ => None,
    }
}

fn validate_profile(path: &str, content: &str, diagnostics: &mut Vec<Diagnostic>) {
    match profile::validate_profile_document(content) {
        Ok(validated) => diagnostics.push(pass(
            path,
            "PROFILE_VALID",
            format!(
                "validated {} root gates and {} registered commands",
                validated.gates.len(),
                validated.commands.len()
            ),
        )),
        Err(error) => diagnostics.push(fail_from_error(path, &error, 1, 1)),
    }
}

fn validate_manifest(
    path: &str,
    content: &str,
    settings: &TaskSettings,
    active_profile: Option<&profile::GateProfile>,
    diagnostics: &mut Vec<Diagnostic>,
    warnings: &mut Vec<Value>,
) -> Option<ParsedManifest> {
    let relative = Path::new(path);
    let default_project = settings
        .task_project
        .as_deref()
        .unwrap_or(&settings.task_repository);
    let project = match settings.resolve_project(relative, None, default_project) {
        Ok(project) => project,
        Err(error) => {
            diagnostics.push(fail_from_error(path, &error, 1, 1));
            return None;
        }
    };
    let mut milestone =
        match Milestone::parse_with_identity(content, &settings.task_repository, &project) {
            Ok(milestone) => milestone,
            Err(error) => {
                let line = if error_code(&error).as_deref() == Some("MILESTONE_DAG_CYCLE") {
                    first_task_line(content)
                } else {
                    1
                };
                diagnostics.push(fail_from_error(path, &error, line, 1));
                return None;
            }
        };
    milestone.project =
        match settings.resolve_project(relative, Some(&milestone.project), default_project) {
            Ok(project) => project,
            Err(error) => {
                diagnostics.push(fail_from_error(path, &error, 1, 1));
                return None;
            }
        };
    if let Some(active_profile) = active_profile {
        match active_profile.validate_guidance_bindings(&milestone.guidance) {
            Ok(manifest_warnings) => {
                for warning in manifest_warnings {
                    warnings.push(json!({"path":path,"message":warning}));
                }
            }
            Err(error) => diagnostics.push(fail_from_error(path, &error, 1, 1)),
        }
    }
    diagnostics.push(pass(
        path,
        "MILESTONE_VALID",
        format!(
            "validated milestone '{}' with {} tasks",
            milestone.id,
            milestone.tasks.len()
        ),
    ));
    Some(ParsedManifest {
        path: path.to_owned(),
        text: content.to_owned(),
        milestone,
    })
}

fn open_manifest_paths(root: &Path, diagnostics: &mut Vec<Diagnostic>) -> Vec<(String, String)> {
    let planned = milestones::list(root, false, Some("planned"), 100);
    let active = milestones::list(root, false, Some("active"), 100);
    if let (Ok(planned), Ok(active)) = (planned, active) {
        let mut paths = BTreeSet::new();
        for listing in [&planned, &active] {
            if let Some(documents) = listing.get("documents").and_then(Value::as_array) {
                for document in documents {
                    if let Some(path) = document.get("path").and_then(Value::as_str) {
                        paths.insert(path.to_owned());
                    }
                }
            }
        }
        if !paths.is_empty() || configured_dirs_empty(root) {
            return paths
                .into_iter()
                .filter_map(|path| match fs::read_to_string(root.join(&path)) {
                    Ok(text) => Some((path, text)),
                    Err(error) => {
                        diagnostics.push(fail(
                            "VALIDATION_IO",
                            &path,
                            1,
                            1,
                            format!("unable to read milestone manifest: {error}"),
                        ));
                        None
                    }
                })
                .collect();
        }
    }
    scan_open_manifests(root, diagnostics)
}

fn configured_dirs_empty(root: &Path) -> bool {
    crate::core::tasks::configured_milestone_dirs(root)
        .iter()
        .all(|directory| !directory.exists())
}

fn scan_open_manifests(root: &Path, diagnostics: &mut Vec<Diagnostic>) -> Vec<(String, String)> {
    fn walk(
        root: &Path,
        directory: &Path,
        found: &mut BTreeMap<String, String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let Ok(entries) = fs::read_dir(directory) else {
            diagnostics.push(fail(
                "MILESTONE_SCAN_FAILED",
                directory
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::to_str)
                    .unwrap_or("docs/milestones"),
                1,
                1,
                "unable to enumerate configured milestone directory",
            ));
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if entry.file_name() != "archive" {
                    walk(root, &path, found, diagnostics);
                }
                continue;
            }
            if !file_type.is_file()
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(milestones::is_milestone_manifest_name)
            {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) => {
                    diagnostics.push(fail(
                        "VALIDATION_IO",
                        relative,
                        1,
                        1,
                        format!("unable to read milestone manifest: {error}"),
                    ));
                    continue;
                }
            };
            let open = frontmatter_status(&content).is_none_or(|status| {
                matches!(status.as_str(), "planned" | "active" | "in_progress")
            });
            if open {
                found.insert(relative.to_owned(), content);
            }
        }
    }

    let mut found = BTreeMap::new();
    for directory in milestones::configured_milestone_dirs(root) {
        if directory.is_dir() {
            walk(root, &directory, &mut found, diagnostics);
        }
    }
    found.into_iter().collect()
}

fn frontmatter_status(content: &str) -> Option<String> {
    let header = content.strip_prefix("---\n")?.split_once("\n---\n")?.0;
    let value: serde_yaml::Value = serde_yaml::from_str(header).ok()?;
    value
        .get("status")
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_owned)
}

fn validate_task_graph(manifests: &[ParsedManifest], diagnostics: &mut Vec<Diagnostic>) {
    let mut tasks = Vec::new();
    let mut open_milestones = BTreeSet::<(String, String)>::new();
    for manifest in manifests {
        let milestone = &manifest.milestone;
        let key = (milestone.project.clone(), milestone.id.clone());
        if !open_milestones.insert(key) {
            diagnostics.push(fail(
                "MILESTONE_ID_DUPLICATE",
                &manifest.path,
                1,
                1,
                format!(
                    "milestone '{}' is declared more than once for project '{}'",
                    milestone.id, milestone.project
                ),
            ));
        }
        for task in &milestone.tasks {
            let mut scopes = task
                .scope
                .iter()
                .chain(&task.extended_scope)
                .cloned()
                .collect::<Vec<_>>();
            scopes.sort();
            scopes.dedup();
            tasks.push(ValidationTask {
                milestone_id: milestone.id.clone(),
                task_ref: task.task_ref.clone(),
                repository: milestone.repository.clone(),
                project: milestone.project.clone(),
                path: manifest.path.clone(),
                line: task_line(&manifest.text, task),
                scopes,
                dependencies: task.depends_on.clone(),
            });
        }
    }

    let mut local_indices = BTreeMap::<(String, String, String), usize>::new();
    for (index, task) in tasks.iter().enumerate() {
        local_indices.insert(
            (
                task.project.clone(),
                task.milestone_id.clone(),
                task.task_ref.clone(),
            ),
            index,
        );
    }

    let mut dependencies = vec![Vec::<usize>::new(); tasks.len()];
    let mut unresolved = false;
    for (index, task) in tasks.iter().enumerate() {
        for dependency in &task.dependencies {
            let Some((prefix, task_ref)) = dependency.split_once(':') else {
                let key = (
                    task.project.clone(),
                    task.milestone_id.clone(),
                    dependency.clone(),
                );
                if let Some(target) = local_indices.get(&key) {
                    dependencies[index].push(*target);
                } else {
                    unresolved = true;
                    diagnostics.push(fail(
                        "MILESTONE_DEPENDENCY_UNKNOWN",
                        &task.path,
                        task.line,
                        1,
                        format!("local dependency '{dependency}' does not name a task in this milestone"),
                    ));
                }
                continue;
            };
            let target_milestone = prefix.rsplit('/').next().unwrap_or(prefix);
            let declared_project = prefix.rsplit_once('/').map(|(project, _)| project);
            let candidates = local_indices
                .iter()
                .filter(|((project, milestone_id, ref_name), _)| {
                    milestone_id == target_milestone
                        && ref_name == task_ref
                        && declared_project.is_none_or(|declared| {
                            project == declared || declared.ends_with(&format!("/{project}"))
                        })
                })
                .map(|(_, index)| *index)
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [target] => dependencies[index].push(*target),
                [] => {
                    let open_target = open_milestones.iter().any(|(project, milestone_id)| {
                        milestone_id == target_milestone
                            && declared_project.is_none_or(|declared| {
                                project == declared || declared.ends_with(&format!("/{project}"))
                            })
                    });
                    if open_target || target_milestone == task.milestone_id {
                        unresolved = true;
                        diagnostics.push(fail(
                            "MILESTONE_DEPENDENCY_UNKNOWN",
                            &task.path,
                            task.line,
                            1,
                            format!("dependency '{dependency}' does not resolve to a task in an open milestone"),
                        ));
                    }
                }
                _ => {
                    unresolved = true;
                    diagnostics.push(fail(
                        "MILESTONE_DEPENDENCY_AMBIGUOUS",
                        &task.path,
                        task.line,
                        1,
                        format!("dependency '{dependency}' matches more than one open task"),
                    ));
                }
            }
        }
        dependencies[index].sort_unstable();
        dependencies[index].dedup();
    }

    let mut remaining = dependencies.iter().map(Vec::len).collect::<Vec<_>>();
    let mut dependents = vec![Vec::<usize>::new(); tasks.len()];
    for (task_index, prerequisites) in dependencies.iter().enumerate() {
        for prerequisite in prerequisites {
            dependents[*prerequisite].push(task_index);
        }
    }
    let mut queue = (0..tasks.len())
        .filter(|index| remaining[*index] == 0)
        .collect::<VecDeque<_>>();
    let mut waves = vec![0usize; tasks.len()];
    let mut visited = 0usize;
    while let Some(index) = queue.pop_front() {
        visited += 1;
        for dependent in &dependents[index] {
            waves[*dependent] = waves[*dependent].max(waves[index] + 1);
            remaining[*dependent] -= 1;
            if remaining[*dependent] == 0 {
                queue.push_back(*dependent);
            }
        }
    }
    if visited != tasks.len() {
        let index = remaining.iter().position(|count| *count > 0).unwrap_or(0);
        if let Some(task) = tasks.get(index) {
            diagnostics.push(fail(
                "MILESTONE_DAG_CYCLE",
                &task.path,
                task.line,
                1,
                "open milestone task dependencies contain a cycle",
            ));
        }
        return;
    }
    if unresolved {
        return;
    }

    for left in 0..tasks.len() {
        for right in left + 1..tasks.len() {
            if waves[left] != waves[right]
                || tasks[left].repository != tasks[right].repository
                || tasks[left].project != tasks[right].project
            {
                continue;
            }
            let overlap = tasks[left].scopes.iter().find_map(|left_scope| {
                tasks[right]
                    .scopes
                    .iter()
                    .find(|right_scope| scope_paths_overlap(left_scope, right_scope))
                    .map(|right_scope| (left_scope, right_scope))
            });
            if let Some((left_scope, right_scope)) = overlap {
                diagnostics.push(fail(
                    "MILESTONE_SCOPE_OVERLAP",
                    &tasks[right].path,
                    tasks[right].line,
                    1,
                    format!(
                        "same-wave tasks '{}' and '{}' overlap at '{}' and '{}'",
                        tasks[left].task_ref, tasks[right].task_ref, left_scope, right_scope
                    ),
                ));
            }
        }
    }
}

fn scope_paths_overlap(left: &str, right: &str) -> bool {
    let left = Path::new(left);
    let right = Path::new(right);
    left.starts_with(right) || right.starts_with(left)
}

fn task_line(content: &str, task: &TaskSpec) -> usize {
    let marker = format!("task_ref: {}", task.task_ref);
    content
        .lines()
        .position(|line| line.trim_start().starts_with(&marker))
        .map_or_else(|| first_task_line(content), |line| line + 1)
}

fn first_task_line(content: &str) -> usize {
    content
        .lines()
        .position(|line| line.trim_start().starts_with("task_ref:"))
        .map_or(1, |line| line + 1)
}

fn active_profile_path(root: &Path) -> String {
    let settings = forge_tasks::tasks::configuration::current(root);
    if let Some(path) = settings
        .as_ref()
        .and_then(|settings| settings.profile_argument.as_ref())
    {
        return path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
    }
    let config = forge_tasks::tasks::configuration::config_path(root);
    if let Ok(content) = fs::read_to_string(&config) {
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            if let Some(path) = value
                .get("acdd_profile")
                .and_then(serde_yaml::Value::as_str)
            {
                return path.to_owned();
            }
        }
    }
    if root.join(".forge/acdd/profile.yaml").is_file() {
        ".forge/acdd/profile.yaml".to_owned()
    } else if root
        .join("crates/forge-tasks/src/tasks/acdd.default.yaml")
        .is_file()
    {
        "crates/forge-tasks/src/tasks/acdd.default.yaml".to_owned()
    } else {
        "<embedded:acdd.default.yaml>".to_owned()
    }
}

fn fail_from_error(path: &str, error: &anyhow::Error, line: usize, column: usize) -> Diagnostic {
    let code = error_code(error).unwrap_or_else(|| "VALIDATION_FAILED".to_owned());
    fail(code, path, line, column, format!("{error:#}"))
}

fn error_code(error: &anyhow::Error) -> Option<String> {
    for cause in error.chain() {
        let message = cause.to_string();
        for token in message.split_whitespace() {
            let token = token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_'
            });
            if token.contains('_')
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Some(token.to_owned());
            }
        }
    }
    None
}

fn pass(path: &str, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        status: "PASS",
        code: code.to_owned(),
        path: path.to_owned(),
        line: 1,
        column: 1,
        message: message.into(),
    }
}

fn fail(
    code: impl Into<String>,
    path: &str,
    line: usize,
    column: usize,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        status: "FAIL",
        code: code.into(),
        path: path.to_owned(),
        line: line.max(1),
        column: column.max(1),
        message: message.into(),
    }
}

fn report(
    scope: &str,
    diagnostics: Vec<Diagnostic>,
    warnings: Vec<Value>,
    documents: usize,
) -> Value {
    let valid = diagnostics
        .iter()
        .all(|diagnostic| diagnostic.status == "PASS");
    json!({
        "valid": valid,
        "scope": scope,
        "documents": documents,
        "diagnostics": diagnostics,
        "warnings": warnings,
    })
}
