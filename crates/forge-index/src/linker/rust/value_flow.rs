//! Rust-only wrapper and reference projection rules used by the shared reducer.

use super::super::value_flow::{
    resolve_type, ReadTrace, RustWrapperKind, SemanticLimits, SemanticResolver, Symbol, SymbolRole,
    TypeTarget, ValueFlowIndex,
};
use forge_core::{
    models::Node,
    semantic::{SourcePosition, TypeExpr},
};
use hashbrown::HashMap;
use std::borrow::Cow;

pub(in crate::linker) fn deserializer_result<'a>(
    owner: &'a Node,
    callee: &str,
    payload: &TypeExpr,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> Option<TypeTarget<'a>> {
    let callee = callee.replace("::", ".");
    let origin = if let Some((base, member)) = callee.rsplit_once('.') {
        let Symbol::Type(TypeTarget::External { module, .. }) =
            resolver.resolve_symbol(owner, base, at, SymbolRole::Type)
        else {
            return None;
        };
        format!("{}.{member}", module.replace("::", "."))
    } else {
        let Symbol::Type(TypeTarget::External { module, .. }) =
            resolver.resolve_symbol(owner, &callee, at, SymbolRole::Callable)
        else {
            return None;
        };
        module.replace("::", ".")
    };
    let (package, member) = origin.rsplit_once('.')?;
    if !matches!(
        (package, member),
        (
            "serde_json" | "serde_yaml",
            "from_str" | "from_slice" | "from_reader"
        ) | ("serde_json", "from_value")
    ) {
        return None;
    }
    let authority = resolver.resolve_member_result(
        &TypeTarget::External {
            module: package.to_owned(),
            import_line: 0,
        },
        member,
        owner,
        at,
    )?;
    if !receiver_matches(
        &authority,
        &["Result", "std::result::Result", "core::result::Result"],
    ) {
        return None;
    }
    let payload = match payload {
        TypeExpr::Applied { base, args } if base == "__rust_call_arguments" => {
            match (member, args.as_slice()) {
                ("from_reader", [_, payload]) => payload,
                ("from_str" | "from_slice" | "from_value", [payload]) => payload,
                _ => &TypeExpr::Unknown,
            }
        }
        _ => &TypeExpr::Unknown,
    };
    Some(TypeTarget::RustWrapper {
        wrapper: RustWrapperKind::Result,
        ok: Box::new(resolve_type(owner, payload, at, resolver, limits)),
        err: None,
    })
}

