use super::*;
use crate::core::typed_facts::FlowState;
use crate::engine::languages::{self, LanguageProfile};
use crate::engine::linker::semantic_context;
use std::collections::BTreeSet;

fn project_scope(
    path: &str,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> String {
    dependencies
        .nearest_manifest_scope_for_path(languages::LanguageFamily("python"), path)
        .unwrap_or_else(|| languages::workspace_path(path).0)
        .to_owned()
}

/// Composes relations that require facts owned by more than one language.
///
/// Route handlers, HTTP clients, HTML requests, and template render contexts
/// are indexed independently by their source profiles. This pass joins those
/// explicit facts after language resolution and returns only graph output.
pub(super) fn compose<'a, F: AsRef<Facts> + Sync>(
    all: &'a BTreeMap<String, F>,
    owners: Option<&std::collections::BTreeSet<String>>,
    flows: Option<&'a HashMap<&'a str, &'a FlowState>>,
    dependencies: &languages::dependency_registry::DependencyRegistry,
    profiles: &HashMap<&'a str, &'static dyn LanguageProfile>,
    semantic_context: &semantic_context::Context<'a>,
    nodes: &[&'a Node],
) -> Vec<Graph> {
    let route_index = routes::RouteIndex::build(nodes);
    use languages::semantic::{
        SemanticFactRole, SemanticIdentity, SemanticOccurrence, SemanticRelation,
    };
    let by_id: HashMap<_, _> = nodes.iter().map(|node| (node.id.as_str(), *node)).collect();
    let mut semantic_facts = BTreeMap::new();
    let mut semantic_errors = BTreeMap::new();
    let mut semantic_targets = BTreeMap::<SemanticIdentity, BTreeSet<String>>::new();
    for (path, source) in all {
        let mut decoded = Vec::new();
        for node in &source.as_ref().nodes {
            match languages::semantic::decode(node) {
                Ok(values) => decoded.extend(values),
                Err(error) => semantic_errors
                    .entry(path.as_str())
                    .or_insert_with(Vec::new)
                    .push((node.line, node.qualname.clone(), error.to_string())),
            }
        }
        for fact in &decoded {
            if let SemanticFactRole::FileProvider { target } = &fact.role {
                if let Some(node) = by_id.get(target.as_str()).filter(|node| {
                    node.kind == "module"
                        && node.path == *path
                        && fact.occurrence.owner_id == node.id
                        && fact.identity.scope == project_scope(path, dependencies)
                        && fact
                            .identity
                            .target
                            .paths(path, &node.language)
                            .contains(&fact.identity.path)
                }) {
                    semantic_targets
                        .entry(fact.identity.clone())
                        .or_default()
                        .insert(node.id.clone());
                }
            }
        }
        semantic_facts.insert(path.as_str(), decoded);
    }
    #[cfg(not(feature = "lang-html"))]
    let _ = (flows, dependencies, profiles, semantic_context);
    #[cfg(feature = "lang-html")]
    let render_contexts =
        html_templates::RenderContextRegistry::build(all, flows, semantic_context, dependencies);
    #[cfg(feature = "lang-html")]
    let expressions = html_templates::ExpressionRegistry::build(all);

    all.par_iter()
        .filter(|(path, _)| owners.is_none_or(|owners| owners.contains(*path)))
        .map(|(path, source)| {
            let facts = source.as_ref();
            let mut graph = Graph {
                edges: Vec::new(),
                coverage: Vec::new(),
            };
            for (line, expression, error) in
                semantic_errors.get(path.as_str()).into_iter().flatten()
            {
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: *line,
                    expression: expression.clone(),
                    status: "unresolved".into(),
                    evidence: format!("invalid persisted semantic fact: {error}").into(),
                });
            }
            let mut occurrences = BTreeMap::<
                (SemanticOccurrence, SemanticRelation),
                (String, BTreeSet<String>),
            >::new();
            for fact in semantic_facts.get(path.as_str()).into_iter().flatten() {
                let SemanticFactRole::CallToFile {
                    import_owner,
                    alias,
                    module,
                    export,
                    relation,
                } = &fact.role
                else {
                    continue;
                };
                let entry = occurrences
                    .entry((fact.occurrence.clone(), *relation))
                    .or_insert_with(|| (fact.expression.clone(), BTreeSet::new()));
                if fact.identity.scope != project_scope(path, dependencies)
                    || !by_id
                        .get(fact.occurrence.owner_id.as_str())
                        .is_some_and(|owner| owner.path == *path)
                    || !by_id.get(import_owner.as_str()).is_some_and(|owner| {
                        owner.path == *path
                            && semantic_context.exact_external_import(
                                owner,
                                alias,
                                module,
                                export,
                                crate::core::semantic::SourcePosition {
                                    line: fact.occurrence.line,
                                    column: fact.occurrence.column,
                                },
                            )
                    })
                {
                    continue;
                }
                if let Some(targets) = semantic_targets.get(&fact.identity) {
                    entry.1.extend(targets.iter().cloned());
                }
            }
            // Aggregate every rule and namespace before promoting one source occurrence.
            for ((occurrence, relation), (expression, targets)) in occurrences {
                let status = match targets.len() {
                    0 => "unresolved",
                    1 => "resolved",
                    _ => "ambiguous",
                };
                if targets.len() == 1 {
                    graph.edges.push(Edge {
                        src: occurrence.owner_id,
                        dst: targets.into_iter().next().expect("one target"),
                        kind: relation.as_str().into(),
                        path: path.clone(),
                        line: occurrence.line,
                        evidence: expression.clone(),
                        confidence: "exact".into(),
                    });
                }
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: occurrence.line,
                    expression,
                    status: status.into(),
                    evidence: "declarative file identity; unique proven target required".into(),
                });
            }

            #[cfg(feature = "lang-html")]
            for (provider, target) in render_contexts.targets(path) {
                graph.edges.push(Edge {
                    src: provider.source.id.clone(),
                    dst: target.id.clone(),
                    kind: "renders".into(),
                    path: path.clone(),
                    line: provider.context.position.line,
                    evidence: format!(
                        "render target: {} from {}",
                        target.path, provider.source.path
                    ),
                    confidence: "exact".into(),
                });
            }

            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "calls_endpoint")
            {
                let (method, url) = reference
                    .expression
                    .split_once(' ')
                    .unwrap_or(("ANY", &reference.expression));
                let (remote, _) = routes::normalize_client_url(url);
                if remote {
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: reference.line,
                        expression: reference.expression.clone(),
                        status: "external".into(),
                        evidence: "remote HTTP endpoint".into(),
                    });
                    continue;
                }
                match route_index.match_route(method, url) {
                    Ok(Some(route)) => {
                        graph.edges.push(Edge {
                            src: reference.source.clone(),
                            dst: route.id.clone(),
                            kind: "calls_endpoint".into(),
                            path: path.clone(),
                            line: reference.line,
                            evidence: reference.expression.clone(),
                            confidence: "exact".into(),
                        });
                        graph.coverage.push(Coverage {
                            path: path.clone(),
                            line: reference.line,
                            expression: reference.expression.clone(),
                            status: "resolved".into(),
                            evidence: format!("endpoint route: {}", route.name).into(),
                        });
                    }
                    Ok(None) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: reference.line,
                        expression: reference.expression.clone(),
                        status: "unresolved".into(),
                        evidence: format!("unmatched endpoint route: {}", reference.expression)
                            .into(),
                    }),
                    Err(()) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: reference.line,
                        expression: reference.expression.clone(),
                        status: "ambiguous".into(),
                        evidence: format!("ambiguous endpoint route: {}", reference.expression)
                            .into(),
                    }),
                }
            }

            #[cfg(feature = "lang-html")]
            for node in facts
                .nodes
                .iter()
                .filter(|node| node.kind == "htmx_request")
            {
                let (method, url) = node.name.split_once(' ').unwrap_or(("GET", &node.name));
                let status = node.details["url_status"]
                    .as_str()
                    .unwrap_or("local_unverified");
                if status == "remote" {
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: node.line,
                        expression: node.name.clone(),
                        status: "external".into(),
                        evidence: "remote HTTP endpoint".into(),
                    });
                    continue;
                }
                if status == "current_page" {
                    graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: node.line,
                        expression: node.name.clone(),
                        status: "resolved".into(),
                        evidence: "current page form/action".into(),
                    });
                    continue;
                }
                match route_index.match_route(method, url) {
                    Ok(Some(route)) => {
                        graph.edges.push(Edge {
                            src: node.id.clone(),
                            dst: route.id.clone(),
                            kind: "calls_endpoint".into(),
                            path: path.clone(),
                            line: node.line,
                            evidence: node.name.clone(),
                            confidence: "exact".into(),
                        });
                        graph.coverage.push(Coverage {
                            path: path.clone(),
                            line: node.line,
                            expression: node.name.clone(),
                            status: "resolved".into(),
                            evidence: format!("endpoint route: {}", route.name).into(),
                        });
                    }
                    Ok(None) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: node.line,
                        expression: node.name.clone(),
                        status: "unresolved".into(),
                        evidence: format!("unmatched endpoint route: {}", node.name).into(),
                    }),
                    Err(()) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: node.line,
                        expression: node.name.clone(),
                        status: "ambiguous".into(),
                        evidence: format!("ambiguous endpoint route: {}", node.name).into(),
                    }),
                }
            }

            #[cfg(feature = "lang-html")]
            if profiles
                .get(path.as_str())
                .is_some_and(|profile| profile.id() == "html")
            {
                for reference in facts.references.iter().filter(|reference| {
                    reference.kind == "references"
                        && (reference.expression.starts_with("template.variable.")
                            || reference.expression.starts_with("template.macro."))
                }) {
                    match expressions.resolve(path, reference) {
                        html_templates::Match::Unique { module, symbol } => {
                            graph.edges.push(Edge {
                                src: reference.source.clone(),
                                dst: symbol.id.clone(),
                                kind: "references".into(),
                                path: path.clone(),
                                line: reference.line,
                                evidence: format!(
                                    "template binding from {} via {}",
                                    symbol.path, module.path
                                ),
                                confidence: "exact".into(),
                            });
                            graph.coverage.push(Coverage {
                                path: path.clone(),
                                line: reference.line,
                                expression: reference.expression.clone(),
                                status: "resolved".into(),
                                evidence: format!("template binding: {}", symbol.path).into(),
                            });
                        }
                        html_templates::Match::Ambiguous => graph.coverage.push(Coverage {
                            path: path.clone(),
                            line: reference.line,
                            expression: reference.expression.clone(),
                            status: "ambiguous".into(),
                            evidence: "multiple template bindings".into(),
                        }),
                        html_templates::Match::Missing => {
                            let key = reference.expression.strip_prefix("template.variable.");
                            let providers: Vec<_> = render_contexts.providers(path).collect();
                            let matching_providers: Vec<_> = key
                                .map(|key| {
                                    providers
                                        .iter()
                                        .copied()
                                        .filter(|provider| {
                                            provider.context.keys.iter().any(|item| item == key)
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let (status, evidence): (&str, Cow<'static, str>) =
                                match (providers.len(), matching_providers.as_slice()) {
                                    (1, [provider]) => {
                                        graph.edges.push(Edge {
                                            src: reference.source.clone(),
                                            dst: provider.source.id.clone(),
                                            kind: "context_provider".into(),
                                            path: path.clone(),
                                            line: reference.line,
                                            evidence: format!(
                                                "render context: {}",
                                                provider.source.path
                                            ),
                                            confidence: "exact".into(),
                                        });
                                        (
                                            "resolved",
                                            format!("render context: {}", provider.source.path)
                                                .into(),
                                        )
                                    }
                                    (provider_count, matching)
                                        if provider_count > 1 && !matching.is_empty() =>
                                    {
                                        ("ambiguous", "multiple render context providers".into())
                                    }
                                    _ => ("unresolved", "no proven template binding".into()),
                                };
                            graph.coverage.push(Coverage {
                                path: path.clone(),
                                line: reference.line,
                                expression: reference.expression.clone(),
                                status: status.into(),
                                evidence,
                            });
                        }
                    }
                }
            }

            graph
        })
        .collect()
}
