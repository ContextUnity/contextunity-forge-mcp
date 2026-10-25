//! Rust-only wrapper and reference projection rules used by the shared reducer.

use super::super::value_flow::{
    ReadTrace, RustWrapperKind, SemanticLimits, SemanticResolver, Symbol, SymbolRole, TypeTarget,
    ValueFlowIndex, resolve_type,
};
use crate::core::{
    models::Node,
    semantic::{SourcePosition, TypeExpr},
};
use hashbrown::HashMap;
use std::borrow::Cow;

pub(in crate::engine::linker) fn constructor_result<'a>(
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
    let returned = index
        .type_of_expression_tracked(owner, callee, at, resolver, trace)
        .cloned()
        .unwrap_or(TypeTarget::Unknown);
    if !matches!(returned, TypeTarget::Unknown) {
        return Some(returned);
    }
    let callee_clean = callee.replace("::", ".");
    if let Some((receiver, member)) = callee_clean.rsplit_once('.') {
        if matches!(
            member,
            "new"
                | "from"
                | "with_capacity"
                | "default"
                | "open"
                | "create"
                | "build"
                | "from_str"
                | "parse"
        ) {
            let type_sym = resolver.resolve_symbol(owner, receiver, at, SymbolRole::Type);
            if let Symbol::Type(target @ (TypeTarget::External { .. } | TypeTarget::Builtin(_))) =
                type_sym
            {
                return Some(target);
            }
        }
    }
    Some(TypeTarget::Unknown)
}

pub(in crate::engine::linker) fn is_adapter(member: &str) -> bool {
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
            | "into_iter"
            | "next"
    )
}

pub(in crate::engine::linker) fn resolve_reference<'a>(
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
    TypeTarget::RustReference {
        mutable,
        target: Box::new(resolve_type(owner, target, at, resolver, limits)),
    }
}

pub(in crate::engine::linker) fn resolve_reference_expr<'a>(
    owner: &'a Node,
    base: &str,
    args: &[TypeExpr],
    at: SourcePosition,
    resolver: &impl SemanticResolver<'a>,
    limits: SemanticLimits,
) -> Option<TypeTarget<'a>> {
    if owner.language != "rust" || !matches!(base, "&" | "&mut") {
        return None;
    }
    Some(match args {
        [target] => resolve_reference(owner, base == "&mut", target, at, resolver, limits),
        _ => TypeTarget::Unknown,
    })
}

pub(in crate::engine::linker) fn resolve_applied<'a>(
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
    if let Symbol::Type(target) = nominal {
        if standard_collection(target) {
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
        if let Some(wrapper) = standard_wrapper(target) {
            let valid_arity = match wrapper {
                RustWrapperKind::Option => args.len() == 1,
                RustWrapperKind::Result => args.len() == 2,
            };
            if !valid_arity {
                return Some(TypeTarget::Unknown);
            }
            return Some(wrapper_type(owner, wrapper, args, at, resolver, false));
        }
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

pub(in crate::engine::linker) fn project<'a>(
    receiver: &TypeTarget<'a>,
    member: &str,
    resolver: &impl SemanticResolver<'a>,
) -> Option<TypeTarget<'a>> {
    let (receiver, borrowed, mutable) = referent(receiver);
    match receiver {
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
                    _ => TypeTarget::Unknown,
                };
            Some(projected)
        }
        TypeTarget::RustCollection { item } => {
            let item = match member {
                "iter" => reference(item, false),
                "into_iter" if borrowed => reference(item, mutable),
                "into_iter" => (**item).clone(),
                _ => return None,
            };
            Some(TypeTarget::RustIterator {
                item: Box::new(item),
            })
        }
        TypeTarget::RustSlice { item } => {
            let item = match member {
                "iter" => reference(item, false),
                "into_iter" if borrowed => reference(item, mutable),
                "into_iter" => (**item).clone(),
                _ => return None,
            };
            Some(TypeTarget::RustIterator {
                item: Box::new(item),
            })
        }
        _ => None,
    }
}

pub(in crate::engine::linker) fn unwrap_payload<'a>(
    target: &TypeTarget<'a>,
) -> Option<TypeTarget<'a>> {
    match target {
        TypeTarget::RustWrapper { ok, .. } => Some((**ok).clone()),
        _ => None,
    }
}

pub(in crate::engine::linker) fn trait_object<'a>(target: &TypeTarget<'a>) -> Option<&'a Node> {
    match target {
        TypeTarget::Local(node) if node.kind == "trait" => Some(*node),
        TypeTarget::RustReference { target, .. } | TypeTarget::RustDeref { target, .. } => {
            trait_object(target)
        }
        _ => None,
    }
}

pub(in crate::engine::linker) fn local_class<'a>(mut target: &TypeTarget<'a>) -> Option<&'a Node> {
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

pub(in crate::engine::linker) fn project_deref_fields<'a>(
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