pub(in crate::linker) fn constructor_result<'a>(
    index: &ValueFlowIndex<'a>,
    owner: &'a Node,
    callee: &str,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    trace: Option<&ReadTrace<'a, '_>>,
) -> Option<TypeTarget<'a>> {
    if owner.language != "rust" {
        return None;
    }
    let callee_clean = callee.replace("::", ".");
    let returned = index
        .type_of_expression_tracked(owner, &callee_clean, at, resolver, trace)
        .cloned()
        .unwrap_or(TypeTarget::Unknown);
    if !matches!(returned, TypeTarget::Unknown) {
        return Some(returned);
    }
    let (receiver, member) = callee_clean.rsplit_once('.')?;
    let (target, _borrowed) = if let Some(t) = index.lookup_tracked(owner, receiver, at, trace) {
        let (t, b, _) = referent(t);
        (t.clone(), b)
    } else {
        let Symbol::Type(target) = resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type)
        else {
            return None;
        };
        (target, false)
    };
    match &target {
        TypeTarget::Local(_) => {
            if let Some(result) = resolver.resolve_member_result(&target, member, owner, at) {
                if !matches!(result, TypeTarget::Unknown) {
                    return Some(result);
                }
            }
            let callable = match resolver.resolve_member(&target, member, owner, at) {
                Symbol::Callable(callable) => callable,
                _ => {
                    match resolver.resolve_symbol(owner, &callee_clean, at, SymbolRole::Callable) {
                        Symbol::Callable(callable) => callable,
                        _ => return None,
                    }
                }
            };
            if let Some(result) = index.return_type_tracked(callable, trace) {
                if !matches!(result, TypeTarget::Unknown) {
                    return Some(result.clone());
                }
            }
            if !matches!(member, "new" | "default" | "create" | "current" | "none") {
                return None;
            }
            return resolver
                .value_flow(callable)
                .and_then(|flow| flow.return_type.as_ref())
                .filter(|returned| matches!(returned, TypeExpr::Named { name } if name == "Self"))
                .map(|_| target.clone());
        }
        TypeTarget::Builtin(name) => {
            if matches!(member, "new" | "from" | "with_capacity" | "default") {
                return Some(target.clone());
            }
            if matches!(member, "from_utf8" | "from_utf8_lossy" | "try_from" | "now") {
                let external = TypeTarget::External {
                    module: name.clone(),
                    import_line: 0,
                };
                if let Some(result) = resolver.resolve_member_result(&external, member, owner, at) {
                    return Some(result);
                }
            }
        }
        TypeTarget::External { module, .. } => {
            let module_ref = import_path(module.trim_start_matches("::"));
            if matches!(module_ref.as_ref(), "serde_json" | "serde_yaml")
                && matches!(
                    member,
                    "from_str" | "from_slice" | "from_reader" | "from_value"
                )
            {
                return Some(TypeTarget::RustWrapper {
                    wrapper: RustWrapperKind::Result,
                    ok: Box::new(TypeTarget::Unknown),
                    err: None,
                });
            }
            let crate_name = module_ref.split('.').next()?;
            if matches!(crate_name, "std" | "alloc" | "core") {
                if matches!(member, "new" | "from" | "with_capacity" | "default") {
                    return Some(target.clone());
                }
            } else if !super::resolution::is_registered_cargo_crate(crate_name)
                && crate_name != "serde_json"
            {
                return None;
            }
            if let Some(target_type) = resolver.resolve_member_result(&target, member, owner, at) {
                let is_option = matches!(
                    member,
                    "child"
                        | "child_by_field_name"
                        | "parent"
                        | "next_sibling"
                        | "prev_sibling"
                        | "next_named_sibling"
                        | "prev_named_sibling"
                        | "parse"
                        | "parse_with"
                );
                let is_result = matches!(
                    member,
                    "open" | "open_in_memory" | "open_with_flags" | "prepare" | "prepare_cached"
                ) || (matches!(module_ref.as_ref(), "serde_json" | "serde_yaml")
                    && matches!(
                        member,
                        "from_str" | "from_slice" | "from_reader" | "from_value"
                    ))
                    || (module_ref == "git2.Repository" && matches!(member, "init" | "discover"))
                    || (matches!(member, "new" | "new_in")
                        && (module_ref.ends_with("NamedTempFile")
                            || module_ref.ends_with("TempDir")
                            || module_ref.ends_with("Regex")));

                if is_option {
                    return Some(TypeTarget::RustWrapper {
                        wrapper: RustWrapperKind::Option,
                        ok: Box::new(target_type),
                        err: None,
                    });
                } else if is_result {
                    return Some(TypeTarget::RustWrapper {
                        wrapper: RustWrapperKind::Result,
                        ok: Box::new(target_type),
                        err: None,
                    });
                } else {
                    return Some(target_type);
                }
            }
        }
        _ => return None,
    }
    match member {
        "open" | "open_in_memory" | "open_with_flags" | "from_str" | "parse" | "connect"
        | "build" | "try_new" => Some(TypeTarget::RustWrapper {
            wrapper: RustWrapperKind::Result,
            ok: Box::new(target),
            err: None,
        }),
        "new" | "default" | "create" | "current" | "none" => Some(target),
        _ => None,
    }
}

