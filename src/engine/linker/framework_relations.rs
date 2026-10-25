use super::interner::{LinkerInterner, PathId, SymbolId};
use super::*;
use crate::core::typed_facts::FlowState;
use crate::engine::languages::{self, LanguageProfile};
use crate::engine::linker::semantic_context;
use std::collections::BTreeSet;

pub(super) struct PreparedSemanticFacts<'a> {
    pub(super) interner: &'a LinkerInterner<'a>,
    pub(super) facts:
        BTreeMap<PathId, Vec<std::borrow::Cow<'a, languages::semantic::SemanticFact>>>,
    pub(super) errors: BTreeMap<PathId, Vec<(usize, String, String)>>,
}

/// Canonical declarative framework rule action model.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DeclarativeRulePrimitive {
    /// Emits a route or endpoint match edge and coverage entry (`EmitRoute`).
    EmitRoute {
        kind: &'static str,
        resolved_prefix: &'static str,
        unmatched_prefix: &'static str,
        ambiguous_prefix: &'static str,
    },
    /// Emits a declarative semantic occurrence edge (`EmitEdge`).
    EmitEdge { evidence: &'static str },
}

type OccurrenceKey = (
    SymbolId,
    usize,
    usize,
    languages::semantic::SemanticRelation,
);
pub(super) type OccurrenceValue = (SymbolId, BTreeSet<SymbolId>, bool, bool);

#[derive(Default)]
pub(super) struct OccurrenceTable {
    entries: BTreeMap<OccurrenceKey, OccurrenceValue>,
}

impl OccurrenceTable {
    pub(super) fn entry(
        &mut self,
        owner: SymbolId,
        line: usize,
        column: usize,
        relation: languages::semantic::SemanticRelation,
        expression: &str,
        interner: &LinkerInterner<'_>,
    ) -> &mut OccurrenceValue {
        let expression = interner
            .lookup_symbol(expression)
            .expect("semantic occurrence expression");
        self.entries
            .entry((owner, line, column, relation))
            .or_insert_with(|| (expression, BTreeSet::new(), false, false))
    }

    pub(super) fn emit_all(
        self,
        graph: &mut Graph,
        path: &str,
        primitive: DeclarativeRulePrimitive,
        interner: &LinkerInterner<'_>,
    ) {
        let DeclarativeRulePrimitive::EmitEdge { evidence } = primitive else {
            return;
        };
        for ((owner, line, _, relation), (expression, targets, ambiguous, unresolved)) in
            self.entries
        {
            emit_occurrence(
                graph,
                path,
                owner,
                line,
                relation.as_str(),
                interner.resolve_symbol(expression).to_owned(),
                targets,
                ambiguous,
                unresolved,
                evidence,
                interner,
            );
        }
    }
}

pub(super) struct CompositionInput<'a, 'lookup> {
    pub(super) nodes: &'lookup [&'a Node],
    pub(super) by_id: &'lookup HashMap<SymbolId, &'a Node>,
    pub(super) semantic: &'lookup PreparedSemanticFacts<'a>,
}

fn extend_semantic_state<'a>(
    state: &'a crate::core::typed_facts::SemanticState,
    line: usize,
    qualname: &str,
    facts: &mut Vec<std::borrow::Cow<'a, languages::semantic::SemanticFact>>,
    errors: &mut Vec<(usize, String, String)>,
) {
    match state {
        crate::core::typed_facts::SemanticState::Valid { facts: values, .. } => {
            facts.extend(values.iter().map(std::borrow::Cow::Borrowed));
        }
        crate::core::typed_facts::SemanticState::Invalid { error, .. } => {
            errors.push((line, qualname.to_owned(), error.clone()));
        }
    }
}

