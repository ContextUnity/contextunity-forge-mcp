use super::*;
use crate::core::models::ReceiverHint;
use crate::core::semantic::{SourcePosition, TypeExpr, ValueFlowFacts, VueSlotFact};
#[path = "vue/template.rs"]
mod template;

/// Represents vue data.
pub struct Vue;
/// Shared vue language profile.
pub static VUE: Vue = Vue;
impl LanguageProfile for Vue {
    fn id(&self) -> &'static str {
        "vue"
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
        typescript::TYPESCRIPT.builtin(name) || compiler_macro(name) || runtime_builtin(name)
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
        extract_file_impl(path, source, module, facts, None)
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
) -> Result<()> {
    let mut rest = source;
    let mut base = 0;
    let mut script_setup_ranges = Vec::new();
    let mut setup_positions = Vec::new();
    let mut script_setup_bindings = std::collections::BTreeSet::new();
    let mut setup_callables = BTreeMap::new();
    let mut duplicate_callables = HashSet::new();
    let mut typed_props = BTreeMap::new();
    let mut typed_iterables = BTreeMap::new();
    let mut typed_slots = Vec::new();
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
        if setup_script && script_setup_ranges.len() < 128 {
            let absolute_start = base + opening;
            let absolute_end = absolute_start + script.len();
            let start = source_position(source, absolute_start);
            let end = source_position(source, absolute_end);
            setup_positions.push((start, end));
            script_setup_ranges.push(serde_json::json!({
                "start_line": start.line,
                "start_column": start.column,
                "end_line": end.line,
                "end_column": end.column,
            }));
        } else if setup_script {
            script_setup_ranges_overflowed = true;
        }
        let lang = if rest[start..opening].contains("lang=\"ts\"")
            || rest[start..opening].contains("lang='ts'")
        {
            "typescript"
        } else {
            "javascript"
        };
        let profile: &dyn LanguageProfile = if lang == "typescript" {
            &typescript::TYPESCRIPT
        } else {
            &typescript::JAVASCRIPT
        };
        let offset = source[..base + opening]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let column = source[..base + opening]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .len();
        let tree = ast::parse_island(
            &mut profile.create_parser(path)?,
            script,
            base + opening,
            tree_sitter::Point::new(offset, column),
        )
        .context("Tree-sitter parse cancelled")?;
        if setup_script {
            script_setup_bindings.extend(profile.bindings(tree.root_node(), source).all);
            collect_setup_callables(tree.root_node(), source, &mut setup_callables, &mut duplicate_callables);
            collect_typed_props(tree.root_node(), source, &mut typed_props, &mut duplicate_typed_props);
            if lang == "typescript" {
                collect_typed_iterables(tree.root_node(), source, &mut typed_iterables, &mut duplicate_typed_iterables);
                if !typed_slots_invalid {
                    typed_slots_invalid = collect_typed_slots(tree.root_node(), source, &mut typed_slots, &mut typed_slot_calls);
                }
            }
            collect_shadowed_producers(tree.root_node(), source, &mut shadowed_producers);
            collect_setup_reassignments(tree.root_node(), source, &mut setup_reassignments);
        } else {
            collect_registered_components(
                tree.root_node(),
                source,
                &mut registered_components,
                &mut duplicate_components,
            );
            collect_setup_reassignments(tree.root_node(), source, &mut component_reassignments);
        }
        ast::extract_tree_with_flows(
            profile,
            tree.root_node(),
            path,
            source,
            module,
            &format!("module:{path}"),
            facts,
            flows.as_deref_mut(),
        );
        base += end + 9;
        rest = &source[base..];
    }

    if let Some(module_node) = facts
        .nodes
        .iter_mut()
        .find(|node| node.kind == "module" && node.path == path)
    {
        module_node.details["vue_script_setup"] = serde_json::Value::Array(
            if script_setup_ranges_overflowed {
                Vec::new()
            } else {
                script_setup_ranges
            },
        );
        if !script_setup_ranges_overflowed && !typed_slots_invalid
            && !shadowed_producers.contains("defineSlots") && !typed_slots.is_empty() {
            if let Some(store) = flows.as_deref_mut() {
                if let Some(flow) = store.get_mut(&module_node.id) {
                    flow.vue_slots.append(&mut typed_slots);
                } else {
                    store.insert(module_node.id.clone(), ValueFlowFacts { vue_slots: typed_slots, ..ValueFlowFacts::default() });
                }
            } else {
                let flow = module_node.details.get("value_flow")
                    .and_then(|value| serde_json::from_value::<ValueFlowFacts>(value.clone()).ok())
                    .unwrap_or_default();
                let mut flow = flow;
                flow.vue_slots.append(&mut typed_slots);
                module_node.details["value_flow"] = serde_json::to_value(flow).expect("valid Vue slot flow");
            }
        }
    }

    let imported_aliases: HashSet<String> = facts.references.iter()
        .filter(|reference| reference.kind == "imports" && reference.module.is_some())
        .filter_map(|reference| reference.alias.clone())
        .collect();
    let imported_define_component = facts.references.iter().any(|reference| {
        reference.kind == "imports"
            && reference.expression == "defineComponent"
            && reference.alias.as_deref() == Some("defineComponent")
            && reference.module.as_deref() == Some("vue")
    });
    registered_components.retain(|name, (alias, wrapped)| {
        !duplicate_components.contains(name)
            && !component_reassignments.contains(alias.as_str())
            && imported_aliases.contains(alias.as_str())
            && (!*wrapped || imported_define_component)
    });
    let component_aliases: BTreeMap<_, _> = registered_components
        .into_iter()
        .map(|(name, (alias, _))| (name, alias))
        .collect();
    typed_iterables.retain(|name, _| {
        !duplicate_typed_iterables.contains(name) && !setup_reassignments.contains(name)
    });
    template::extract(path, source, module, template::TemplateBindings {
        setup_positions: &setup_positions,
        registered_components: &component_aliases,
        typed_iterables: &typed_iterables,
    }, facts, flows)?;
    if !script_setup_ranges_overflowed {
        let nodes_by_id: HashMap<&str, &Node> = facts.nodes.iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        let template_scopes: HashSet<&str> = facts.nodes.iter()
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
                    *field_path_counts.entry((position.line, member)).or_default() += 1;
                }
            }
        }
        let mut typed_prop_fields = BTreeMap::new();
        for (binding, properties) in &typed_props {
            if duplicate_typed_props.contains(binding)
                || setup_reassignments.contains(binding)
                || shadowed_producers.contains("defineProps")
            { continue; }
            for (member_path, position) in properties {
                let Some(member) = member_path.rsplit('.').next() else { continue; };
                if field_path_counts.get(&(position.line, member)) != Some(&1) { continue; }
                let mut matches = facts.nodes.iter().filter(|node| {
                    node.path == path && node.kind == "field" && node.name == member
                        && node.line == position.line
                });
                if let Some(field) = matches.next() {
                    if matches.next().is_none() {
                        typed_prop_fields.insert((binding.clone(), member_path.clone()), field.id.clone());
                    }
                }
            }
        }
        for reference in &mut facts.references {
            if reference.kind == "calls" {
                let at = Position { line: reference.line, column: reference.column };
                let owner = nodes_by_id.get(reference.source.as_str()).copied();
                let in_template = owner.is_some_and(|node| node.kind == "template_scope");
                let in_setup = setup_positions.iter().any(|(start, end)| *start <= at && at < *end);
                if (in_template || in_setup)
                    && setup_callables.get(reference.expression.as_str()).is_some_and(|(hint, declaration)| {
                        !setup_reassignments.contains(reference.expression.as_str())
                            && !duplicate_callables.contains(reference.expression.as_str())
                            && !shadowed_producers.contains(match hint {
                                ReceiverHint::VueSetupMacroCallable => "defineEmits",
                                _ => return false,
                            })
                            && (in_template || *declaration < at)
                    })
                    && !setup_callable_shadowed(owner, reference.expression.as_str(), &facts.nodes)
                {
                    reference.receiver_hint = setup_callables.get(reference.expression.as_str()).map(|(hint, _)| hint.clone());
                }
            }
            if reference.kind == "references"
                && template_scopes.contains(reference.source.as_str())
                && script_setup_bindings.contains(reference.expression.as_str())
            {
                reference.receiver_hint = Some(ReceiverHint::VueSetupBinding);
            }
            if reference.kind == "references"
                && template_scopes.contains(reference.source.as_str())
            {
                let expression = if reference.expression.contains("?.") {
                    std::borrow::Cow::Owned(reference.expression.replace("?.", "."))
                } else {
                    std::borrow::Cow::Borrowed(reference.expression.as_str())
                };
                if let Some((binding, member_path)) = expression.split_once('.') {
                    let owner = nodes_by_id.get(reference.source.as_str()).copied();
                    if !member_path.is_empty()
                        && !setup_callable_shadowed(owner, binding, &facts.nodes)
                    {
                        if let Some(field_id) = typed_prop_fields.get(&(binding.to_owned(), member_path.to_owned())) {
                            reference.receiver_hint = Some(ReceiverHint::VueTypedPropMember { field_id: field_id.clone() });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_typed_iterables(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, TypeExpr>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "lexical_declaration"
            || !statement.child_by_field_name("kind").is_some_and(|kind| text(kind, source) == "const")
        {
            continue;
        }
        for declaration in statement.named_children(&mut statement.walk()) {
            if declaration.kind() != "variable_declarator" { continue; }
            let (Some(name), Some(annotation)) = (
                declaration.child_by_field_name("name").filter(|node| node.kind() == "identifier"),
                declaration.child_by_field_name("type"),
            ) else { continue; };
            let Some(ty) = annotation.named_child(0) else { continue; };
            let element = if ty.kind() == "array_type" {
                ty.named_child(0)
            } else if ty.kind() == "generic_type"
                && ty.child_by_field_name("name").is_some_and(|base| text(base, source) == "Array")
            {
                ty.child_by_field_name("type_arguments").and_then(|args| {
                    (args.named_child_count() == 1).then(|| args.named_child(0)).flatten()
                })
            } else {
                None
            };
            let Some(element) = element.filter(|node| matches!(node.kind(), "type_identifier" | "identifier")) else { continue; };
            let binding = text(name, source).to_owned();
            if output.insert(binding.clone(), TypeExpr::Named { name: text(element, source).to_owned() }).is_some() {
                duplicates.insert(binding);
            }
        }
    }
}

fn collect_typed_slots(
    root: Syntax<'_>,
    source: &str,
    output: &mut Vec<VueSlotFact>,
    calls: &mut usize,
) -> bool {
    let is_slot_call = |node: Syntax<'_>| {
        node.kind() == "call_expression"
            && node.child_by_field_name("function")
                .is_some_and(|callee| callee.kind() == "identifier" && text(callee, source) == "defineSlots")
    };
    for statement in root.named_children(&mut root.walk()) {
        if matches!(statement.kind(), "lexical_declaration" | "variable_declaration") {
            for declaration in statement.named_children(&mut statement.walk()) {
                if declaration.kind() == "variable_declarator"
                    && declaration.child_by_field_name("value").is_some_and(is_slot_call)
                { return true; }
            }
            continue;
        }
        if statement.kind() != "expression_statement" { continue; }
        let Some(call) = statement.named_child(0).filter(|node| node.kind() == "call_expression") else { continue; };
        if !is_slot_call(call) { continue; }
        *calls += 1;
        if *calls > 1 { return true; }
        let Some(args) = call.child_by_field_name("type_arguments") else { return true; };
        if args.named_child_count() != 1 { return true; }
        let Some(slots) = args.named_child(0).filter(|node| node.kind() == "object_type") else { return true; };
        for signature in slots.named_children(&mut slots.walk()) {
            if signature.kind() != "method_signature" { return true; }
            let Some(name) = signature.child_by_field_name("name").filter(|node| node.kind() == "property_identifier") else { return true; };
            let Some(params) = signature.child_by_field_name("parameters") else { return true; };
            if params.named_child_count() != 1 { return true; }
            let Some(param) = params.named_child(0).filter(|node| node.kind() == "required_parameter") else { return true; };
            let Some(props) = param.child_by_field_name("type")
                .and_then(|annotation| annotation.named_child(0))
                .filter(|node| node.kind() == "object_type") else { return true; };
            for property in props.named_children(&mut props.walk()) {
                if property.kind() != "property_signature" { return true; }
                let Some(binding) = property.child_by_field_name("name").filter(|node| node.kind() == "property_identifier") else { return true; };
                let Some(ty) = property.child_by_field_name("type")
                    .and_then(|annotation| annotation.named_child(0))
                    .filter(|node| matches!(node.kind(), "type_identifier" | "identifier")) else { return true; };
                if output.len() >= 128 { return true; }
                let at = source_position(source, binding.start_byte());
                output.push(VueSlotFact {
                    slot: text(name, source).to_owned(),
                    binding: text(binding, source).to_owned(),
                    position: SourcePosition { line: at.line, column: at.column },
                    type_expr: TypeExpr::Named { name: text(ty, source).to_owned() },
                });
            }
        }
    }
    false
}

fn collect_registered_components(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, (String, bool)>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "export_statement"
            || !text(statement, source).trim_start().starts_with("export default")
        {
            continue;
        }
        let Some(value) = statement.child_by_field_name("value") else {
            continue;
        };
        let (options, wrapped) = if value.kind() == "object" {
            (value, false)
        } else if value.kind() == "call_expression"
            && value
                .child_by_field_name("function")
                .is_some_and(|callee| text(callee, source) == "defineComponent")
        {
            let Some(arguments) = value.child_by_field_name("arguments") else {
                continue;
            };
            if arguments.named_child_count() != 1 {
                continue;
            }
            let Some(options) = arguments.named_child(0).filter(|node| node.kind() == "object") else {
                continue;
            };
            (options, true)
        } else {
            continue;
        };
        let mut cursor = options.walk();
        let mut properties = options
            .named_children(&mut cursor)
            .filter(|node| {
                node.kind() == "pair"
                    && node
                        .child_by_field_name("key")
                        .is_some_and(|key| text(key, source) == "components")
            });
        let Some(property) = properties.next() else {
            continue;
        };
        if properties.next().is_some() {
            continue;
        }
        let Some(registrations) = property
            .child_by_field_name("value")
            .filter(|node| node.kind() == "object")
        else {
            continue;
        };
        for entry in registrations.named_children(&mut registrations.walk()) {
            if entry.kind() != "pair" {
                continue;
            }
            let (Some(key), Some(value)) = (
                entry.child_by_field_name("key"),
                entry.child_by_field_name("value"),
            ) else {
                continue;
            };
            if value.kind() != "identifier" {
                continue;
            }
            let name = match key.kind() {
                "property_identifier" => text(key, source),
                "string" => {
                    let raw = text(key, source);
                    if raw.len() < 2 || raw.contains('\\') {
                        continue;
                    }
                    &raw[1..raw.len() - 1]
                }
                _ => continue,
            };
            if name.is_empty() || (output.len() >= 128 && !output.contains_key(name)) {
                continue;
            }
            if output
                .insert(name.to_owned(), (text(value, source).to_owned(), wrapped))
                .is_some()
            {
                duplicates.insert(name.to_owned());
            }
        }
    }
}

fn collect_typed_props(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, BTreeMap<String, Position>>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "lexical_declaration"
            || !statement.child_by_field_name("kind").is_some_and(|kind| text(kind, source) == "const")
        {
            continue;
        }
        for declaration in statement.named_children(&mut statement.walk()) {
            if declaration.kind() != "variable_declarator" { continue; }
            let (Some(name), Some(value)) = (
                declaration.child_by_field_name("name"),
                declaration.child_by_field_name("value"),
            ) else { continue; };
            if name.kind() != "identifier" || value.kind() != "call_expression"
                || !value.child_by_field_name("function").is_some_and(|callee| text(callee, source) == "defineProps")
            { continue; }
            let Some(type_arguments) = value.child_by_field_name("type_arguments") else { continue; };
            let Some(object_type) = type_arguments.named_child(0).filter(|node| node.kind() == "object_type") else { continue; };
            if type_arguments.named_child_count() != 1 { continue; }
            let mut properties = BTreeMap::new();
            collect_typed_prop_members(object_type, source, "", &mut properties, 0);
            let binding = text(name, source).to_owned();
            if output.insert(binding.clone(), properties).is_some() {
                duplicates.insert(binding);
            }
        }
    }
}

fn collect_typed_prop_members(
    object_type: Syntax<'_>,
    source: &str,
    prefix: &str,
    output: &mut BTreeMap<String, Position>,
    depth: usize,
) {
    if depth >= 8 || output.len() >= 128 { return; }
    let mut unique = BTreeMap::new();
    let mut duplicates = HashSet::new();
    for property in object_type.named_children(&mut object_type.walk()) {
        if property.kind() != "property_signature" { continue; }
        let Some(key) = property.child_by_field_name("name").filter(|key| key.kind() == "property_identifier") else { continue; };
        let member = text(key, source).to_owned();
        if unique.insert(member.clone(), (property, key)).is_some() {
            duplicates.insert(member);
        }
    }
    for member in duplicates { unique.remove(&member); }
    for (member, (property, key)) in unique {
        if output.len() >= 128 { break; }
        let path = if prefix.is_empty() { member } else { format!("{prefix}.{member}") };
        output.insert(path.clone(), source_position(source, key.start_byte()));
        if let Some(nested) = property.child_by_field_name("type")
            .and_then(|annotation| annotation.named_child(0))
            .filter(|node| node.kind() == "object_type")
        {
            collect_typed_prop_members(nested, source, &path, output, depth + 1);
        }
    }
}

fn collect_setup_callables(
    root: Syntax<'_>,
    source: &str,
    output: &mut BTreeMap<String, (ReceiverHint, Position)>,
    duplicates: &mut HashSet<String>,
) {
    for statement in root.named_children(&mut root.walk()) {
        if statement.kind() != "lexical_declaration"
            || !statement.child_by_field_name("kind").is_some_and(|kind| text(kind, source) == "const")
        {
            continue;
        }
        for declaration in statement.named_children(&mut statement.walk()) {
            if declaration.kind() != "variable_declarator" { continue; }
            let (Some(name), Some(value)) = (
                declaration.child_by_field_name("name"),
                declaration.child_by_field_name("value"),
            ) else { continue; };
            if value.kind() != "call_expression" { continue; }
            let Some(callee) = value.child_by_field_name("function") else { continue; };
            let producer = text(callee, source);
            let callable = match (name.kind(), producer) {
                ("identifier", "defineEmits") => Some((text(name, source), ReceiverHint::VueSetupMacroCallable)),
                _ => None,
            };
            if let Some((binding, hint)) = callable {
                let position = source_position(source, declaration.end_byte());
                if output.insert(binding.to_owned(), (hint, position)).is_some() {
                    duplicates.insert(binding.to_owned());
                }
            }
        }
    }
}

fn collect_shadowed_producers(root: Syntax<'_>, source: &str, output: &mut HashSet<String>) {
    for statement in root.named_children(&mut root.walk()) {
        match statement.kind() {
            "function_declaration" | "class_declaration" => {
                if let Some(name) = statement.child_by_field_name("name") {
                    let name = text(name, source);
                    if matches!(name, "defineEmits" | "defineProps" | "defineSlots") { output.insert(name.to_owned()); }
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                for declaration in statement.named_children(&mut statement.walk()) {
                    if let Some(name) = declaration.child_by_field_name("name") {
                        let name = text(name, source);
                        if matches!(name, "defineEmits" | "defineProps" | "defineSlots") { output.insert(name.to_owned()); }
                    }
                }
            }
            "import_statement" => {
                let mut pending = vec![statement];
                while let Some(node) = pending.pop() {
                    if node.kind() == "import_specifier" {
                        if let Some(local) = node.child_by_field_name("alias")
                            .or_else(|| node.child_by_field_name("name"))
                        {
                            let name = text(local, source);
                            if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
                                output.insert(name.to_owned());
                            }
                        }
                        continue;
                    }
                    if node.kind() == "identifier" {
                        let name = text(node, source);
                        if matches!(name, "defineEmits" | "defineProps" | "defineSlots") {
                            output.insert(name.to_owned());
                        }
                        continue;
                    }
                    if matches!(node.kind(), "import_statement" | "import_clause" | "named_imports" | "namespace_import") {
                        pending.extend(node.named_children(&mut node.walk()));
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_setup_reassignments(root: Syntax<'_>, source: &str, output: &mut HashSet<String>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "assignment_expression" | "augmented_assignment_expression") {
            if let Some(left) = node.child_by_field_name("left") {
                if left.kind() == "identifier" { output.insert(text(left, source).to_owned()); }
            }
        } else if node.kind() == "update_expression" {
            if let Some(argument) = node.child_by_field_name("argument") {
                if argument.kind() == "identifier" { output.insert(text(argument, source).to_owned()); }
            }
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
}

fn setup_callable_shadowed(owner: Option<&Node>, name: &str, nodes: &[Node]) -> bool {
    let Some(owner) = owner else { return true; };
    let mut scope = owner.qualname.as_str();
    while !scope.is_empty() {
        if nodes.iter().any(|node| {
            node.path == owner.path && node.kind != "module" && node.qualname == scope
                && node.details["bindings"].as_array().is_some_and(|bindings| {
                    bindings.iter().any(|binding| binding.as_str() == Some(name))
                })
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

pub(crate) fn compiler_macro(name: &str) -> bool {
    matches!(
        name,
        "defineProps"
            | "defineEmits"
            | "defineExpose"
            | "withDefaults"
            | "defineOptions"
            | "defineSlots"
            | "defineModel"
    )
}

pub(crate) fn runtime_builtin(name: &str) -> bool {
    matches!(
        name,
        "ref"
            | "computed"
            | "reactive"
            | "shallowRef"
            | "shallowReactive"
            | "toRef"
            | "toRefs"
            | "unref"
            | "isRef"
            | "watch"
            | "watchEffect"
            | "onMounted"
            | "onUnmounted"
            | "onUpdated"
            | "onBeforeMount"
            | "onBeforeUnmount"
            | "nextTick"
            | "inject"
            | "provide"
            | "useSlots"
            | "useAttrs"
            | "useI18n"
            | "useFetch"
            | "useAsyncData"
            | "useLazyFetch"
            | "useLazyAsyncData"
            | "useRoute"
            | "useRouter"
            | "navigateTo"
            | "useState"
            | "useCookie"
            | "useHead"
            | "useSeoMeta"
            | "useError"
            | "createError"
            | "clearError"
            | "defineNuxtComponent"
            | "defineNuxtRouteMiddleware"
            | "definePageMeta"
            | "abortNavigation"
            | "setResponseStatus"
            | "useRuntimeConfig"
            | "useNuxtApp"
            | "useRequestEvent"
            | "useRequestHeaders"
            | "useResponseEvent"
    )
}

pub(crate) fn admits_macro(
    module: &crate::core::models::Node,
    at: crate::core::semantic::SourcePosition,
) -> bool {
    if module.kind != "module" || module.language != "vue" {
        return false;
    }
    let Some(ranges) = module.details.get("vue_script_setup").and_then(|value| value.as_array())
    else {
        return false;
    };
    if ranges.len() > 128 {
        return false;
    }

    let at = Position {
        line: at.line,
        column: at.column,
    };
    let mut admitted = false;
    for range in ranges {
        let Some(object) = range.as_object() else {
            return false;
        };
        if object.len() != 4
            || !["start_line", "start_column", "end_line", "end_column"]
                .iter()
                .all(|key| object.contains_key(*key))
        {
            return false;
        }
        let coordinate = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
        };
        let (Some(start_line), Some(start_column), Some(end_line), Some(end_column)) = (
            coordinate("start_line"),
            coordinate("start_column"),
            coordinate("end_line"),
            coordinate("end_column"),
        ) else {
            return false;
        };
        let start = Position {
            line: start_line,
            column: start_column,
        };
        let end = Position {
            line: end_line,
            column: end_column,
        };
        if start >= end {
            return false;
        }
        admitted |= at >= start && at < end;
    }
    admitted
}

pub(crate) fn extract_typed(
    path: &str,
    source: &str,
    module: &str,
    typed: &mut crate::core::typed_facts::TypedFacts,
) -> Result<()> {
    extract_file_impl(
        path,
        source,
        module,
        &mut typed.facts,
        Some(&mut typed.flows),
    )?;
    VUE.finish(&mut typed.facts);
    Ok(())
}