pub(in crate::linker) fn is_adapter(member: &str) -> bool {
    matches!(
        member,
        "?" | "unwrap"
            | "expect"
            | "ok"
            | "as_ref"
            | "as_mut"
            | "as_deref"
            | "cloned"
            | "copied"
            | "iter"
            | "iter_mut"
            | "into_iter"
            | "par_iter"
            | "par_iter_mut"
            | "into_par_iter"
            | "next"
            | "map"
            | "filter"
            | "enumerate"
            | "take"
            | "skip"
            | "step_by"
            | "collect"
            | "trim"
            | "trim_start"
            | "trim_end"
            | "args"
            | "to_owned"
            | "file_type"
            | "elapsed"
            | "prefix"
            | "suffix"
            | "tempdir"
            | "tempdir_in"
            | "tempfile"
            | "tempfile_in"
            | "head"
            | "peel_to_commit"
            | "peel_to_tree"
            | "tree"
            | "index"
            | "path"
            | "depth"
            | "timestamp"
            | "into"
            | "as_bytes"
            | "lines"
            | "split"
            | "chars"
            | "bytes"
            | "join"
            | "to_str"
            | "to_string_lossy"
            | "to_path_buf"
            | "canonicalize"
            | "file_name"
            | "extension"
            | "current_dir"
            | "output"
            | "status"
            | "get"
            | "get_ref"
            | "as_str"
            | "as_array"
            | "arg"
            | "and_then"
            | "filter_map"
            | "flat_map"
            | "inspect"
            | "fuse"
            | "__rust_filter_map_ok"
            | "as_slice"
            | "query_row"
            | "__rust_identity_some"
            | "prepare"
            | "prepare_cached"
            | "query"
            | "parse"
            | "parse_with"
            | "child"
            | "named_child"
            | "child_by_field_name"
            | "parent"
            | "next_sibling"
            | "prev_sibling"
            | "next_named_sibling"
            | "prev_named_sibling"
            | "children"
            | "named_children"
            | "__rust_iter_item"
            | "__rust_closure_row"
    )
}

fn receiver_matches(receiver: &TypeTarget<'_>, expected: &[&str]) -> bool {
    let (receiver, _, _) = referent(receiver);
    let matches = |name: &str| {
        expected
            .iter()
            .any(|expected| import_path(name) == import_path(expected))
    };
    match receiver {
        TypeTarget::External { module, .. } | TypeTarget::Builtin(module) => matches(module),
        TypeTarget::ConfiguredReceiver { authority, .. } => match authority.as_ref() {
            super::super::value_flow::ConfiguredReceiverAuthority::Hierarchy { rule, .. } => {
                rule.bases.iter().any(|base| matches(base))
            }
            _ => false,
        },
        _ => false,
    }
}

