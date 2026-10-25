use super::super::interner::{LinkerInterner, SymbolId};
use super::super::receivers;
use super::super::semantic_context::LanguageMemberLookup;
use super::super::typescript::resolution::{
    has_indexed_name, has_indexed_symbol, has_indexed_symbol_id,
};
use crate::core::models::{Node, ReceiverHint, Reference};
use crate::engine::languages::LanguageProfile;
use hashbrown::{HashMap, HashSet};

pub(in crate::engine::linker) fn nullable_parameter_wrapper<F>(
    profile: Option<&dyn LanguageProfile>,
    raw_annotation: &str,
    node: &Node,
    references: &[Reference],
    external_import_line: F,
) -> bool
where
    F: FnOnce(&str, &Node) -> Option<usize>,
{
    if !profile.is_some_and(|profile| profile.id() == "python") {
        return false;
    }
    let Some(annotation) = super::super::receiver_type_name(raw_annotation) else {
        return false;
    };
    let (head, member) = annotation.split_once('.').unwrap_or((&annotation, ""));
    let Some(import_line) = external_import_line(head, node) else {
        return false;
    };
    if import_line > node.line {
        return false;
    }
    references.iter().any(|reference| {
        if reference.kind != "imports"
            || reference.line != import_line
            || reference.alias.as_deref() != Some(head)
        {
            return false;
        }
        let Some(module @ ("typing" | "typing_extensions")) = reference.module.as_deref() else {
            return false;
        };
        match (member, reference.expression.as_str()) {
            ("", "Optional" | "Union") => true,
            ("Optional" | "Union", expression) if expression == module => true,
            _ => false,
        }
    })
}

pub(in crate::engine::linker) fn dynamic_type_aliases<'ctx, 'refs: 'ctx, 'nodes: 'ctx>(
    profile: Option<&dyn LanguageProfile>,
    references: &'refs [Reference],
    by_id: &HashMap<SymbolId, &'nodes Node>,
    interner: &LinkerInterner<'_>,
    file_module: &'ctx str,
) -> HashMap<&'ctx str, HashSet<&'ctx str>> {
    let mut aliases = HashMap::new();
    if !profile.is_some_and(|profile| profile.id() == "python") {
        return aliases;
    }
    for reference in references.iter().filter(|reference| {
        reference.kind == "imports"
            && reference.expression == "Any"
            && matches!(
                reference.module.as_deref(),
                Some("typing" | "typing_extensions")
            )
    }) {
        let scope = interner
            .lookup_node(by_id, reference.source.as_str())
            .map_or(file_module, |node| node.qualname.as_str());
        aliases
            .entry(scope)
            .or_default()
            .insert(reference.alias.as_deref().unwrap_or("Any"));
    }
    aliases
}

pub(in crate::engine::linker) fn is_dynamic_type_alias<'scope>(
    name: &str,
    owner: &Node,
    aliases: &HashMap<&'scope str, HashMap<&str, Vec<&Node>>>,
    dynamic_aliases: &HashMap<&'scope str, HashSet<&str>>,
) -> bool {
    let mut scope = owner
        .qualname
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent);
    loop {
        if aliases
            .get(scope)
            .is_some_and(|entries| entries.contains_key(name))
        {
            return dynamic_aliases
                .get(scope)
                .is_some_and(|names| names.contains(name));
        }
        if scope.is_empty() {
            return false;
        }
        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
    }
}

pub(in crate::engine::linker) fn is_dynamic_typing_annotation<'scope>(
    name: &str,
    member: &str,
    owner: &Node,
    aliases: &HashMap<&'scope str, HashMap<&str, Vec<&Node>>>,
    dynamic_aliases: &HashMap<&'scope str, HashSet<&str>>,
) -> bool {
    matches!(name, "typing" | "typing_extensions")
        && (member == "Any" || is_dynamic_type_alias(name, owner, aliases, dynamic_aliases))
}

pub(in crate::engine::linker) fn has_protocol_or_abc_base(node: &Node) -> bool {
    node.details
        .get("bases")
        .and_then(|bases| bases.as_str())
        .is_some_and(|bases| bases.contains("Protocol") || bases.contains("ABC"))
}

pub(in crate::engine::linker) fn path_constructor_aliases<'ctx, 'refs: 'ctx, 'nodes: 'ctx>(
    profile: Option<&dyn LanguageProfile>,
    references: &'refs [Reference],
    by_id: &HashMap<SymbolId, &'nodes Node>,
    interner: &LinkerInterner<'_>,
    file_module: &'ctx str,
) -> HashMap<(&'ctx str, &'ctx str), &'ctx Reference> {
    if !profile.is_some_and(|profile| profile.id() == "python") {
        return HashMap::new();
    }
    references
        .iter()
        .filter(|reference| {
            reference.kind == "imports" && reference.module.as_deref() == Some("pathlib")
        })
        .filter_map(|reference| {
            let alias = reference.alias.as_deref()?;
            let scope = interner
                .lookup_node(by_id, reference.source.as_str())
                .map_or(file_module, |node| node.qualname.as_str());
            Some(((scope, alias), reference))
        })
        .collect()
}

