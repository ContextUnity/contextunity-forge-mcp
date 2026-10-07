pub(crate) mod javascript_packages;
pub(crate) mod typescript_paths;

use super::{module_stem, workspace_path, ImportPath};

fn match_mapping<'a>(pattern: &str, value: &'a str) -> Option<&'a str> {
    match pattern.split_once('*') {
        Some((prefix, suffix)) if !suffix.contains('*') => {
            value.strip_prefix(prefix)?.strip_suffix(suffix)
        }
        None if pattern == value => Some(""),
        _ => None,
    }
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
    let (_, local_path) = workspace_path(path);
    module_stem(local_path)
        .trim_end_matches("/index")
        .replace('/', ".")
}
