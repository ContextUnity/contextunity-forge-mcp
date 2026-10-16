use super::routes::prefixed_path;
use crate::core::models::{Edge, Node};
use crate::core::typed_facts::TypedFacts;
use crate::engine::languages;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Materializes Django include prefixes from extracted route facts without rereading source.
pub(crate) fn compose_django_route_includes(
    all: &mut BTreeMap<String, TypedFacts>,
) -> BTreeSet<String> {
    const MAX_INCLUDE_DEPTH: usize = 16;
    const MAX_COMPOSED_ROUTES: usize = 10_000;

    let mut removed: HashSet<String> = all
        .values()
        .flat_map(|typed| {
            typed
                .facts
                .nodes
                .iter()
                .filter(|node| node.details["included_from"].is_string())
                .map(|node| node.id.clone())
        })
        .collect();
    // Generated handles live in the original route's facts bucket, so a delta may
    // load those references without loading their old parent route owner. Capture
    // those foreign source IDs before removing generated nodes, then rebuild them
    // from the canonical source route below.
    removed.extend(
        all.values()
            .flat_map(|typed| typed.facts.references.iter())
            .filter(|reference| reference.source.contains(":included:"))
            .map(|reference| reference.source.clone()),
    );
    let mut changed_paths = BTreeSet::new();
    for (path, typed) in all.iter_mut() {
        let reference_count = typed.facts.references.len();
        let had_composed_routes = typed
            .facts
            .nodes
            .iter()
            .any(|node| node.details["included_from"].is_string());
        typed
            .facts
            .nodes
            .retain(|node| !removed.contains(node.id.as_str()));
        typed.facts.edges.retain(|edge| {
            !removed.contains(edge.src.as_str()) && !removed.contains(edge.dst.as_str())
        });
        typed
            .facts
            .references
            .retain(|reference| !removed.contains(reference.source.as_str()));
        if had_composed_routes || typed.facts.references.len() != reference_count {
            changed_paths.insert(path.clone());
        }
    }

    let module_paths: HashMap<(String, String), String> = all
        .iter()
        .flat_map(|(path, typed)| {
            let workspace = languages::workspace_path(path).0.to_owned();
            typed
                .facts
                .nodes
                .iter()
                .filter(|node| node.kind == "module" && node.language == "python")
                .map(move |node| ((workspace.clone(), node.qualname.clone()), path.clone()))
        })
        .collect();

    let mut processed = HashSet::new();
    let mut route_ancestry: HashMap<String, HashSet<String>> = HashMap::new();
    let mut composed_count = 0;
    loop {
        let include_nodes: Vec<(String, Node)> = all
            .iter()
            .flat_map(|(path, typed)| {
                typed
                    .facts
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.kind == "route"
                            && node.details["include_module"].is_string()
                            && node.details["include_depth"].as_u64().unwrap_or(0)
                                < MAX_INCLUDE_DEPTH as u64
                    })
                    .map(move |node| (path.clone(), node.clone()))
            })
            .collect();
        let mut additions = Vec::new();
        for (owner_path, include) in include_nodes {
            let Some(module) = include.details["include_module"].as_str() else {
                continue;
            };
            let workspace = languages::workspace_path(&owner_path).0.to_owned();
            let Some(child_path) = module_paths.get(&(workspace, module.to_owned())) else {
                continue;
            };
            let Some(include_path) = include.details["route_path"].as_str() else {
                continue;
            };
            let child_routes: Vec<Node> = all
                .get(child_path)
                .into_iter()
                .flat_map(|typed| typed.facts.nodes.iter())
                .filter(|node| {
                    node.kind == "route" && node.details["include_module"].as_str().is_none()
                })
                .cloned()
                .collect();
            for child in child_routes {
                let ancestors = route_ancestry.get(&child.id);
                if ancestors.is_some_and(|ancestors| {
                    ancestors.len() >= MAX_INCLUDE_DEPTH
                        || ancestors.contains(&include.id)
                }) {
                    continue;
                }
                if !processed.insert((include.id.clone(), child.id.clone())) {
                    continue;
                }
                if composed_count >= MAX_COMPOSED_ROUTES {
                    break;
                }
                let Some(child_path_value) = child.details["route_path"].as_str() else {
                    continue;
                };
                let route_path = prefixed_path(Some(include_path), child_path_value);
                let method = child.details["method"].as_str().unwrap_or("ANY");
                let name = format!("{method} {route_path}");
                let id = format!("route:{owner_path}:included:{}:{}", include.id, child.id);
                if all
                    .get(&owner_path)
                    .is_some_and(|typed| typed.facts.nodes.iter().any(|node| node.id == id))
                {
                    continue;
                }
                let mut ancestry = ancestors.cloned().unwrap_or_default();
                ancestry.insert(include.id.clone());
                route_ancestry.insert(id.clone(), ancestry);
                let mut route = child.clone();
                let source_path = child.details["source_path"]
                    .as_str()
                    .unwrap_or(child.path.as_str())
                    .to_owned();
                let source_line = child.details["source_line"]
                    .as_u64()
                    .unwrap_or(child.line as u64);
                let source_id = child.details["source_id"]
                    .as_str()
                    .unwrap_or(child.id.as_str())
                    .to_owned();
                let origin = all
                    .get(&source_path)
                    .filter(|_| {
                        languages::workspace_path(&source_path).0
                            == languages::workspace_path(&owner_path).0
                    })
                    .and_then(|source_facts| {
                        source_facts.facts.nodes.iter().find(|node| {
                            node.id == source_id
                                && node.kind == "route"
                                && node.path == source_path
                        })
                    });
                let source_qualname = origin.map(|node| node.qualname.clone());
                route.id = id.clone();
                route.name = name.clone();
                route.path = owner_path.clone();
                route.line = include.line;
                route.end_line = include.end_line;
                let parent_module = all
                    .get(&owner_path)
                    .and_then(|typed| typed.facts.nodes.iter().find(|node| node.kind == "module"))
                    .map_or(owner_path.as_str(), |module| module.qualname.as_str());
                route.qualname = format!("{parent_module}.{name}");
                route.details["route_path"] = json!(route_path);
                route.details["name"] = json!(name);
                route.details["source_path"] = json!(source_path);
                route.details["source_line"] = json!(source_line);
                route.details["source_id"] = json!(source_id);
                route.details["source_qualname"] = source_qualname
                    .map_or(serde_json::Value::Null, |qualname| json!(qualname));
                route.details["included_from"] = json!(include.id);
                route.details["include_depth"] = json!(
                    include.details["include_depth"]
                        .as_u64()
                        .unwrap_or(0)
                        .max(child.details["include_depth"].as_u64().unwrap_or(0))
                        + 1
                );
                let mut namespaces = Vec::new();
                for candidate in [
                    include.details["route_namespace"].as_str(),
                    include.details["include_namespace"].as_str(),
                    child.details["route_namespace"].as_str(),
                ]
                .into_iter()
                .flatten()
                {
                    namespaces.extend(candidate.split(':').filter(|part| !part.is_empty()));
                }
                route.details["route_namespace"] = if namespaces.is_empty() {
                    serde_json::Value::Null
                } else {
                    json!(namespaces.join(":"))
                };
                additions.push((owner_path.clone(), route, include.clone(), child));
                composed_count += 1;
            }
        }
        if additions.is_empty() {
            break;
        }
        for (owner_path, route, include, child) in additions {
            let source_path = route.details["source_path"]
                .as_str()
                .unwrap_or(child.path.as_str())
                .to_owned();
            let source_id = route.details["source_id"]
                .as_str()
                .unwrap_or(child.id.as_str())
                .to_owned();
            let child_edges = all
                .get(&child.path)
                .map(|source_facts| {
                    let edges = source_facts
                        .facts
                        .edges
                        .iter()
                        .filter(|edge| edge.src == child.id && edge.kind != "contains")
                        .cloned()
                        .collect::<Vec<_>>();
                    edges
                })
                .unwrap_or_default();
            let source_references = all
                .get(&source_path)
                .filter(|_| {
                    languages::workspace_path(&source_path).0
                        == languages::workspace_path(&owner_path).0
                })
                .filter(|source_facts| {
                    source_facts.facts.nodes.iter().any(|node| {
                        node.id == source_id
                            && node.kind == "route"
                            && node.path == source_path
                    })
                })
                .map(|source_facts| {
                    source_facts
                        .facts
                        .references
                        .iter()
                        .filter(|reference| reference.source == source_id)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let Some(owner) = all.get_mut(&owner_path) else {
                continue;
            };
            owner.facts.nodes.push(route.clone());
            owner.facts.edges.push(Edge {
                src: format!("module:{owner_path}"),
                dst: route.id.clone(),
                kind: "contains".into(),
                path: owner_path.clone(),
                line: include.line,
                evidence: "composed Django include route".into(),
                confidence: "exact".into(),
            });
            for mut edge in child_edges {
                edge.src = route.id.clone();
                edge.path = owner_path.clone();
                edge.line = include.line;
                owner.facts.edges.push(edge);
            }
            changed_paths.insert(owner_path);
            if let Some(origin) = all.get_mut(&source_path) {
                for mut reference in source_references {
                    reference.source = route.id.clone();
                    origin.facts.references.push(reference);
                }
                changed_paths.insert(source_path);
            }
        }
    }
    changed_paths
}