pub(in crate::engine::linker) fn pathlib_member(
    profile: Option<&dyn LanguageProfile>,
    origin: &str,
    imported_expression: &str,
    tail: &str,
    member: &str,
) -> Option<&'static str> {
    if origin != "pathlib" {
        return None;
    }
    let class = if imported_expression == "pathlib" {
        tail
    } else if tail.is_empty() {
        imported_expression
    } else {
        return None;
    };
    let canonical = match class {
        "Path" => "pathlib.Path",
        "PosixPath" => "pathlib.PosixPath",
        "WindowsPath" => "pathlib.WindowsPath",
        "PurePath" => "pathlib.PurePath",
        "PurePosixPath" => "pathlib.PurePosixPath",
        "PureWindowsPath" => "pathlib.PureWindowsPath",
        _ => return None,
    };
    profile
        .filter(|profile| profile.id() == "python")
        .is_some_and(|profile| profile.builtin_member(canonical, member))
        .then_some(canonical)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::engine::linker) fn imported_module_member<'a>(
    profile: Option<&dyn LanguageProfile>,
    module: &'a Node,
    tail: &str,
    node_rebindings: &HashMap<SymbolId, HashSet<SymbolId>>,
    interner: &LinkerInterner<'_>,
    by_qual: &HashMap<&'a str, Vec<&'a Node>>,
    lookup_key: &mut String,
    member_lookup: &LanguageMemberLookup<'_, 'a>,
) -> Option<receivers::Member<'a>> {
    if !profile.is_some_and(|profile| profile.id() == "python")
        || module.kind != "module"
        || module.language != "python"
    {
        return None;
    }
    let (class_name, member) = tail.split_once('.')?;
    if member.contains('.') || has_indexed_name(interner, node_rebindings, module, class_name) {
        return None;
    }

    let declarations = by_qual
        .get(super::super::qualified(
            lookup_key,
            &module.qualname,
            class_name,
        ))?
        .iter()
        .copied()
        .filter(|candidate| candidate.path == module.path);
    let mut declarations = declarations;
    let (Some(class), None) = (declarations.next(), declarations.next()) else {
        return None;
    };
    (class.kind == "class").then(|| member_lookup.python(class, member, false))
}

/// Borrowed Python view of the shared resolution inputs needed by Python member lookup.
/// The resolution phase builds it from its existing `FileResolver` field borrows.
pub(in crate::engine::linker) struct PythonResolutionView<'ctx, 'a, 'keys, F> {
    pub(in crate::engine::linker) input: &'ctx super::super::resolution::ResolutionInput<'a, F>,
    pub(in crate::engine::linker) path: &'ctx str,
    pub(in crate::engine::linker) file_module: &'ctx str,
    pub(in crate::engine::linker) aliases:
        &'ctx HashMap<&'keys str, HashMap<&'a str, Vec<&'a Node>>>,
    pub(in crate::engine::linker) alias_import_positions:
        &'ctx HashMap<(&'keys str, &'keys str), crate::core::semantic::SourcePosition>,
    pub(in crate::engine::linker) member_lookup: &'ctx LanguageMemberLookup<'a, 'a>,
}

/// Computes Python-only dynamic receiver evidence before the shared dynamic fallback runs.
/// The caller invokes this at the existing dynamic-reference position; `Unknown` and
/// `literal_builtin == false` leave the reference to the shared status/evidence handling.
pub(in crate::engine::linker) fn dynamic_receiver_member<
    'ctx,
    'a,
    'keys,
    'result,
    'aliases,
    'external,
    F,
    AliasFor,
    ExternalFor,
