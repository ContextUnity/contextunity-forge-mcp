use super::*;
use crate::core::models::ReceiverHint;
use crate::core::semantic::{SourcePosition, TypeExpr, ValueFlowFacts, VueSlotFact};
mod setup_facts;
mod template;
use setup_facts::{collect_setup_facts, SetupFactOutputs};

/// Represents vue data.
pub struct Vue;
/// Shared vue language profile.
pub static VUE: Vue = Vue;
impl LanguageProfile for Vue {
    fn id(&self) -> &'static str {
        "vue"
    }
    fn ast_search_capabilities(&self) -> AstSearchCapabilities {
        typescript::TYPESCRIPT.ast_search_capabilities()
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        typescript::TYPESCRIPT.manifest_filenames()
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        typescript::TYPESCRIPT.extract_manifest_dependencies(filename, content)
    }
    fn is_stdlib(&self, module: &str) -> bool {
        typescript::TYPESCRIPT.is_stdlib(module)
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("javascript")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["vue"]
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        typescript::TYPESCRIPT.grammar(path)
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str> {
        typescript::TYPESCRIPT.symbol_kind(kind)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        typescript::TYPESCRIPT.node_prefix(kind)
    }
    fn module_name(&self, path: &str) -> String {
        typescript::TYPESCRIPT.module_name(path)
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        typescript::TYPESCRIPT.normalize_import(owner, module)
    }
    fn normalize_import_with_root(
        &self,
        root: Option<&std::path::Path>,
        owner: &str,
        module: &str,
    ) -> Option<ImportPath> {
        typescript::TYPESCRIPT.normalize_import_with_root(root, owner, module)
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        typescript::TYPESCRIPT.external_import(module)
    }
    fn builtin(&self, name: &str) -> bool {
        typescript::TYPESCRIPT.builtin(name)
    }
    fn builtin_type(&self, name: &str) -> bool {
        typescript::TYPESCRIPT.builtin_type(name)
    }
    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        typescript::TYPESCRIPT.builtin_member(receiver, member)
    }
    fn builtin_generic(&self, receiver: &str) -> bool {
        typescript::TYPESCRIPT.builtin_generic(receiver)
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        let registry = dependency_registry::DependencyRegistry::bundled_framework_rules();
        let rules = registry.language_context_for_path(path);
        extract_file_impl(path, source, module, facts, None, &rules)
    }

    fn finish(&self, facts: &mut Facts) {
        typescript::TYPESCRIPT.finish(facts);
        if !facts
            .nodes
            .iter()
            .any(|node| node.details["default_export"] == true)
        {
            if let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") {
                module.details["default_export"] = serde_json::json!(true);
            }
        }
    }
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&VUE];

