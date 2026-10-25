use super::*;

pub(super) fn emit_inherited_method_edges<F>(
    input: &ResolutionInput<'_, F>,
    graph: &mut Graph,
    path: &str,
    candidate: &Node,
    source_container_id: &str,
) {
    let Some(target_methods) = input
        .interner
        .lookup_symbol(candidate.id.as_str())
        .and_then(|id| input.children_by_parent.get(&id))
    else {
        return;
    };
    let Some(source_methods) = input
        .interner
        .lookup_symbol(source_container_id)
        .and_then(|id| input.children_by_parent.get(&id))
    else {
        return;
    };
    let is_interface = candidate.kind == "interface"
        || candidate.kind == "trait"
        || python::resolution::has_protocol_or_abc_base(candidate);
    let method_kind = if is_interface {
        "implements"
    } else {
        "overrides"
    };
    for source_method in source_methods
        .iter()
        .filter(|method| matches!(method.kind.as_str(), "function" | "method"))
    {
        if let Some(target_method) = target_methods.iter().find(|method| {
            method.name == source_method.name
                && matches!(method.kind.as_str(), "function" | "method")
        }) {
            graph.edges.push(Edge {
                src: source_method.id.clone(),
                dst: target_method.id.clone(),
                kind: method_kind.into(),
                path: path.to_owned(),
                line: source_method.line,
                evidence: format!(
                    "{}::{}: {method_kind} {}::{}",
                    source_method.qualname,
                    source_method.name,
                    target_method.qualname,
                    target_method.name
                ),
                confidence: "exact".into(),
            });
        }
    }
}
