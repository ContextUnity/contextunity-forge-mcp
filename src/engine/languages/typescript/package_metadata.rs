fn local_package_reference(value: &str) -> bool {
    ["workspace:", "link:", "file:", "portal:"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

fn local_json_package(value: &serde_json::Value) -> bool {
    value.get("link").and_then(serde_json::Value::as_bool) == Some(true)
        || value.as_str().is_some_and(local_package_reference)
        || ["version", "specifier", "resolved"].iter().any(|field| {
            value
                .get(*field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(local_package_reference)
        })
}

fn npm_dependency_keys(value: &serde_json::Value, output: &mut Vec<String>) {
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(entries) = value.get(section).and_then(serde_json::Value::as_object) {
            output.extend(
                entries
                    .iter()
                    .filter(|(_, descriptor)| !local_json_package(descriptor))
                    .map(|(name, _)| name.clone()),
            );
        }
    }
}

fn package_lock_dependencies(content: &str) -> Vec<String> {
    let Ok(lock) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    npm_dependency_keys(&lock, &mut dependencies);
    if let Some(entries) = lock
        .get("dependencies")
        .and_then(serde_json::Value::as_object)
    {
        let mut pending: Vec<_> = entries.values().collect();
        while let Some(entry) = pending.pop() {
            if local_json_package(entry) {
                continue;
            }
            npm_dependency_keys(entry, &mut dependencies);
            if let Some(nested) = entry
                .get("dependencies")
                .and_then(serde_json::Value::as_object)
            {
                pending.extend(nested.values());
            }
        }
    }
    if let Some(packages) = lock.get("packages").and_then(serde_json::Value::as_object) {
        for (path, package) in packages {
            if local_json_package(package) {
                continue;
            }
            if let Some((_, name)) = path.rsplit_once("node_modules/") {
                if !name.is_empty() {
                    dependencies.push(name.to_owned());
                }
            }
            npm_dependency_keys(package, &mut dependencies);
        }
    }
    dependencies
}

fn pnpm_package_name(locator: &str) -> Option<&str> {
    let locator = locator.trim_start_matches('/');
    let version = if let Some(scoped) = locator.strip_prefix('@') {
        let package = scoped.find('/').map(|index| index + 2)?;
        locator[package..]
            .find(['@', '/'])
            .map(|index| index + package)
    } else {
        locator.find(['@', '/'])
    }?;
    let descriptor = &locator[version + 1..];
    (version > 0 && !descriptor.is_empty() && !local_package_reference(descriptor))
        .then_some(&locator[..version])
}

fn local_yaml_package(value: &serde_yaml::Value) -> bool {
    value.as_str().is_some_and(local_package_reference)
        || ["specifier", "version"].iter().any(|field| {
            value
                .get(*field)
                .and_then(serde_yaml::Value::as_str)
                .is_some_and(local_package_reference)
        })
}

fn pnpm_lock_dependencies(content: &str) -> Vec<String> {
    let Ok(lock) = serde_yaml::from_str::<serde_yaml::Value>(content) else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(entries) = lock.get(section).and_then(serde_yaml::Value::as_mapping) {
            dependencies.extend(
                entries
                    .iter()
                    .filter(|(_, descriptor)| !local_yaml_package(descriptor))
                    .filter_map(|(name, _)| name.as_str())
                    .map(str::to_owned),
            );
        }
    }
    if let Some(importers) = lock
        .get("importers")
        .and_then(serde_yaml::Value::as_mapping)
    {
        for importer in importers.values() {
            for section in [
                "dependencies",
                "devDependencies",
                "optionalDependencies",
                "peerDependencies",
            ] {
                if let Some(entries) = importer
                    .get(section)
                    .and_then(serde_yaml::Value::as_mapping)
                {
                    dependencies.extend(
                        entries
                            .iter()
                            .filter(|(_, descriptor)| !local_yaml_package(descriptor))
                            .filter_map(|(name, _)| name.as_str())
                            .map(str::to_owned),
                    );
                }
            }
        }
    }
    for section in ["packages", "snapshots"] {
        if let Some(packages) = lock.get(section).and_then(serde_yaml::Value::as_mapping) {
            dependencies.extend(
                packages
                    .keys()
                    .filter_map(serde_yaml::Value::as_str)
                    .filter_map(pnpm_package_name)
                    .map(str::to_owned),
            );
        }
    }
    dependencies
}

fn yarn_package_name(selector: &str) -> Option<&str> {
    let selector = selector.trim().trim_matches(['\'', '"']);
    let version = if let Some(scoped) = selector.strip_prefix('@') {
        scoped.find('@').map(|index| index + 1)
    } else {
        selector.find('@')
    }?;
    let descriptor = &selector[version + 1..];
    (version > 0 && !descriptor.is_empty() && !local_package_reference(descriptor))
        .then_some(&selector[..version])
}

fn yarn_lock_dependencies(content: &str) -> Vec<String> {
    let mut dependencies = Vec::new();
    for line in content.lines() {
        if line.is_empty() || line.starts_with([' ', '\t', '#']) || !line.ends_with(':') {
            continue;
        }
        for selector in line.trim_end_matches(':').split(',') {
            if let Some(name) = yarn_package_name(selector) {
                dependencies.push(name.to_owned());
            }
        }
    }
    dependencies
}

pub(super) fn extract_dependencies(filename: &str, content: &str) -> Vec<String> {
    match filename {
        "package-lock.json" => return package_lock_dependencies(content),
        "pnpm-lock.yaml" => return pnpm_lock_dependencies(content),
        "yarn.lock" => return yarn_lock_dependencies(content),
        _ => {}
    }
    if filename != "package.json" {
        return Vec::new();
    }
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    ["dependencies", "devDependencies", "peerDependencies"]
        .into_iter()
        .filter_map(|section| manifest.get(section)?.as_object())
        .flat_map(|dependencies| {
            dependencies
                .iter()
                .filter(|(_, value)| !local_json_package(value))
                .map(|(name, _)| name.clone())
        })
        .collect()
}
