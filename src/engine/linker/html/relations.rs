use super::super::*;
use super::templates::{ExpressionRegistry, Match, RenderContextRegistry};
use crate::core::semantic::ValueFlowFacts;
use crate::core::typed_facts::FlowState;

fn is_html_file(facts: &Facts) -> bool {
    facts
        .nodes
        .iter()
        .any(|node| node.kind == "module" && node.language == "html")
}

fn is_request_with_rules(
    node: &Node,
    flow: &ValueFlowFacts,
    syntax_rules: &[languages::manifests::syntax::SyntaxRule],
) -> bool {
    node.language == "html"
        && !flow.fact_limit_exceeded
        && flow.rule_origins().any(|origin| {
            syntax_rules.iter().any(|rule| {
                rule.pack == origin.pack
                    && rule.id == origin.rule
                    && matches!(
                        &rule.operation,
                        languages::manifests::syntax::SyntaxOperation::HtmlRequestAttributes {
                            request_node, ..
                        } if node.kind == *request_node
                    )
            })
        })
}

pub(in crate::engine::linker) fn is_request(
    node: &Node,
    flow: &ValueFlowFacts,
    path: &str,
    dependencies: &languages::dependency_registry::DependencyRegistry,
) -> bool {
    node.language == "html"
        && !flow.fact_limit_exceeded
        && is_request_with_rules(
            node,
            flow,
            &dependencies.language_context_for_path(path).syntax_rules,
        )
}

pub(in crate::engine::linker) fn emit_render_targets(
    path: &str,
    render_contexts: &RenderContextRegistry<'_>,
    graph: &mut Graph,
) {
    for (provider, target) in render_contexts.targets(path) {
        graph.push_edge(
            path,
            provider.source.id.clone(),
            target.id.clone(),
            "renders",
            provider.context.position.line,
            format!(
                "render target: {} from {}",
                target.path, provider.source.path
            ),
            "exact",
        );
    }
}

pub(in crate::engine::linker) fn emit_request_routes(
    path: &str,
    facts: &Facts,
    flows: Option<&HashMap<super::super::interner::SymbolId, &FlowState>>,
    interner: &super::super::interner::LinkerInterner<'_>,
    routes: &super::super::routes::RouteIndex<'_>,
    dependencies: &languages::dependency_registry::DependencyRegistry,
    graph: &mut Graph,
) {
    if !is_html_file(facts) {
        return;
    }
    let context = dependencies.language_context_for_path(path);
    for node in facts.nodes.iter().filter(|node| {
        let Some(FlowState::Valid { facts, .. }) = interner
            .lookup_symbol(&node.id)
            .and_then(|id| flows.and_then(|flows| flows.get(&id).copied()))
        else {
            return false;
        };
        is_request_with_rules(node, facts, &context.syntax_rules)
    }) {
        let (method, url) = node.name.split_once(' ').unwrap_or(("GET", &node.name));
        let status = node.details["url_status"].as_str().unwrap_or("");
        match status {
            "remote" => graph.push_coverage(
                path,
                node.line,
                node.name.clone(),
                "external",
                "remote HTTP endpoint",
            ),
            "current_page" => graph.push_coverage(
                path,
                node.line,
                node.name.clone(),
                "resolved",
                "current page form/action",
            ),
            "local_unverified" => {
                super::super::framework_relations::emit_route_match(
                    graph,
                    path,
                    &node.id,
                    node.line,
                    super::super::framework_relations::ENDPOINT_ROUTE_PRIMITIVE,
                    &node.name,
                    routes.match_route(method, url),
                    None,
                    &node.name,
                );
            }
            _ => graph.push_coverage(
                path,
                node.line,
                node.name.clone(),
                "unresolved",
                format!("unverified HTTP endpoint: {}", node.name),
            ),
        }
    }
}

pub(in crate::engine::linker) fn emit_template_references(
    path: &str,
    facts: &Facts,
    expressions: &ExpressionRegistry<'_>,
    render_contexts: &RenderContextRegistry<'_>,
    dependencies: &languages::dependency_registry::DependencyRegistry,
    graph: &mut Graph,
) {
    if !is_html_file(facts) {
        return;
    }
    let context = dependencies.language_context_for_path(path);
    let providers: Vec<_> = render_contexts.providers(path).collect();
    for reference in facts.references.iter().filter(|reference| {
        reference.kind == "references"
            && (reference.expression.starts_with("template.variable.")
                || reference.expression.starts_with("template.macro."))
    }) {
        match expressions.resolve_with_context(path, reference, &context) {
            Match::Unique { module, symbol, .. } => {
                graph.push_edge(
                    path,
                    reference.source.clone(),
                    symbol.id.clone(),
                    "references",
                    reference.line,
                    format!("template binding from {} via {}", symbol.path, module.path),
                    "exact",
                );
                graph.reference_coverage(
                    path,
                    reference,
                    "resolved",
                    format!("template binding: {}", symbol.path).into(),
                );
            }
            Match::Ambiguous => graph.reference_coverage(
                path,
                reference,
                "ambiguous",
                "multiple template bindings".into(),
            ),
            Match::Missing => {
                let key = reference.expression.strip_prefix("template.variable.");
                let has_matching = key.is_some_and(|key| {
                    providers
                        .iter()
                        .any(|provider| provider.context.keys.iter().any(|item| item == key))
                });
                let (status, evidence): (&str, std::borrow::Cow<'static, str>) =
                    match (providers.as_slice(), has_matching) {
                        ([provider], true) => {
                            graph.push_edge(
                                path,
                                reference.source.clone(),
                                provider.source.id.clone(),
                                "context_provider",
                                reference.line,
                                format!("render context: {}", provider.source.path),
                                "exact",
                            );
                            (
                                "resolved",
                                format!("render context: {}", provider.source.path).into(),
                            )
                        }
                        ([_, _, ..], true) => {
                            ("ambiguous", "multiple render context providers".into())
                        }
                        _ => ("unresolved", "no proven template binding".into()),
                    };
                graph.reference_coverage(path, reference, status, evidence);
            }
        }
    }
}
