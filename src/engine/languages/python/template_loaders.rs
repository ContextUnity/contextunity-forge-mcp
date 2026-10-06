use crate::core::models::{Facts, Node as IndexedNode};
use crate::engine::ast::{field, text};
use crate::engine::languages::SyntaxContext;
use std::path::{Component, Path};
use tree_sitter::Node;

fn loader_context(mut node: Node<'_>) -> Option<(Node<'_>, Option<Node<'_>>)> {
    let mut function = None;
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "function_definition" => {
                if function.replace(parent).is_some() {
                    return None;
                }
            }
            "class_definition" | "lambda" => return None,
            _ => {}
        }
        node = parent;
    }
    if node.kind() != "module" {
        return None;
    }
    if let Some(function) = function {
        let wrapper = function.parent()?;
        let wrapper = if wrapper.kind() == "decorated_definition" { wrapper } else { function };
        if wrapper.parent()?.kind() != "module" {
            return None;
        }
    }
    Some((node, function))
}

fn statement(node: Node<'_>) -> Node<'_> {
    if node.kind() == "expression_statement" {
        node.named_child(0).unwrap_or(node)
    } else {
        node
    }
}

fn import_binding<'a>(node: Node<'_>, name: Node<'_>, source: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
    let module = field(node, source, "module_name")?;
    let imported = field(name, source, "name").unwrap_or_else(|| text(name, source));
    let alias = field(name, source, "alias").unwrap_or(imported);
    Some((module, imported, alias))
}

pub(super) fn binding_count(root: Node<'_>, source: &str, name: &str) -> usize {
    fn targets(node: Node<'_>, source: &str, name: &str) -> usize {
        match node.kind() {
            "identifier" => usize::from(text(node, source) == name),
            "attribute" | "subscript" => 0,
            _ => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .map(|child| targets(child, source, name))
                    .sum()
            }
        }
    }
    fn visit(node: Node<'_>, source: &str, name: &str) -> usize {
        match node.kind() {
            "assignment" | "annotated_assignment" | "augmented_assignment" | "for_statement" => {
                let own = node.child_by_field_name("left")
                    .map_or(0, |left| targets(left, source, name));
                let mut cursor = node.walk();
                own + node.named_children(&mut cursor).map(|child| visit(child, source, name)).sum::<usize>()
            }
            "named_expression" | "as_pattern" => {
                let own = node.child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("alias"))
                    .map_or(0, |target| targets(target, source, name));
                let mut cursor = node.walk();
                own + node.named_children(&mut cursor).map(|child| visit(child, source, name)).sum::<usize>()
            }
            "import_from_statement" | "import_statement" => {
                let mut imports = node.walk();
                node.children_by_field_name("name", &mut imports)
                    .filter(|child| {
                        field(*child, source, "alias")
                            .or_else(|| field(*child, source, "name"))
                            .unwrap_or_else(|| text(*child, source))
                            == name
                    })
                    .count()
            }
            "function_definition" | "class_definition" =>
                (field(node, source, "name") == Some(name)) as usize,
            "delete_statement" => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor).map(|target| targets(target, source, name)).sum()
            }
            _ => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor).map(|child| visit(child, source, name)).sum()
            }
        }
    }
    visit(root, source, name)
}

pub(super) fn imported(root: Node<'_>, source: &str, alias: &str, module: &str, name: &str, before: usize) -> bool {
    if binding_count(root, source, alias) != 1 {
        return false;
    }
    let mut cursor = root.walk();
    let found = root.named_children(&mut cursor).any(|node| {
        if node.kind() != "import_from_statement" || node.end_byte() >= before {
            return false;
        }
        let mut imports = node.walk();
        let matched = node.children_by_field_name("name", &mut imports).any(|child| {
            import_binding(node, child, source)
                .is_some_and(|(origin, symbol, bound)| origin == module && symbol == name && bound == alias)
        });
        matched
    });
    found
}

