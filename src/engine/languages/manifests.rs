use super::{profiles, ImportPath, LanguageFamily, LanguageProfile};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, collections::HashSet, path::Path};

#[derive(Default)]
/// Represents dependency registry data.
pub struct DependencyRegistry {
    packages: HashMap<LanguageFamily, HashSet<String>>,
    scoped_packages: HashMap<LanguageFamily, HashMap<String, HashSet<String>>>,
    scoped_origins: HashMap<LanguageFamily, HashMap<String, HashMap<String, String>>>,
    javascript_packages: HashMap<String, HashMap<String, Vec<JavascriptPackage>>>,
    javascript_workspaces: HashMap<String, Vec<String>>,
    javascript_package_scopes: HashSet<String>,
    nuxt_package_scopes: HashSet<String>,
    typescript_paths: HashMap<String, TypescriptPaths>,
    digest: String,
}

struct JavascriptPackage {
    scope: String,
    exports: serde_json::Value,
}

#[derive(Clone)]
struct TypescriptPaths {
    base: String,
    mappings: Vec<(String, Vec<String>)>,
}

struct TypescriptConfig {
    scope: String,
    extends: Option<String>,
    paths: Option<TypescriptPaths>,
}

impl DependencyRegistry {
    fn record_javascript_package(&mut self, scope: &str, content: &str) {
        self.javascript_package_scopes.insert(scope.to_owned());
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
            self.javascript_workspaces.insert(
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
        let workspace = super::workspace_path(scope).0;
        self.javascript_packages
            .entry(workspace.to_owned())
            .or_default()
            .entry(name.to_owned())
            .or_default()
            .push(JavascriptPackage {
                scope: scope.to_owned(),
                exports: exports.clone(),
            });
    }

    fn parse_typescript_config(scope: &str, content: &str) -> Option<TypescriptConfig> {
        let config = parse_jsonc(content)?;
        let options = config.get("compilerOptions");
        let workspace = super::workspace_path(scope).0;
        let floor = usize::from(!workspace.is_empty());
        let paths = if let Some(values) = options.and_then(|options| options.get("paths")) {
            let base = options
                .and_then(|options| options.get("baseUrl"))
                .and_then(|value| value.as_str())
                .unwrap_or(".");
            let base = joined_scope(scope, base, floor)?;
            let mut mappings: Vec<(String, Vec<String>)> = values
                .as_object()
                .into_iter()
                .flat_map(|paths| paths.iter())
                .filter_map(|(pattern, targets)| {
                    let targets: Vec<String> = targets
                        .as_array()?
                        .iter()
                        .filter_map(|target| target.as_str().map(str::to_owned))
                        .collect();
                    (!targets.is_empty()).then(|| (pattern.clone(), targets))
                })
                .collect();
            mappings.sort_by(|(left, _), (right, _)| {
                let rank = |pattern: &str| {
                    (
                        !pattern.contains('*'),
                        pattern
                            .split_once('*')
                            .map_or(pattern.len(), |(prefix, _)| prefix.len()),
                        pattern.len(),
                    )
                };
                rank(right).cmp(&rank(left))
            });
            Some(TypescriptPaths { base, mappings })
        } else {
            None
        };
        Some(TypescriptConfig {
            scope: scope.to_owned(),
            extends: config
                .get("extends")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            paths,
        })
    }

    /// Resolves local package exports and TypeScript path aliases from the manifest snapshot.
    pub(crate) fn normalize_javascript_import(
        &self,
        profile_id: &str,
        owner: &str,
        module: &str,
        indexed_modules: &HashSet<&str>,
    ) -> Option<ImportPath> {
        if !matches!(profile_id, "javascript" | "typescript")
            || module.starts_with('.')
            || module.starts_with('/')
        {
            return None;
        }
        if profile_id == "typescript" {
            let workspace = super::workspace_path(owner).0;
            let mut directory = owner
                .rsplit_once('/')
                .map_or("", |(directory, _)| directory);
            loop {
                if let Some(paths) = self.typescript_paths.get(directory) {
                    for (pattern, targets) in &paths.mappings {
                        if let Some(capture) = match_mapping(pattern, module) {
                            for target in targets {
                                let target = target.replace('*', capture);
                                if let Some(path) = joined_scope(
                                    &paths.base,
                                    &target,
                                    usize::from(!workspace.is_empty()),
                                ) {
                                    let stem = super::module_stem(&path).trim_end_matches("/index");
                                    if indexed_modules.contains(stem) {
                                        return Some(ImportPath::absolute(javascript_namespace(
                                            &path,
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
                if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                    break;
                }
                directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
            }
        }
        let (package_name, subpath) = split_javascript_package(module)?;
        let workspace = super::workspace_path(owner).0;
        let packages = self.javascript_packages.get(workspace)?.get(package_name)?;
        let mut directory = owner
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        let declaration = loop {
            if let Some(patterns) = self.javascript_workspaces.get(directory) {
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

    /// Performs collect.
    pub fn collect(root: Option<&Path>) -> Self {
        let Some(root) = root else {
            return Self::default();
        };
        let adapter = crate::engine::scanner::load_adapter(root, None).ok();
        Self::collect_with_adapter(root, adapter.as_ref())
    }

    /// Performs collect with adapter.
    pub fn collect_with_adapter(
        root: &Path,
        adapter: Option<&crate::engine::scanner::Adapter>,
    ) -> Self {
        let mut registry = Self::default();
        let mut roots = vec![(
            root.to_path_buf(),
            String::new(),
            adapter
                .map(|adapter| adapter.ignored_names.clone())
                .unwrap_or_default(),
        )];
        if let Some(adapter) = adapter {
            roots.extend(adapter.linked_workspaces.iter().map(|workspace| {
                (
                    workspace.path.clone(),
                    format!("[{}]/", workspace.name),
                    workspace.ignored_names.clone(),
                )
            }));
        }
        let profiles: Vec<_> = profiles()
            .filter(|profile| !profile.manifest_filenames().is_empty())
            .collect();
        let mut visited = HashSet::new();
        let mut snapshots = std::collections::BTreeMap::new();
        let mut typescript_configs = HashMap::new();
        let mut pnpm_workspaces = HashMap::new();
        let mut parsed_families = Vec::with_capacity(profiles.len());
        for (root, prefix, ignored_names) in roots {
            let ignored_names: HashSet<std::ffi::OsString> =
                ignored_names.into_iter().map(Into::into).collect();
            let mut walk = ignore::WalkBuilder::new(&root);
            walk.hidden(false)
                .follow_links(false)
                .git_ignore(true)
                .filter_entry(move |entry| {
                    !entry.path_is_symlink()
                        && !ignored_names.contains(entry.file_name())
                        && !matches!(
                            entry.file_name().to_str(),
                            Some(
                                ".git"
                                    | "node_modules"
                                    | "target"
                                    | ".venv"
                                    | "venv"
                                    | "__pycache__"
                            )
                        )
                });
            for entry in walk.build().filter_map(Result::ok) {
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let Some(filename) = entry.file_name().to_str() else {
                    continue;
                };
                let matches = |profile: &&dyn LanguageProfile| {
                    profile
                        .manifest_filenames()
                        .iter()
                        .any(|pattern| filename_matches(pattern, filename))
                };
                if !profiles.iter().any(matches) || !visited.insert(entry.path().to_path_buf()) {
                    continue;
                }
                // Manifest parsing is bounded independently of source-file limits.
                if entry
                    .metadata()
                    .map_or(true, |metadata| metadata.len() > 4 * 1024 * 1024)
                {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                let Some(directory) = entry
                    .path()
                    .parent()
                    .and_then(|parent| parent.strip_prefix(&root).ok())
                else {
                    continue;
                };
                let mut scope = prefix.clone();
                scope.push_str(&directory.to_string_lossy().replace('\\', "/"));
                let scope = scope.trim_end_matches('/').to_owned();
                if filename == "package.json" {
                    registry.record_javascript_package(&scope, &content);
                } else if filename == "pnpm-workspace.yaml" {
                    pnpm_workspaces.insert(
                        scope.clone(),
                        pnpm_workspace_patterns(&content).unwrap_or_default(),
                    );
                } else if filename.starts_with("tsconfig") && filename.ends_with(".json") {
                    if let Some(config) = Self::parse_typescript_config(&scope, &content) {
                        let path = if scope.is_empty() {
                            filename.to_owned()
                        } else {
                            format!("{scope}/{filename}")
                        };
                        typescript_configs.insert(path, config);
                    }
                }
                snapshots.insert(
                    entry.path().to_path_buf(),
                    Sha256::digest(content.as_bytes()),
                );
                if filename == "pnpm-workspace.yaml"
                    || (filename.starts_with("tsconfig") && filename.ends_with(".json"))
                {
                    continue;
                }
                parsed_families.clear();
                for profile in profiles.iter().copied().filter(|profile| matches(profile)) {
                    let family = profile.family();
                    if parsed_families.contains(&family) {
                        continue;
                    }
                    parsed_families.push(family);
                    let dependencies = profile.extract_manifest_dependencies(filename, &content);
                    {
                        let origin = entry
                            .path()
                            .strip_prefix(&root)
                            .map(|relative| {
                                format!(
                                    "{}{}",
                                    prefix,
                                    relative.to_string_lossy().replace('\\', "/")
                                )
                            })
                            .unwrap_or_else(|_| filename.to_owned());
                        let origins = registry
                            .scoped_origins
                            .entry(family)
                            .or_default()
                            .entry(scope.clone())
                            .or_default();
                        for dependency in &dependencies {
                            let recorded = origins.entry(dependency.clone()).or_default();
                            if recorded.is_empty() || preferred_manifest(&origin, recorded) {
                                *recorded = origin.clone();
                            }
                        }
                        registry
                            .scoped_packages
                            .entry(family)
                            .or_default()
                            .entry(scope.clone())
                            .or_default()
                            .extend(dependencies.iter().cloned());
                    }
                    registry
                        .packages
                        .entry(family)
                        .or_default()
                        .extend(dependencies);
                }
            }
        }
        registry.javascript_workspaces.extend(pnpm_workspaces);
        let mut resolved_configs = HashMap::new();
        for (path, config) in &typescript_configs {
            if path == "tsconfig.json" || path.ends_with("/tsconfig.json") {
                if let Some(Some(paths)) = resolve_typescript_config(
                    path,
                    &typescript_configs,
                    &mut resolved_configs,
                    &mut HashSet::new(),
                ) {
                    registry
                        .typescript_paths
                        .insert(config.scope.clone(), paths);
                }
            }
        }
        let mut hasher = Sha256::new();
        for (path, digest) in snapshots {
            let path = path.to_string_lossy();
            hasher.update((path.len() as u64).to_be_bytes());
            hasher.update(path.as_bytes());
            hasher.update(digest);
        }
        registry.digest = hex::encode(hasher.finalize());
        registry
    }

    /// Performs digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn declares_for_path(
        &self,
        family: LanguageFamily,
        path: &str,
        package: &str,
    ) -> bool {
        let Some(scope) = self.nearest_manifest_scope_for_path(family, path) else {
            return false;
        };
        self.scoped_packages
            .get(&family)
            .and_then(|scopes| scopes.get(scope))
            .is_some_and(|packages| packages.contains(package))
    }

    pub(crate) fn nuxt_package_scope_for_path(&self, path: &str) -> Option<&str> {
        let workspace = super::workspace_path(path).0;
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            if let Some(scope) = self.javascript_package_scopes.get(directory) {
                return self.nuxt_package_scopes.get(scope).map(String::as_str);
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                return None;
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    pub(crate) fn nearest_manifest_scope_for_path(
        &self,
        family: LanguageFamily,
        path: &str,
    ) -> Option<&str> {
        let scopes = self.scoped_packages.get(&family)?;
        let workspace = super::workspace_path(path).0;
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            if let Some((scope, _)) = scopes.get_key_value(directory) {
                return Some(scope.as_str());
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                return None;
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    /// Performs classification.
    pub fn classification(
        &self,
        profile: &dyn LanguageProfile,
        module: &str,
    ) -> Option<&'static str> {
        if module.is_empty() {
            return None;
        }
        if profile.is_stdlib(module) {
            return Some("standard library");
        }
        let declared = self
            .packages
            .get(&profile.family())
            .is_some_and(|packages| {
                if packages.contains(module) {
                    return true;
                }
                // Borrowed prefixes preserve scoped npm and Go import-path identities.
                module.char_indices().any(|(index, character)| {
                    matches!(character, '.' | '/' | ':') && packages.contains(&module[..index])
                })
            });
        Some(if declared {
            "external dependency (manifest)"
        } else {
            "external dependency"
        })
    }

    /// Classifies an import using the nearest declaring manifest in its workspace.
    pub fn classification_for_path(
        &self,
        profile: &dyn LanguageProfile,
        module: &str,
        path: &str,
    ) -> Option<String> {
        let classification = self.classification(profile, module)?;
        if classification != "external dependency (manifest)" {
            return Some(classification.to_owned());
        }
        let workspace = super::workspace_path(path).0;
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            if let Some(origins) = self
                .scoped_origins
                .get(&profile.family())
                .and_then(|scopes| scopes.get(directory))
            {
                let origin = origins.get(module).or_else(|| {
                    module.char_indices().rev().find_map(|(index, character)| {
                        matches!(character, '.' | '/' | ':')
                            .then(|| origins.get(&module[..index]))
                            .flatten()
                    })
                });
                if let Some(origin) = origin {
                    return Some(format!("external dependency (manifest: {origin})"));
                }
            }
            if directory.is_empty() || (!workspace.is_empty() && directory == workspace) {
                return Some("external dependency".to_owned());
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
}

fn preferred_manifest(candidate: &str, recorded: &str) -> bool {
    let rank = |path: &str| match path.rsplit('/').next().unwrap_or(path) {
        "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => 0,
        _ => 1,
    };
    (rank(candidate), candidate) < (rank(recorded), recorded)
}

fn resolve_typescript_config(
    path: &str,
    configs: &HashMap<String, TypescriptConfig>,
    memo: &mut HashMap<String, Option<Option<TypescriptPaths>>>,
    visiting: &mut HashSet<String>,
) -> Option<Option<TypescriptPaths>> {
    if let Some(cached) = memo.get(path) {
        return cached.clone();
    }
    let config = configs.get(path)?;
    if visiting.len() >= 16 || !visiting.insert(path.to_owned()) {
        return None;
    }
    let resolved = (|| {
        let inherited = if let Some(source) = config.extends.as_deref() {
            if !source.starts_with("./") && !source.starts_with("../") {
                return None;
            }
            let workspace = super::workspace_path(&config.scope).0;
            let path = joined_scope(&config.scope, source, usize::from(!workspace.is_empty()))?;
            let target = if configs.contains_key(&path) {
                path
            } else if configs.contains_key(&format!("{path}.json")) {
                format!("{path}.json")
            } else if configs.contains_key(&format!("{path}/tsconfig.json")) {
                format!("{path}/tsconfig.json")
            } else {
                return None;
            };
            resolve_typescript_config(&target, configs, memo, visiting)?
        } else {
            None
        };
        Some(config.paths.clone().or(inherited))
    })();
    visiting.remove(path);
    memo.insert(path.to_owned(), resolved.clone());
    resolved
}

fn match_mapping<'a>(pattern: &str, value: &'a str) -> Option<&'a str> {
    match pattern.split_once('*') {
        Some((prefix, suffix)) if !suffix.contains('*') => {
            value.strip_prefix(prefix)?.strip_suffix(suffix)
        }
        None if pattern == value => Some(""),
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

fn parse_jsonc(content: &str) -> Option<serde_json::Value> {
    if let Ok(value) = serde_json::from_str(content) {
        return Some(value);
    }
    let bytes = content.as_bytes();
    let mut cleaned = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut quoted = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            cleaned.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            quoted = true;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            cleaned.push(b' ');
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            cleaned.push(b' ');
            index += 2;
            while index + 1 < bytes.len() && (bytes[index], bytes[index + 1]) != (b'*', b'/') {
                if bytes[index] == b'\n' {
                    cleaned.push(b'\n');
                }
                index += 1;
            }
            if index + 1 >= bytes.len() {
                return None;
            }
            index += 2;
            continue;
        }
        cleaned.push(byte);
        index += 1;
    }
    let mut without_trailing = Vec::with_capacity(cleaned.len());
    let mut quoted = false;
    let mut escaped = false;
    for (index, &byte) in cleaned.iter().enumerate() {
        if quoted {
            without_trailing.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
        } else if byte == b',' {
            let mut next = index + 1;
            while cleaned.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if matches!(cleaned.get(next), Some(b'}' | b']')) {
                continue;
            }
        }
        without_trailing.push(byte);
    }
    serde_json::from_slice(&without_trailing).ok()
}

fn joined_scope(scope: &str, target: &str, floor: usize) -> Option<String> {
    if target.starts_with('/') || target.contains('\\') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = scope.split('/').filter(|part| !part.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.len() <= floor {
                    return None;
                }
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn javascript_namespace(path: &str) -> String {
    let (_, local_path) = super::workspace_path(path);
    super::module_stem(local_path)
        .trim_end_matches("/index")
        .replace('/', ".")
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

fn filename_matches(pattern: &str, filename: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => filename.starts_with(prefix) && filename.ends_with(suffix),
        None => pattern == filename,
    }
}

/// Reports whether manifest filename applies.
pub fn is_manifest_filename(filename: &str) -> bool {
    profiles().any(|profile| {
        profile
            .manifest_filenames()
            .iter()
            .any(|pattern| filename_matches(pattern, filename))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requirement_patterns_are_filename_scoped() {
        assert!(filename_matches(
            "requirements*.txt",
            "requirements-dev.txt"
        ));
        assert!(!filename_matches(
            "requirements*.txt",
            "requirements-dev.yaml"
        ));
        assert!(filename_matches("Cargo.toml", "Cargo.toml"));
    }
}
