use crate::core::models::*;
use crate::engine::languages::{self, module_name, LanguageFamily, LanguageProfile};
use hashbrown::HashMap;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;
pub fn link(all: &BTreeMap<String, Facts>) -> Graph {
    link_with_root(all, None, None)
}
pub fn link_owners(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
) -> Graph {
    link_with_root(all, owners, None)
}
pub fn link_with_root(
    all: &BTreeMap<String, Facts>,
    owners: Option<&std::collections::BTreeSet<String>>,
    root: Option<&Path>,
) -> Graph {
    let nodes: Vec<&Node> = all.values().flat_map(|f| f.nodes.iter()).collect();
    let mut by_name: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
    let mut by_qual: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(nodes.len());
    let mut by_suffix: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut by_id: HashMap<&str, &Node> = HashMap::with_capacity(nodes.len());
    let mut by_module: HashMap<&str, Vec<&Node>> = HashMap::with_capacity(all.len());
    let needs_discovery = all.iter().any(|(path, facts)| {
        owners.is_none_or(|o| o.contains(path))
            && facts
                .docs
                .iter()
                .any(|doc| !doc.referenced_symbols.is_empty())
    });
    for n in &nodes {
        by_name.entry(&n.name).or_default().push(n);
        by_qual.entry(&n.qualname).or_default().push(n);
        by_id.insert(&n.id, n);
        by_module.entry(&n.path).or_default().push(n);
        if needs_discovery {
            for (i, c) in n.qualname.char_indices() {
                if c == '.' {
                    by_suffix.entry(&n.qualname[i + 1..]).or_default().push(n);
                }
            }
        }
    }
    let mut node_bindings: HashMap<&str, hashbrown::HashSet<&str>> = HashMap::new();
    let mut node_rebindings: HashMap<&str, hashbrown::HashSet<&str>> = HashMap::new();
    for n in &nodes {
        if let Some(arr) = n.details["bindings"].as_array() {
            if !arr.is_empty() {
                let set: hashbrown::HashSet<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                node_bindings.insert(&n.id, set);
            }
        }
        if let Some(arr) = n.details["rebindings"].as_array() {
            if !arr.is_empty() {
                let set: hashbrown::HashSet<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                node_rebindings.insert(&n.id, set);
            }
        }
    }
    let discovery_lookup = |name: &str| -> &[&Node] {
        by_qual
            .get(name)
            .or_else(|| by_suffix.get(name))
            .map(Vec::as_slice)
            .unwrap_or_default()
    };
    let lookup =
        |name: &str| -> &[&Node] { by_qual.get(name).map(Vec::as_slice).unwrap_or_default() };
    let profiles: HashMap<&str, &dyn LanguageProfile> = all
        .iter()
        .filter_map(|(path, f)| {
            f.nodes
                .iter()
                .find(|n| n.kind == "module")
                .and_then(|n| languages::by_id(&n.language))
                .map(|p| (path.as_str(), p))
        })
        .collect();
    let mut modules_by_namespace: HashMap<LanguageFamily, HashMap<String, Vec<&Node>>> =
        HashMap::new();
    for node in nodes.iter().filter(|n| n.kind == "module") {
        if let Some(profile) = profiles.get(node.path.as_str()) {
            let (ws, local_path) = languages::workspace_path(&node.path);
            let local_module = profile.module_name(local_path);
            let mut namespaces = vec![node.qualname.clone()];
            if !ws.is_empty() && node.qualname.starts_with(&format!("{ws}.")) {
                namespaces.push(node.qualname[ws.len() + 1..].to_string());
            }
            if !namespaces.contains(&local_module) {
                namespaces.push(local_module);
            }
            for ns in namespaces {
                let list = modules_by_namespace
                    .entry(profile.family())
                    .or_default()
                    .entry(ns)
                    .or_default();
                if !list.iter().any(|existing| existing.id == node.id) {
                    list.push(node);
                }
            }
        }
    }
    let parts: Vec<Graph> = all
        .par_iter()
        .filter(|(path, _)| owners.is_none_or(|o| o.contains(*path)))
        .map(|(path, facts)| {
            let mut graph = Graph {
                edges: facts.edges.clone(),
                coverage: Vec::with_capacity(facts.references.len()),
            };
            let profile = profiles.get(path.as_str()).copied();
            let file_module = facts
                .nodes
                .iter()
                .find(|n| n.kind == "module")
                .map(|n| n.qualname.clone())
                .unwrap_or_else(|| profile.map_or_else(|| module_name(path), |p| p.module_name(path)));
            let mut aliases: HashMap<String, HashMap<String, Vec<&Node>>> = HashMap::new();
            let mut lookup_key = String::new();
            for r in facts.references.iter().filter(|r| r.kind == "imports") {
                let normalized = profile.and_then(|p| {
                    p.normalize_import_with_root(
                        root,
                        path,
                        r.module.as_deref().unwrap_or(&r.expression),
                    )
                });
                let mut modules=Vec::new();
                let mut selected_namespace="";
                if let (Some(profile),Some(normalized))=(profile,&normalized) {
                    let workspace=languages::workspace_path(path).0;
                    for owner_workspace in [true,false] {
                        if !owner_workspace && normalized.relative { break; }
                        let mut namespace=normalized.namespace.as_str();
                        loop {
                            if owner_workspace || namespace.contains('.') {
                                modules=modules_by_namespace.get(&profile.family()).and_then(|namespaces|namespaces.get(namespace))
                                    .into_iter().flatten().copied()
                                    .filter(|n|(languages::workspace_path(&n.path).0==workspace)==owner_workspace).collect();
                            }
                            if !modules.is_empty(){selected_namespace=namespace;break;}
                            if !normalized.symbol_path {break;}
                            match namespace.rsplit_once('.') {Some((parent,_))=>namespace=parent,None=>break}
                        }
                        if !modules.is_empty(){break;}
                    }
                }
                let symbol=normalized.as_ref().map_or_else(||r.expression.clone(),|n|n.namespace.clone());
                let mut candidates=Vec::new();
                if let ([module],Some(normalized))=(modules.as_slice(),&normalized) {
                    let tail=if normalized.symbol_path {normalized.namespace.strip_prefix(selected_namespace).unwrap_or("").trim_start_matches('.')}else if r.expression=="*" || r.module.as_deref()==Some(&r.expression) {""}else{&r.expression};
                    if tail.is_empty(){candidates.push(*module);}else{
                        let target = qualified(&mut lookup_key, &module.qualname, tail);
                        candidates=by_module.get(module.path.as_str()).into_iter().flatten().copied().filter(|n| if tail=="default" {n.details["default_export"]==true}else{n.kind!="component" && n.qualname==target}).collect();
                    }
                }
                candidates.sort_by(|a,b|a.id.cmp(&b.id));
                candidates.dedup_by_key(|n|&n.id);
                if let Some(alias)=&r.alias {
                    let scope=by_id.get(r.source.as_str()).map(|n|n.qualname.clone()).unwrap_or_else(||file_module.clone());
                    let local_declaration=by_qual.get(qualified(&mut lookup_key, &scope, alias)).is_some_and(|nodes|nodes.iter().any(|n|n.path==*path));
                    let targets=if local_declaration {Vec::new()}else{candidates.clone()};
                    aliases.entry(scope).or_default().entry(alias.clone()).or_default().extend(targets);
                }
                let resolved=modules.len()==1 && (r.alias.is_none() || candidates.len()==1);
                let status=if resolved {"resolved"}else if modules.len()>1 || candidates.len()>1 {"ambiguous"}else{"unresolved"};
                let evidence=format!("import {symbol}: {} modules, {} alias targets",modules.len(),candidates.len());
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: r.line,
                    expression: r.expression.clone(),
                    status: status.into(),
                    evidence,
                });
                if modules.len() == 1 {
                    graph.edges.push(Edge {
                        src: r.source.clone(),
                        dst: modules[0].id.clone(),
                        kind: "imports".into(),
                        path: path.clone(),
                        line: r.line,
                        evidence: r.expression.clone(),
                        confidence: "exact".into(),
                    });
                }
            }
            let alias_for = |name: &str, owner: Option<&Node>, definition_scope: bool| {
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                if definition_scope { scope = scope.rsplit_once('.').map_or("", |(p, _)| p); }
                loop {
                    let class_scope = !profile.is_some_and(|p|p.class_scope()) && !definition_scope && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                        && by_qual.get(scope).is_some_and(|nodes| nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl")));
                    if !class_scope {
                        if let Some(targets) = aliases.get(scope).and_then(|a| a.get(name)) { return Some(targets); }
                    }
                    if scope.is_empty() { return None; }
                    scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                }
            };
            let mut candidates = Vec::new();
            for r in facts.references.iter().filter(|r| {
                matches!(
                    r.kind.as_str(),
                    "calls" | "inherits" | "implements" | "bases" | "decorates" | "mutates" | "handles" | "references"
                )
            }) {
                if r.dynamic {
                    graph.coverage.push(Coverage {
                        path: path.clone(), line: r.line, expression: r.expression.clone(),
                        status: "unresolved".into(),
                        evidence: "computed receiver or dynamic callee; callable identity is unknown".into(),
                    });
                    continue;
                }
                let expression = r.expression.replace("::", ".");
                let (first, tail) = expression.split_once('.').unwrap_or((&expression, ""));
                let owner = by_id.get(r.source.as_str()).copied();
                let mut scope = owner.map(|n| n.qualname.as_str()).unwrap_or("");
                let definition_scope = matches!(r.kind.as_str(), "decorates" | "inherits" | "implements" | "bases");
                if definition_scope { scope = scope.rsplit_once('.').map_or("", |(p, _)| p); }
                let mut shadowed = false;
                let imported = alias_for(first, owner, definition_scope);
                candidates.clear();
                while !scope.is_empty() {
                    if let Some(scope_nodes) = by_qual.get(scope) {
                        if !profile.is_some_and(|p|p.class_scope()) && !definition_scope && owner.is_some_and(|n| matches!(n.kind.as_str(), "function" | "method"))
                            && scope_nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl")) {
                            scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                            continue;
                        }
                        if profile.is_none_or(|p|p.value_binding_applies(r)) && scope_nodes.iter().filter(|n|n.path==*path).any(|n| {
                            node_bindings.get(n.id.as_str()).is_some_and(|b| b.contains(first))
                        }) {
                            shadowed = true;
                            break;
                        }
                    }
                    if aliases.get(scope).is_some_and(|a| a.contains_key(first)) { break; }
                    if tail.is_empty() {
                        candidates.extend(lookup(qualified(&mut lookup_key, scope, &expression)).iter().copied().filter(|n|n.path==*path));
                        if !candidates.is_empty() {
                            break;
                        }
                    }
                    scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
                }
                let known_receiver = !tail.is_empty()
                    && owner.is_some_and(|n|profile.is_some_and(|p|p.receiver(first,n)))
                    && owner.is_some_and(|n| {
                        if node_rebindings.get(n.id.as_str()).is_some_and(|b| b.contains(first)) {
                            return false;
                        }
                        n.qualname.rsplit_once('.').is_some_and(|(scope, _)| {
                            by_qual.get(scope).is_some_and(|nodes| {
                                nodes.iter().any(|n| n.path==*path && matches!(n.kind.as_str(), "class" | "impl"))
                            })
                        })
                    });
                if shadowed
                    && !known_receiver
                    && matches!(r.kind.as_str(), "calls" | "handles" | "mutates")
                {
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: r.line,
                        expression: r.expression.clone(),
                        status: "unresolved".into(),
                        evidence:
                            "callee is shadowed by a parameter or local binding of unknown callable identity"
                                .into(),
                    });
                    continue;
                }
                if candidates.is_empty() {
                    if let Some(imported) = imported {
                        if tail.is_empty() {
                            candidates.extend_from_slice(imported);
                        } else {
                            for n in imported {
                                candidates.extend(lookup(qualified(&mut lookup_key, &n.qualname, tail)).iter().copied().filter(|target|target.path==n.path));
                            }
                        }
                    }
                }
                if candidates.is_empty() && imported.is_none() {
                    if !tail.is_empty() {
                        candidates.extend_from_slice(lookup(&expression));
                    }
                    if candidates.is_empty() {
                        if tail.is_empty() {
                            candidates.extend_from_slice(lookup(qualified(&mut lookup_key, &file_module, &expression)));
                        } else if known_receiver {
                            if let Some(owner) = owner.filter(|_| known_receiver) {
                                let scope = owner.qualname.rsplit_once('.').map_or("", |(p, _)| p);
                                candidates.extend_from_slice(lookup(qualified(&mut lookup_key, scope, tail)));
                            }
                        } else {
                            candidates.extend_from_slice(lookup(qualified(&mut lookup_key, &file_module, &expression)));
                        }
                    }
                }
                if imported.is_none() {
                    candidates.retain(|n| {
                        n.path == *path
                            || (profile.is_some_and(|p| p.sibling_accessible(path, &n.path))
                                && by_module
                                    .get(n.path.as_str())
                                    .into_iter()
                                    .flatten()
                                    .any(|m| m.kind == "module" && m.qualname == file_module))
                            || (r.kind == "references" && n.qualname == expression)
                    });
                }
                candidates.retain(|n| match r.kind.as_str() {
                    "inherits" | "bases" => matches!(n.kind.as_str(), "class" | "interface" | "struct"),
                    "implements" => matches!(n.kind.as_str(), "trait" | "interface"),
                    "mutates" => n.kind == "field",
                    "references" => matches!(n.kind.as_str(), "class" | "enum" | "type" | "struct"),
                    _ => matches!(
                        n.kind.as_str(),
                        "function" | "method" | "class" | "struct" | "enum" | "macro"
                    ),
                });
                candidates.sort_by(|a, b| a.id.cmp(&b.id));
                candidates.dedup_by_key(|n| &n.id);
                let mut source = Some(r.source.clone());
                let mut source_heuristic = false;
                if r.kind == "implements" {
                    if let Some(implementation) = owner.filter(|n| n.kind == "impl") {
                        let clean = implementation.name.split('<').next().unwrap_or(&implementation.name).trim().replace("::", ".");
                        let (head, rest) = clean.split_once('.').unwrap_or((&clean, ""));
                        let mut types: Vec<&Node> = if let Some(imported) = alias_for(head, owner, true) {
                            if rest.is_empty() { imported.clone() } else {
                                imported.iter().flat_map(|n| lookup(&format!("{}.{}", n.qualname, rest)).iter().copied().filter(move |target|target.path==n.path)).collect()
                            }
                        } else {
                            lookup(qualified(&mut lookup_key, &file_module, &clean)).to_vec()
                        };
                        types.retain(|n| matches!(n.kind.as_str(), "struct" | "enum"));
                        if types.is_empty() && alias_for(head, owner, true).is_none() {
                            types = by_name.get(clean.as_str()).into_iter().flatten().copied()
                                .filter(|n| matches!(n.kind.as_str(), "struct" | "enum")).collect();
                            source_heuristic = !types.is_empty();
                        }
                        source = (types.len() == 1).then(|| types[0].id.clone());
                    }
                }
                let is_builtin = candidates.is_empty()
                    && !shadowed && tail.is_empty() && imported.is_none()
                    && !by_module.get(path.as_str()).into_iter().flatten().any(|n| n.name == first)
                    && profile.is_some_and(|p|p.builtin(&expression));
                let status = if (candidates.len() == 1 || is_builtin) && source.is_some() && !source_heuristic {
                    "resolved"
                } else if candidates.is_empty() || source.is_none() || source_heuristic {
                    "unresolved"
                } else {
                    "ambiguous"
                };
                let evidence = if is_builtin {
                    format!("standard library or built-in callee: {expression}")
                } else {
                    format!("{} lexically justified candidates; implementor resolved={}, heuristic={source_heuristic}", candidates.len(), source.is_some())
                };
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: r.line,
                    expression: r.expression.clone(),
                    status: status.into(),
                    evidence,
                });
                if let (Some(source), [candidate]) = (source, candidates.as_slice()) {
                    let target = candidate.id.clone();
                    let (src, dst) = if r.kind == "decorates" {
                        (target, source)
                    } else {
                        (source, target)
                    };
                    graph.edges.push(Edge {
                        src,
                        dst,
                        kind: if r.kind=="bases" { if candidate.kind=="interface" && owner.is_some_and(|n|n.kind!="interface") {"implements"}else{"inherits"}.into() }else{r.kind.clone()},
                        path: path.clone(),
                        line: r.line,
                        evidence: r.expression.clone(),
                        confidence: if source_heuristic { "heuristic" } else { "exact" }.into(),
                    });
                }
            }
            for doc in &facts.docs {
                for symbol in &doc.referenced_symbols {
                    let normalized = symbol.replace("::", ".");
                    let mut candidates = discovery_lookup(&normalized);
                    if candidates.is_empty() {
                        candidates = by_name.get(normalized.as_str()).map(Vec::as_slice).unwrap_or_default();
                    }
                    for node in candidates {
                        for (src, dst, kind) in [
                            (&doc.doc_id, &node.id, "documents"),
                            (&node.id, &doc.doc_id, "references_doc"),
                        ] {
                            graph.edges.push(Edge {
                                src: src.clone(),
                                dst: dst.clone(),
                                kind: kind.into(),
                                path: path.clone(),
                                line: 1,
                                evidence: symbol.clone(),
                                confidence: "exact".into(),
                            });
                        }
                    }
                }
            }
            graph
        })
        .collect();
    let mut result = Graph {
        edges: Vec::with_capacity(parts.iter().map(|g| g.edges.len()).sum()),
        coverage: Vec::with_capacity(parts.iter().map(|g| g.coverage.len()).sum()),
    };
    for g in parts {
        result.edges.extend(g.edges);
        result.coverage.extend(g.coverage);
    }
    result.edges.sort_by(|a, b| {
        (&a.path, a.line, &a.src, &a.dst, &a.kind).cmp(&(&b.path, b.line, &b.src, &b.dst, &b.kind))
    });
    result
        .coverage
        .sort_by(|a, b| (&a.path, a.line, &a.expression).cmp(&(&b.path, b.line, &b.expression)));
    result
}

fn qualified<'a>(buffer: &'a mut String, namespace: &str, name: &str) -> &'a str {
    buffer.clear();
    buffer.push_str(namespace);
    buffer.push('.');
    buffer.push_str(name);
    buffer
}