pub(in crate::linker) fn declared_adapter_result<'a>(
    receiver: &TypeTarget<'a>,
    member: &str,
    owner: &'a Node,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
) -> Option<TypeTarget<'a>> {
    let (receiver, borrowed, _) = referent(receiver);
    if member == "__rust_identity_some" {
        return Some(
            if !borrowed
                && matches!(
                    receiver,
                    TypeTarget::RustWrapper {
                        wrapper: RustWrapperKind::Option,
                        ..
                    }
                )
            {
                resolver
                    .resolve_call_result(owner, member, None, at, Some(receiver.clone()))
                    .unwrap_or(TypeTarget::Unknown)
            } else {
                TypeTarget::Unknown
            },
        );
    }
    let builtin_receiver = match receiver {
        TypeTarget::Builtin(name)
            if matches!(
                name.as_str(),
                "str" | "String" | "Path" | "PathBuf" | "Iterator" | "Slice"
            ) || name.starts_with("std::")
                || name.starts_with("std.") =>
        {
            Some(TypeTarget::External {
                module: name.clone(),
                import_line: 0,
            })
        }
        _ => None,
    };
    let target = resolver
        .resolve_member_result(receiver, member, owner, at)
        .or_else(|| {
            builtin_receiver
                .as_ref()
                .and_then(|receiver| resolver.resolve_member_result(receiver, member, owner, at))
        })?;
    if member == "into_iter" && receiver_matches(receiver, &["walkdir::WalkDir"]) {
        Some(TypeTarget::RustIterator {
            item: Box::new(TypeTarget::RustWrapper {
                wrapper: RustWrapperKind::Result,
                ok: Box::new(target),
                err: None,
            }),
        })
    } else if member == "next" && receiver_matches(receiver, &["rusqlite::Rows"]) {
        Some(TypeTarget::RustWrapper {
            wrapper: RustWrapperKind::Result,
            ok: Box::new(TypeTarget::RustWrapper {
                wrapper: RustWrapperKind::Option,
                ok: Box::new(target),
                err: None,
            }),
            err: None,
        })
    } else if matches!(member, "get" | "get_ref") && receiver_matches(receiver, &["rusqlite::Row"])
    {
        Some(TypeTarget::RustWrapper {
            wrapper: RustWrapperKind::Result,
            ok: Box::new(
                if receiver_matches(
                    &target,
                    &["Result", "std::result::Result", "core::result::Result"],
                ) {
                    TypeTarget::Unknown
                } else {
                    target
                },
            ),
            err: None,
        })
    } else if matches!(member, "get" | "as_str" | "as_array") {
        Some(TypeTarget::RustWrapper {
            wrapper: RustWrapperKind::Option,
            ok: Box::new(reference(&target, false)),
            err: None,
        })
    } else if matches!(member, "prepare" | "prepare_cached" | "query")
        || (member == "output" && receiver_matches(receiver, &["std::process::Command"]))
        || (matches!(
            member,
            "tempdir" | "tempdir_in" | "tempfile" | "tempfile_in"
        ) && receiver_matches(receiver, &["tempfile::Builder"]))
        || (matches!(
            member,
            "head" | "find_reference" | "find_commit" | "find_tree" | "index"
        ) && receiver_matches(receiver, &["git2::Repository"]))
        || (matches!(member, "tree" | "parent") && receiver_matches(receiver, &["git2::Commit"]))
        || (matches!(member, "resolve" | "peel_to_commit" | "peel_to_tree")
            && receiver_matches(receiver, &["git2::Reference"]))
    {
        Some(TypeTarget::RustWrapper {
            wrapper: RustWrapperKind::Result,
            ok: Box::new(target),
            err: None,
        })
    } else if (matches!(member, "parse" | "parse_with")
        && receiver_matches(receiver, &["tree_sitter::Parser"]))
        || matches!(
            member,
            "child"
                | "named_child"
                | "child_by_field_name"
                | "parent"
                | "next_sibling"
                | "prev_sibling"
                | "next_named_sibling"
                | "prev_named_sibling"
        )
    {
        Some(TypeTarget::RustWrapper {
            wrapper: RustWrapperKind::Option,
            ok: Box::new(target),
            err: None,
        })
    } else if matches!(member, "children" | "named_children") {
        Some(TypeTarget::RustIterator {
            item: Box::new(target),
        })
    } else {
        Some(target)
    }
}

pub(in crate::linker) fn resolve_reference<'a>(
    owner: &'a Node,
    mutable: bool,
    target: &TypeExpr,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> TypeTarget<'a> {
    if owner.language != "rust" {
        return TypeTarget::Unknown;
    }
    let inner = resolve_type(owner, target, at, resolver, limits);
    if matches!(inner, TypeTarget::External { .. }) {
        return inner;
    }
    TypeTarget::RustReference {
        mutable,
        target: Box::new(inner),
    }
}

pub(in crate::linker) fn resolve_reference_expr<'a>(
    owner: &'a Node,
    base: &str,
    args: &[TypeExpr],
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> Option<TypeTarget<'a>> {
    if owner.language != "rust" {
        return None;
    }
    if base == "()" {
        if owner.kind == "type" {
            return Some(TypeTarget::Unknown);
        }
        return Some(TypeTarget::RustTuple(
            args.iter()
                .map(|item| resolve_type(owner, item, at, resolver, limits))
                .collect(),
        ));
    }
    if !matches!(base, "&" | "&mut") {
        return None;
    }
    Some(match args {
        [target] => resolve_reference(owner, base == "&mut", target, at, resolver, limits),
        _ => TypeTarget::Unknown,
    })
}

