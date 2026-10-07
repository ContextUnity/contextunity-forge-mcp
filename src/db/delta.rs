use super::*;

fn delta_components(
    conn: &Connection,
    modified: &BTreeSet<String>,
    facts: &BTreeMap<String, TypedFacts>,
    replace_files: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let mut st = conn.prepare("SELECT path.path,owner.path FROM nodes n JOIN path_dictionary path ON path.path_id=n.path_id JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE n.kind='component'")?;
    let previous = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let mut st = conn.prepare("SELECT owner.path FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE n.kind='module' AND owner.path NOT IN(SELECT value FROM json_each(?1))")?;
    let paths = st
        .query_map([serde_json::to_string(modified)?], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let current = component_owners(paths.iter().map(String::as_str).chain(
        facts.iter().filter_map(|(path, f)| {
            f.nodes
                .iter()
                .any(|n| n.kind == "module")
                .then_some(path.as_str())
        }),
    ));
    for (root, owner) in &previous {
        if current.get(root) != Some(owner) {
            replace_files.insert(owner.clone());
        }
    }
    for (root, owner) in &current {
        if previous.get(root) != Some(owner) {
            replace_files.insert(owner.clone());
        }
    }
    Ok(current)
}
/// Performs delta.
pub fn delta(root: &Path, db: &Path, modified: &[PathBuf]) -> Result<Value> {
    let started = Instant::now();
    if modified.is_empty() {
        bail!("delta requires at least one modified path");
    }
    let root = scanner::canonical_root(root)?;
    let generation_lock = crate::db::cache::exclusive_lock(db)?;
    let (schema_compatible, previous_adapter_path) = {
        let probe = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let schema_compatible = probe
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some(scanner::ENGINE_SCHEMA_VERSION);
        let raw_policy: Option<String> = probe
            .query_row(
                "SELECT value FROM metadata WHERE key='adapter'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let adapter_path = raw_policy
            .map(|raw| {
                let value: Value = serde_json::from_str(&raw)?;
                crate::db::writer::persisted_adapter_path(&root, &value)
            })
            .transpose()?
            .flatten();
        (schema_compatible, adapter_path)
    };
    if !schema_compatible {
        drop(generation_lock);
        return build(&root, db, previous_adapter_path.as_deref());
    }
    let admitted = crate::db::reader::open(db, &root)?;
    let admitted_identity = crate::db::cache::identity(db)?;
    let expected: String = admitted.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    let adapter = read_policy(&root, &admitted)?;
    let current_adapter = scanner::load_adapter(&root, adapter.adapter_path.as_deref())?;
    scanner::check_root_scope(&root, current_adapter.limits.allow_broad_root)?;
    for path in modified {
        scanner::checked_child(&root, path)?;
    }
    let manifests_changed = modified.iter().any(|path| {
        path.file_name().and_then(|name| name.to_str()).is_some_and(
            crate::engine::languages::dependency_registry::is_dependency_manifest_filename,
        )
    });
    if current_adapter.digest != adapter.digest
        || current_adapter.linked_workspaces != adapter.linked_workspaces
        || manifests_changed
    {
        drop(admitted);
        drop(generation_lock);
        return build(&root, db, adapter.adapter_path.as_deref());
    }
    let dependencies =
        crate::engine::languages::dependency_registry::DependencyRegistry::collect_with_scan_config(
            &root,
            Some(&adapter),
        );
    let previous_manifest_digest = admitted
        .query_row(
            "SELECT value FROM metadata WHERE key='manifest_digest'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if previous_manifest_digest.as_deref() != Some(dependencies.digest()) {
        drop(admitted);
        drop(generation_lock);
        return build(&root, db, adapter.adapter_path.as_deref());
    }
    let previous = crate::db::reader::inventory_snapshot(&admitted)?;
    let scanning = Instant::now();
    let scan = scanner::scan_reusing(&root, &adapter, &previous)?;
    let scan_ms = scanning.elapsed().as_secs_f64() * 1000.;
    let modified: BTreeSet<String> = modified
        .iter()
        .map(|p| {
            scanner::checked_child(&root, p)?;
            Ok(p.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Result<_>>()?;
    let inventory: BTreeMap<_, _> = scan.entries.iter().map(|f| (f.path.as_str(), f)).collect();
    let previous: BTreeMap<_, _> = previous.iter().map(|f| (f.path.as_str(), f)).collect();
    for (path, old) in &previous {
        if !modified.contains(*path) && inventory.get(path).is_none_or(|f| f.digest != old.digest) {
            return Err(SourceSnapshotMismatch(format!("unlisted source change: {path}")).into());
        }
    }
    for path in inventory.keys() {
        if !previous.contains_key(path) && !modified.contains(*path) {
            return Err(SourceSnapshotMismatch(format!("unlisted source addition: {path}")).into());
        }
    }
    for path in &modified {
        if !inventory.contains_key(path.as_str()) {
            if root.join(path).exists() {
                bail!("modified path outside admitted inventory: {path}");
            }
            if !previous.contains_key(path.as_str()) {
                bail!("deletion was not in previous inventory: {path}");
            }
        }
    }
    let extracting = Instant::now();
    let mut facts = BTreeMap::new();
    let mut changed_resolution = BTreeSet::new();
    for path in &modified {
        if let Some(file) = inventory.get(path.as_str()) {
            let extracted = extract_typed(&root, file, &adapter)?;
            if !previous.contains_key(path.as_str())
                || resolution_identity_changed(&admitted, path, &extracted)?
                || (linker::needs_reference_identity(path, &extracted)
                    && reference_identity_changed(&admitted, path, &extracted)?)
            {
                changed_resolution.insert(path.clone());
            }
            facts.insert(path.clone(), extracted);
        } else {
            changed_resolution.insert(path.clone());
        }
    }
    let mut names = BTreeSet::new();
    let encoded = serde_json::to_string(&changed_resolution)?;
    {
        let mut st = admitted.prepare(
            "SELECT owner.path,n.name,n.qualname,n.kind FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE owner.path IN(SELECT value FROM json_each(?1))",
        )?;
        for row in st.query_map([&encoded], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (owner, name, qual, kind) = row?;
            names.insert(qual.clone());
            if !matches!(kind.as_str(), "field" | "method" | "route" | "document") {
                let mod_name = module_name(&owner);
                let (_, local_path) = crate::engine::languages::workspace_path(&owner);
                let local_module = module_name(local_path);
                let src_module = local_path
                    .split_once("/src/")
                    .map(|(_, r)| module_name(r))
                    .or_else(|| local_path.strip_prefix("src/").map(module_name));
                let is_top = qual == name
                    || qual == format!("{mod_name}.{name}")
                    || qual == format!("{local_module}.{name}")
                    || src_module
                        .as_ref()
                        .is_some_and(|m| qual == format!("{m}.{name}"));
                if is_top {
                    names.insert(name);
                }
            }
        }
    }
    for path in &changed_resolution {
        let mod_name = module_name(path);
        names.insert(mod_name.clone());
        let (_, local_path) = crate::engine::languages::workspace_path(path);
        let local_module = module_name(local_path);
        names.insert(local_module.clone());
        let src_module = local_path
            .split_once("/src/")
            .map(|(_, r)| module_name(r))
            .or_else(|| local_path.strip_prefix("src/").map(module_name));
        if let Some(src_mod) = &src_module {
            names.insert(src_mod.clone());
        }
        let mut stable_module = false;
        if let Some(f) = facts.get(path) {
            let previous_module: Option<(String, String, String)> = admitted
                .prepare_cached("SELECT n.id,n.qualname,n.language FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE owner.path=?1 AND n.kind='module'")?
                .query_row([path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            stable_module = previous_module
                .as_ref()
                .is_some_and(|(id, qualname, language)| {
                    f.nodes.iter().any(|n| {
                        n.kind == "module"
                            && &n.id == id
                            && &n.qualname == qualname
                            && &n.language == language
                    })
                });
            for n in &f.nodes {
                names.insert(n.qualname.clone());
                if !matches!(n.kind.as_str(), "field" | "method" | "route" | "document") {
                    let is_top = n.qualname == n.name
                        || n.qualname == format!("{mod_name}.{}", n.name)
                        || n.qualname == format!("{local_module}.{}", n.name)
                        || src_module
                            .as_ref()
                            .is_some_and(|m| n.qualname == format!("{m}.{}", n.name));
                    if is_top {
                        names.insert(n.name.clone());
                    }
                }
            }
        }
        if !stable_module {
            if let Some((_, last)) = local_module.rsplit_once('.') {
                names.insert(last.to_owned());
            }
        }
    }
    let extract_ms = extracting.elapsed().as_secs_f64() * 1000.;
    let hydrating = Instant::now();
    let mut affected = modified.clone();
    {
        if !changed_resolution.is_empty() {
            let mut st = admitted.prepare(
                "SELECT p.path FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.target_hash IN(SELECT path_hash FROM files WHERE path IN(SELECT value FROM json_each(?1)))",
            )?;
            for owner in st.query_map([serde_json::to_string(&changed_resolution)?], |r| {
                r.get::<_, String>(0)
            })? {
                affected.insert(owner?);
            }
        }
        #[cfg(feature = "lang-html")]
        {
            let html_targets: BTreeMap<_, Vec<_>> = modified
                .iter()
                .filter(|path| {
                    inventory
                        .get(path.as_str())
                        .or_else(|| previous.get(path.as_str()))
                        .is_some_and(|file| file.language == "html")
                })
                .fold(BTreeMap::new(), |mut targets, path| {
                    targets
                        .entry(stable_hash64(path))
                        .or_insert_with(Vec::new)
                        .push(path.as_str());
                    targets
                });
            if !html_targets.is_empty() {
                let target_hashes: Vec<_> = html_targets.keys().copied().collect();
                let mut st = admitted.prepare(
                    "SELECT p.path,d.symbol,d.target_hash FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.kind='renders' AND d.target_hash IN(SELECT value FROM json_each(?1))",
                )?;
                let family = crate::engine::languages::LanguageFamily("python");
                for row in st.query_map([serde_json::to_string(&target_hashes)?], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })? {
                    let (owner, symbol, hash) = row?;
                    let Some(target) =
                        crate::engine::languages::html::local_template_target(&owner, &symbol)
                    else {
                        continue;
                    };
                    if inventory
                        .get(owner.as_str())
                        .is_some_and(|file| file.language == "python")
                        && html_targets
                            .get(&hash)
                            .is_some_and(|paths| paths.contains(&target.as_str()))
                        && crate::engine::languages::workspace_path(&owner).0
                            == crate::engine::languages::workspace_path(&target).0
                        && dependencies.nearest_manifest_scope_for_path(family, &owner)
                            == dependencies.nearest_manifest_scope_for_path(family, &target)
                    {
                        affected.insert(owner);
                    }
                }
            }
        }
        let name_hashes: Vec<_> = names.iter().map(|name| stable_hash64(name)).collect();
        let encoded_names = serde_json::to_string(&names)?;
        let encoded_hashes = serde_json::to_string(&name_hashes)?;
        let mut st=admitted.prepare("SELECT p.path FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash LEFT JOIN nodes key_node ON typeof(k.key)='integer' AND key_node.node_id=k.key JOIN path_dictionary p ON p.path_id=s.owner_id WHERE s.kind_id=1 AND s.key_hash IN(SELECT value FROM json_each(?1)) AND (CASE WHEN typeof(k.key)='integer' THEN key_node.qualname ELSE k.key END) IN(SELECT value FROM json_each(?2))")?;
        for owner in st.query_map(params![encoded_hashes, encoded_names], |r| {
            r.get::<_, String>(0)
        })? {
            affected.insert(owner?);
        }
    }
    let mut import_frontier: BTreeSet<_> = changed_resolution
        .iter()
        .filter(|path| {
            inventory
                .get(path.as_str())
                .or_else(|| previous.get(path.as_str()))
                .is_some_and(|file| {
                    matches!(
                        file.language.as_str(),
                        "python" | "javascript" | "typescript" | "vue"
                    )
                })
        })
        .cloned()
        .collect();
    let mut visited_imports = import_frontier.clone();
    let mut importing_owners = admitted.prepare(
        "SELECT DISTINCT p.path FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.kind='imports' AND d.target_hash IN(SELECT path_hash FROM files WHERE path IN(SELECT value FROM json_each(?1)))",
    )?;
    while !import_frontier.is_empty() {
        let owners = importing_owners
            .query_map([serde_json::to_string(&import_frontier)?], |row| {
                row.get::<_, String>(0)
            })?;
        let mut next = BTreeSet::new();
        for owner in owners {
            let owner = owner?;
            if inventory.get(owner.as_str()).is_some_and(|file| {
                matches!(
                    file.language.as_str(),
                    "python" | "javascript" | "typescript" | "vue"
                )
            }) && visited_imports.insert(owner.clone())
            {
                affected.insert(owner.clone());
                next.insert(owner);
            }
        }
        import_frontier = next;
    }
    drop(importing_owners);
    let mut replace_files = modified.clone();
    let component_owners = delta_components(&admitted, &modified, &facts, &mut replace_files)?;
    affected.extend(replace_files.iter().cloned());
    let template_inputs_changed = modified.iter().any(|path| {
        inventory
            .get(path.as_str())
            .or_else(|| previous.get(path.as_str()))
            .is_some_and(|file| matches!(file.language.as_str(), "html" | "python"))
    });
    let old_loader_roots: Vec<(String, String)> = if template_inputs_changed {
        let mut st = admitted.prepare("SELECT p.path,n.name FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE p.path IN(SELECT value FROM json_each(?1)) AND n.kind='template_loader_root'")?;
        let rows = st.query_map([serde_json::to_string(&modified)?], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    } else {
        Vec::new()
    };
    if template_inputs_changed {
        let family = crate::engine::languages::LanguageFamily("python");
        let mut changed_roots = Vec::new();
        for (provider, directory) in &old_loader_roots {
            if modified.contains(provider) {
                changed_roots.push((provider.as_str(), directory.as_str()));
            }
        }
        for (provider, file_facts) in &facts {
            if !modified.contains(provider) || !inventory.contains_key(provider.as_str()) {
                continue;
            }
            for node in file_facts
                .nodes
                .iter()
                .filter(|node| node.kind == "template_loader_root")
            {
                changed_roots.push((provider.as_str(), node.name.as_str()));
            }
        }
        for (provider, directory) in changed_roots {
            let Some(scope) = dependencies.nearest_manifest_scope_for_path(family, provider) else {
                continue;
            };
            if !dependencies.declares_for_path(family, provider, "jinja2") {
                continue;
            }
            for (path, file) in &inventory {
                if file.language == "html"
                    && path
                        .strip_prefix(directory)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                    && dependencies.nearest_manifest_scope_for_path(family, path) == Some(scope)
                {
                    affected.insert((*path).to_owned());
                }
            }
        }
    }
    #[cfg(feature = "lang-html")]
    if template_inputs_changed {
        let family = crate::engine::languages::LanguageFamily("python");
        let modified_python: Vec<_> = modified
            .iter()
            .filter(|path| {
                inventory
                    .get(path.as_str())
                    .or_else(|| previous.get(path.as_str()))
                    .is_some_and(|file| file.language == "python")
            })
            .collect();
        if !modified_python.is_empty() {
            let mut old_targets = admitted.prepare(
                "SELECT DISTINCT f.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE d.kind='renders' AND owner.path IN(SELECT value FROM json_each(?1))",
            )?;
            for row in old_targets.query_map([serde_json::to_string(&modified_python)?], |row| {
                row.get::<_, String>(0)
            })? {
                let target = row?;
                if inventory
                    .get(target.as_str())
                    .is_some_and(|file| file.language == "html")
                {
                    affected.insert(target);
                }
            }
            for provider in modified_python {
                let Some(provider_facts) = facts.get(provider.as_str()) else {
                    continue;
                };
                if !dependencies.declares_for_path(family, provider, "django") {
                    continue;
                }
                let scope = dependencies.nearest_manifest_scope_for_path(family, provider);
                let workspace = crate::engine::languages::workspace_path(provider).0;
                for node in provider_facts
                    .nodes
                    .iter()
                    .filter(|node| node.language == "python")
                {
                    let Some(flow) = provider_facts.flows.get(node) else {
                        continue;
                    };
                    for context in &flow.template_contexts {
                        let Some(target) = crate::engine::languages::html::local_template_target(
                            provider,
                            &context.target,
                        ) else {
                            continue;
                        };
                        if inventory
                            .get(target.as_str())
                            .is_some_and(|file| file.language == "html")
                            && crate::engine::languages::workspace_path(&target).0 == workspace
                            && dependencies.nearest_manifest_scope_for_path(family, &target)
                                == scope
                        {
                            affected.insert(target);
                        }
                    }
                }
            }
        }
        let mut frontier: BTreeSet<_> = affected
            .iter()
            .filter(|path| {
                inventory
                    .get(path.as_str())
                    .is_some_and(|file| file.language == "html")
            })
            .cloned()
            .collect();
        let mut visited = frontier.clone();
        let mut old_includes = admitted.prepare(
            "SELECT owner.path,f.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE d.kind='includes' AND owner.path IN(SELECT value FROM json_each(?1))",
        )?;
        for _ in 0..16 {
            if frontier.is_empty() {
                break;
            }
            let mut next = BTreeSet::new();
            for row in old_includes.query_map([serde_json::to_string(&frontier)?], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })? {
                let (owner, target) = row?;
                if inventory
                    .get(target.as_str())
                    .is_some_and(|file| file.language == "html")
                    && crate::engine::languages::workspace_path(&owner).0
                        == crate::engine::languages::workspace_path(&target).0
                    && dependencies.nearest_manifest_scope_for_path(family, &owner)
                        == dependencies.nearest_manifest_scope_for_path(family, &target)
                    && visited.insert(target.clone())
                {
                    affected.insert(target.clone());
                    next.insert(target);
                }
            }
            for owner in &frontier {
                let Some(file_facts) = facts.get(owner.as_str()) else {
                    continue;
                };
                for reference in file_facts
                    .references
                    .iter()
                    .filter(|reference| reference.kind == "includes")
                {
                    let Some(target) = crate::engine::languages::html::local_template_target(
                        owner,
                        &reference.expression,
                    ) else {
                        continue;
                    };
                    if inventory
                        .get(target.as_str())
                        .is_some_and(|file| file.language == "html")
                        && crate::engine::languages::workspace_path(owner).0
                            == crate::engine::languages::workspace_path(&target).0
                        && dependencies.nearest_manifest_scope_for_path(family, owner)
                            == dependencies.nearest_manifest_scope_for_path(family, &target)
                        && visited.insert(target.clone())
                    {
                        affected.insert(target.clone());
                        next.insert(target);
                    }
                }
            }
            frontier = next;
        }
    }
    let mut load_owners: BTreeSet<_> = affected
        .difference(&modified)
        .filter(|path| inventory.contains_key(path.as_str()))
        .cloned()
        .collect();
    if template_inputs_changed {
        let mut roots = BTreeSet::new();
        for path in affected.iter().filter(|path| {
            inventory
                .get(path.as_str())
                .is_some_and(|file| file.language == "html")
        }) {
            let mut ancestor = path.rsplit_once('/').map(|(directory, _)| directory);
            while let Some(directory) = ancestor {
                roots.insert(directory.to_owned());
                ancestor = directory.rsplit_once('/').map(|(parent, _)| parent);
            }
        }
        if !roots.is_empty() {
            let mut st = admitted.prepare("SELECT DISTINCT p.path FROM nodes n INDEXED BY idx_nodes_name JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.name IN(SELECT value FROM json_each(?1)) AND n.kind='template_loader_root'")?;
            for row in st.query_map([serde_json::to_string(&roots)?], |row| {
                row.get::<_, String>(0)
            })? {
                let provider = row?;
                if inventory.contains_key(provider.as_str()) && !modified.contains(&provider) {
                    load_owners.insert(provider);
                }
            }
        }
    }
    #[cfg(feature = "lang-html")]
    if template_inputs_changed {
        let family = crate::engine::languages::LanguageFamily("javascript");
        let mut frontier: BTreeSet<_> = affected
            .iter()
            .filter(|path| {
                inventory
                    .get(path.as_str())
                    .is_some_and(|file| file.language == "html")
            })
            .cloned()
            .collect();
        let mut visited = frontier.clone();
        let mut incoming = admitted.prepare(
            "SELECT DISTINCT d.target_hash, owner.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id WHERE d.kind='includes' AND d.target_hash IN(SELECT value FROM json_each(?1))",
        )?;
        for _ in 0..16 {
            if frontier.is_empty() {
                break;
            }
            let mut targets = BTreeMap::<_, Vec<&str>>::new();
            for path in &frontier {
                targets.entry(stable_hash64(path)).or_default().push(path);
            }
            let hashes: Vec<_> = targets.keys().copied().collect();
            let mut next = BTreeSet::new();
            for row in incoming.query_map([serde_json::to_string(&hashes)?], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })? {
                let (hash, owner) = row?;
                let Some(owner_file) = inventory.get(owner.as_str()) else {
                    continue;
                };
                if owner_file.language != "html"
                    || !targets.get(&hash).is_some_and(|paths| {
                        paths.iter().any(|target| {
                            crate::engine::languages::workspace_path(&owner).0
                                == crate::engine::languages::workspace_path(target).0
                                && dependencies.nearest_manifest_scope_for_path(family, &owner)
                                    == dependencies.nearest_manifest_scope_for_path(family, target)
                        })
                    })
                {
                    continue;
                }
                if visited.insert(owner.clone()) {
                    if !modified.contains(&owner) {
                        load_owners.insert(owner.clone());
                    }
                    next.insert(owner);
                }
            }
            frontier = next;
        }
    }
    let mut loaded_fact_files = 0;
    {
        let mut st=admitted.prepare("SELECT path,facts_blob FROM local_facts WHERE path IN(SELECT value FROM json_each(?1))")?;
        for row in st.query_map([serde_json::to_string(&load_owners)?], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })? {
            let (path, blob) = row?;
            let f = decode_facts(&blob)?;
            facts.insert(path, f);
            loaded_fact_files += 1;
        }
    }
    {
        let catalog: Vec<_> = scan
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.language.as_str()))
            .collect();
        let mut st = admitted.prepare("SELECT facts_blob FROM local_facts WHERE path=?1")?;
        let mut attempted = BTreeSet::new();
        loop {
            let mut loaded = false;
            for path in linker::required_full_typed_facts(&facts, &affected, &catalog) {
                if facts.contains_key(&path) || !attempted.insert(path.clone()) {
                    continue;
                }
                if let Some(raw) = st
                    .query_row([&path], |row| row.get::<_, Vec<u8>>(0))
                    .optional()?
                {
                    facts.insert(path, decode_facts(&raw)?);
                    loaded_fact_files += 1;
                    loaded = true;
                }
            }
            if !loaded {
                break;
            }
        }
    }
    assign_components(&mut facts, &component_owners);
    let mut tokens = BTreeSet::new();
    let mut doc_symbols = BTreeSet::new();
    for f in facts.values() {
        tokens.extend(reference_keys(f).into_iter().map(str::to_owned));
        doc_symbols.extend(
            f.docs
                .iter()
                .flat_map(|doc| doc.referenced_symbols.iter())
                .map(|symbol| symbol.replace("::", "."))
                .filter(|symbol| !symbol.is_empty()),
        );
    }
    // Cold linking discovers documentation targets by any qualified-name suffix.
    // Reuse the existing FTS index to narrow delta candidates before checking
    // the exact dot-boundary suffix. Symbols outside its ASCII token contract
    // retain the full scan so their links cannot silently disappear.
    let mut suffix_ids = BTreeSet::new();
    if !doc_symbols.is_empty() {
        let mut terms = BTreeSet::new();
        let mut fallback_symbols = BTreeSet::new();
        for symbol in &doc_symbols {
            let token = symbol
                .is_ascii()
                .then(|| symbol.rsplit(|c: char| !c.is_ascii_alphanumeric()).next())
                .flatten()
                .filter(|token| !token.is_empty() && token.len() <= 128);
            if let Some(token) = token {
                terms.insert(token.to_owned());
            } else {
                fallback_symbols.insert(symbol.as_str());
            }
        }
        let mut st = admitted.prepare("SELECT n.id,n.qualname FROM node_search s JOIN nodes n ON n.node_id=s.rowid WHERE node_search MATCH ?1")?;
        for chunk in terms.into_iter().collect::<Vec<_>>().chunks(32) {
            let query = chunk
                .iter()
                .map(|term| format!("\"{term}\""))
                .collect::<Vec<_>>()
                .join(" OR ");
            for row in st.query_map([query], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (id, qualname) = row?;
                if qualname
                    .match_indices('.')
                    .any(|(at, _)| doc_symbols.contains(&qualname[at + 1..]))
                {
                    suffix_ids.insert(id);
                }
            }
        }
        if !fallback_symbols.is_empty() {
            let mut st =
                admitted.prepare("SELECT id,qualname FROM nodes WHERE instr(qualname,'.')>0")?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (id, qualname) = row?;
                if qualname
                    .match_indices('.')
                    .any(|(at, _)| fallback_symbols.contains(&qualname[at + 1..]))
                {
                    suffix_ids.insert(id);
                }
            }
        }
    }
    let encoded = serde_json::to_string(&tokens)?;
    let suffix_ids = serde_json::to_string(&suffix_ids)?;
    {
        let sql="SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line,n.end_line,n.is_test,n.language,n.generated,n.details,EXISTS(SELECT 1 FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash WHERE s.kind_id=2 AND s.key_hash=n.node_hash AND k.key=n.id) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.id IN(SELECT id FROM nodes WHERE kind='module' UNION SELECT id FROM nodes WHERE name IN(SELECT value FROM json_each(?1)) UNION SELECT id FROM nodes WHERE qualname IN(SELECT value FROM json_each(?1)) UNION SELECT n2.id FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash JOIN nodes n2 ON n2.node_hash=s.key_hash AND n2.id=k.key WHERE s.kind_id=2 UNION SELECT value FROM json_each(?2))";
        let mut st = admitted.prepare(sql)?;
        let nodes = st.query_map(params![encoded, suffix_ids], |r| {
            Ok(Node {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                qualname: r.get(3)?,
                path: r.get(4)?,
                line: r.get(5)?,
                end_line: r.get(6)?,
                is_test: r.get(7)?,
                language: r.get(8)?,
                generated: r.get(9)?,
                details: serde_json::from_str(&r.get::<_, String>(10)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
            })
        })?;
        let complete: BTreeSet<_> = facts.keys().cloned().collect();
        for node in nodes {
            let node = node?;
            if complete.contains(&node.path) || modified.contains(&node.path) {
                continue;
            }
            facts
                .entry(node.path.clone())
                .or_insert_with(TypedFacts::default)
                .push_node(node);
        }
    }
    let hydrate_ms = hydrating.elapsed().as_secs_f64() * 1000.;
    let linking = Instant::now();
    let graph = linker::link_compact_with_typed_registry(
        &facts,
        Some(&affected),
        Some(&root),
        &dependencies,
    );
    let link_ms = linking.elapsed().as_secs_f64() * 1000.;
    let persisting = Instant::now();
    drop(admitted);
    if crate::db::cache::identity(db)? != admitted_identity {
        bail!("database changed during delta admission");
    }
    let mut conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.set_prepared_statement_cache_capacity(128);
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-64000; PRAGMA mmap_size=268435456;",
    )?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: String = tx.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    if current != expected {
        bail!("generation changed during delta admission");
    }
    let deleting = Instant::now();
    let mut path_cache = PathDictionaryCache::new();
    path_cache.load_from_db(&tx)?;
    tx.execute_batch("CREATE TEMP TABLE changed_edge_keys(src_hash INTEGER,dst_hash INTEGER,kind TEXT,PRIMARY KEY(src_hash,dst_hash,kind));")?;
    for path in &affected {
        let owner_id = path_cache.get_or_insert(&tx, path)?;
        cached(&tx,"INSERT OR IGNORE INTO changed_edge_keys SELECT src_hash,dst_hash,kind FROM edge_occurrences WHERE owner_id=?1",[owner_id])?;
    }
    let mut changed_edge_batch = MultiValueBatch::<3, BorrowedSqlValue<'_>>::new(
        &tx,
        "INSERT OR IGNORE INTO changed_edge_keys VALUES",
    )?;
    for e in &graph.edges {
        changed_edge_batch.push([
            BorrowedSqlValue::Integer(stable_hash64(&e.src)),
            BorrowedSqlValue::Integer(stable_hash64(&e.dst)),
            BorrowedSqlValue::Text(e.kind.as_str()),
        ])?;
    }
    changed_edge_batch.flush()?;
    drop(changed_edge_batch);
    let mut commitment_owners = affected.clone();
    commitment_owners.extend(
        tx.prepare_cached("SELECT DISTINCT p.path FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE (e.src_hash,e.dst_hash,e.kind)IN(SELECT src_hash,dst_hash,kind FROM changed_edge_keys)")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    tx.execute("DELETE FROM edges WHERE (src_hash,dst_hash,kind)IN(SELECT src_hash,dst_hash,kind FROM changed_edge_keys)",[])?;
    for path in &affected {
        let owner_id = path_cache.get_or_insert(&tx, path)?;
        for (table, column) in [
            ("edge_occurrences", "owner_id"),
            ("dependencies", "owner_id"),
            ("resolution_coverage", "path_id"),
            ("coverage_owner_language", "path_id"),
            ("coverage_language_counts", "path_id"),
        ] {
            cached(
                &tx,
                &format!("DELETE FROM {table} WHERE {column}=?1"),
                [owner_id],
            )?;
        }
    }
    tx.execute_batch(
        "CREATE TEMP TABLE replaced_nodes(id TEXT PRIMARY KEY,node_hash INTEGER NOT NULL)",
    )?;
    tx.execute("INSERT OR IGNORE INTO replaced_nodes SELECT n.id,n.node_hash FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE owner.path IN(SELECT value FROM json_each(?1))", [serde_json::to_string(&replace_files)?])?;
    for path in &replace_files {
        delete_file(&tx, &mut path_cache, path)?;
    }
    let delete_ms = deleting.elapsed().as_secs_f64() * 1000.;
    let changed_entries: Vec<_> = scan
        .entries
        .iter()
        .filter(|f| replace_files.contains(&f.path))
        .cloned()
        .collect();
    let mut node_paths: hashbrown::HashMap<String, Option<CachedNodePath>> =
        hashbrown::HashMap::with_capacity(facts.values().map(|f| f.nodes.len()).sum());
    for f in facts.values() {
        for n in &f.nodes {
            node_paths.insert(
                n.id.clone(),
                Some(CachedNodePath::new(&n.id, n.path.clone())),
            );
        }
    }
    let files_writing = Instant::now();
    persist_files(&tx, &mut path_cache, &changed_entries, &facts, None, false)?;
    let files_ms = files_writing.elapsed().as_secs_f64() * 1000.;
    let graph_writing = Instant::now();
    persist_graph(
        &tx,
        &mut path_cache,
        &graph,
        &facts,
        Some(&affected),
        &mut node_paths,
        false,
    )?;
    tx.execute(
        "DELETE FROM coverage_evidence WHERE evidence_id NOT IN(SELECT evidence_id FROM resolution_coverage UNION SELECT evidence_id FROM edges UNION SELECT confidence_id FROM edges UNION SELECT evidence_id FROM edge_occurrences UNION SELECT confidence_id FROM edge_occurrences)",
        [],
    )?;
    let graph_ms = graph_writing.elapsed().as_secs_f64() * 1000.;
    let aggregating = Instant::now();
    let dangling: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM replaced_nodes r WHERE NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=r.id AND n.node_hash=r.node_hash) AND EXISTS(SELECT 1 FROM edges e WHERE e.src_hash=r.node_hash OR e.dst_hash=r.node_hash))", [], |r| r.get(0))?;
    if dangling {
        bail!("delta leaves an edge with a missing endpoint; rebuild index");
    }
    tx.execute(
        "WITH ranked AS (
            SELECT e.*,e.owner_id AS path_id,p.path AS owner_path,
                   row_number() OVER edge_group AS representative,
                   count(*) OVER edge_group AS occurrences
            FROM changed_edge_keys k JOIN edge_occurrences e
              ON k.src_hash=e.src_hash AND k.dst_hash=e.dst_hash AND k.kind=e.kind
            JOIN path_dictionary p ON p.path_id=e.owner_id
            WINDOW edge_group AS (
                PARTITION BY e.src_hash,e.dst_hash,e.kind ORDER BY p.path,e.ordinal
                ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
            )
        )
        INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count)
        SELECT src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrences
        FROM ranked WHERE representative=1",
        [],
    )?;
    commitment_owners.extend(
        tx.prepare_cached("SELECT DISTINCT p.path FROM edges e JOIN path_dictionary p ON p.path_id=e.path_id WHERE (e.src_hash,e.dst_hash,e.kind)IN(SELECT src_hash,dst_hash,kind FROM changed_edge_keys)")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    let aggregate_ms = aggregating.elapsed().as_secs_f64() * 1000.;
    let commitments_writing = Instant::now();
    let commitments_ms = commitments_writing.elapsed().as_secs_f64() * 1000.;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('corpus_hash',?1)",
        [commitments::hash(&serde_json::to_vec(&scan.entries)?)],
    )?;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('inventory_snapshot',?1)",
        [serde_json::to_string(&scan.entries)?],
    )?;
    cached(
        &tx,
        "INSERT OR REPLACE INTO metadata VALUES('manifest_digest',?1)",
        [dependencies.digest()],
    )?;
    let persist_ms = persisting.elapsed().as_secs_f64() * 1000.;
    let sealing = Instant::now();
    let seal = commitments::seal_owners(&tx, Some(&commitment_owners))?;
    let seal_ms = sealing.elapsed().as_secs_f64() * 1000.;
    let verifying = Instant::now();
    commitments::verify_owners(&tx, Some(&commitment_owners))?;
    let latest = scanner::scan_reusing(&root, &adapter, &scan.entries)?;
    if latest.entries != scan.entries {
        return Err(SourceSnapshotMismatch(format!(
            "workspace changed during delta; transaction rolled back (scan={scan_ms:.0}ms extract={extract_ms:.0}ms hydrate={hydrate_ms:.0}ms link={link_ms:.0}ms persist={persist_ms:.0}ms seal={seal_ms:.0}ms verify={:.0}ms)",
            verifying.elapsed().as_secs_f64() * 1000.
        ))
        .into());
    }
    let verify_ms = verifying.elapsed().as_secs_f64() * 1000.;
    let committing = Instant::now();
    tx.commit()?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    let commit_ms = committing.elapsed().as_secs_f64() * 1000.;
    drop(conn);
    let cached = crate::db::cache::publish_verified(db, &root, &seal).unwrap_or(false);
    Ok(
        json!({"files":scan.files,"changed_files":modified.len(),"affected_owners":affected.len(),"reparsed_files":modified.iter().filter(|p|inventory.contains_key(p.as_str())).count(),"loaded_fact_files":loaded_fact_files,"rewritten_files":changed_entries.len(),"verification_cache":cached,"output_root":seal,"scan_ms":scan_ms,"extract_ms":extract_ms,"hydrate_ms":hydrate_ms,"link_ms":link_ms,"persist_ms":persist_ms,"delete_ms":delete_ms,"files_ms":files_ms,"graph_ms":graph_ms,"aggregate_ms":aggregate_ms,"commitments_ms":commitments_ms,"seal_ms":seal_ms,"verify_ms":verify_ms,"commit_ms":commit_ms,"elapsed_ms":started.elapsed().as_secs_f64()*1000.}),
    )
}

