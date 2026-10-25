use crate::core::models::{Facts, Node, ReceiverHint, Reference, compact_graph::Graph};
use crate::core::semantic::SourcePosition;
use crate::engine::languages::{self, dependency_registry::DependencyRegistry};
use crate::engine::linker::{
    framework_relations::{
        DeclarativeRulePrimitive, OccurrenceTable, PreparedSemanticFacts, project_scope,
    },
    interner::{LinkerInterner, SymbolId},
    semantic_context,
    value_flow::{SemanticResolver, Symbol, SymbolRole, TypeTarget},
};
use hashbrown::HashMap;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::engine::linker) fn compose_file_relations<'a>(
    path: &str,
    by_id: &HashMap<SymbolId, &'a Node>,
    dependencies: &DependencyRegistry,
    semantic_context: &semantic_context::Context<'a>,
    prepared: &PreparedSemanticFacts<'_>,
    semantic_targets: &BTreeMap<languages::semantic::SemanticIdentity, BTreeSet<SymbolId>>,
    graph: &mut Graph,
) {
    use languages::semantic::{SemanticFactRole, SemanticRelation, SemanticTarget};

    let interner = prepared.interner;
    let Some(path_facts) = interner
        .lookup_path(path)
        .and_then(|path| prepared.facts.get(&path))
    else {
        return;
    };
    let file_scope = project_scope(path, languages::LanguageFamily("python"), dependencies);
    let mut occurrences = OccurrenceTable::default();
    let mut attribute_counts = BTreeMap::<(SymbolId, &str), usize>::new();
    for fact in path_facts {
        let owner = interner
            .lookup_symbol(&fact.occurrence.owner_id)
            .expect("semantic owner");
        if let languages::semantic::SemanticFactRole::ClassAttributeToFile { attribute, .. } =
            &fact.role
        {
            *attribute_counts
                .entry((owner, attribute.as_str()))
                .or_default() += 1;
        }
        if matches!(&fact.identity.target, SemanticTarget::ImportedSymbol { .. }) {
            continue;
        }
        let SemanticFactRole::CallToFile {
            import_owner,
            alias,
            module,
            export,
            relation,
        } = &fact.role
        else {
            continue;
        };
        let entry = occurrences.entry(
            owner,
            fact.occurrence.line,
            fact.occurrence.column,
            *relation,
            &fact.expression,
            interner,
        );
        if fact.identity.scope != file_scope
            || !by_id.get(&owner).is_some_and(|owner| owner.path == path)
            || !interner
                .lookup_node(by_id, import_owner.as_str())
                .is_some_and(|owner| {
                    owner.path == path
                        && semantic_context.exact_external_import(
                            owner,
                            alias,
                            module,
                            export,
                            SourcePosition {
                                line: fact.occurrence.line,
                                column: fact.occurrence.column,
                            },
                        )
                })
        {
            continue;
        }
        if let Some(targets) = semantic_targets.get(&fact.identity) {
            entry.1.extend(targets.iter().cloned());
        }
    }
    occurrences.emit_all(
        graph,
        path,
        DeclarativeRulePrimitive::EmitEdge {
            evidence: "declarative file identity; unique proven target required",
        },
        interner,
    );

    let mut class_file_occurrences = OccurrenceTable::default();
    for fact in path_facts {
        let SemanticFactRole::ClassAttributeToFile {
            attribute,
            relation,
        } = &fact.role
        else {
            continue;
        };
        if *relation != SemanticRelation::RendersTemplate
            || fact.identity.scope != file_scope
            || !matches!(
                &fact.identity.target,
                SemanticTarget::ProjectPathSuffix { language, .. } if language == "html"
            )
        {
            continue;
        }
        let owner = interner
            .lookup_symbol(&fact.occurrence.owner_id)
            .expect("semantic owner");
        if !by_id.get(&owner).is_some_and(|owner| {
            owner.path == path && owner.language == "python" && owner.kind == "class"
        }) {
            continue;
        }
        let entry = class_file_occurrences.entry(
            owner,
            fact.occurrence.line,
            fact.occurrence.column,
            *relation,
            &fact.expression,
            interner,
        );
        if attribute_counts
            .get(&(owner, attribute.as_str()))
            .is_some_and(|count| *count > 1)
        {
            entry.2 = true;
            continue;
        }
        if let Some(targets) = semantic_targets.get(&fact.identity) {
            entry.1.extend(targets.iter().cloned());
        }
    }
    class_file_occurrences.emit_all(
        graph,
        path,
        DeclarativeRulePrimitive::EmitEdge {
            evidence: "declarative class attribute; unique template file required",
        },
        interner,
    );
}