pub(in crate::linker) fn resolve_applied<'a>(
    owner: &'a Node,
    base: &str,
    args: &[TypeExpr],
    nominal: &Symbol<'a>,
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
) -> Option<TypeTarget<'a>> {
    if owner.language != "rust" {
        return None;
    }
    if matches!(base, "[]" | "__rust_array_type") {
        return Some(match args {
            [item] => TypeTarget::RustSlice {
                item: Box::new(resolve_type(
                    owner,
                    item,
                    at,
                    resolver,
                    SemanticLimits::default(),
                )),
            },
            _ => TypeTarget::Unknown,
        });
    }
    let result_alias = matches!(
        base,
        "anyhow::Result"
            | "anyhow.Result"
            | "std::io::Result"
            | "std.io.Result"
            | "std::fmt::Result"
            | "std.fmt.Result"
    ) || matches!(nominal, Symbol::Type(target) if result_alias_type(target));
    if result_alias && args.len() == 1 {
        return Some(wrapper_type(
            owner,
            RustWrapperKind::Result,
            args,
            at,
            resolver,
            true,
        ));
    }
    let is_collection = matches!(
        base,
        "Vec" | "std::vec::Vec" | "alloc::vec::Vec" | "std.vec.Vec" | "alloc.vec.Vec"
    ) || matches!(nominal, Symbol::Type(target) if standard_collection(target));
    if is_collection {
        return Some(match args {
            [item] => TypeTarget::RustCollection {
                item: Box::new(resolve_type(
                    owner,
                    item,
                    at,
                    resolver,
                    SemanticLimits::default(),
                )),
            },
            _ => TypeTarget::Unknown,
        });
    }
    let standard_opt_or_res = if matches!(
        base,
        "Option"
            | "std::option::Option"
            | "core::option::Option"
            | "std.option.Option"
            | "core.option.Option"
    ) {
        Some(RustWrapperKind::Option)
    } else if matches!(
        base,
        "Result"
            | "std::result::Result"
            | "core::result::Result"
            | "std.result.Result"
            | "core.result.Result"
    ) {
        Some(RustWrapperKind::Result)
    } else if let Symbol::Type(target) = nominal {
        standard_wrapper(target)
    } else {
        None
    };
    if let Some(wrapper) = standard_opt_or_res {
        let valid_arity = match wrapper {
            RustWrapperKind::Option => args.len() == 1,
            RustWrapperKind::Result => args.len() == 2,
        };
        if !valid_arity {
            return Some(TypeTarget::Unknown);
        }
        return Some(wrapper_type(owner, wrapper, args, at, resolver, false));
    }
    if let Symbol::Type(target) = nominal {
        if args.len() == 1 {
            if let Some(wrapper) = deref_wrapper(target) {
                let mut pointee =
                    resolve_type(owner, &args[0], at, resolver, SemanticLimits::default());
                if matches!(pointee, TypeTarget::Unknown) {
                    if let TypeExpr::Applied { base, .. } = &args[0] {
                        if let Symbol::Type(TypeTarget::External {
                            module,
                            import_line,
                        }) = resolver.resolve_symbol(owner, base, at, SymbolRole::Type)
                        {
                            if matches!(
                                import_path(&module).split('.').next(),
                                Some("std" | "alloc" | "core")
                            ) {
                                pointee = TypeTarget::External {
                                    module,
                                    import_line,
                                };
                            }
                        }
                    }
                }
                return Some(TypeTarget::RustDeref {
                    wrapper,
                    target: Box::new(pointee),
                });
            }
        }
    }
    None
}

fn wrapper_type<'a>(
    owner: &'a Node,
    wrapper: RustWrapperKind,
    args: &[TypeExpr],
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    aliased_error: bool,
) -> TypeTarget<'a> {
    let ok = resolve_type(owner, &args[0], at, resolver, SemanticLimits::default());
    let err = match wrapper {
        RustWrapperKind::Option => None,
        RustWrapperKind::Result if aliased_error => Some(Box::new(TypeTarget::Unknown)),
        RustWrapperKind::Result => Some(Box::new(resolve_type(
            owner,
            &args[1],
            at,
            resolver,
            SemanticLimits::default(),
        ))),
    };
    TypeTarget::RustWrapper {
        wrapper,
        ok: Box::new(ok),
        err,
    }
}

