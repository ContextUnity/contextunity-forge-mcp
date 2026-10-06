use super::super::{module_stem, workspace_path};
use super::{javascript_namespace, joined_scope, match_mapping, ImportPath};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Registry {
    configs: HashMap<String, TypescriptConfig>,
    paths: HashMap<String, TypescriptPaths>,
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

impl Registry {
    pub(super) fn record_config(&mut self, scope: &str, filename: &str, content: &str) {
        if let Some(config) = parse_typescript_config(scope, content) {
            let path = if scope.is_empty() {
                filename.to_owned()
            } else {
                format!("{scope}/{filename}")
            };
            self.configs.insert(path, config);
        }
    }

    pub(super) fn resolve_all(&mut self) {
        let configs = std::mem::take(&mut self.configs);
        let mut resolved_configs = HashMap::new();
        for (path, config) in &configs {
            if path == "tsconfig.json" || path.ends_with("/tsconfig.json") {
                if let Some(Some(paths)) = resolve_typescript_config(
                    path,
                    &configs,
                    &mut resolved_configs,
                    &mut HashSet::new(),
                ) {
                    self.paths.insert(config.scope.clone(), paths);
                }
            }
        }
    }

    pub(super) fn resolve_import(
        &self,
        owner: &str,
        module: &str,
        indexed_modules: &HashSet<&str>,
    ) -> Option<ImportPath> {
        let workspace = workspace_path(owner).0;
        let mut directory = owner
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        loop {
            if let Some(paths) = self.paths.get(directory) {
                for (pattern, targets) in &paths.mappings {
                    if let Some(capture) = match_mapping(pattern, module) {
                        for target in targets {
                            let target = target.replace('*', capture);
                            if let Some(path) = joined_scope(
                                &paths.base,
                                &target,
                                usize::from(!workspace.is_empty()),
                            ) {
                                let stem = module_stem(&path).trim_end_matches("/index");
                                if indexed_modules.contains(stem) {
                                    return Some(ImportPath::absolute(javascript_namespace(&path)));
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
        None
    }
}

fn parse_typescript_config(scope: &str, content: &str) -> Option<TypescriptConfig> {
    let config = parse_jsonc(content)?;
    let options = config.get("compilerOptions");
    let workspace = workspace_path(scope).0;
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
            let workspace = workspace_path(&config.scope).0;
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
