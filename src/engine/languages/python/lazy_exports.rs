use crate::engine::{
    ast::{field, text},
    languages::LazyExport,
};
use std::collections::HashMap;
use tree_sitter::Node;

fn literal<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    let value = text(node, source);
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') || value.contains('\\') {
        return None;
    }
    let value = value.strip_prefix(quote)?.strip_suffix(quote)?;
    (!value.contains(quote)).then_some(value)
}

fn literal_set<'a>(mut node: Node<'_>, source: &'a str) -> Option<Vec<&'a str>> {
    if node.kind() == "call" {
        if field(node, source, "function") != Some("frozenset") {
            return None;
        }
        let arguments = node.child_by_field_name("arguments")?;
        if arguments.named_child_count() != 1 {
            return None;
        }
        node = arguments.named_child(0)?;
    }
    if !matches!(node.kind(), "set" | "list" | "tuple") {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(|item| literal(item, source))
        .collect()
}

fn invalidate_rebound_sets<'a>(
    node: Node<'_>,
    source: &'a str,
    originals: &HashMap<&'a str, usize>,
    sets: &mut HashMap<&'a str, Option<Vec<&'a str>>>,
) {
    fn target<'a>(
        left: Node<'_>,
        binding_id: usize,
        source: &'a str,
        originals: &HashMap<&'a str, usize>,
        sets: &mut HashMap<&'a str, Option<Vec<&'a str>>>,
    ) {
        if left.kind() == "identifier" {
            let name = text(left, source);
            if originals
                .get(name)
                .is_some_and(|original| *original != binding_id)
            {
                sets.insert(name, None);
            }
        } else if matches!(
            left.kind(),
            "tuple_pattern" | "list_pattern" | "pattern_list" | "parenthesized_expression"
        ) {
            let mut cursor = left.walk();
            for child in left.named_children(&mut cursor) {
                target(child, binding_id, source, originals, sets);
            }
        }
    }
    if matches!(
        node.kind(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        if let Some(name) = field(node, source, "name").filter(|name| originals.contains_key(name))
        {
            sets.insert(name, None);
        }
        return;
    }
    if matches!(node.kind(), "assignment" | "augmented_assignment") {
        if let Some(left) = node.child_by_field_name("left") {
            target(left, node.id(), source, originals, sets);
        }
    }
    if node.kind() == "delete_statement" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            target(child, node.id(), source, originals, sets);
        }
    }
    if matches!(node.kind(), "import_statement" | "import_from_statement") {
        let mut cursor = node.walk();
        for item in node.children_by_field_name("name", &mut cursor) {
            let name = field(item, source, "alias")
                .or_else(|| field(item, source, "name"))
                .unwrap_or_else(|| text(item, source));
            if originals.contains_key(name) {
                sets.insert(name, None);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        invalidate_rebound_sets(child, source, originals, sets);
    }
}

fn export_names<'a>(
    condition: Node<'_>,
    parameter: &str,
    source: &'a str,
    sets: &HashMap<&str, Option<Vec<&'a str>>>,
) -> Option<Vec<&'a str>> {
    if condition.kind() != "comparison_operator" || condition.named_child_count() != 2 {
        return None;
    }
    if text(condition.named_child(0)?, source) != parameter {
        return None;
    }
    let right = condition.named_child(1)?;
    match condition.child_by_field_name("operators")?.kind() {
        "==" => Some(vec![literal(right, source)?]),
        "in" if right.kind() == "identifier" => {
            sets.get(text(right, source)).and_then(Clone::clone)
        }
        "in" => literal_set(right, source),
        _ => None,
    }
}

fn imported_binding(
    statement: Node<'_>,
    source: &str,
    bindings: &mut HashMap<String, (String, Option<String>)>,
) -> bool {
    if statement.kind() == "import_from_statement" {
        let Some(module) = field(statement, source, "module_name") else {
            return false;
        };
        let mut cursor = statement.walk();
        for item in statement.children_by_field_name("name", &mut cursor) {
            let name = field(item, source, "name").unwrap_or_else(|| text(item, source));
            if name == "*" {
                return false;
            }
            let alias = field(item, source, "alias").unwrap_or(name);
            let (module, member) = if module.chars().all(|ch| ch == '.') {
                (format!("{module}{name}"), None)
            } else {
                (module.to_owned(), Some(name.to_owned()))
            };
            if bindings
                .insert(alias.to_owned(), (module, member))
                .is_some()
            {
                return false;
            }
        }
        true
    } else if statement.kind() == "import_statement" {
        let mut cursor = statement.walk();
        for item in statement.children_by_field_name("name", &mut cursor) {
            let module = field(item, source, "name").unwrap_or_else(|| text(item, source));
            let alias = field(item, source, "alias")
                .unwrap_or_else(|| module.split('.').next().unwrap_or(module));
            let actual = if field(item, source, "alias").is_some() {
                module
            } else {
                alias
            };
            if bindings
                .insert(alias.to_owned(), (actual.to_owned(), None))
                .is_some()
            {
                return false;
            }
        }
        true
    } else {
        false
    }
}