fn standard_wrapper(target: &TypeTarget<'_>) -> Option<RustWrapperKind> {
    match target {
        TypeTarget::Builtin(name) => match name.as_str() {
            "Option" => Some(RustWrapperKind::Option),
            "Result" => Some(RustWrapperKind::Result),
            _ => None,
        },
        TypeTarget::External { module, .. } => match import_path(module).as_ref() {
            "std.option.Option" | "core.option.Option" => Some(RustWrapperKind::Option),
            "std.result.Result" | "core.result.Result" => Some(RustWrapperKind::Result),
            _ => None,
        },
        _ => None,
    }
}

fn standard_collection(target: &TypeTarget<'_>) -> bool {
    match target {
        TypeTarget::Builtin(name) => name == "Vec",
        TypeTarget::External { module, .. } => {
            matches!(
                import_path(module).as_ref(),
                "std.vec.Vec" | "alloc.vec.Vec"
            )
        }
        _ => false,
    }
}

fn result_alias_type(target: &TypeTarget<'_>) -> bool {
    matches!(target, TypeTarget::External { module, .. }
        if matches!(import_path(module).as_ref(), "anyhow.Result" | "std.io.Result" | "std.fmt.Result"))
}

fn deref_wrapper(target: &TypeTarget<'_>) -> Option<&'static str> {
    match target {
        TypeTarget::Builtin(name) if name == "Box" => Some("Box"),
        TypeTarget::External { module, .. } => match import_path(module).as_ref() {
            "std.boxed.Box" | "alloc.boxed.Box" => Some("Box"),
            "std.sync.Arc" | "alloc.sync.Arc" => Some("Arc"),
            "std.rc.Rc" | "alloc.rc.Rc" => Some("Rc"),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn import_path(module: &str) -> Cow<'_, str> {
    if module.contains("::") {
        Cow::Owned(module.replace("::", "."))
    } else {
        Cow::Borrowed(module)
    }
}

pub(in crate::linker) fn project<'a>(
    receiver: &TypeTarget<'a>,
    member: &str,
    resolver: &impl SemanticResolver<'a>,
) -> Option<TypeTarget<'a>> {
    if member == "__rust_identity_some" {
        return None;
    }
    let (receiver, borrowed, mutable) = referent(receiver);
    if member == "__rust_closure_row" {
        if let TypeTarget::External { module, .. } = receiver {
            let clean = module.replace("::", ".");
            if matches!(
                clean.as_str(),
                "rusqlite.Connection" | "rusqlite.Statement" | "rusqlite.Transaction"
            ) {
                return Some(TypeTarget::External {
                    module: "rusqlite::Row".to_string(),
                    import_line: 0,
                });
            }
        }
        return Some(TypeTarget::Unknown);
    }
    match receiver {
        TypeTarget::Builtin(name)
            if matches!(name.as_str(), "str" | "String")
                && matches!(member, "trim" | "trim_start" | "trim_end") =>
        {
            Some(TypeTarget::Builtin("str".to_owned()))
        }
        TypeTarget::RustWrapper { wrapper, ok, err } => {
            let projected = match member {
                "?" | "unwrap" | "expect" if !borrowed => (**ok).clone(),
                "ok" if *wrapper == RustWrapperKind::Result && !borrowed => {
                    TypeTarget::RustWrapper {
                        wrapper: RustWrapperKind::Option,
                        ok: ok.clone(),
                        err: None,
                    }
                }
                "as_ref" => TypeTarget::RustWrapper {
                    wrapper: *wrapper,
                    ok: Box::new(reference(ok, false)),
                    err: err
                        .as_deref()
                        .map(|error| Box::new(reference(error, false))),
                },
                "as_mut" if !borrowed || mutable => TypeTarget::RustWrapper {
                    wrapper: *wrapper,
                    ok: Box::new(reference(ok, true)),
                    err: err.as_deref().map(|error| Box::new(reference(error, true))),
                },
                "as_deref" => match deref_target(ok, resolver) {
                    Some(target) => TypeTarget::RustWrapper {
                        wrapper: *wrapper,
                        ok: Box::new(reference(&target, false)),
                        err: err
                            .as_deref()
                            .map(|error| Box::new(reference(error, false))),
                    },
                    None => TypeTarget::Unknown,
                },
                "cloned" | "copied" if !borrowed || *wrapper == RustWrapperKind::Option => {
                    clone_shared_reference(ok).map_or(TypeTarget::Unknown, |ok| {
                        TypeTarget::RustWrapper {
                            wrapper: *wrapper,
                            ok: Box::new(ok),
                            err: err.clone(),
                        }
                    })
                }
                "filter" => TypeTarget::RustWrapper {
                    wrapper: *wrapper,
                    ok: ok.clone(),
                    err: err.clone(),
                },
                "iter" => TypeTarget::RustIterator {
                    item: Box::new(reference(ok, false)),
                },
                "into_iter" => TypeTarget::RustIterator {
                    item: Box::new(if borrowed {
                        reference(ok, mutable)
                    } else {
                        (**ok).clone()
                    }),
                },
                _ => TypeTarget::Unknown,
            };
            Some(projected)
        }
        TypeTarget::RustIterator { item } => {
            if member == "__rust_iter_item" {
                return Some((**item).clone());
            }
            let projected =
                match member {
                    "next" if !borrowed || mutable => TypeTarget::RustWrapper {
                        wrapper: RustWrapperKind::Option,
                        ok: item.clone(),
                        err: None,
                    },
                    "cloned" | "copied" if !borrowed || mutable => iterator_item_by_value(item)
                        .map_or(TypeTarget::Unknown, |item| TypeTarget::RustIterator {
                            item: Box::new(item),
                        }),
                    "into_iter" | "filter" | "take" | "skip" | "step_by" | "inspect" | "fuse" => {
                        receiver.clone()
                    }
                    "__rust_filter_map_ok" => match item.as_ref() {
                        TypeTarget::RustWrapper {
                            wrapper: RustWrapperKind::Result,
                            ok,
                            ..
                        } => TypeTarget::RustIterator { item: ok.clone() },
                        _ => TypeTarget::Unknown,
                    },
                    "map" | "enumerate" | "filter_map" | "flat_map" => TypeTarget::RustIterator {
                        item: Box::new(reference(&TypeTarget::Unknown, false)),
                    },
                    "collect" => TypeTarget::RustCollection { item: item.clone() },
                    _ => TypeTarget::Unknown,
                };
            Some(projected)
        }
        TypeTarget::RustCollection { item } => {
            if member == "__rust_iter_item" {
                return Some(reference(item, false));
            }
            let item = match member {
                "as_ref" => {
                    return Some(reference(
                        &TypeTarget::RustSlice { item: item.clone() },
                        false,
                    ))
                }
                "as_slice" => return Some(TypeTarget::RustSlice { item: item.clone() }),
                "iter" => reference(item, false),
                "iter_mut" if !borrowed || mutable => reference(item, true),
                "into_iter" if borrowed => reference(item, mutable),
                "into_iter" => (**item).clone(),
                _ => return None,
            };
            Some(TypeTarget::RustIterator {
                item: Box::new(item),
            })
        }
        TypeTarget::RustSlice { item } => {
            if member == "__rust_iter_item" {
                return Some(reference(item, false));
            }
            let item = match member {
                "as_ref" => return Some(reference(receiver, false)),
                "as_slice" => return Some(TypeTarget::RustSlice { item: item.clone() }),
                "iter" => reference(item, false),
                "iter_mut" if !borrowed || mutable => reference(item, true),
                "into_iter" if borrowed => reference(item, mutable),
                "into_iter" => (**item).clone(),
                _ => return None,
            };
            Some(TypeTarget::RustIterator {
                item: Box::new(item),
            })
        }
        _ if standard_collection(receiver) => {
            let item = match member {
                "iter" => reference(&TypeTarget::Unknown, false),
                "into_iter" if borrowed => reference(&TypeTarget::Unknown, mutable),
                "into_iter" => TypeTarget::Unknown,
                _ => return None,
            };
            Some(TypeTarget::RustIterator {
                item: Box::new(item),
            })
        }
        _ => None,
    }
}

pub(in crate::linker) fn unwrap_payload<'a>(target: &TypeTarget<'a>) -> Option<TypeTarget<'a>> {
    match target {
        TypeTarget::RustWrapper { ok, .. } => Some((**ok).clone()),
        _ => None,
    }
}