fn single_argument(node: Node<'_>) -> Option<Node<'_>> {
    let arguments = node.child_by_field_name("arguments")?;
    (arguments.named_child_count() == 1).then(|| arguments.named_child(0)).flatten()
}

fn unshadowed_in_function(function: Option<Node<'_>>, source: &str, name: &str) -> bool {
    let Some(function) = function else { return true; };
    let Some(body) = function.child_by_field_name("body") else { return false; };
    if binding_count(body, source, name) != 0 {
        return false;
    }
    let Some(parameters) = function.child_by_field_name("parameters") else { return false; };
    let mut cursor = parameters.walk();
    let shadowed_parameter = parameters.named_children(&mut cursor).any(|parameter| {
        parameter.kind() == "identifier" && text(parameter, source) == name
            || parameter.named_child(0).is_some_and(|child| child.kind() == "identifier" && text(child, source) == name)
    });
    !shadowed_parameter
}

fn local_loader<'tree>(function: Node<'tree>, source: &str, name: &str, before: usize) -> Option<Node<'tree>> {
    let body = function.child_by_field_name("body")?;
    if binding_count(body, source, name) != 1 {
        return None;
    }
    let mut parameters = function.child_by_field_name("parameters")?.walk();
    if function.child_by_field_name("parameters")?.named_children(&mut parameters).any(|parameter| {
        parameter.kind() == "identifier" && text(parameter, source) == name
            || parameter.named_child(0).is_some_and(|child| child.kind() == "identifier" && text(child, source) == name)
    }) {
        return None;
    }
    let mut cursor = body.walk();
    let assignment = body.named_children(&mut cursor)
        .map(statement)
        .find(|item| item.kind() == "assignment"
            && item.end_byte() < before
            && item.child_by_field_name("left")
                .is_some_and(|left| left.kind() == "identifier" && text(left, source) == name))
        .and_then(|assignment| assignment.child_by_field_name("right"));
    assignment
}

pub(super) fn literal<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
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

enum StaticPath {
    Path(String),
    Text(String),
}

impl StaticPath {
    fn path(self) -> Option<String> {
        match self {
            Self::Path(path) => Some(path),
            Self::Text(_) => None,
        }
    }

    fn into_string(self) -> String {
        match self {
            Self::Path(path) | Self::Text(path) => path,
        }
    }
}

