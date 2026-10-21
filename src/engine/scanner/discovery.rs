//! Bounded manifest-backed workspace component discovery.

use std::{
    collections::{btree_map::Entry, BTreeMap},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::core::response::QueryOptions;

use super::{dependency_manifests_for_build, Adapter};

struct ManifestLocation {
    workspace: String,
    base: PathBuf,
    relative_path: PathBuf,
    root: String,
    manifest_path: String,
}

struct Component {
    workspace: String,
    name: Option<String>,
    ecosystem: String,
    unit: String,
    root: String,
    manifest_path: String,
    description: Option<String>,
    workspace_members: Option<Vec<String>>,
}

impl Component {
    fn into_value(self) -> Value {
        let mut row = serde_json::Map::new();
        row.insert("workspace".into(), Value::String(self.workspace));
        row.insert("name".into(), self.name.map_or(Value::Null, Value::String));
        row.insert("ecosystem".into(), Value::String(self.ecosystem));
        row.insert("unit".into(), Value::String(self.unit));
        row.insert("root".into(), Value::String(self.root));
        row.insert("manifest_path".into(), Value::String(self.manifest_path));
        if let Some(description) = self.description {
            row.insert("description".into(), Value::String(description));
        }
        if let Some(workspace_members) = self.workspace_members {
            row.insert("workspace_members".into(), json!(workspace_members));
        }
        Value::Object(row)
    }
}

/// Discovers manifest-backed components from the scanner's admitted manifest walk.
pub(crate) fn manifest_components(
    root: &Path,
    adapter: &Adapter,
    page: &QueryOptions,
    generation: &str,
) -> Result<Value> {
    anyhow::ensure!((1..=100).contains(&page.limit), "limit must be 1..=100");

    let manifests = match dependency_manifests_for_build(root, adapter) {
        Ok(manifests) => manifests,
        Err(_) => return Ok(unavailable(page, generation, "manifest_walk_failed")),
    };
    if manifests.len() > adapter.limits.max_files {
        return Ok(unavailable(page, generation, "manifest_file_count_limit"));
    }

    let mut components = BTreeMap::<(String, String, String), Component>::new();
    let mut diagnostics = Vec::new();
    let mut diagnostic_total = 0usize;
    let mut parsed_bytes = 0u64;

    for manifest in manifests {
        if !is_supported_manifest(&manifest) {
            continue;
        }
        let location = match manifest_location(root, adapter, &manifest) {
            Ok(location) => location,
            Err(_) => return Ok(unavailable(page, generation, "manifest_path_unavailable")),
        };
        let checked_path = match super::checked_child(&location.base, &location.relative_path) {
            Ok(path) => path,
            Err(_) => return Ok(unavailable(page, generation, "manifest_path_unavailable")),
        };
        let metadata = match fs::symlink_metadata(&checked_path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            _ => return Ok(unavailable(page, generation, "manifest_read_unavailable")),
        };
        if metadata.len() > adapter.limits.max_file_bytes {
            return Ok(unavailable(page, generation, "manifest_file_size_limit"));
        }
        parsed_bytes = match parsed_bytes.checked_add(metadata.len()) {
            Some(total) if total <= adapter.limits.max_total_bytes => total,
            _ => return Ok(unavailable(page, generation, "manifest_total_bytes_limit")),
        };
        let bytes = match fs::read(&checked_path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(unavailable(page, generation, "manifest_read_unavailable")),
        };
        let content = match String::from_utf8(bytes) {
            Ok(content) => content,
            Err(_) => {
                add_diagnostic(
                    &mut diagnostics,
                    &mut diagnostic_total,
                    page.limit,
                    &location,
                    "invalid_utf8",
                    "manifest content is not valid UTF-8",
                )?;
                continue;
            }
        };

        match manifest.file_name().and_then(|name| name.to_str()) {
            Some("Cargo.toml") => parse_cargo(
                &content,
                &location,
                &mut components,
                &mut diagnostics,
                &mut diagnostic_total,
                page.limit,
            )?,
            Some("pyproject.toml") => parse_python(
                &content,
                &location,
                &mut components,
                &mut diagnostics,
                &mut diagnostic_total,
                page.limit,
            )?,
            Some("package.json") => parse_npm(
                &content,
                &location,
                &mut components,
                &mut diagnostics,
                &mut diagnostic_total,
                page.limit,
            )?,
            Some("pnpm-workspace.yaml") => parse_pnpm_workspace(
                &content,
                &location,
                &mut components,
                &mut diagnostics,
                &mut diagnostic_total,
                page.limit,
            )?,
            Some("go.mod") => parse_go(
                &content,
                &location,
                &mut components,
                &mut diagnostics,
                &mut diagnostic_total,
                page.limit,
            )?,
            _ => {}
        }
    }

    let total = components.len();
    let start = page.offset.min(total);
    let end = start.saturating_add(page.limit).min(total);
    let has_more = end < total;
    let items = components
        .into_values()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(Component::into_value)
        .collect::<Vec<_>>();

    Ok(json!({
        "availability": "available",
        "total": total,
        "offset": page.offset,
        "limit": page.limit,
        "has_more": has_more,
        "next_offset": has_more.then_some(end),
        "generation": generation,
        "items": items,
        "diagnostics": diagnostics,
        "diagnostic_total": diagnostic_total,
        "diagnostics_truncated": diagnostic_total > page.limit,
        "diagnostics_available": true,
    }))
}

fn unavailable(page: &QueryOptions, generation: &str, cause: &str) -> Value {
    json!({
        "availability": "unavailable",
        "cause": cause,
        "offset": page.offset,
        "limit": page.limit,
        "generation": generation,
        "items": [],
        "diagnostics": [],
        "diagnostics_available": false,
        "diagnostics_cause": cause,
    })
}

fn is_supported_manifest(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Cargo.toml" | "pyproject.toml" | "package.json" | "pnpm-workspace.yaml" | "go.mod")
    )
}