pub(in crate::linker) fn trait_object<'a>(target: &TypeTarget<'a>) -> Option<&'a Node> {
    match target {
        TypeTarget::Local(node) if node.kind == "trait" => Some(*node),
        TypeTarget::RustReference { target, .. } | TypeTarget::RustDeref { target, .. } => {
            trait_object(target)
        }
        _ => None,
    }
}

pub(in crate::linker) fn local_class<'a>(mut target: &TypeTarget<'a>) -> Option<&'a Node> {
    while let TypeTarget::RustDeref { target: inner, .. }
    | TypeTarget::RustReference { target: inner, .. } = target
    {
        target = inner;
    }
    match target {
        TypeTarget::Local(node) => Some(*node),
        _ => None,
    }
}

pub(in crate::linker) fn project_deref_fields<'a>(
    fields: &mut HashMap<&'a str, HashMap<String, TypeTarget<'a>>>,
    owners: &[&'a Node],
    resolver: &impl SemanticResolver<'a>,
) {
    let mut projected = Vec::new();
    for owner in owners
        .iter()
        .filter(|owner| owner.language == "rust" && owner.kind == "struct")
    {
        let mut target = *owner;
        for _ in 0..64 {
            let Some(next) = resolver.deref_target(target) else {
                break;
            };
            if let Some(inherited) = fields.get(next.id.as_str()) {
                for (name, value) in inherited {
                    projected.push((owner.id.as_str(), name.clone(), value.clone()));
                }
            }
            target = next;
        }
    }
    for (owner, name, value) in projected {
        fields
            .entry(owner)
            .or_default()
            .entry(name)
            .or_insert(value);
    }
}