impl<'a> PreparedSemanticFacts<'a> {
    pub(super) fn build<F: AsRef<Facts> + Sync>(
        all: &'a BTreeMap<String, F>,
        interner: &'a LinkerInterner<'a>,
    ) -> Self {
        let files: Vec<_> = all
            .par_iter()
            .filter_map(|(path, source)| {
                let mut facts = Vec::new();
                let mut errors = Vec::new();
                let source = source.as_ref();
                if source.semantic.is_complete() {
                    for (_, entry) in source.semantic.entries() {
                        extend_semantic_state(
                            &entry.state,
                            entry.line,
                            &entry.qualname,
                            &mut facts,
                            &mut errors,
                        );
                    }
                } else {
                    for (index, node) in source.nodes.iter().enumerate() {
                        if let Some(state) = source.semantic.state(index) {
                            extend_semantic_state(
                                state,
                                node.line,
                                &node.qualname,
                                &mut facts,
                                &mut errors,
                            );
                            continue;
                        }
                        match languages::semantic::decode(node) {
                            Ok(values) => {
                                facts.extend(values.into_iter().map(std::borrow::Cow::Owned))
                            }
                            Err(error) => {
                                errors.push((node.line, node.qualname.clone(), error.to_string()))
                            }
                        }
                    }
                }
                (!facts.is_empty() || !errors.is_empty()).then(|| {
                    (
                        interner.lookup_path(path).expect("catalog path"),
                        facts,
                        errors,
                    )
                })
            })
            .collect();
        let mut prepared = Self {
            interner,
            facts: BTreeMap::new(),
            errors: BTreeMap::new(),
        };
        for (path, facts, errors) in files {
            if !facts.is_empty() {
                prepared.facts.insert(path, facts);
            }
            if !errors.is_empty() {
                prepared.errors.insert(path, errors);
            }
        }
        prepared
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_occurrence(
    graph: &mut Graph,
    path: &str,
    owner_id: SymbolId,
    line: usize,
    kind: &str,
    expression: String,
    targets: BTreeSet<SymbolId>,
    ambiguous: bool,
    unresolved: bool,
    evidence: &'static str,
    interner: &LinkerInterner<'_>,
) {
    let status = if ambiguous || targets.len() > 1 {
        "ambiguous"
    } else if unresolved || targets.is_empty() {
        "unresolved"
    } else {
        "resolved"
    };
    if status == "resolved" {
        graph.edges.push(Edge {
            src: interner.resolve_symbol(owner_id).to_owned(),
            dst: interner
                .resolve_symbol(targets.into_iter().next().expect("resolved target"))
                .to_owned(),
            kind: kind.into(),
            path: path.to_owned(),
            line,
            evidence: expression.clone(),
            confidence: "exact".into(),
        });
    }
    graph.coverage.push(Coverage {
        path: path.to_owned(),
        line,
        expression,
        status: status.into(),
        evidence: evidence.into(),
    });
}

pub(super) const ENDPOINT_ROUTE_PRIMITIVE: DeclarativeRulePrimitive =
    DeclarativeRulePrimitive::EmitRoute {
        kind: "calls_endpoint",
        resolved_prefix: "endpoint route",
        unmatched_prefix: "unmatched endpoint route",
        ambiguous_prefix: "ambiguous endpoint route",
    };

const NAMED_ROUTE_PRIMITIVE: DeclarativeRulePrimitive = DeclarativeRulePrimitive::EmitRoute {
    kind: "references_route",
    resolved_prefix: "unique route name",
    unmatched_prefix: "unmatched route name",
    ambiguous_prefix: "ambiguous route name",
};

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_route_match(
    graph: &mut Graph,
    path: &str,
    src: &str,
    line: usize,
    primitive: DeclarativeRulePrimitive,
    expression: &str,
    matched: Result<Option<&Node>, ()>,
    resolved_value: Option<&str>,
    error_value: &str,
) {
    let DeclarativeRulePrimitive::EmitRoute {
        kind,
        resolved_prefix,
        unmatched_prefix,
        ambiguous_prefix,
    } = primitive
    else {
        return;
    };
    let (status, evidence) = match matched {
        Ok(Some(route)) => {
            graph.edges.push(Edge {
                src: src.to_owned(),
                dst: route.id.clone(),
                kind: kind.into(),
                path: path.to_owned(),
                line,
                evidence: expression.to_owned(),
                confidence: "exact".into(),
            });
            (
                "resolved",
                format!(
                    "{resolved_prefix}: {}",
                    resolved_value.unwrap_or(&route.name)
                ),
            )
        }
        Ok(None) => ("unresolved", format!("{unmatched_prefix}: {error_value}")),
        Err(()) => ("ambiguous", format!("{ambiguous_prefix}: {error_value}")),
    };
    graph.coverage.push(Coverage {
        path: path.to_owned(),
        line,
        expression: expression.to_owned(),
        status: status.into(),
        evidence: evidence.into(),
    });
}

pub(super) fn project_scope(
    path: &str,
    family: languages::LanguageFamily,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> String {
    dependencies
        .nearest_manifest_scope_for_path(family, path)
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
    flows: Option<&'a HashMap<SymbolId, &'a FlowState>>,
    dependencies: &languages::dependency_registry::DependencyRegistry,
    _profiles: &HashMap<&'a str, &'static dyn LanguageProfile>,
    semantic_context: &semantic_context::Context<'a>,
    input: CompositionInput<'a, '_>,
) -> Vec<Graph> {
    let CompositionInput {
        nodes,
        by_id,
        semantic: prepared,
    } = input;
    let interner = prepared.interner;
    let route_index = routes::RouteIndex::build(nodes);
    let http_target_contexts = routes::HttpTargetContexts::build(
        all.iter()
            .filter(|(path, _)| owners.is_none_or(|owners| owners.contains(*path)))
            .filter(|(_, source)| {
                source.as_ref().references.iter().any(|reference| {
                    reference.kind == "calls_endpoint"
                        && matches!(
                            &reference.receiver_hint,
                            Some(crate::core::models::ReceiverHint::ConfiguredReference { .. })
                        )
                })
            })
            .map(|(path, _)| path.as_str()),
        dependencies,
    );
    use languages::semantic::{SemanticFactRole, SemanticIdentity};
    let semantic_facts = &prepared.facts;
    let semantic_errors = &prepared.errors;
    let mut semantic_targets = BTreeMap::<SemanticIdentity, BTreeSet<SymbolId>>::new();
    for (&path, decoded) in semantic_facts {
        let path = interner.resolve_path(path);
        for fact in decoded {
            if let SemanticFactRole::FileProvider { target } = &fact.role {
                if let Some(node) = interner.lookup_node(by_id, target.as_str()).filter(|node| {
                    node.kind == "module"
                        && node.path == path
                        && fact.occurrence.owner_id == node.id
                        && fact
                            .identity
                            .target
                            .paths(path, &node.language)
                            .contains(&fact.identity.path)
                }) {
                    semantic_targets
                        .entry(fact.identity.clone())
                        .or_default()
                        .insert(interner.lookup_symbol(&node.id).expect("catalog target"));
                }
            }
        }
    }
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
            for owner in &facts.nodes {
                if let Some(FlowState::Valid { facts: flow, .. }) = interner
                    .lookup_symbol(&owner.id)
                    .and_then(|id| flows.and_then(|flows| flows.get(&id).copied()))
                {
                    super::projections::compose(&mut graph, owner, flow, semantic_context, by_id, dependencies);
                }
            }
            for (line, expression, error) in
                interner.lookup_path(path).and_then(|path| semantic_errors.get(&path)).into_iter().flatten()
            {
                graph.coverage.push(Coverage {
                    path: path.clone(),
                    line: *line,
                    expression: expression.clone(),
                    status: "unresolved".into(),
                    evidence: format!("invalid persisted semantic fact: {error}").into(),
                });
            }
            super::python::semantic_relations::compose_file_relations(
                path,
                by_id,
                dependencies,
                semantic_context,
                prepared,
                &semantic_targets,
                &mut graph,
            );
            #[cfg(feature = "lang-html")]
            super::html::relations::emit_render_targets(path, &render_contexts, &mut graph);

            let mut file_rules = None;
            let mut unscoped_endpoint_rule = None;
            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "calls_endpoint")
            {
                let rules =
                    file_rules.get_or_insert_with(|| dependencies.language_context_for_path(path));
                let has_unscoped_endpoint_rule = *unscoped_endpoint_rule.get_or_insert_with(|| {
                    rules.syntax_rules.iter().any(|rule| {
                        matches!(
                            rule.operation,
                            languages::manifests::syntax::SyntaxOperation::HttpClientCall {
                                target_context: None,
                                ..
                            } | languages::manifests::syntax::SyntaxOperation::HtmlRequestAttributes { .. }
                        )
                    }) || rules.semantic_rules.iter().any(|rule| {
                        rule.active
                            && matches!(
                                rule.rule.operation,
                                languages::manifests::semantic::SemanticOperation::EndpointProjection { .. }
                            )
                    })
                });
                let origin = match &reference.receiver_hint {
                    Some(crate::core::models::ReceiverHint::ConfiguredReference { origin }) => {
                        let origin_is_configured = rules.syntax_rules.iter().any(|rule| {
                            rule.pack == origin.pack
                                && rule.id == origin.rule
                                && matches!(
                                    rule.operation,
                                    languages::manifests::syntax::SyntaxOperation::HttpClientCall { .. }
                                        | languages::manifests::syntax::SyntaxOperation::HtmlRequestAttributes { .. }
                                )
                        }) || rules.semantic_rules.iter().any(|rule| {
                            rule.active
                                && rule.pack == origin.pack
                                && rule.rule.id == origin.rule
                                && matches!(
                                    rule.rule.operation,
                                    languages::manifests::semantic::SemanticOperation::EndpointProjection { .. }
                                )
                        });
                        if !origin_is_configured {
                            continue;
                        }
                        Some(origin)
                    }
                    _ if has_unscoped_endpoint_rule => None,
                    _ => continue,
                };
                let (method, url) = reference
                    .expression
                    .split_once(' ')
                    .unwrap_or(("ANY", &reference.expression));
                if routes::is_remote_client_url(url) {
                    graph.reference_coverage(path, reference, "external", "remote HTTP endpoint".into());
                    continue;
                }
                let target_context = origin.and_then(|origin| http_target_contexts.get(path, origin));
                emit_route_match(
                    &mut graph,
                    path,
                    &reference.source,
                    reference.line,
                    ENDPOINT_ROUTE_PRIMITIVE,
                    &reference.expression,
                    route_index.match_route_in_context(method, url, target_context, path),
                    None,
                    &reference.expression,
                );
            }

            #[cfg(feature = "lang-html")]
            super::html::relations::emit_request_routes(
                path,
                facts,
                flows,
                interner,
                &route_index,
                dependencies,
                &mut graph,
            );

            for reference in facts
                .references
                .iter()
                .filter(|reference| reference.kind == "references_route")
            {
                let Some(crate::core::models::ReceiverHint::NamedRoute { origin, key: name }) =
                    &reference.receiver_hint
                else {
                    continue;
                };
                let rules =
                    file_rules.get_or_insert_with(|| dependencies.language_context_for_path(path));
                if !rules.syntax_rules.iter().any(|rule| {
                    rule.pack == origin.pack
                        && rule.id == origin.rule
                        && matches!(
                            rule.operation,
                            languages::manifests::syntax::SyntaxOperation::NamedRouteCall { .. }
                                | languages::manifests::syntax::SyntaxOperation::TemplateNamedRoute { .. }
                        )
                }) {
                    continue;
                }
                emit_route_match(
                    &mut graph,
                    path,
                    &reference.source,
                    reference.line,
                    NAMED_ROUTE_PRIMITIVE,
                    &reference.expression,
                    route_index.match_route_name(name),
                    Some(name),
                    name,
                );
            }

            #[cfg(feature = "lang-html")]
            super::html::relations::emit_template_references(
                path,
                facts,
                &expressions,
                &render_contexts,
                dependencies,
                &mut graph,
            );

            graph
        })
        .collect()
}