fn import_module_target(
    call: Node<'_>,
    export: &str,
    parameter: &str,
    source: &str,
    importlib_bound: bool,
) -> Option<String> {
    if !importlib_bound
        || call.kind() != "call"
        || field(call, source, "function") != Some("importlib.import_module")
    {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let module = arguments.named_child(0)?;
    let module = if let Some(module) = literal(module, source) {
        module.to_owned()
    } else if module.kind() == "string" {
        let start = module.named_child(0)?;
        if !matches!(text(start, source), "f'" | "f\"") {
            return None;
        }
        let mut result = String::new();
        let mut interpolations = 0;
        let mut cursor = module.walk();
        for part in module.named_children(&mut cursor) {
            match part.kind() {
                "string_start" | "string_end" => {}
                "string_content" if !text(part, source).contains('\\') => {
                    result.push_str(text(part, source))
                }
                "interpolation"
                    if part.named_child_count() == 1
                        && part.named_child(0)?.kind() == "identifier"
                        && text(part.named_child(0)?, source) == parameter =>
                {
                    result.push_str(export);
                    interpolations += 1;
                }
                _ => return None,
            }
        }
        if interpolations != 1 {
            return None;
        }
        result
    } else {
        return None;
    };
    if module.starts_with('.') {
        if arguments.named_child_count() != 2
            || text(arguments.named_child(1)?, source) != "__name__"
        {
            return None;
        }
    } else if arguments.named_child_count() != 1 {
        return None;
    }
    Some(module)
}

fn branch_exports(
    branch: Node<'_>,
    names: &[&str],
    parameter: &str,
    source: &str,
    importlib_bound: bool,
    getattr_bound: bool,
) -> Option<Vec<LazyExport>> {
    let block = branch.child_by_field_name("consequence")?;
    let mut bindings: HashMap<String, (String, Option<String>)> = HashMap::new();
    let mut cursor = block.walk();
    let mut result = None;
    for statement in block.named_children(&mut cursor) {
        if statement.kind() == "comment" {
            continue;
        }
        if statement.kind() == "return_statement" {
            if result.is_some() {
                return None;
            }
            let value = statement.named_child(0)?;
            let exports = names
                .iter()
                .map(|name| {
                    let (target, relative_to_module) = if value.kind() == "identifier" {
                        (bindings.get(text(value, source)).cloned(), false)
                    } else if value.kind() == "call"
                        && field(value, source, "function") == Some("getattr")
                        && !getattr_bound
                        && !bindings.contains_key("getattr")
                    {
                        let args = value.child_by_field_name("arguments")?;
                        if args.named_child_count() != 2
                            || text(args.named_child(1)?, source) != parameter
                        {
                            return None;
                        }
                        let (module, member) = bindings.get(text(args.named_child(0)?, source))?;
                        if member.is_some() {
                            return None;
                        }
                        (Some((module.clone(), Some((*name).to_owned()))), false)
                    } else {
                        let module = import_module_target(
                            value,
                            name,
                            parameter,
                            source,
                            importlib_bound && !bindings.contains_key("importlib"),
                        );
                        let relative = module
                            .as_ref()
                            .is_some_and(|module| module.starts_with('.'));
                        (module.map(|module| (module, None)), relative)
                    };
                    let target = target?;
                    Some(LazyExport {
                        name: (*name).to_owned(),
                        module: target.0,
                        member: target.1,
                        relative_to_module,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            result = Some(exports);
        } else if result.is_some() || !imported_binding(statement, source, &mut bindings) {
            return None;
        }
    }
    result
}

pub(super) fn extract(root: Node<'_>, source: &str) -> Vec<LazyExport> {
    let mut cursor = root.walk();
    let getters: Vec<_> = root
        .named_children(&mut cursor)
        .filter(|node| {
            node.kind() == "function_definition"
                && field(*node, source, "name") == Some("__getattr__")
        })
        .collect();
    if getters.len() != 1 {
        return Vec::new();
    }
    let getter = getters[0];
    let module_rebindings = crate::engine::ast::scope_bindings(root, source).rebindings;
    if module_rebindings
        .iter()
        .any(|name| matches!(name.as_str(), "__getattr__" | "frozenset"))
    {
        return Vec::new();
    }
    let Some(parameters) = getter.child_by_field_name("parameters") else {
        return Vec::new();
    };
    if parameters.named_child_count() != 1 {
        return Vec::new();
    }
    let Some(parameter) = parameters
        .named_child(0)
        .and_then(|node| {
            if node.kind() == "identifier" {
                Some(node)
            } else {
                node.named_child(0)
            }
        })
        .filter(|node| node.kind() == "identifier")
    else {
        return Vec::new();
    };
    let parameter = text(parameter, source);
    if crate::engine::ast::scope_bindings(getter, source)
        .rebindings
        .iter()
        .any(|name| name == parameter)
    {
        return Vec::new();
    }
    let mut sets = HashMap::new();
    let mut set_originals = HashMap::new();
    let mut importlib_bound = false;
    let mut getattr_bound = false;
    let mut frozenset_shadowed = false;
    let mut getter_rebound = false;
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        if matches!(
            statement.kind(),
            "import_statement" | "import_from_statement"
        ) {
            let mut names = HashMap::new();
            if imported_binding(statement, source, &mut names) {
                if let Some((module, member)) = names.get("importlib") {
                    importlib_bound = module == "importlib" && member.is_none();
                }
                if names.contains_key("getattr") {
                    getattr_bound = true;
                }
                if names.contains_key("frozenset") {
                    frozenset_shadowed = true;
                }
                if names.contains_key("__getattr__") {
                    getter_rebound = true;
                }
            }
        }
        let assignment = statement
            .named_child(0)
            .filter(|node| node.kind() == "assignment");
        if let Some(assignment) = assignment {
            if let Some(left) = assignment
                .child_by_field_name("left")
                .filter(|node| node.kind() == "identifier")
            {
                let name = text(left, source);
                let value = assignment
                    .child_by_field_name("right")
                    .filter(|right| right.kind() == "call" || right.kind() == "tuple")
                    .and_then(|right| literal_set(right, source));
                if value.is_some() {
                    set_originals.insert(name, assignment.id());
                }
                if sets.contains_key(name) {
                    sets.insert(name, None);
                } else {
                    sets.insert(name, value);
                }
                if name == "importlib" {
                    importlib_bound = false;
                }
                if name == "getattr" {
                    getattr_bound = true;
                }
                if name == "frozenset" {
                    frozenset_shadowed = true;
                }
                if name == "__getattr__" {
                    getter_rebound = true;
                }
            }
        }
        if matches!(statement.kind(), "function_definition" | "class_definition") {
            if field(statement, source, "name") == Some("importlib") {
                importlib_bound = false;
            }
            if field(statement, source, "name") == Some("getattr") {
                getattr_bound = true;
            }
            if field(statement, source, "name") == Some("frozenset") {
                frozenset_shadowed = true;
            }
            if statement.kind() == "class_definition"
                && field(statement, source, "name") == Some("__getattr__")
            {
                getter_rebound = true;
            }
        }
    }
    invalidate_rebound_sets(root, source, &set_originals, &mut sets);
    if module_rebindings.iter().any(|name| name == "getattr") {
        getattr_bound = true;
    }
    if module_rebindings.iter().any(|name| name == "importlib") {
        importlib_bound = false;
    }
    if frozenset_shadowed || getter_rebound {
        return Vec::new();
    }
    let Some(body) = getter.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut cursor = body.walk();
    let mut terminated = false;
    let mut exported_names = std::collections::HashSet::new();
    for branch in body.named_children(&mut cursor) {
        if branch.kind() == "comment" {
            continue;
        }
        if terminated {
            return Vec::new();
        }
        if matches!(branch.kind(), "raise_statement" | "return_statement") {
            terminated = true;
            continue;
        }
        if branch.kind() == "expression_statement"
            && branch
                .named_child(0)
                .is_some_and(|node| node.kind() == "string")
        {
            continue;
        }
        if branch.kind() != "if_statement" {
            return Vec::new();
        }
        let mut clauses = vec![branch];
        let mut alternatives = branch.walk();
        clauses.extend(
            branch
                .children_by_field_name("alternative", &mut alternatives)
                .filter(|node| node.kind() == "elif_clause"),
        );
        for clause in clauses {
            let Some(names) = clause
                .child_by_field_name("condition")
                .and_then(|condition| export_names(condition, parameter, source, &sets))
            else {
                return Vec::new();
            };
            let Some(exports) = branch_exports(
                clause,
                &names,
                parameter,
                source,
                importlib_bound,
                getattr_bound,
            ) else {
                return Vec::new();
            };
            for export in exports {
                if !exported_names.insert(export.name.clone()) {
                    return Vec::new();
                }
                result.push(export);
            }
        }
    }
    result
}