fn referent<'target, 'a>(target: &'target TypeTarget<'a>) -> (&'target TypeTarget<'a>, bool, bool) {
    let mut referent = target;
    let mut borrowed = false;
    let mut mutable = true;
    for _ in 0..8 {
        let TypeTarget::RustReference {
            mutable: reference_mutable,
            target,
        } = referent
        else {
            break;
        };
        borrowed = true;
        mutable &= *reference_mutable;
        referent = target;
    }
    (referent, borrowed, borrowed && mutable)
}

fn reference<'a>(target: &TypeTarget<'a>, mutable: bool) -> TypeTarget<'a> {
    TypeTarget::RustReference {
        mutable,
        target: Box::new(target.clone()),
    }
}

fn clone_shared_reference<'a>(target: &TypeTarget<'a>) -> Option<TypeTarget<'a>> {
    match target {
        TypeTarget::RustReference {
            mutable: false,
            target,
        } => Some((**target).clone()),
        _ => None,
    }
}

fn iterator_item_by_value<'a>(target: &TypeTarget<'a>) -> Option<TypeTarget<'a>> {
    match target {
        TypeTarget::RustReference {
            mutable: false,
            target,
        } => Some((**target).clone()),
        _ => None,
    }
}

fn deref_target<'a>(
    target: &TypeTarget<'a>,
    resolver: &impl SemanticResolver<'a>,
) -> Option<TypeTarget<'a>> {
    match target {
        TypeTarget::RustReference { target, .. } | TypeTarget::RustDeref { target, .. } => {
            Some((**target).clone())
        }
        TypeTarget::Local(node) => resolver.deref_target(node).map(TypeTarget::Local),
        TypeTarget::Builtin(name) if name == "String" => {
            Some(TypeTarget::Builtin("str".to_owned()))
        }
        TypeTarget::Builtin(name) if name == "PathBuf" => {
            Some(TypeTarget::Builtin("Path".to_owned()))
        }
        TypeTarget::External { module, .. }
            if matches!(
                import_path(module).as_ref(),
                "std.string.String" | "alloc.string.String"
            ) =>
        {
            Some(TypeTarget::Builtin("str".to_owned()))
        }
        TypeTarget::RustCollection { item } => Some(TypeTarget::RustSlice { item: item.clone() }),
        _ => None,
    }
}