fn manifest_location(root: &Path, adapter: &Adapter, path: &Path) -> Result<ManifestLocation> {
    let linked = adapter
        .linked_workspaces
        .iter()
        .filter(|workspace| path.starts_with(&workspace.path))
        .max_by_key(|workspace| workspace.path.components().count());
    let (workspace, base) = match linked {
        Some(workspace) => (workspace.name.as_str(), workspace.path.as_path()),
        None if path.starts_with(root) => ("primary", root),
        None => anyhow::bail!("manifest is outside the selected workspace"),
    };
    let relative_path = path
        .strip_prefix(base)
        .context("manifest is outside its workspace root")?;
    anyhow::ensure!(super::safe_relative(relative_path), "unsafe manifest path");
    let manifest_path = relative_path
        .to_str()
        .context("manifest path is not UTF-8")?;
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let root_path = parent.to_str().context("component root is not UTF-8")?;
    let root = if root_path.is_empty() { "." } else { root_path };

    Ok(ManifestLocation {
        workspace: workspace.to_owned(),
        base: base.to_path_buf(),
        relative_path: relative_path.to_path_buf(),
        root: root.replace('\\', "/"),
        manifest_path: manifest_path.replace('\\', "/"),
    })
}

fn add_diagnostic(
    diagnostics: &mut Vec<Value>,
    total: &mut usize,
    limit: usize,
    location: &ManifestLocation,
    code: &str,
    reason: &str,
) -> Result<()> {
    *total = total
        .checked_add(1)
        .context("manifest diagnostic count overflow")?;
    if diagnostics.len() < limit {
        diagnostics.push(json!({
            "workspace": location.workspace,
            "manifest_path": location.manifest_path,
            "code": code,
            "reason": reason,
        }));
    }
    Ok(())
}