#[derive(Clone, Copy)]
enum ImportResolution {
    Exact,
    Missing,
    Ambiguous,
}

struct ExternalImportCall<'a> {
    alias: &'a str,
    module: &'a str,
    export: &'a str,
    at: SourcePosition,
}

struct SymbolRelationRules<'a> {
    semantic: &'a [languages::semantic::EffectiveSemanticRule],
    di_calls: &'a [languages::manifests::DiCallRule],
    di_rule_origins:
        &'a std::collections::HashMap<usize, crate::core::semantic::ExtractionRuleOrigin>,
}

fn import_scope_distance(caller: &Node, importer: &Node) -> Option<usize> {
    if caller.path != importer.path {
        return None;
    }
    if caller.id == importer.id {
        return Some(0);
    }
    if importer.kind == "module" {
        return Some(usize::MAX);
    }
    if importer.kind == "class" {
        return None;
    }
    caller
        .qualname
        .strip_prefix(&importer.qualname)
        .filter(|suffix| suffix.starts_with('.'))
        .map(|suffix| suffix.matches('.').count())
}

fn has_local_binding(
    owner: &Node,
    alias: &str,
    semantic_context: &semantic_context::Context<'_>,
) -> bool {
    owner
        .binding_names()
        .chain(owner.rebinding_names())
        .any(|candidate| candidate == alias)
        || semantic_context.node_has_rebinding(owner, alias)
}

fn has_visible_rebinding(
    caller: &Node,
    importer: &Node,
    alias: &str,
    by_module: &HashMap<&str, Vec<&Node>>,
    semantic_context: &semantic_context::Context<'_>,
) -> bool {
    let Some(import_distance) = import_scope_distance(caller, importer) else {
        return true;
    };
    has_local_binding(importer, alias, semantic_context)
        || has_local_binding(caller, alias, semantic_context)
        || by_module
            .get(caller.path.as_str())
            .into_iter()
            .flatten()
            .any(|scope| {
                matches!(
                    scope.kind.as_str(),
                    "module" | "class" | "function" | "method"
                ) && scope.id != importer.id
                    && import_scope_distance(caller, scope)
                        .is_some_and(|distance| distance < import_distance)
                    && has_local_binding(scope, alias, semantic_context)
            })
}

#[allow(clippy::too_many_arguments)]
fn declared_import_alias(
    facts: &Facts,
    file_mask: u64,
    by_id: &HashMap<SymbolId, &Node>,
    interner: &LinkerInterner<'_>,
    caller: &Node,
    alias: &str,
    module: &str,
    export: &str,
) -> bool {
    let req = super::super::interner::package_mask(module);
    if (file_mask & req) != req {
        return false;
    }
    facts.references.iter().any(|reference| {
        if reference.kind != "imports"
            || reference.expression != export
            || reference.module.as_deref() != Some(module)
            || reference.alias.as_deref().unwrap_or(&reference.expression) != alias
        {
            return false;
        }
        interner
            .lookup_node(by_id, reference.source.as_str())
            .is_some_and(|importer| import_scope_distance(caller, importer).is_some())
    })
}