fn static_path(
    node: Node<'_>,
    root: Node<'_>,
    local_function: Option<Node<'_>>,
    source: &str,
    owner: &str,
    depth: usize,
) -> Option<StaticPath> {
    if depth == 0 {
        return None;
    }
    match node.kind() {
        "identifier" if text(node, source) != "__file__" => {
            let name = text(node, source);
            if binding_count(root, source, name) != 1
                || !unshadowed_in_function(local_function, source, name) {
                return None;
            }
            let mut cursor = root.walk();
            let path = root.named_children(&mut cursor)
                .map(statement)
                .find(|item| item.kind() == "assignment"
                    && item.child_by_field_name("left")
                        .is_some_and(|left| left.kind() == "identifier" && text(left, source) == name))
                .and_then(|assignment| assignment.child_by_field_name("right"))
                .filter(|value| value.end_byte() < node.start_byte())
                .and_then(|value| static_path(value, root, local_function, source, owner, depth - 1));
            path
        }
        "identifier" if text(node, source) == "__file__" => {
            (binding_count(root, source, "__file__") == 0
                && unshadowed_in_function(local_function, source, "__file__"))
                .then(|| StaticPath::Text(owner.to_owned()))
        }
        "attribute" if field(node, source, "attribute") == Some("parent") => {
            let object = node.child_by_field_name("object")?;
            let path = static_path(object, root, local_function, source, owner, depth - 1)?.path()?;
            Some(StaticPath::Path(Path::new(&path).parent()?.to_string_lossy().into_owned()))
        }
        "subscript" => {
            let collection = node.child_by_field_name("value")?;
            if collection.kind() != "attribute" || field(collection, source, "attribute") != Some("parents") {
                return None;
            }
            let index = node.child_by_field_name("subscript")?;
            if index.kind() != "integer" {
                return None;
            }
            let hops = text(index, source).parse::<usize>().ok()?.checked_add(1)?;
            if hops > 16 {
                return None;
            }
            let object = collection.child_by_field_name("object")?;
            let path = static_path(object, root, local_function, source, owner, depth - 1)?.path()?;
            let mut parent = Path::new(&path);
            for _ in 0..hops {
                parent = parent.parent()?;
            }
            Some(StaticPath::Path(parent.to_string_lossy().into_owned()))
        }
        "binary_operator" if field(node, source, "operator") == Some("/") => {
            let left = static_path(node.child_by_field_name("left")?, root, local_function, source, owner, depth - 1)?.path()?;
            let right = literal(node.child_by_field_name("right")?, source)?;
            let relative = Path::new(right);
            if relative.as_os_str().is_empty()
                || !relative.components().all(|part| matches!(part, Component::Normal(_)))
            {
                return None;
            }
            Some(StaticPath::Path(Path::new(&left).join(relative).to_string_lossy().replace('\\', "/")))
        }
        "call" => {
            let function = node.child_by_field_name("function")?;
            if function.kind() == "identifier"
                && imported(root, source, text(function, source), "pathlib", "Path", node.start_byte())
                && unshadowed_in_function(local_function, source, text(function, source)) {
                let argument = single_argument(node)?;
                (argument.kind() == "identifier" && text(argument, source) == "__file__")
                    .then(|| static_path(argument, root, local_function, source, owner, depth - 1)
                        .and_then(|value| match value {
                            StaticPath::Text(path) => Some(StaticPath::Path(path)),
                            StaticPath::Path(_) => None,
                        })).flatten()
            } else if function.kind() == "attribute" && field(function, source, "attribute") == Some("resolve")
                && node.child_by_field_name("arguments")?.named_child_count() == 0 {
                static_path(function.child_by_field_name("object")?, root, local_function, source, owner, depth - 1)
                    .and_then(|value| value.path().map(StaticPath::Path))
            } else if function.kind() == "identifier" && text(function, source) == "str"
                && binding_count(root, source, "str") == 0
                && unshadowed_in_function(local_function, source, "str") {
                static_path(single_argument(node)?, root, local_function, source, owner, depth - 1)
                    .and_then(|value| value.path().map(StaticPath::Text))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn extract(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
    let Some(function) = ctx.node.child_by_field_name("function") else { return; };
    if function.kind() != "identifier" {
        return;
    }
    let Some(arguments) = ctx.node.child_by_field_name("arguments") else { return; };
    let mut candidate_cursor = arguments.walk();
    let has_loader = arguments.named_children(&mut candidate_cursor)
        .any(|argument| argument.kind() == "keyword_argument"
            && field(argument, ctx.source, "name") == Some("loader"));
    if !has_loader {
        return;
    }
    let Some((root, local_function)) = loader_context(ctx.node) else { return; };
    if let Some(function_scope) = local_function {
        let Some(body) = function_scope.child_by_field_name("body") else { return; };
        let Some(return_statement) = ctx.node.parent() else { return; };
        if return_statement.kind() != "return_statement" || return_statement.parent() != Some(body) {
            return;
        }
    }
    if !imported(root, ctx.source, text(function, ctx.source), "jinja2", "Environment", ctx.node.start_byte()) {
        return;
    }
    if !unshadowed_in_function(local_function, ctx.source, text(function, ctx.source)) {
        return;
    }
    let mut cursor = arguments.walk();
    let mut roots = arguments.named_children(&mut cursor).filter_map(|argument| {
        if argument.kind() != "keyword_argument" || field(argument, ctx.source, "name") != Some("loader") {
            return None;
        }
        let loader = argument.child_by_field_name("value")?;
        let loader = if loader.kind() == "identifier" {
            local_loader(local_function?, ctx.source, text(loader, ctx.source), ctx.node.start_byte())?
        } else {
            loader
        };
        if loader.kind() != "call" {
            return None;
        }
        let provider = loader.child_by_field_name("function")?;
        if provider.kind() != "identifier"
            || !imported(root, ctx.source, text(provider, ctx.source), "jinja2", "FileSystemLoader", loader.start_byte())
            || !unshadowed_in_function(local_function, ctx.source, text(provider, ctx.source))
        {
            return None;
        }
        let module = facts.nodes.iter().find(|node| node.kind == "module" && node.language == "python")?;
        static_path(single_argument(loader)?, root, local_function, ctx.source, &module.path, 16)
            .map(StaticPath::into_string)
    });
    let Some(path) = roots.next() else { return; };
    if roots.next().is_some() || path.is_empty() || Path::new(&path).is_absolute() {
        return;
    }
    drop(roots);
    let Some(module) = facts.nodes.iter().find(|node| node.kind == "module" && node.language == "python") else { return; };
    facts.nodes.push(IndexedNode {
        id: format!("py:{}:{}:template_loader_root:{}", module.path, ctx.line(), path),
        kind: "template_loader_root".into(),
        name: path.clone(),
        qualname: format!("{}.template_loader_root", module.qualname),
        path: module.path.clone(),
        line: ctx.line(),
        end_line: ctx.line(),
        is_test: false,
        language: "python".into(),
        generated: false,
        details: serde_json::json!({"provider":"jinja2","import_modules":["jinja2","pathlib"]}),
    });
}
