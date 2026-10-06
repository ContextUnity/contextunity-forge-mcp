use super::super::workspace_path;
use super::{javascript_namespace, joined_scope, match_mapping, ImportPath};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Registry {
    packages: HashMap<String, HashMap<String, Vec<JavascriptPackage>>>,
    workspaces: HashMap<String, Vec<String>>,
    pnpm_workspaces: HashMap<String, Vec<String>>,
    package_scopes: HashSet<String>,
    nuxt_package_scopes: HashSet<String>,
}

struct JavascriptPackage {
    scope: String,
    exports: serde_json::Value,
}

impl Registry {
    pub(super) fn record_manifest(&mut self, scope: &str, content: &str) {
        self.package_scopes.insert(scope.to_owned());
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(content) else {
            return;
        };
        if ["dependencies", "devDependencies"]
            .into_iter()
            .any(|section| {
                manifest
                    .get(section)
                    .is_some_and(|value| value.get("nuxt").is_some())
            })
        {
            self.nuxt_package_scopes.insert(scope.to_owned());
        }
        let workspaces = manifest.get("workspaces").and_then(|value| {
            value
                .as_array()
                .or_else(|| value.get("packages")?.as_array())
        });
        if let Some(workspaces) = workspaces {
            self.workspaces.insert(
                scope.to_owned(),
                workspaces
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect(),
            );
        }
        let (Some(name), Some(exports)) = (
            manifest.get("name").and_then(|value| value.as_str()),
            manifest.get("exports"),
        ) else {
            return;
        };
        let workspace = workspace_path(scope).0;
        self.packages
            .entry(workspace.to_owned())
            .or_default()
            .entry(name.to_owned())
            .or_default()
            .push(JavascriptPackage {
                scope: scope.to_owned(),
                exports: exports.clone(),
            });
    }

    pub(super) fn record_pnpm_workspace(&mut self, scope: &str, content: &str) {
        self.pnpm_workspaces.insert(
            scope.to_owned(),
            pnpm_workspace_patterns(content).unwrap_or_default(),
        );
    }

    pub(super) fn apply_pnpm_workspaces(&mut self) {
        self.workspaces
            .extend(std::mem::take(&mut self.pnpm_workspaces));
    }

    pub(super) fn nuxt_package_scope_for_path(&self, path: &str) -> Option<&str> {
        let workspace = workspace_path(path).0;
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            if let Some(scope) = self.package_scopes.get(directory) {
                return self.nuxt_package_scopes.get(scope).map(String::as_str);
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                return None;
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    pub(super) fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        let (package_name, subpath) = split_javascript_package(module)?;
        let workspace = workspace_path(owner).0;
        let packages = self.packages.get(workspace)?.get(package_name)?;
        let mut directory = owner
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        let declaration = loop {
            if let Some(patterns) = self.workspaces.get(directory) {
                break Some((directory, patterns));
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                break None;
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        };
        let mut resolved = None;
        for package in packages {
            if let Some((root, patterns)) = declaration {
                let relative = if root.is_empty() {
                    package.scope.as_str()
                } else if let Some(relative) = package
                    .scope
                    .strip_prefix(root)
                    .and_then(|rest| rest.strip_prefix('/'))
                {
                    relative
                } else {
                    continue;
                };
                let included = patterns.iter().any(|pattern| {
                    !pattern.starts_with('!')
                        && match_mapping(pattern.trim_start_matches("./"), relative).is_some()
                });
                let excluded = patterns.iter().any(|pattern| {
                    pattern.strip_prefix('!').is_some_and(|pattern| {
                        match_mapping(pattern.trim_start_matches("./"), relative).is_some()
                    })
                });
                if !included || excluded {
                    continue;
                }
            }
            let Some(target) = javascript_export_target(&package.exports, &subpath) else {
                continue;
            };
            if !target.starts_with("./") {
                continue;
            }
            let Some(path) = joined_scope(
                &package.scope,
                &target,
                package
                    .scope
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .count(),
            ) else {
                continue;
            };
            let namespace = javascript_namespace(&path);
            if resolved.replace(namespace).is_some() {
                return None;
            }
        }
        resolved.map(ImportPath::absolute)
    }
}

fn split_javascript_package(module: &str) -> Option<(&str, String)> {
    let (name, rest) = if module.starts_with('@') {
        let mut parts = module.splitn(3, '/');
        let scope = parts.next()?;
        let package = parts.next()?;
        let name = &module[..scope.len() + 1 + package.len()];
        (name, parts.next().unwrap_or(""))
    } else {
        module.split_once('/').unwrap_or((module, ""))
    };
    (!name.is_empty()).then(|| {
        (
            name,
            if rest.is_empty() {
                ".".to_owned()
            } else {
                format!("./{rest}")
            },
        )
    })
}

fn javascript_export_target(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    fn target(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(path) => Some(path.clone()),
            serde_json::Value::Object(conditions) => {
                ["import", "default", "types", "require", "node"]
                    .into_iter()
                    .find_map(|key| conditions.get(key).and_then(target))
            }
            _ => None,
        }
    }
    match exports {
        serde_json::Value::String(_) if subpath == "." => target(exports),
        serde_json::Value::Object(map) => {
            if let Some(exact) = map.get(subpath) {
                return target(exact);
            }
            let mut selected: Option<(usize, usize, &serde_json::Value, &str)> = None;
            for (pattern, value) in map {
                if !pattern.starts_with("./") {
                    continue;
                }
                let Some((prefix, suffix)) = pattern.split_once('*') else {
                    continue;
                };
                let Some(capture) = match_mapping(pattern, subpath) else {
                    continue;
                };
                if selected
                    .as_ref()
                    .is_none_or(|(best_prefix, best_suffix, _, _)| {
                        (prefix.len(), suffix.len()) > (*best_prefix, *best_suffix)
                    })
                {
                    selected = Some((prefix.len(), suffix.len(), value, capture));
                }
            }
            selected
                .and_then(|(_, _, value, capture)| {
                    target(value).map(|path| path.replace('*', capture))
                })
                .or_else(|| (subpath == ".").then(|| target(exports)).flatten())
        }
        _ => None,
    }
}

fn pnpm_workspace_patterns(content: &str) -> Option<Vec<String>> {
    let workspace: serde_yaml::Value = serde_yaml::from_str(content).ok()?;
    workspace
        .get("packages")?
        .as_sequence()?
        .iter()
        .map(|pattern| pattern.as_str().map(str::to_owned))
        .collect()
}