fn insert_component(
    components: &mut BTreeMap<(String, String, String), Component>,
    mut next: Component,
) {
    let key = (
        next.workspace.clone(),
        next.root.clone(),
        next.ecosystem.clone(),
    );
    match components.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(next);
        }
        Entry::Occupied(mut entry) => {
            let current = entry.get_mut();
            if current.name.is_none() && next.name.is_some() {
                let workspace_members = current.workspace_members.take();
                *current = next;
                if let Some(members) = workspace_members {
                    append_workspace_members(&mut current.workspace_members, members);
                }
            } else {
                if current.description.is_none() {
                    current.description = next.description.take();
                }
                if current.workspace_members.is_none() && next.workspace_members.is_some() {
                    current.workspace_members = next.workspace_members.take();
                } else if let Some(members) = next.workspace_members.take() {
                    append_workspace_members(&mut current.workspace_members, members);
                }
            }
        }
    }
}

fn append_workspace_members(target: &mut Option<Vec<String>>, members: Vec<String>) {
    let target = target.get_or_insert_with(Vec::new);
    for member in members {
        if !target.contains(&member) {
            target.push(member);
        }
    }
}

fn parse_cargo(
    content: &str,
    location: &ManifestLocation,
    components: &mut BTreeMap<(String, String, String), Component>,
    diagnostics: &mut Vec<Value>,
    diagnostic_total: &mut usize,
    diagnostic_limit: usize,
) -> Result<()> {
    let document = match toml::from_str::<toml::Value>(content) {
        Ok(document) => document,
        Err(_) => {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "malformed_manifest",
                "Cargo manifest is not valid TOML",
            )?;
            return Ok(());
        }
    };
    let workspace = document.get("workspace");
    if workspace.is_some() && workspace.is_some_and(|table| table.as_table().is_none()) {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_workspace_shape",
            "Cargo workspace declaration must be a table",
        )?;
    }
    let workspace_members = workspace
        .and_then(|table| table.get("members"))
        .and_then(|members| members.as_array())
        .map(|values| {
            let mut members = Vec::new();
            for value in values {
                if let Some(member) = value.as_str() {
                    members.push(member.to_owned());
                }
            }
            members
        });
    if workspace.is_some()
        && workspace
            .and_then(|table| table.get("members"))
            .is_some_and(|members| {
                members.as_array().is_none()
                    || members
                        .as_array()
                        .is_some_and(|values| values.iter().any(|value| value.as_str().is_none()))
            })
    {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_workspace_members",
            "Cargo workspace members must be an array of paths or patterns",
        )?;
    }

    let package = document.get("package");
    let name = package
        .and_then(|table| table.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty());
    if package.is_some() && name.is_none() {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "missing_package_name",
            "Cargo package manifest does not declare a usable package name",
        )?;
    }
    let description = package
        .and_then(|table| table.get("description"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    if package
        .and_then(|table| table.get("description"))
        .is_some_and(|value| value.as_str().is_none())
    {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_description",
            "Cargo package description must be a string",
        )?;
    }

    if let Some(name) = name {
        insert_component(
            components,
            Component {
                workspace: location.workspace.clone(),
                name: Some(name.to_owned()),
                ecosystem: "rust".into(),
                unit: "package".into(),
                root: location.root.clone(),
                manifest_path: location.manifest_path.clone(),
                description,
                workspace_members,
            },
        );
    } else if workspace.is_some() {
        insert_component(
            components,
            Component {
                workspace: location.workspace.clone(),
                name: None,
                ecosystem: "rust".into(),
                unit: "workspace".into(),
                root: location.root.clone(),
                manifest_path: location.manifest_path.clone(),
                description: None,
                workspace_members,
            },
        );
    } else if package.is_none() {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "unsupported_manifest_shape",
            "Cargo manifest declares neither a package nor a workspace",
        )?;
    }
    Ok(())
}