#[allow(clippy::too_many_arguments)]
fn exact_import_for_call<'a>(
    facts: &Facts,
    file_mask: u64,
    by_id: &HashMap<SymbolId, &'a Node>,
    caller: &'a Node,
    call: ExternalImportCall<'_>,
    by_module: &HashMap<&'a str, Vec<&'a Node>>,
    semantic_context: &semantic_context::Context<'a>,
    interner: &LinkerInterner<'_>,
) -> ImportResolution {
    let req = super::super::interner::package_mask(call.module);
    if (file_mask & req) != req {
        return ImportResolution::Missing;
    }
    let mut exact = BTreeMap::<usize, BTreeSet<&'a str>>::new();
    for reference in facts.references.iter().filter(|reference| {
        reference.kind == "imports"
            && reference.expression == call.export
            && reference.module.as_deref() == Some(call.module)
            && reference.alias.as_deref().unwrap_or(&reference.expression) == call.alias
    }) {
        let Some(importer) = interner.lookup_node(by_id, reference.source.as_str()) else {
            continue;
        };
        let Some(distance) = import_scope_distance(caller, importer) else {
            continue;
        };
        if has_visible_rebinding(caller, importer, call.alias, by_module, semantic_context) {
            continue;
        }
        if semantic_context.exact_external_import(
            importer,
            call.alias,
            call.module,
            call.export,
            call.at,
        ) {
            exact
                .entry(distance)
                .or_default()
                .insert(importer.id.as_str());
        }
    }
    let Some((_, owners)) = exact.first_key_value() else {
        return ImportResolution::Missing;
    };
    match owners.len() {
        0 => ImportResolution::Missing,
        1 => ImportResolution::Exact,
        _ => ImportResolution::Ambiguous,
    }
}

