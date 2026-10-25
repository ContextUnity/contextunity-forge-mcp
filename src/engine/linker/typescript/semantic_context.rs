use super::super::value_flow::{Symbol, SymbolRole, TypeTarget};
use crate::core::{models::Node, semantic::TypeExpr};
use crate::engine::languages::LanguageProfile;

pub(in crate::engine::linker) fn is_builtin_promise_type(
    owner: &Node,
    profile: &dyn LanguageProfile,
    name: &str,
    role: SymbolRole,
) -> bool {
    matches!(owner.language.as_str(), "typescript" | "javascript" | "vue")
        && matches!(role, SymbolRole::Type)
        && name == "Promise"
        && profile.builtin("Promise")
}

pub(in crate::engine::linker) fn visible(owner: &Node, candidate: &Node) -> bool {
    matches!(owner.language.as_str(), "javascript" | "typescript" | "vue")
        && candidate.kind == "function"
}

pub(in crate::engine::linker) fn admits_payload(
    owner: &Node,
    wrapper: &TypeTarget<'_>,
    argument_count: usize,
) -> bool {
    matches!(owner.language.as_str(), "typescript" | "javascript" | "vue")
        && argument_count == 1
        && matches!(wrapper, TypeTarget::Builtin(name) if name == "Promise")
}

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) fn prototype_visible_at(
    node: &Node,
    path: &str,
    line: usize,
    column: usize,
) -> bool {
    if node.language != "javascript" || node.details["prototype"] != true || node.path != path {
        return true;
    }
    let assigned_line = node.details["prototype_assignment_line"]
        .as_u64()
        .unwrap_or(node.line as u64) as usize;
    let assigned_column = node.details["prototype_assignment_column"]
        .as_u64()
        .or_else(|| node.details["column"].as_u64())
        .unwrap_or(0) as usize;
    (assigned_line, assigned_column) <= (line, column)
}

#[cfg(not(feature = "lang-typescript"))]
pub(in crate::engine::linker) fn prototype_visible_at(
    _node: &Node,
    _path: &str,
    _line: usize,
    _column: usize,
) -> bool {
    true
}

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) use crate::engine::languages::typescript::typed_dom_receiver;

#[cfg(not(feature = "lang-typescript"))]
pub(in crate::engine::linker) fn typed_dom_receiver(_name: &str) -> Option<&'static str> {
    None
}

#[cfg(feature = "lang-typescript")]
pub(in crate::engine::linker) use crate::engine::languages::typescript::typed_dom_property;

#[cfg(not(feature = "lang-typescript"))]
pub(in crate::engine::linker) fn typed_dom_property(
    _receiver: &str,
    _property: &str,
) -> Option<&'static str> {
    None
}

pub(in crate::engine::linker) fn resolve_applied<'a>(
    owner: &'a Node,
    args: &[TypeExpr],
    nominal: &Symbol<'a>,
) -> Option<TypeTarget<'a>> {
    if matches!(owner.language.as_str(), "typescript" | "javascript" | "vue")
        && args.len() == 1
        && matches!(nominal, Symbol::Type(TypeTarget::Builtin(name)) if name == "Promise")
    {
        Some(TypeTarget::Builtin("Promise".to_owned()))
    } else {
        None
    }
}