>(
    context: &PythonResolutionView<'ctx, 'a, 'keys, F>,
    reference: &Reference,
    path_constructor_aliases: &HashMap<(&str, &str), &Reference>,
    candidates: &mut Vec<&'a Node>,
    lookup_key: &mut String,
    mut alias_for: AliasFor,
    mut external_for: ExternalFor,
) -> (receivers::Member<'result>, bool)
where
    'a: 'aliases,
    'a: 'result,
    'external: 'result,
    AliasFor: for<'name, 'owner> FnMut(
        &'name str,
        Option<&'owner Node>,
        bool,
    ) -> Option<&'aliases Vec<&'a Node>>,
    ExternalFor: for<'name, 'owner> FnMut(
        &'name str,
        Option<&'owner Node>,
        bool,
    ) -> Option<&'external (String, usize)>,
{
    let input = context.input;
    let path = context.path;
    let file_module = context.file_module;
    let Some(profile) = input
        .profiles
        .get(path)
        .copied()
        .filter(|profile| profile.id() == "python")
    else {
        return (receivers::Member::Unknown, false);
    };
    let owner_id = input.interner.lookup_symbol(reference.source.as_str());
    let owner = owner_id.and_then(|id| input.by_id.get(&id).copied());
    let by_qual = input.by_qual;
    let node_bindings = input.node_bindings;
    let node_rebindings = input.node_rebindings;
    let aliases = context.aliases;
    let alias_import_positions = context.alias_import_positions;
    let member_lookup = context.member_lookup;

    let mut computed: receivers::Member<'result> = receivers::Member::Unknown;
    let mut literal_builtin = false;
    match reference.receiver_hint.as_ref() {
        Some(ReceiverHint::StringLiteral { member }) => {
            literal_builtin = profile.builtin_member("str", member);
        }
        Some(ReceiverHint::Super { member }) => {
            let super_id = input.interner.lookup_symbol("super");
            let shadowed_super = owner.is_some_and(|owner| {
                let mut scope = owner.qualname.as_str();
                while !scope.is_empty() {
                    if super_id.is_some_and(|super_id| {
                        by_qual.get(scope).is_some_and(|nodes| {
                            nodes.iter().any(|node| {
                                node.path == path
                                    && has_indexed_symbol(
                                        input.interner,
                                        node_bindings,
                                        node,
                                        super_id,
                                    )
                            })
                        })
                    }) || aliases
                        .get(scope)
                        .is_some_and(|bindings| bindings.contains_key("super"))
                    {
                        return true;
                    }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                }
                false
            });
            if !shadowed_super {
                if let Some((owner_id, owner)) = owner_id.zip(owner).filter(|&(owner_id, owner)| {
                    owner.details["receiver_name"].as_str().is_some_and(|name| {
                        matches!(name, "self" | "cls")
                            && !input.interner.lookup_symbol(name).is_some_and(|name_id| {
                                has_indexed_symbol_id(node_rebindings, owner_id, name_id)
                            })
                    })
                }) {
                    let _ = owner_id;
                    if let Some((scope, _)) = owner.qualname.rsplit_once('.') {
                        if let Some([class]) = by_qual.get(scope).map(Vec::as_slice) {
                            if class.kind == "class" && class.path == path {
                                computed = member_lookup.python(class, member, true);
                            }
                        }
                    }
                }
            }
        }
        Some(
            ReceiverHint::CallResult { callee, member }
            | ReceiverHint::TypedCallResult {
                value: crate::core::semantic::ValueExpr::Call { callee, site: None },
                member,
            },
        ) => {
            let (head, tail) = callee.split_once('.').unwrap_or((callee, ""));
            let head_id = input.interner.lookup_symbol(head);
            let mut scope = owner.map_or(file_module, |owner| owner.qualname.as_str());
            let mut constructor_shadowed = false;
            candidates.clear();
            while !scope.is_empty() {
                if head_id.is_some_and(|head_id| {
                    by_qual.get(scope).is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node.path == path
                                && has_indexed_symbol(input.interner, node_bindings, node, head_id)
                        })
                    })
                }) {
                    constructor_shadowed = true;
                    break;
                }
                if let Some(nodes) = by_qual.get(super::super::qualified(lookup_key, scope, callee))
                {
                    candidates.extend(nodes.iter().copied().filter(|node| node.path == path));
                }
                if !candidates.is_empty()
                    && alias_import_positions
                        .get(&(scope, head))
                        .is_some_and(|position| {
                            position.line <= reference.line
                                && candidates
                                    .iter()
                                    .any(|candidate| candidate.line < position.line)
                        })
                {
                    candidates.clear();
                    constructor_shadowed = true;
                    break;
                }
                if !candidates.is_empty()
                    || aliases
                        .get(scope)
                        .is_some_and(|entries| entries.contains_key(head))
                {
                    break;
                }
                scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
            if candidates.is_empty()
                && !constructor_shadowed
                && aliases
                    .get(scope)
                    .is_some_and(|entries| entries.contains_key(head))
            {
                if let Some(imported) = alias_for(head, owner, false) {
                    if tail.is_empty() {
                        candidates.extend_from_slice(imported);
                    } else {
                        for target in imported {
                            if let Some(nodes) = by_qual.get(super::super::qualified(
                                lookup_key,
                                &target.qualname,
                                tail,
                            )) {
                                candidates.extend(
                                    nodes
                                        .iter()
                                        .copied()
                                        .filter(|node| node.path == target.path),
                                );
                            }
                        }
                    }
                }
            }
            candidates.sort_by_key(|node| &node.id);
            candidates.dedup_by_key(|node| &node.id);
            if let [class] = candidates.as_slice() {
                if class.kind == "class" {
                    computed = member_lookup.python(class, member, false);
                }
            }
            if candidates.is_empty() && !constructor_shadowed {
                if let Some((origin, import_line)) =
                    external_for(head, owner, false).filter(|(_, line)| *line <= reference.line)
                {
                    if let Some(import) = path_constructor_aliases
                        .get(&(scope, head))
                        .filter(|import| import.line == *import_line)
                    {
                        if let Some(canonical) =
                            pathlib_member(Some(profile), origin, &import.expression, tail, member)
                        {
                            computed = receivers::Member::External(
                                origin.as_str(),
                                *import_line,
                                Some(canonical),
                                None,
                            );
                        }
                    }
                }
            }
        }
        _ => {}
    }

    (computed, literal_builtin)
}