fn extract_file_impl(
    path: &str,
    source: &str,
    module: &str,
    facts: &mut Facts,
    mut flows: Option<&mut crate::core::typed_facts::FlowStore>,
    rules: &Arc<LanguageContext>,
) -> Result<()> {
    extract_file_routes(path, module, facts, rules);
    let module_id = format!("module:{path}");
    let mut rest = source;
    let mut base = 0;
    let mut setup_positions = Vec::new();
    let mut script_setup_bindings = std::collections::BTreeSet::new();
    let mut setup_callables = BTreeMap::new();
    let mut duplicate_callables = HashSet::new();
    let mut typed_props = BTreeMap::new();
    let mut typed_iterables = BTreeMap::new();
    let mut typed_slots = Vec::new();
    let mut macro_calls = Vec::new();
    let mut typed_slots_invalid = false;
    let mut typed_slot_calls = 0;
    let mut duplicate_typed_iterables = HashSet::new();
    let mut duplicate_typed_props = HashSet::new();
    let mut shadowed_producers = HashSet::new();
    let mut setup_reassignments = HashSet::new();
    let mut registered_components = BTreeMap::new();
    let mut duplicate_components = HashSet::new();
    let mut component_reassignments = HashSet::new();
    let mut script_setup_ranges_overflowed = false;
    while let Some(start) = rest.find("<script") {
        let opening = start
            + rest[start..]
                .find('>')
                .context("unterminated Vue script tag")?
            + 1;
        let end = opening
            + rest[opening..]
                .find("</script>")
                .context("unterminated Vue script block")?;
        let tag = &rest[start..opening];
        let script = &rest[opening..end];
        let setup_script = is_setup_script_tag(tag);
        let absolute_start = base + opening;
        let start_pos = source_position(source, absolute_start);
        if setup_script && setup_positions.len() < 128 {
            let end = source_position(source, absolute_start + script.len());
            setup_positions.push((start_pos, end));
        } else if setup_script {
            script_setup_ranges_overflowed = true;
        }
        let lang = if tag.contains("lang=\"ts\"") || tag.contains("lang='ts'") {
            "typescript"
        } else {
            "javascript"
        };
        let profile: &dyn LanguageProfile = if lang == "typescript" {
            &typescript::TYPESCRIPT
        } else {
            &typescript::JAVASCRIPT
        };
        let tree = with_warm_parser(lang, profile.grammar(path), |parser| {
            ast::parse_island(
                parser,
                script,
                absolute_start,
                tree_sitter::Point::new(start_pos.line - 1, start_pos.column),
            )
        })?
        .context("Tree-sitter parse cancelled")?;
        {
            let mut output = SetupFactOutputs {
                macro_calls: &mut macro_calls,
                setup_callables: &mut setup_callables,
                duplicate_callables: &mut duplicate_callables,
                typed_props: &mut typed_props,
                duplicate_typed_props: &mut duplicate_typed_props,
                typed_iterables: &mut typed_iterables,
                duplicate_typed_iterables: &mut duplicate_typed_iterables,
                typed_slots: &mut typed_slots,
                typed_slots_invalid: &mut typed_slots_invalid,
                typed_slot_calls: &mut typed_slot_calls,
                shadowed_producers: &mut shadowed_producers,
                setup_reassignments: &mut setup_reassignments,
                registered_components: &mut registered_components,
                duplicate_components: &mut duplicate_components,
                component_reassignments: &mut component_reassignments,
            };
            collect_setup_facts(
                tree.root_node(),
                source,
                setup_script,
                lang == "typescript",
                &mut output,
                rules,
            );
        }
        let bindings = ast::extract_tree_with_language_context(
            profile,
            tree.root_node(),
            path,
            source,
            module,
            &module_id,
            facts,
            flows.as_deref_mut(),
            rules,
        );
        if setup_script {
            script_setup_bindings.extend(bindings.all);
        }
        base += end + 9;
        rest = &source[base..];
    }

    macro_calls.retain(|fact| {
        !shadowed_producers.contains(&fact.callee)
            && !setup_reassignments.contains(&fact.callee)
            && fact
                .binding
                .as_ref()
                .is_none_or(|binding| !setup_reassignments.contains(binding))
    });
    if let Some(module_node) = facts
        .nodes
        .iter_mut()
        .find(|node| node.kind == "module" && node.path == path)
    {
        if script_setup_ranges_overflowed {
            macro_calls.clear();
        }
        if typed_slots_invalid
            || !macro_calls
                .iter()
                .any(|fact| fact.role == crate::core::semantic::ComponentMacroRole::Slots)
        {
            typed_slots.clear();
        }
        if !macro_calls.is_empty() || !typed_slots.is_empty() {
            let mut enrich = |flow: &mut ValueFlowFacts| {
                for fact in typed_slots.drain(..) {
                    flow.push_vue_slot(fact);
                }
                for fact in macro_calls.iter().cloned() {
                    flow.push_component_macro(fact);
                }
                for fact in &macro_calls {
                    if let (Some(binding), Some(annotation)) = (&fact.binding, &fact.annotation) {
                        for value in &mut flow.bindings {
                            if value.name == *binding
                                && matches!(&value.value, crate::core::semantic::ValueExpr::Call { callee, .. } if *callee == fact.callee)
                            {
                                value.value = crate::core::semantic::ValueExpr::Annotated {
                                    type_expr: annotation.clone(),
                                };
                            }
                        }
                    }
                }
            };
            if let Some(store) = flows.as_deref_mut() {
                if !matches!(
                    store.state(module_node),
                    Some(crate::core::typed_facts::FlowState::Invalid(_))
                ) {
                    let mut flow = store.take(&module_node.id).unwrap_or_default();
                    enrich(&mut flow);
                    store.insert(module_node.id.clone(), flow);
                    module_node.details["value_flow"] = serde_json::Value::Null;
                }
            } else {
                let mut flow = module_node
                    .details
                    .get("value_flow")
                    .and_then(|value| serde_json::from_value::<ValueFlowFacts>(value.clone()).ok())
                    .unwrap_or_default();
                enrich(&mut flow);
                module_node.details["value_flow"] =
                    serde_json::to_value(flow).expect("valid component flow");
            }
        }
    }

    let imported_aliases: HashSet<&str> = facts
        .references
        .iter()
        .filter(|reference| reference.kind == "imports" && reference.module.is_some())
        .filter_map(|reference| reference.alias.as_deref())
        .collect();
    let imported_component_wrappers: HashSet<String> = rules
        .syntax_rules
        .iter()
        .filter_map(|rule| {
            let manifests::syntax::SyntaxOperation::ComponentOptions { module, export, .. } =
                &rule.operation
            else {
                return None;
            };
            facts
                .references
                .iter()
                .any(|reference| {
                    reference.kind == "imports"
                        && reference.expression == *export
                        && reference.alias.as_deref() == Some(export)
                        && reference.module.as_deref() == Some(module)
                })
                .then(|| rule.key())
        })
        .collect();
    registered_components.retain(|name, component| {
        !duplicate_components.contains(name)
            && !component_reassignments.contains(component.alias.as_str())
            && imported_aliases.contains(component.alias.as_str())
            && component
                .wrapper
                .as_ref()
                .is_none_or(|key| imported_component_wrappers.contains(key))
    });
    typed_iterables.retain(|name, _| {
        !duplicate_typed_iterables.contains(name) && !setup_reassignments.contains(name)
    });
    template::extract(
        path,
        source,
        module,
        template::TemplateBindings {
            setup_positions: &setup_positions,
            registered_components: &registered_components,
            typed_iterables: &typed_iterables,
        },
        facts,
        flows,
    )?;
    if !script_setup_ranges_overflowed {
        let nodes_by_id: HashMap<&str, &Node> = facts
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        let template_scopes: HashSet<&str> = facts
            .nodes
            .iter()
            .filter(|node| node.kind == "template_scope")
            .map(|node| node.id.as_str())
            .collect();
        // Synthetic type-literal fields carry a line and name, but no column or
        // parent path. Reject a repeated name on the same line rather than
        // assigning one indexed field to two different nested property paths.
        let mut field_path_counts: HashMap<(usize, &str), usize> = HashMap::new();
        for properties in typed_props.values() {
            for (member_path, position) in properties {
                if let Some(member) = member_path.rsplit('.').next() {
                    *field_path_counts
                        .entry((position.line, member))
                        .or_default() += 1;
                }
            }
        }
        let mut typed_prop_fields = BTreeMap::new();
        for (binding, properties) in &typed_props {
            if duplicate_typed_props.contains(binding) || setup_reassignments.contains(binding) {
                continue;
            }
            let mut origins = macro_calls.iter().filter(|fact| {
                fact.role == crate::core::semantic::ComponentMacroRole::Props
                    && fact.binding.as_ref() == Some(binding)
            });
            let Some(origin) = origins.next().map(|fact| &fact.origin) else {
                continue;
            };
            if origins.next().is_some() {
                continue;
            }
            for (member_path, position) in properties {
                let Some(member) = member_path.rsplit('.').next() else {
                    continue;
                };
                if field_path_counts.get(&(position.line, member)) != Some(&1) {
                    continue;
                }
                let mut matches = facts.nodes.iter().filter(|node| {
                    node.path == path
                        && node.kind == "field"
                        && node.name == member
                        && node.line == position.line
                });
                if let Some(field) = matches.next() {
                    if matches.next().is_none() {
                        typed_prop_fields.insert(
                            (binding.as_str(), member_path.as_str()),
                            (field.id.as_str(), origin),
                        );
                    }
                }
            }
        }
        for reference in &mut facts.references {
            if reference.kind == "calls" {
                let at = Position {
                    line: reference.line,
                    column: reference.column,
                };
                let owner = nodes_by_id.get(reference.source.as_str()).copied();
                let in_template = owner.is_some_and(|node| node.kind == "template_scope");
                let in_setup = setup_positions
                    .iter()
                    .any(|(start, end)| *start <= at && at < *end);
                if (in_template || in_setup)
                    && setup_callables
                        .get(reference.expression.as_str())
                        .is_some_and(|(hint, declaration)| {
                            !setup_reassignments.contains(reference.expression.as_str())
                                && !duplicate_callables.contains(reference.expression.as_str())
                                && match hint {
                                    ReceiverHint::ConfiguredMacroCallable { producer, .. } => {
                                        !shadowed_producers.contains(producer)
                                            && !setup_reassignments.contains(producer)
                                    }
                                    _ => return false,
                                }
                                && (in_template || *declaration < at)
                        })
                    && !setup_callable_shadowed(owner, reference.expression.as_str(), &facts.nodes)
                {
                    reference.receiver_hint = setup_callables
                        .get(reference.expression.as_str())
                        .map(|(hint, _)| hint.clone());
                }
            } else if reference.kind == "references"
                && template_scopes.contains(reference.source.as_str())
            {
                if script_setup_bindings.contains(reference.expression.as_str()) {
                    reference.receiver_hint = Some(ReceiverHint::VueSetupBinding);
                }
                let expression = if reference.expression.contains("?.") {
                    std::borrow::Cow::Owned(reference.expression.replace("?.", "."))
                } else {
                    std::borrow::Cow::Borrowed(reference.expression.as_str())
                };
                if let Some((binding, member_path)) = expression.split_once('.') {
                    if !member_path.is_empty() {
                        if let Some((field_id, origin)) =
                            typed_prop_fields.get(&(binding, member_path))
                        {
                            let owner = nodes_by_id.get(reference.source.as_str()).copied();
                            if !setup_callable_shadowed(owner, binding, &facts.nodes) {
                                reference.receiver_hint = Some(ReceiverHint::ConfiguredTypedMember {
                                    origin: (*origin).clone(),
                                    field_id: (*field_id).to_owned(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn extract_file_routes(path: &str, module: &str, facts: &mut Facts, rules: &LanguageContext) {
    for rule in &rules.syntax_rules {
        let manifests::syntax::SyntaxOperation::FileRoute {
            directory,
            extension,
            index,
            method,
        } = &rule.operation
        else {
            continue;
        };
        let Some(relative) = path
            .strip_prefix(directory)
            .and_then(|value| value.strip_suffix(extension))
        else {
            continue;
        };
        // Each hyphen-delimited part is a static identifier; other syntax has no mapping.
        let mut segments: Vec<_> = relative.split('/').collect();
        if segments.iter().any(|segment| {
            !segment.split('-').all(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        }) {
            continue;
        }
        if segments.last().is_some_and(|segment| *segment == index) {
            segments.pop();
        }
        let route_path = format!("/{}", segments.join("/"));
        let id = format!("route:{path}:1:0:{method}:{route_path}");
        let origin = crate::core::semantic::ExtractionRuleOrigin {
            pack: rule.pack.clone(),
            rule: rule.id.clone(),
        };
        if facts.nodes.iter().any(|node| node.id == id) {
            continue;
        }
        let name = format!("{method} {route_path}");
        facts.pending_flow_nodes.push(facts.nodes.len());
        facts.nodes.push(Node {
    native_semantic: None,
            id: id.clone(), kind: "route".into(), name: name.clone(),
            qualname: format!("{module}.{name}"), path: path.into(), line: 1, end_line: 1,
            is_test: crate::core::models::is_test(path), language: "vue".into(), generated: false,
            details: serde_json::json!({"method":method,"route_path":route_path,"handler":null,"handlers":[],"route_name":null,"include_module":null,"include_namespace":null,"value_flow":ValueFlowFacts { framework: Some(Box::new(crate::core::semantic::FrameworkScopeFacts { facts: vec![crate::core::semantic::FrameworkFact::RuleOrigin(origin)], ..Default::default() })), ..Default::default() }}),
        });
        facts.edges.push(crate::core::models::Edge {
            src: format!("module:{path}"),
            dst: id,
            kind: "contains".into(),
            path: path.into(),
            line: 1,
            evidence: "file route projection".into(),
            confidence: "exact".into(),
        });
    }
}

fn setup_callable_shadowed(owner: Option<&Node>, name: &str, nodes: &[Node]) -> bool {
    let Some(owner) = owner else {
        return true;
    };
    let mut scope = owner.qualname.as_str();
    while !scope.is_empty() {
        if nodes.iter().any(|node| {
            node.path == owner.path
                && node.kind != "module"
                && node.qualname == scope
                && node.binding_names().any(|candidate| candidate == name)
        }) {
            return true;
        }
        scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
    }
    false
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Position {
    line: usize,
    column: usize,
}

fn source_position(source: &str, offset: usize) -> Position {
    let prefix = &source[..offset];
    Position {
        line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        column: prefix.rsplit('\n').next().unwrap_or("").len(),
    }
}

fn is_setup_script_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    let prefix = b"<script";
    if !bytes.starts_with(prefix) || bytes.len() <= prefix.len() {
        return false;
    }
    let boundary = bytes[prefix.len()];
    if !boundary.is_ascii_whitespace() && boundary != b'>' {
        return false;
    }
    let Some(attributes) = bytes.get(prefix.len()..bytes.len() - 1) else {
        return false;
    };
    if bytes.last() != Some(&b'>') {
        return false;
    }

    let mut setup = false;
    let mut source = false;
    let mut index = 0;
    while index < attributes.len() {
        while index < attributes.len()
            && (attributes[index].is_ascii_whitespace() || attributes[index] == b'/')
        {
            index += 1;
        }
        let name_start = index;
        while index < attributes.len()
            && !attributes[index].is_ascii_whitespace()
            && !matches!(attributes[index], b'=' | b'/')
        {
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = &attributes[name_start..index];
        setup |= name.eq_ignore_ascii_case(b"setup");
        source |= name.eq_ignore_ascii_case(b"src");

        while index < attributes.len() && attributes[index].is_ascii_whitespace() {
            index += 1;
        }
        if attributes.get(index) == Some(&b'=') {
            index += 1;
            while index < attributes.len() && attributes[index].is_ascii_whitespace() {
                index += 1;
            }
            if let Some(quote @ (b'\'' | b'"')) = attributes.get(index).copied() {
                index += 1;
                while index < attributes.len() && attributes[index] != quote {
                    index += 1;
                }
                if index == attributes.len() {
                    return false;
                }
                index += 1;
            } else {
                while index < attributes.len() && !attributes[index].is_ascii_whitespace() {
                    index += 1;
                }
            }
        }
    }
    setup && !source
}

pub(crate) fn extract_typed(
    path: &str,
    source: &str,
    module: &str,
    typed: &mut crate::core::typed_facts::TypedFacts,
    rules: &Arc<LanguageContext>,
) -> Result<()> {
    extract_file_impl(
        path,
        source,
        module,
        &mut typed.facts,
        Some(&mut typed.flows),
        rules,
    )?;
    VUE.finish(&mut typed.facts);
    typed.project_named_selectors();
    Ok(())
}