fn parse_python(
    content: &str,
    location: &ManifestLocation,
    components: &mut BTreeMap<(String, String, String), Component>,
    diagnostics: &mut Vec<Value>,
    diagnostic_total: &mut usize,
    diagnostic_limit: usize,
) -> Result<()> {
    let document = match toml::from_str::<toml::Value>(content) {
        Ok(document) => document,
        Err(_) => {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "malformed_manifest",
                "Python project manifest is not valid TOML",
            )?;
            return Ok(());
        }
    };
    let project = document.get("project");
    let poetry = document.get("tool").and_then(|tool| tool.get("poetry"));
    if project.is_none() && poetry.is_none() {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "unsupported_manifest_shape",
            "pyproject.toml declares neither a PEP 621 project nor a Poetry package",
        )?;
        return Ok(());
    }
    let invalid_name = [project, poetry].into_iter().flatten().any(|table| {
        table
            .get("name")
            .is_some_and(|name| name.as_str().is_none_or(|name| name.trim().is_empty()))
    });
    if invalid_name {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_package_name",
            "Python project name must be a non-empty string",
        )?;
    }
    let name = project
        .and_then(|table| table.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            poetry
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str)
                .filter(|name| !name.trim().is_empty())
        });
    if name.is_none() && !invalid_name {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "missing_package_name",
            "Python project manifest does not declare a usable project name",
        )?;
    }
    let description = project
        .and_then(|table| table.get("description"))
        .and_then(toml::Value::as_str)
        .or_else(|| {
            poetry
                .and_then(|table| table.get("description"))
                .and_then(toml::Value::as_str)
        })
        .map(str::to_owned);
    if [project, poetry].into_iter().flatten().any(|table| {
        table
            .get("description")
            .is_some_and(|description| description.as_str().is_none())
    }) {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_description",
            "Python project description must be a string",
        )?;
    }
    if let Some(name) = name {
        insert_component(
            components,
            Component {
                workspace: location.workspace.clone(),
                name: Some(name.to_owned()),
                ecosystem: "python".into(),
                unit: "package".into(),
                root: location.root.clone(),
                manifest_path: location.manifest_path.clone(),
                description,
                workspace_members: None,
            },
        );
    }
    Ok(())
}

fn parse_npm(
    content: &str,
    location: &ManifestLocation,
    components: &mut BTreeMap<(String, String, String), Component>,
    diagnostics: &mut Vec<Value>,
    diagnostic_total: &mut usize,
    diagnostic_limit: usize,
) -> Result<()> {
    let document = match serde_json::from_str::<Value>(content) {
        Ok(Value::Object(document)) => Value::Object(document),
        _ => {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "malformed_manifest",
                "npm package manifest is not a JSON object",
            )?;
            return Ok(());
        }
    };
    let workspace_members = match document.get("workspaces") {
        None => None,
        Some(Value::Array(values)) => Some(parse_json_string_array(
            values,
            location,
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
        )?),
        Some(Value::Object(workspaces)) => match workspaces.get("packages") {
            Some(Value::Array(values)) => Some(parse_json_string_array(
                values,
                location,
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
            )?),
            _ => {
                add_diagnostic(
                    diagnostics,
                    diagnostic_total,
                    diagnostic_limit,
                    location,
                    "invalid_workspace_members",
                    "npm workspaces must declare a packages array",
                )?;
                Some(Vec::new())
            }
        },
        Some(_) => {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "invalid_workspace_members",
                "npm workspaces must be an array or an object with packages",
            )?;
            Some(Vec::new())
        }
    };
    let name = document
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty());
    if document.get("name").is_some() && name.is_none() {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_package_name",
            "npm package name must be a non-empty string",
        )?;
    }
    if name.is_none() && workspace_members.is_none() && document.get("name").is_none() {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "missing_package_name",
            "npm package manifest does not declare a usable package name",
        )?;
    }
    let description = document
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if document
        .get("description")
        .is_some_and(|description| !description.is_string())
    {
        add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_description",
            "npm package description must be a string",
        )?;
    }
    if name.is_some() || workspace_members.is_some() {
        insert_component(
            components,
            Component {
                workspace: location.workspace.clone(),
                name: name.map(str::to_owned),
                ecosystem: "npm".into(),
                unit: if name.is_some() {
                    "package"
                } else {
                    "workspace"
                }
                .into(),
                root: location.root.clone(),
                manifest_path: location.manifest_path.clone(),
                description,
                workspace_members,
            },
        );
    }
    Ok(())
}

