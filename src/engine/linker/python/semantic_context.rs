use super::super::semantic_context::{Context, Imported};
use super::super::value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget};
use crate::core::models::{Facts, Node, Reference};
use crate::core::semantic::{SourcePosition, TypeExpr};
use crate::engine::languages::{
    LanguageFamily, LanguageProfile, dependency_registry::DependencyRegistry,
};
use crate::engine::linker::interner::LinkerInterner;
use crate::engine::linker::traits::ModulesByNamespace;

pub(in crate::engine::linker) fn receiver_access(owner: &Node, head: &str) -> Option<(bool, bool)> {
    (owner.language == "python"
        && matches!(head, "self" | "cls")
        && owner.details["receiver_name"].as_str() == Some(head))
    .then_some((false, false))
}

pub(in crate::engine::linker) fn is_external_generic_base(
    module: &str,
    clean_base: &str,
    profile: Option<&dyn LanguageProfile>,
) -> bool {
    matches!(module, "typing" | "typing_extensions" | "collections.abc")
        && profile.is_some_and(|profile| profile.builtin_generic(clean_base))
}

pub(in crate::engine::linker) fn external_factory_result<'a>(
    context: &Context<'a>,
    owner: &'a Node,
    callee: &str,
    at: SourcePosition,
) -> Option<TypeTarget<'a>> {
    if owner.language != "python" {
        return None;
    }
    let (head, member) = callee.split_once('.').unwrap_or((callee, ""));
    if !matches!(member, "" | "getLogger" | "LoggerAdapter" | "connect") {
        return None;
    }
    let mut scope = owner.qualname.as_str();
    loop {
        for node in context.nodes_in_path(scope, &owner.path) {
            if let Some(import) = context.imported_alias(node, head) {
                let Imported::External(imported_module, position, reference, true) = import else {
                    return None;
                };
                if !matches!(imported_module.as_str(), "logging" | "sqlite3") || *position > at {
                    return None;
                }
                let canonical = match (reference.expression.as_str(), member) {
                    ("logging", "getLogger") | ("getLogger", "") => "logging.Logger",
                    ("logging", "LoggerAdapter") | ("LoggerAdapter", "") => "logging.LoggerAdapter",
                    ("sqlite3", "connect") | ("connect", "") => "sqlite3.Connection",
                    _ => return None,
                };
                let Symbol::Type(TypeTarget::External {
                    module,
                    import_line,
                }) = context.resolve_symbol(owner, callee, at, SymbolRole::Callable)
                else {
                    return None;
                };
                if !matches!(module.as_str(), "logging" | "sqlite3") || position.line != import_line
                {
                    return None;
                }
                return Some(TypeTarget::Builtin(canonical.to_owned()));
            }
        }
        scope = scope.rsplit_once('.')?.0;
    }
}

pub(in crate::engine::linker) enum ExternalMemberOutcome {
    Supported(&'static str),
    Unsupported,
}

pub(in crate::engine::linker) fn external_member(
    owner: &Node,
    module: &str,
    member: &str,
) -> Option<ExternalMemberOutcome> {
    if owner.language != "python" {
        return None;
    }

    let origin = match module {
        "sqlite3.Cursor" | "sqlite3.Connection" => "builtin:sqlite3",
        "logging.LoggerAdapter" => "builtin:logging.LoggerAdapter",
        _ => return None,
    };

    Some(
        if crate::engine::languages::by_id("python")
            .is_some_and(|profile| profile.builtin_member(module, member))
        {
            ExternalMemberOutcome::Supported(origin)
        } else {
            ExternalMemberOutcome::Unsupported
        },
    )
}

pub(in crate::engine::linker) fn is_nullable_generic(
    owner: &Node,
    clean_base: &str,
    nominal: &Symbol<'_>,
) -> bool {
    owner.language == "python"
        && match nominal {
            Symbol::Type(TypeTarget::Builtin(wrapper)) => {
                matches!(wrapper.as_str(), "Optional" | "Union")
            }
            Symbol::Unknown => matches!(clean_base, "Union" | "Optional"),
            _ => false,
        }
}

pub(in crate::engine::linker) fn typing_nullable_generic(
    module: &str,
    reference: &Reference,
    member: &str,
) -> Option<&'static str> {
    if !matches!(module, "typing" | "typing_extensions") {
        return None;
    }
    match (reference.expression.as_str(), member) {
        ("Optional", "") => Some("Optional"),
        ("Union", "") => Some("Union"),
        (origin, "Optional") if origin == module => Some("Optional"),
        (origin, "Union") if origin == module => Some("Union"),
        _ => None,
    }
}

