use super::*;
use crate::core::typed_facts::FlowState;
use crate::engine::languages::{self, LanguageProfile};
use crate::engine::linker::semantic_context;

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
                            evidence: format!("endpoint route: {}", route.name),
                        });
                    }
                    Ok(None) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: reference.line,
                        expression: reference.expression.clone(),
                        status: "unresolved".into(),
                        evidence: format!("unmatched endpoint route: {}", reference.expression),
                    }),
                    Err(()) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: reference.line,
                        expression: reference.expression.clone(),
                        status: "ambiguous".into(),
                        evidence: format!("ambiguous endpoint route: {}", reference.expression),
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
                            evidence: format!("endpoint route: {}", route.name),
                        });
                    }
                    Ok(None) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: node.line,
                        expression: node.name.clone(),
                        status: "unresolved".into(),
                        evidence: format!("unmatched endpoint route: {}", node.name),
                    }),
                    Err(()) => graph.coverage.push(Coverage {
                        path: path.clone(),
                        line: node.line,
                        expression: node.name.clone(),
                        status: "ambiguous".into(),
                        evidence: format!("ambiguous endpoint route: {}", node.name),
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
                                evidence: format!("template binding: {}", symbol.path),
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
                            let (status, evidence) =
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
                                            format!("render context: {}", provider.source.path),
                                        )
                                    }
                                    (provider_count, matching)
                                        if provider_count > 1 && !matching.is_empty() =>
                                    {
                                        (
                                            "ambiguous",
                                            "multiple render context providers".to_owned(),
                                        )
                                    }
                                    _ => ("unresolved", "no proven template binding".to_owned()),
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