fn parse_json_string_array(
    values: &[Value],
    location: &ManifestLocation,
    diagnostics: &mut Vec<Value>,
    diagnostic_total: &mut usize,
    diagnostic_limit: usize,
) -> Result<Vec<String>> {
    let mut members = Vec::new();
    for value in values {
        if let Some(member) = value.as_str() {
            members.push(member.to_owned());
        } else {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "invalid_workspace_member",
                "npm workspace members must be strings",
            )?;
        }
    }
    Ok(members)
}

fn parse_pnpm_workspace(
    content: &str,
    location: &ManifestLocation,
    components: &mut BTreeMap<(String, String, String), Component>,
    diagnostics: &mut Vec<Value>,
    diagnostic_total: &mut usize,
    diagnostic_limit: usize,
) -> Result<()> {
    let document = match serde_yaml::from_str::<serde_yaml::Value>(content) {
        Ok(serde_yaml::Value::Mapping(document)) => serde_yaml::Value::Mapping(document),
        _ => {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "malformed_manifest",
                "pnpm workspace manifest is not a YAML mapping",
            )?;
            return Ok(());
        }
    };
    let workspace_members = match document.get("packages") {
        Some(serde_yaml::Value::Sequence(values)) => {
            let mut members = Vec::new();
            for value in values {
                if let Some(member) = value.as_str() {
                    members.push(member.to_owned());
                } else {
                    add_diagnostic(
                        diagnostics,
                        diagnostic_total,
                        diagnostic_limit,
                        location,
                        "invalid_workspace_member",
                        "pnpm workspace members must be strings",
                    )?;
                }
            }
            members
        }
        _ => {
            add_diagnostic(
                diagnostics,
                diagnostic_total,
                diagnostic_limit,
                location,
                "missing_workspace_members",
                "pnpm workspace manifest must declare a packages sequence",
            )?;
            Vec::new()
        }
    };
    insert_component(
        components,
        Component {
            workspace: location.workspace.clone(),
            name: None,
            ecosystem: "npm".into(),
            unit: "workspace".into(),
            root: location.root.clone(),
            manifest_path: location.manifest_path.clone(),
            description: None,
            workspace_members: Some(workspace_members),
        },
    );
    Ok(())
}

fn parse_go(
    content: &str,
    location: &ManifestLocation,
    components: &mut BTreeMap<(String, String, String), Component>,
    diagnostics: &mut Vec<Value>,
    diagnostic_total: &mut usize,
    diagnostic_limit: usize,
) -> Result<()> {
    let module = content.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("module")?;
        if !rest.chars().next().is_some_and(char::is_whitespace) {
            return None;
        }
        let value = rest
            .trim()
            .split_once("//")
            .map_or(rest.trim(), |(value, _)| value.trim());
        if value.is_empty() || value.starts_with('(') {
            return Some(Err(()));
        }
        let module_value = value.split_whitespace().next().unwrap_or_default();
        let module = if module_value.starts_with('"') {
            match serde_json::from_str::<String>(module_value) {
                Ok(module) => module,
                Err(_) => return Some(Err(())),
            }
        } else {
            module_value.to_owned()
        };
        if module.trim().is_empty() {
            Some(Err(()))
        } else {
            Some(Ok(module))
        }
    });
    match module {
        Some(Ok(name)) => insert_component(
            components,
            Component {
                workspace: location.workspace.clone(),
                name: Some(name),
                ecosystem: "go".into(),
                unit: "package".into(),
                root: location.root.clone(),
                manifest_path: location.manifest_path.clone(),
                description: None,
                workspace_members: None,
            },
        ),
        Some(Err(())) => add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "invalid_module_path",
            "Go module directive does not declare a usable module path",
        )?,
        None => add_diagnostic(
            diagnostics,
            diagnostic_total,
            diagnostic_limit,
            location,
            "missing_module_directive",
            "Go module manifest does not declare a module directive",
        )?,
    }
    Ok(())
}