/// Returns whether the first visible Python import is available at this reference position.
/// Non-Python references and scopes with no matching import retain the shared resolver's fallthrough.
pub(in crate::engine::linker) fn import_visible_at<'ctx, 'a, 'keys, F>(
    context: &PythonResolutionView<'ctx, 'a, 'keys, F>,
    reference: &Reference,
    definition_scope: bool,
) -> bool {
    let path = context.path;
    if !context
        .input
        .profiles
        .get(path)
        .is_some_and(|profile| profile.id() == "python")
    {
        return true;
    }
    let owner = context
        .input
        .interner
        .lookup_node(context.input.by_id, reference.source.as_str());
    let mut import_scope = owner
        .map(|node| super::super::resolution::reference_scope(node, path))
        .unwrap_or("");
    let first = reference
        .expression
        .split_once('.')
        .map_or(reference.expression.as_str(), |(first, _)| first);
    let at = reference.position();
    let by_qual = context.input.by_qual;
    let aliases = context.aliases;
    let alias_import_positions = context.alias_import_positions;
    if definition_scope {
        import_scope = import_scope
            .rsplit_once('.')
            .map_or("", |(parent, _)| parent);
    }
    loop {
        let class_scope = !definition_scope
            && owner.is_some_and(|node| matches!(node.kind.as_str(), "function" | "method"))
            && by_qual.get(import_scope).is_some_and(|nodes| {
                nodes
                    .iter()
                    .any(|node| node.path == path && matches!(node.kind.as_str(), "class" | "impl"))
            });
        if !class_scope
            && aliases
                .get(import_scope)
                .is_some_and(|entries| entries.contains_key(first))
        {
            return alias_import_positions
                .get(&(import_scope, first))
                .is_none_or(|position| *position <= at);
        }
        if import_scope.is_empty() {
            return true;
        }
        import_scope = import_scope
            .rsplit_once('.')
            .map_or("", |(parent, _)| parent);
    }
}

/// Looks up the Python member for a typed, implicit, or imported class receiver.
/// A returned `Some(Unknown)` is a handled lookup; `None` preserves shared resolution fallback.
pub(in crate::engine::linker) fn implicit_receiver_member<'ctx, 'a, 'keys, F>(
    context: &PythonResolutionView<'ctx, 'a, 'keys, F>,
    reference: &Reference,
    typed_receiver: Option<super::super::ReceiverType<'a>>,
    known_receiver: bool,
    shadowed: bool,
    imported: Option<&Vec<&'a Node>>,
    lookup_key: &mut String,
) -> Option<receivers::Member<'a>> {
    let input = context.input;
    let path = context.path;
    let file_module = context.file_module;
    let profile = input.profiles.get(path).copied();
    let owner = input
        .interner
        .lookup_node(input.by_id, reference.source.as_str());
    let by_qual = input.by_qual;
    let member_lookup = context.member_lookup;
    let (first, tail) = reference
        .expression
        .split_once('.')
        .unwrap_or((&reference.expression, ""));
    let kind = reference.kind.as_str();
    let receiver_class =
        if profile.is_some_and(|profile| profile.id() == "python") && !tail.is_empty() {
            if let Some(super::super::ReceiverType::Local(class)) = typed_receiver {
                Some(class)
            } else if known_receiver {
                owner
                    .and_then(|owner| owner.qualname.rsplit_once('.'))
                    .and_then(|(scope, _)| by_qual.get(scope))
                    .and_then(|nodes| {
                        let mut classes = nodes
                            .iter()
                            .copied()
                            .filter(|node| node.path == path && node.kind == "class");
                        let class = classes.next()?;
                        classes.next().is_none().then_some(class)
                    })
            } else if !shadowed {
                let classes = imported.map(Vec::as_slice).unwrap_or_else(|| {
                    by_qual
                        .get(super::super::qualified(lookup_key, file_module, first))
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                });
                match classes {
                    [class] if class.kind == "class" => Some(*class),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };
    receiver_class
        .filter(|_| matches!(kind, "calls" | "handles" | "references"))
        .map(|class| member_lookup.python(class, tail, false))
}