pub(in crate::engine::linker) fn external_import_type<'a>(
    owner: &Node,
    module: &str,
    reference: &Reference,
    head: &str,
    member: &str,
    role: SymbolRole,
    import_line: usize,
) -> Option<Symbol<'a>> {
    if owner.language != "python" {
        return None;
    }
    if matches!(role, SymbolRole::Constructor) {
        return Some(Symbol::Unknown);
    }

    let nullable = typing_nullable_generic(module, reference, member);
    if matches!(module, "typing" | "typing_extensions" | "collections.abc")
        && (nullable.is_some()
            || crate::engine::languages::by_id("python").is_some_and(|profile| {
                profile.builtin_generic(head) || profile.builtin_member(head, "get")
            }))
    {
        return Some(Symbol::Type(TypeTarget::Builtin(
            nullable.unwrap_or(head).to_owned(),
        )));
    }

    if module == "logging"
        && (matches!(member, "Logger" | "LoggerAdapter")
            || matches!(head, "Logger" | "LoggerAdapter"))
    {
        let class = if matches!(member, "Logger" | "LoggerAdapter") {
            member
        } else {
            head
        };
        return Some(Symbol::Type(TypeTarget::External {
            module: format!("logging.{class}"),
            import_line,
        }));
    }

    if matches!(role, SymbolRole::Type)
        && module == "sqlite3"
        && ((reference.expression == "sqlite3" && matches!(member, "Cursor" | "Connection"))
            || (matches!(reference.expression.as_str(), "Cursor" | "Connection")
                && member.is_empty()))
    {
        let class = if reference.expression == "sqlite3" {
            member
        } else {
            reference.expression.as_str()
        };
        return Some(Symbol::Type(TypeTarget::External {
            module: format!("sqlite3.{class}"),
            import_line,
        }));
    }

    None
}

pub(in crate::engine::linker) fn nullable_payload<'a>(
    wrapper: &str,
    args: &'a [TypeExpr],
) -> Option<&'a TypeExpr> {
    match (wrapper, args) {
        ("Optional", [payload]) => Some(payload),
        ("Union", [TypeExpr::Named { name }, payload]) if name == "None" => Some(payload),
        ("Union", [payload, TypeExpr::Named { name }]) if name == "None" => Some(payload),
        _ => None,
    }
}

pub(in crate::engine::linker) fn has_proven_type_checking(
    path: &str,
    facts: &Facts,
    modules_by_namespace: &ModulesByNamespace<'_>,
    dependencies: &DependencyRegistry,
    family: LanguageFamily,
    interner: &LinkerInterner<'_>,
    node_rebindings: &crate::engine::linker::NodeBindingIndex,
) -> bool {
    let rebound = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module" && node.path == path)
        .is_some_and(|module| {
            module.rebinding_names().any(|name| name == "TYPE_CHECKING")
                || super::super::typescript::resolution::has_indexed_name(
                    interner,
                    node_rebindings,
                    module,
                    "TYPE_CHECKING",
                )
        });
    if rebound {
        return false;
    }

    let (workspace, _) = crate::engine::languages::workspace_path(path);
    let is_shadowed = |module_name: &str| {
        modules_by_namespace
            .get(&family)
            .and_then(|namespaces| namespaces.get(module_name))
            .is_some_and(|modules| {
                modules.iter().any(|module| {
                    crate::engine::languages::workspace_path(&module.path).0 == workspace
                })
            })
    };

    facts.references.iter().any(|reference| {
        if reference.kind != "imports" {
            return false;
        }
        let proves = |module: &str| {
            (module == "typing" && !is_shadowed("typing"))
                || (module == "typing_extensions"
                    && !is_shadowed("typing_extensions")
                    && (dependencies.declares_for_path(family, path, "typing-extensions")
                        || dependencies.declares_for_path(family, path, "typing_extensions")))
        };
        if reference.expression == "TYPE_CHECKING" {
            return reference.module.as_deref().is_some_and(proves);
        }
        reference
            .module
            .as_deref()
            .is_some_and(|module| reference.expression == module && proves(module))
    })
}

pub(in crate::engine::linker) fn type_only_import_is_unproven(
    profile: &dyn LanguageProfile,
    path: &str,
    facts: &Facts,
    modules_by_namespace: &ModulesByNamespace<'_>,
    dependencies: &DependencyRegistry,
    interner: &LinkerInterner<'_>,
    node_rebindings: &crate::engine::linker::NodeBindingIndex,
) -> bool {
    profile.id() == "python"
        && !has_proven_type_checking(
            path,
            facts,
            modules_by_namespace,
            dependencies,
            profile.family(),
            interner,
            node_rebindings,
        )
}

pub(in crate::engine::linker) fn dynamic_type_alias(
    language: &str,
    module: &str,
    expression: &str,
) -> bool {
    language == "python" && matches!(module, "typing" | "typing_extensions") && expression == "Any"
}

pub(in crate::engine::linker) fn logging_factory_import_is_safe(
    module: &Node,
    alias: &str,
    position: SourcePosition,
) -> bool {
    if module.language != "python" {
        return false;
    }
    module.details["python_logging_factories"]
        .as_array()
        .is_some_and(|entries| {
            let mut matching = entries.iter().filter(|entry| {
                entry["alias"].as_str() == Some(alias)
                    && entry["line"].as_u64() == Some(position.line as u64)
                    && entry["column"].as_u64() == Some(position.column as u64)
            });
            matching.next().is_some_and(|entry| {
                entry.as_object().is_some_and(|object| object.len() == 4)
                    && entry["safe"].as_bool() == Some(true)
            }) && matching.next().is_none()
        })
}
