use super::*;
#[cfg(any(
    feature = "lang-cpp",
    feature = "lang-csharp",
    feature = "lang-java",
    feature = "lang-php",
    feature = "lang-ruby"
))]
use crate::engine::ast::relations;
use std::collections::BTreeSet;

pub(super) fn child<'tree>(node: Syntax<'tree>, kinds: &[&str]) -> Option<Syntax<'tree>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|n| kinds.contains(&n.kind()));
    found
}

#[cfg(any(feature = "lang-java", feature = "lang-kotlin", feature = "lang-php"))]
pub(super) fn named<'a>(node: Syntax<'_>, source: &'a str, kinds: &[&str]) -> Option<&'a str> {
    child(node, kinds).map(|n| text(n, source))
}

#[cfg(any(
    feature = "lang-cpp",
    feature = "lang-csharp",
    feature = "lang-java",
    feature = "lang-kotlin",
    feature = "lang-php",
    feature = "lang-ruby"
))]
pub(super) fn call_reference(
    ctx: &SyntaxContext<'_, '_>,
    facts: &mut Facts,
    expression: &str,
    dynamic: bool,
) {
    facts.references.push(Reference {
        source: ctx.owner.into(),
        expression: ast::bounded_expression(expression),
        kind: "calls".into(),
        line: ctx.line(),
        alias: None,
        module: None,
        dynamic: dynamic
            || !expression
                .chars()
                .all(|c| c.is_alphanumeric() || "_.:".contains(c)),
        receiver_hint: None,
    });
}

#[cfg(any(feature = "lang-java", feature = "lang-ruby"))]
pub(super) fn member_call(
    ctx: &SyntaxContext<'_, '_>,
    facts: &mut Facts,
    name: Syntax<'_>,
    receiver: Option<Syntax<'_>>,
) {
    let expression = match receiver {
        Some(receiver) => format!("{}.{}", text(receiver, ctx.source), text(name, ctx.source)),
        None => text(name, ctx.source).into(),
    };
    call_reference(ctx, facts, &expression, false);
}

pub(super) fn import(
    ctx: &SyntaxContext<'_, '_>,
    facts: &mut Facts,
    module: &str,
    alias: Option<String>,
) {
    ctx.import(facts, module.into(), alias, Some(module.into()));
}

#[cfg(any(
    feature = "lang-cpp",
    feature = "lang-csharp",
    feature = "lang-java",
    feature = "lang-php",
    feature = "lang-ruby"
))]
pub(super) fn types(
    ctx: &SyntaxContext<'_, '_>,
    facts: &mut Facts,
    container: Syntax<'_>,
    kind: &str,
    ignored: &[&str],
) {
    let mut cursor = container.walk();
    for node in container
        .named_children(&mut cursor)
        .filter(|n| !ignored.contains(&n.kind()))
    {
        relations::reference(
            facts,
            ctx.owner,
            relations::type_name(node, ctx.source),
            kind,
            ctx.line(),
        );
    }
}

pub(super) fn declarator(node: Syntax<'_>) -> Syntax<'_> {
    let mut current = node;
    while let Some(inner) = current.child_by_field_name("declarator").or_else(|| {
        (current.kind() == "parenthesized_declarator")
            .then(|| current.named_child(0))
            .flatten()
    }) {
        current = inner;
    }
    current
}

pub(super) fn bindings(
    profile: &dyn LanguageProfile,
    root: Syntax<'_>,
    source: &str,
    parameters: &[&str],
    locals: &[&str],
) -> ast::ScopeBindings {
    let mut bound = BTreeSet::new();
    let mut rebindings = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node != root && profile.symbol(node).is_some() {
            continue;
        }
        let parameter = parameters.contains(&node.kind());
        if parameter || locals.contains(&node.kind()) {
            let mut cursor = node.walk();
            let mut names: Vec<_> = node
                .children_by_field_name("declarator", &mut cursor)
                .map(declarator)
                .collect();
            if names.is_empty() {
                names.extend(
                    node.child_by_field_name("name")
                        .or_else(|| node.child_by_field_name("left"))
                        .or_else(|| {
                            child(node, &["simple_identifier", "identifier", "variable_name"])
                        }),
                );
            }
            for name in names {
                let name = text(name, source).trim_start_matches('$').to_owned();
                bound.insert(name.clone());
                if !parameter {
                    rebindings.insert(name);
                }
            }
        }
        let mut c = node.walk();
        stack.extend(node.named_children(&mut c));
    }
    ast::ScopeBindings {
        all: bound.into_iter().collect(),
        rebindings: rebindings.into_iter().collect(),
    }
}
