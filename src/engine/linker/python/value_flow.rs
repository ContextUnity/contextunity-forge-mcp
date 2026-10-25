use super::super::value_flow::{
    ReadTrace, SemanticLimits, SemanticResolver, Symbol, SymbolRole, TypeTarget, ValueFlowIndex,
    node_position, resolve_type, symbol_type,
};
use crate::core::{
    models::Node,
    semantic::{SourcePosition, TypeExpr},
};

pub(in crate::engine::linker) fn literal_type<'a>(
    owner: &Node,
    expression: &str,
) -> Option<TypeTarget<'a>> {
    if owner.language != "python" {
        return None;
    }
    crate::core::semantic::python_literal_type(expression)
        .map(|name| TypeTarget::Builtin(name.to_owned()))
}

pub(in crate::engine::linker) fn constructor_result<'a>(
    owner: &'a Node,
    callee: &str,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
) -> Option<TypeTarget<'a>> {
    if owner.language != "python" {
        return None;
    }
    if let Symbol::Type(target @ (TypeTarget::Local(_) | TypeTarget::Builtin(_))) =
        resolver.resolve_symbol(owner, callee, at, SymbolRole::Constructor)
    {
        return Some(target);
    }
    resolver.resolve_external_factory(owner, callee, at)
}

pub(in crate::engine::linker) fn constructor_symbol<'a>(
    owner: &'a Node,
    expression: &str,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
) -> Option<Symbol<'a>> {
    (owner.language == "python")
        .then(|| resolver.resolve_symbol(owner, expression, at, SymbolRole::Constructor))
}

pub(in crate::engine::linker) fn dotted_member_result<'a>(
    index: &ValueFlowIndex<'a>,
    owner: &'a Node,
    callee: &str,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    trace: Option<&ReadTrace<'a, '_>>,
) -> Option<TypeTarget<'a>> {
    if owner.language != "python" {
        return None;
    }
    let mut split = callee.rsplit_once('.');
    while let Some((prefix, _member)) = split {
        let receiver = index
            .lookup_tracked(owner, prefix, at, trace)
            .cloned()
            .unwrap_or_else(|| {
                symbol_type(resolver.resolve_symbol(owner, prefix, at, SymbolRole::Type))
            });
        if !matches!(receiver, TypeTarget::Unknown | TypeTarget::Ambiguous) {
            return Some(
                resolver
                    .resolve_member_result(&receiver, &callee[prefix.len() + 1..], owner, at)
                    .unwrap_or(TypeTarget::Unknown),
            );
        }
        split = prefix.rsplit_once('.');
    }
    None
}

pub(in crate::engine::linker) fn collection_return_element<'a>(
    owner: &'a Node,
    expression: &TypeExpr,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> Option<TypeTarget<'a>> {
    let TypeExpr::Applied { base, args } = expression else {
        return None;
    };
    if !matches!(base.as_str(), "list" | "List" | "Sequence" | "Iterable") || args.len() != 1 {
        return None;
    }
    let element = resolve_type(owner, &args[0], node_position(owner), resolver, limits);
    (!matches!(element, TypeTarget::Unknown | TypeTarget::Ambiguous)).then_some(element)
}

pub(in crate::engine::linker) fn typed_dict_class<'a>(
    node: &'a Node,
    resolver: &impl SemanticResolver<'a>,
) -> bool {
    let Some(bases) = node.details["bases"]
        .as_str()
        .and_then(|bases| bases.strip_prefix('('))
        .and_then(|bases| bases.strip_suffix(')'))
    else {
        return false;
    };
    let (base, options) = bases
        .split_once(',')
        .map_or((bases, ""), |(base, options)| (base, options));
    if !options.is_empty()
        && !options.split_once('=').is_some_and(|(key, value)| {
            key.trim() == "total" && matches!(value.trim(), "False" | "True")
        })
    {
        return false;
    }
    let base = base.trim();
    if !matches!(
        base,
        "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict"
    ) {
        return false;
    }
    matches!(
        resolver.resolve_symbol(node, base, node_position(node), SymbolRole::Type),
        Symbol::Type(TypeTarget::External { module, .. })
            if matches!(module.as_str(), "typing" | "typing_extensions")
    )
}

pub(in crate::engine::linker) fn nullable_type_expr(annotation: &str) -> Option<TypeExpr> {
    let annotation = strip_outer_parentheses(annotation.trim());
    if let Some(parts) = split_pipe_types(annotation) {
        if parts.len() >= 2 {
            let args = parts.into_iter().map(nullable_argument).collect();
            return Some(TypeExpr::Applied {
                base: "Union".to_owned(),
                args,
            });
        }
    }
    let (base, rest) = annotation.split_once('[')?;
    let base = base.trim();
    let wrapper = base.rsplit('.').next()?;
    if !matches!(wrapper, "Optional" | "Union") {
        return None;
    }
    let arguments = rest.strip_suffix(']')?;
    let mut depth = 0_usize;
    let mut start = 0_usize;
    let mut args = Vec::new();
    for (index, character) in arguments.char_indices() {
        match character {
            '[' => depth = depth.checked_add(1)?,
            ']' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                let argument = arguments[start..index].trim();
                if argument.is_empty() {
                    return None;
                }
                args.push(nullable_argument(argument));
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let argument = arguments[start..].trim();
    if argument.is_empty() {
        return None;
    }
    args.push(nullable_argument(argument));
    Some(TypeExpr::Applied {
        base: base.to_owned(),
        args,
    })
}

fn nullable_argument(annotation: &str) -> TypeExpr {
    let unparenthesized = strip_outer_parentheses(annotation);
    nullable_type_expr(unparenthesized).unwrap_or_else(|| TypeExpr::Named {
        name: unparenthesized.to_owned(),
    })
}

fn split_pipe_types(annotation: &str) -> Option<Vec<&str>> {
    let mut depth = 0_usize;
    let mut start = 0_usize;
    let mut parts = Vec::new();
    for (index, character) in annotation.char_indices() {
        match character {
            '[' | '(' => depth = depth.checked_add(1)?,
            ']' | ')' => depth = depth.checked_sub(1)?,
            '|' if depth == 0 => {
                let part = annotation[start..index].trim();
                if part.is_empty() {
                    return None;
                }
                parts.push(part);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 || parts.is_empty() {
        return None;
    }
    let last = annotation[start..].trim();
    if last.is_empty() {
        return None;
    }
    parts.push(last);
    Some(parts)
}

fn strip_outer_parentheses(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let mut depth = 0_usize;
        let mut closed_at_end = false;
        for (index, character) in trimmed.char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && index == trimmed.len() - 1 {
                        closed_at_end = true;
                    }
                }
                _ => {}
            }
        }
        if closed_at_end {
            trimmed[1..trimmed.len() - 1].trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    }
}