fn resolution_identity_changed(conn: &Connection, path: &str, facts: &TypedFacts) -> Result<bool> {
    fn identity<'a>(
        node: &'a Node,
        contracts: &crate::engine::linker::contracts::Contracts<'a>,
    ) -> Result<String> {
        let mut value = serde_json::to_value(node)?;
        let object = value
            .as_object_mut()
            .context("node identity must be an object")?;
        object.remove("line");
        object.remove("end_line");
        object.insert("details".into(), contracts.details(node));
        Ok(serde_json::to_string(&value)?)
    }
    let old_blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get(0),
        )
        .optional()?;
    let Some(old_blob) = old_blob else {
        return Ok(true);
    };
    let old_facts = decode_facts(&old_blob)?;
    let old_contracts = crate::engine::linker::contracts::Contracts::new_typed(&old_facts);
    let new_contracts = crate::engine::linker::contracts::Contracts::new_typed(facts);
    let old = old_facts
        .nodes
        .iter()
        .filter(|node| node.kind != "component")
        .map(|node| identity(node, &old_contracts))
        .collect::<Result<BTreeSet<_>>>()?;
    let new = facts
        .nodes
        .iter()
        .filter(|node| node.kind != "component")
        .map(|node| identity(node, &new_contracts))
        .collect::<Result<BTreeSet<_>>>()?;
    Ok(old != new)
}
fn reference_identity_changed(conn: &Connection, path: &str, facts: &TypedFacts) -> Result<bool> {
    let old: Option<Vec<u8>> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    let Some(old) = old else {
        return Ok(true);
    };
    let old = decode_facts(&old)?;
    Ok(linker::reference_identity_changed(path, &old, facts))
}
fn delete_file(tx: &Connection, path_cache: &mut PathDictionaryCache, path: &str) -> Result<()> {
    let owner_id = path_cache.get_or_insert(tx, path)?;
    cached(
        tx,
        "DELETE FROM node_search WHERE rowid IN(SELECT node_id FROM nodes WHERE owner_path_id=?1)",
        [owner_id],
    )?;
    cached(tx,"INSERT INTO doc_search(doc_search,rowid,section_title,content,invariants)SELECT 'delete',rowid,section_title,content,invariants FROM doc_sections WHERE path=?1",[path])?;
    cached(
        tx,
        "UPDATE shared_keys SET key=(SELECT qualname FROM nodes WHERE node_id=shared_keys.key) WHERE typeof(key)='integer' AND key IN(SELECT node_id FROM nodes WHERE owner_path_id=?1)",
        [owner_id],
    )?;
    cached(tx, "DELETE FROM nodes WHERE owner_path_id=?1", [owner_id])?;
    for (table, column) in [
        ("source_inventory", "path"),
        ("files", "path"),
        ("local_facts", "path"),
        ("shared_owners", "owner_id"),
        ("errors", "path"),
        ("doc_sections", "path"),
    ] {
        let value = if column == "owner_id" {
            rusqlite::types::Value::Integer(owner_id)
        } else {
            rusqlite::types::Value::Text(path.to_owned())
        };
        cached(
            tx,
            &format!("DELETE FROM {table} WHERE {column}=?1"),
            [value],
        )?;
    }
    Ok(())
}
