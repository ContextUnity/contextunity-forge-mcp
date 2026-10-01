use crate::engine::languages::manifest::{toml_string, visit_toml_pairs};
use tree_sitter::Node;

fn package(requirement: &str) -> Option<String> {
    let requirement = requirement.trim();
    let requirement = if requirement.starts_with("-e ") || requirement.starts_with("--editable ") {
        requirement.split("#egg=").nth(1)?
    } else {
        requirement
    };
    if !requirement.as_bytes().first()?.is_ascii_alphanumeric() {
        return None;
    }
    let name = requirement
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
        .next()?;
    let tail = &requirement[name.len()..];
    if tail.chars().next().is_some_and(|ch| {
        !ch.is_whitespace() && !matches!(ch, '[' | '<' | '>' | '=' | '!' | '~' | ';' | '@')
    }) || [".whl", ".tar.gz", ".zip"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return None;
    }
    Some(name.to_ascii_lowercase().replace(['-', '.'], "_"))
}

fn requirements(values: &str, result: &mut Vec<String>) {
    result.extend(values.lines().filter_map(package));
}

fn array_dependencies(value: Node<'_>, source: &str, result: &mut Vec<String>) {
    if value.kind() != "array" {
        return;
    }
    let mut cursor = value.walk();
    for item in value
        .named_children(&mut cursor)
        .filter(|item| item.kind() == "string")
    {
        if let Some(name) = toml_string(item, source).and_then(|value| package(&value)) {
            result.push(name);
        }
    }
}

fn setup_dependencies(source: &str, result: &mut Vec<String>) {
    let mut section = "";
    let mut active = false;
    for raw in source.lines() {
        let line = raw.trim();
        if line.starts_with(['#', ';']) || line.is_empty() {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = name;
            active = false;
        } else if raw.starts_with(char::is_whitespace) && active {
            requirements(line, result);
        } else if let Some((key, value)) = line.split_once('=') {
            active = section == "options.extras_require"
                || (section == "options"
                    && matches!(
                        key.trim(),
                        "install_requires" | "setup_requires" | "tests_require"
                    ));
            if active {
                requirements(value, result);
            }
        } else {
            active = false;
        }
    }
}

pub(super) fn dependencies(filename: &str, source: &str) -> Vec<String> {
    let filename = filename.rsplit('/').next().unwrap_or(filename);
    let mut result = Vec::new();
    if filename.starts_with("requirements") && filename.ends_with(".txt") {
        requirements(source, &mut result);
    } else if filename == "setup.cfg" {
        setup_dependencies(source, &mut result);
    } else if matches!(filename, "pyproject.toml" | "Pipfile") {
        visit_toml_pairs(source, &mut |path, value| {
            if path == "project.dependencies"
                || path.starts_with("project.optional-dependencies.")
                || path.starts_with("dependency-groups.")
                || path == "build-system.requires"
            {
                array_dependencies(value, source, &mut result);
            } else {
                let dependency = path
                    .strip_prefix("tool.poetry.dependencies.")
                    .or_else(|| path.strip_prefix("tool.poetry.dev-dependencies."))
                    .or_else(|| {
                        path.strip_prefix("tool.poetry.group.")
                            .and_then(|tail| tail.split_once(".dependencies.").map(|(_, key)| key))
                    })
                    .or_else(|| {
                        (filename == "Pipfile")
                            .then(|| {
                                path.strip_prefix("packages.")
                                    .or_else(|| path.strip_prefix("dev-packages."))
                            })
                            .flatten()
                    });
                if let Some(dependency) = dependency.filter(|key| *key != "python") {
                    if let Some(name) = package(dependency) {
                        result.push(name);
                    }
                }
            }
        });
    }
    result.sort_unstable();
    result.dedup();
    result
}