#[allow(clippy::too_many_arguments)]
fn imported_type_targets<'a>(
    facts: &Facts,
    file_mask: u64,
    by_id: &HashMap<SymbolId, &'a Node>,
    caller: &'a Node,
    expression: &str,
    at: SourcePosition,
    by_module: &HashMap<&'a str, Vec<&'a Node>>,
    semantic_context: &semantic_context::Context<'a>,
    interner: &LinkerInterner<'_>,
) -> Option<BTreeSet<SymbolId>> {
    if file_mask == 0 {
        return None;
    }
    let mut imports = BTreeMap::<usize, BTreeSet<&str>>::new();
    for reference in facts.references.iter().filter(|reference| {
        reference.kind == "imports"
            && reference.expression == expression
            && reference.alias.as_deref().unwrap_or(&reference.expression) == expression
            && reference.line <= at.line
    }) {
        let Some(module) = reference
            .module
            .as_deref()
            .filter(|module| !module.is_empty())
        else {
            continue;
        };
        let Some(importer) = interner.lookup_node(by_id, reference.source.as_str()) else {
            continue;
        };
        let Some(distance) = import_scope_distance(caller, importer) else {
            continue;
        };
        if has_visible_rebinding(caller, importer, expression, by_module, semantic_context) {
            continue;
        }
        imports.entry(distance).or_default().insert(module);
    }
    let (_, modules) = imports.first_key_value()?;
    let mut targets = BTreeSet::new();
    for module in modules {
        let module_path = module.replace('.', "/");
        let module_file = format!("{module_path}.py");
        let package_file = format!("{module_path}/__init__.py");
        let mut module_targets = by_module
            .get(module_file.as_str())
            .into_iter()
            .flatten()
            .chain(by_module.get(package_file.as_str()).into_iter().flatten())
            .filter(|target| {
                target.kind == "class"
                    && target.language == "python"
                    && target.name == expression
                    && (target.path == module_file || target.path == package_file)
            });
        let target = module_targets.next()?;
        targets.insert(interner.lookup_symbol(&target.id).expect("catalog target"));
        for target in module_targets {
            targets.insert(interner.lookup_symbol(&target.id).expect("catalog target"));
        }
    }
    (!targets.is_empty()).then_some(targets)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::engine::linker) fn compose_symbol_relations<'a>(
    path: &str,
    facts: &Facts,
    references: &[Reference],
    dependencies: &languages::dependency_registry::DependencyRegistry,
    semantic_context: &semantic_context::Context<'a>,
    by_module: &HashMap<&'a str, Vec<&'a Node>>,
    by_id: &HashMap<SymbolId, &'a Node>,
    prepared: &PreparedSemanticFacts<'_>,
    graph: &mut Graph,
) {
    use languages::{
        manifests::{SemanticCallee, SemanticOperation, SemanticTarget},
        semantic::SemanticRelation,
    };

    let interner = prepared.interner;
    let class_facts = interner
        .lookup_path(path)
        .and_then(|path| prepared.facts.get(&path))
        .map_or(&[][..], Vec::as_slice);
    let has_injects = references
        .iter()
        .any(|reference| reference.kind == "injects");
    if !has_injects
        && !class_facts.iter().any(|fact| {
            matches!(
                fact.role,
                languages::semantic::SemanticFactRole::ClassAttributeToSymbol { .. }
            )
        })
    {
        return;
    }
    let mut occurrences = OccurrenceTable::default();
    let scope = project_scope(path, languages::LanguageFamily("python"), dependencies);
    let file_mask = super::super::interner::file_import_mask(facts);

    if has_injects {
        let context = dependencies.language_context_for_path(path);
        let rules = SymbolRelationRules {
            semantic: &context.semantic_rules,
            di_calls: &context.di_calls,
            di_rule_origins: &context.di_rule_origins,
        };
        for effective in rules
            .semantic
            .iter()
            .filter(|rule| rule.active && rule.scope == scope)
        {
            let SemanticOperation::CallToSymbol {
                language,
                callee: SemanticCallee::Imported { module, export },
                argument,
                target:
                    SemanticTarget::ImportedSymbol {
                        language: target_language,
                    },
                relation: SemanticRelation::InjectsDependency,
            } = &effective.rule.operation
            else {
                continue;
            };
            if language != "python" || target_language != "python" {
                continue;
            }

            for injection in references
                .iter()
                .filter(|reference| reference.kind == "injects")
            {
                let Some(caller) = interner
                    .lookup_node(by_id, injection.source.as_str())
                    .filter(|owner| owner.path == path && owner.language == *language)
                else {
                    continue;
                };
                let mut calls: Vec<_> = references
                    .iter()
                    .filter(|reference| {
                        reference.kind == "calls"
                            && reference.source == injection.source
                            && reference.line == injection.line
                            && reference.column <= injection.column
                            && rules.di_calls.iter().enumerate().any(|(index, rule)| {
                                rule.callee == reference.expression
                                    && rule.argument == *argument
                                    && matches!(&injection.receiver_hint,
                                    Some(ReceiverHint::ConfiguredReference { origin })
                                        if rules.di_rule_origins.get(&index) == Some(origin))
                            })
                            && (reference.expression == *export
                                || declared_import_alias(
                                    facts,
                                    file_mask,
                                    by_id,
                                    interner,
                                    caller,
                                    &reference.expression,
                                    module,
                                    export,
                                ))
                    })
                    .collect();
                let Some(nearest_column) = calls.iter().map(|call| call.column).max() else {
                    continue;
                };
                calls.retain(|call| call.column == nearest_column);

                let entry = occurrences.entry(
                    interner.lookup_symbol(&caller.id).expect("catalog caller"),
                    injection.line,
                    injection.column,
                    SemanticRelation::InjectsDependency,
                    &injection.expression,
                    interner,
                );
                if calls.len() != 1 {
                    entry.2 = true;
                    continue;
                }
                let call = calls[0];
                match exact_import_for_call(
                    facts,
                    file_mask,
                    by_id,
                    caller,
                    ExternalImportCall {
                        alias: &call.expression,
                        module,
                        export,
                        at: SourcePosition {
                            line: call.line,
                            column: call.column,
                        },
                    },
                    by_module,
                    semantic_context,
                    interner,
                ) {
                    ImportResolution::Missing => {
                        entry.3 = true;
                        continue;
                    }
                    ImportResolution::Ambiguous => {
                        entry.2 = true;
                        continue;
                    }
                    ImportResolution::Exact => {}
                }

                match semantic_context.resolve_symbol(
                    caller,
                    &injection.expression,
                    SourcePosition {
                        line: injection.line,
                        column: injection.column,
                    },
                    SymbolRole::Callable,
                ) {
                    Symbol::Callable(target) if target.language == *target_language => {
                        entry
                            .1
                            .insert(interner.lookup_symbol(&target.id).expect("catalog target"));
                    }
                    Symbol::Ambiguous => entry.2 = true,
                    Symbol::Unknown | Symbol::Callable(_) | Symbol::Type(_) => entry.3 = true,
                }
            }
        }
    }

    occurrences.emit_all(
        graph,
        path,
        DeclarativeRulePrimitive::EmitEdge {
            evidence:
                "declarative imported symbol; exact import and unique callable target required",
        },
        interner,
    );

    let mut attribute_counts = BTreeMap::<(SymbolId, &str), usize>::new();
    for fact in class_facts {
        if let languages::semantic::SemanticFactRole::ClassAttributeToSymbol { attribute, .. } =
            &fact.role
        {
            *attribute_counts
                .entry((
                    interner
                        .lookup_symbol(&fact.occurrence.owner_id)
                        .expect("semantic owner"),
                    attribute.as_str(),
                ))
                .or_default() += 1;
        }
    }
    let mut model_occurrences = OccurrenceTable::default();
    for fact in class_facts {
        let languages::semantic::SemanticFactRole::ClassAttributeToSymbol {
            attribute,
            relation,
        } = &fact.role
        else {
            continue;
        };
        let SemanticTarget::PythonSymbol {
            language: target_language,
        } = &fact.identity.target
        else {
            continue;
        };
        if *relation != SemanticRelation::UsesModel
            || target_language != "python"
            || fact.identity.scope != scope
        {
            continue;
        }
        let owner = interner
            .lookup_symbol(&fact.occurrence.owner_id)
            .expect("semantic owner");
        let Some(caller) = by_id.get(&owner).copied().filter(|owner| {
            owner.path == path && owner.language == "python" && owner.kind == "class"
        }) else {
            continue;
        };
        let entry = model_occurrences.entry(
            owner,
            fact.occurrence.line,
            fact.occurrence.column,
            *relation,
            &fact.expression,
            interner,
        );
        if attribute_counts
            .get(&(owner, attribute.as_str()))
            .is_some_and(|count| *count > 1)
        {
            entry.2 = true;
            continue;
        }
        let expression = fact.expression.as_str();
        let valid_expression = !expression.is_empty()
            && expression.split('.').all(|segment| {
                let mut characters = segment.chars();
                characters
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                    && characters.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            });
        if !valid_expression {
            entry.3 = true;
            continue;
        }
        match semantic_context.resolve_symbol(
            caller,
            expression,
            SourcePosition {
                line: fact.occurrence.line,
                column: fact.occurrence.column,
            },
            SymbolRole::Type,
        ) {
            Symbol::Type(TypeTarget::Local(target)) if target.language == "python" => {
                entry
                    .1
                    .insert(interner.lookup_symbol(&target.id).expect("catalog target"));
            }
            Symbol::Type(TypeTarget::Candidates {
                targets,
                complete,
                exhausted,
            }) => {
                if targets.iter().all(|target| target.language == "python") {
                    if targets.len() > 1 {
                        entry.2 = true;
                    } else if complete && !exhausted {
                        if let Some(target) = targets.first() {
                            entry.1.insert(
                                interner.lookup_symbol(&target.id).expect("catalog target"),
                            );
                        } else {
                            entry.3 = true;
                        }
                    } else {
                        entry.3 = true;
                    }
                } else {
                    entry.3 = true;
                }
            }
            Symbol::Type(TypeTarget::Ambiguous) => entry.2 = true,
            Symbol::Type(TypeTarget::Unknown) | Symbol::Unknown => {
                if let Some(targets) = imported_type_targets(
                    facts,
                    file_mask,
                    by_id,
                    caller,
                    expression,
                    SourcePosition {
                        line: fact.occurrence.line,
                        column: fact.occurrence.column,
                    },
                    by_module,
                    semantic_context,
                    interner,
                ) {
                    if targets.len() > 1 {
                        entry.2 = true;
                    } else {
                        entry.1.extend(targets);
                    }
                } else {
                    entry.3 = true;
                }
            }
            Symbol::Type(_) => entry.3 = true,
            Symbol::Ambiguous => entry.2 = true,
            Symbol::Callable(_) => entry.3 = true,
        }
    }
    model_occurrences.emit_all(
        graph,
        path,
        DeclarativeRulePrimitive::EmitEdge {
            evidence: "declarative class attribute; exact Python type and unique target required",
        },
        interner,
    );
}
