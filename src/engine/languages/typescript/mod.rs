use super::*;
use crate::core::models::ReceiverHint;
use crate::engine::ast::{relations, routes};

mod dom;
mod frameworks;
mod prototypes;
mod types;

#[cfg(test)]
pub(super) use dom::first_callback_parameter;
use dom::*;
use frameworks::*;
use prototypes::*;
use types::*;

mod commonjs;
pub(crate) mod linker;
pub(crate) use commonjs::commonjs_bindings;
mod package_metadata;
mod scope_facts;
mod value_flow;
/// Performs language.
pub fn language(path: &str, language: &str) -> tree_sitter::Language {
    if language == "javascript"
        || path.ends_with(".js")
        || path.ends_with(".jsx")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
    {
        tree_sitter_javascript::language()
    } else if path.ends_with(".tsx") {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    }
}
/// Performs kind.
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration"
        | "generator_function_declaration"
        | "arrow_function"
        | "function_expression" => Some("function"),
        "method_definition" | "method_signature" => Some("method"),
        "class_declaration" | "class" => Some("class"),
        "enum_declaration" => Some("enum"),
        "enum_assignment" => Some("field"),
        "interface_declaration" => Some("interface"),
        "field_definition" => Some("field"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}

pub(crate) fn commonjs_default_callable(node: Syntax<'_>, source: &str) -> bool {
    if !matches!(node.kind(), "function_expression" | "arrow_function") {
        return false;
    }
    let Some(assignment) = node
        .parent()
        .filter(|parent| parent.kind() == "assignment_expression")
    else {
        return false;
    };
    if assignment
        .child_by_field_name("right")
        .is_none_or(|right| right.id() != node.id())
        || !assignment.parent().is_some_and(|parent| {
            parent.kind() == "expression_statement"
                && parent.parent().is_some_and(|root| root.kind() == "program")
        })
    {
        return false;
    }
    let Some(left) = assignment
        .child_by_field_name("left")
        .filter(|left| left.kind() == "member_expression")
    else {
        return false;
    };
    left.child_by_field_name("object")
        .is_some_and(|object| object.kind() == "identifier" && text(object, source) == "module")
        && left
            .child_by_field_name("property")
            .is_some_and(|property| {
                property.kind() == "property_identifier" && text(property, source) == "exports"
            })
}

fn required_module(value: Syntax<'_>, source: &str) -> Option<String> {
    if value.kind() != "call_expression" {
        return None;
    }
    let callee = value.child_by_field_name("function")?;
    if text(callee, source).trim() != "require" {
        return None;
    }
    let argument = value.child_by_field_name("arguments")?.named_child(0)?;
    if argument.kind() != "string" {
        return None;
    }
    let raw = text(argument, source).trim();
    let quote = raw.chars().next()?;
    if !matches!(quote, '\'' | '"') || raw.chars().last()? != quote {
        return None;
    }
    Some(raw[1..raw.len() - 1].to_owned())
}

fn collect_require_bindings(
    pattern: Syntax<'_>,
    source: &str,
    imports: &mut Vec<(String, Option<String>)>,
) {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            let name = text(pattern, source).to_owned();
            imports.push((name.clone(), Some(name)));
        }
        "pair_pattern" => {
            let Some(key) = pattern.child_by_field_name("key") else {
                return;
            };
            let Some(value) = pattern.child_by_field_name("value") else {
                return;
            };
            let local = if value.kind() == "assignment_pattern" {
                value.child_by_field_name("left").unwrap_or(value)
            } else {
                value
            };
            if matches!(
                local.kind(),
                "identifier" | "shorthand_property_identifier_pattern"
            ) {
                imports.push((
                    text(key, source).to_owned(),
                    Some(text(local, source).to_owned()),
                ));
            }
        }
        "assignment_pattern" => {
            if let Some(left) = pattern.child_by_field_name("left") {
                collect_require_bindings(left, source, imports);
            }
        }
        "object_pattern" | "array_pattern" | "rest_pattern" => {
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                collect_require_bindings(child, source, imports);
            }
        }
        _ => {}
    }
}

fn require_is_shadowed(ctx: &SyntaxContext<'_, '_>) -> bool {
    let mut current = Some(ctx.node);
    while let Some(scope) = current {
        if ctx.shadowed_require_scopes.contains(&scope.id()) {
            return true;
        }
        current = scope.parent();
    }
    false
}

fn pattern_binds_name(pattern: Syntax<'_>, source: &str, name: &str) -> bool {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            text(pattern, source).trim() == name
        }
        "pair_pattern" => pattern
            .child_by_field_name("value")
            .is_some_and(|value| pattern_binds_name(value, source, name)),
        "assignment_pattern" => pattern
            .child_by_field_name("left")
            .is_some_and(|left| pattern_binds_name(left, source, name)),
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            let candidate = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"));
            if let Some(candidate) = candidate {
                return pattern_binds_name(candidate, source, name);
            }
            let mut cursor = pattern.walk();
            let binds = pattern
                .named_children(&mut cursor)
                .any(|child| pattern_binds_name(child, source, name));
            binds
        }
        _ => false,
    }
}

fn pattern_binds_require(pattern: Syntax<'_>, source: &str) -> bool {
    pattern_binds_name(pattern, source, "require")
}

fn collect_pattern_binding_names(pattern: Syntax<'_>, source: &str, names: &mut Vec<String>) {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            names.push(text(pattern, source).to_owned());
        }
        "pair_pattern" => {
            if let Some(value) = pattern.child_by_field_name("value") {
                collect_pattern_binding_names(value, source, names);
            }
        }
        "assignment_pattern" => {
            if let Some(left) = pattern.child_by_field_name("left") {
                collect_pattern_binding_names(left, source, names);
            }
        }
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            if let Some(candidate) = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"))
            {
                collect_pattern_binding_names(candidate, source, names);
                return;
            }
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                collect_pattern_binding_names(child, source, names);
            }
        }
        _ => {}
    }
}

pub(crate) fn typed_dom_receiver(name: &str) -> Option<&'static str> {
    match name {
        "Document" | "document" => Some("Document"),
        "Window" | "window" => Some("Window"),
        "EventTarget" => Some("EventTarget"),
        "Event" | "CustomEvent" | "MouseEvent" | "KeyboardEvent" | "DragEvent"
        | "ClipboardEvent" | "FocusEvent" => Some("Event"),
        "URL" => Some("URL"),
        "URLSearchParams" => Some("URLSearchParams"),
        "Headers" => Some("Headers"),
        "DOMTokenList" => Some("DOMTokenList"),
        "H3Event" => Some("H3Event"),
        "Storage" => Some("Storage"),
        "Element" | "HTMLElement" | "Node" => Some("Element"),
        "ParentNode" => Some("ParentNode"),
        "ChildNode" => Some("ChildNode"),
        "DocumentFragment" => Some("DocumentFragment"),
        _ if (name.starts_with("HTML") || name.starts_with("SVG")) && name.ends_with("Element") => {
            Some("Element")
        }
        _ => None,
    }
}

pub(crate) fn typed_dom_property(receiver: &str, property: &str) -> Option<&'static str> {
    match (typed_dom_receiver(receiver)?, property) {
        (
            "Element",
            "parentElement"
            | "firstElementChild"
            | "lastElementChild"
            | "nextElementSibling"
            | "previousElementSibling",
        ) => Some("Element"),
        ("Element", "ownerDocument") => Some("Document"),
        ("Element", "classList") => Some("DOMTokenList"),
        ("Document", "documentElement" | "body" | "head" | "activeElement") => Some("Element"),
        ("Document", "defaultView") => Some("Window"),
        ("Window", "document") => Some("Document"),
        ("Event", "target" | "currentTarget") => Some("Element"),
        ("URL", "searchParams") => Some("URLSearchParams"),
        ("H3Event", "headers") => Some("Headers"),
        _ => None,
    }
}

/// TypeScript family profile that shares syntax rules with JavaScript.
pub struct TypeScript {
    /// Enables JavaScript syntax and file extensions when true.
    pub javascript: bool,
}
/// Shared typescript language profile.
pub static TYPESCRIPT: TypeScript = TypeScript { javascript: false };
/// Shared javascript language profile.
pub static JAVASCRIPT: TypeScript = TypeScript { javascript: true };

#[derive(Default)]
struct TsCandidate {
    source: String,
    expression: String,
    line: usize,
    column: usize,
    is_member: bool,
}

#[derive(Default)]
struct JsPrototypeAlias {
    owner: String,
    member: String,
    implementation: String,
    line: usize,
    column: usize,
}

#[derive(Default)]
struct TypeScriptScratch {
    candidates: Vec<TsCandidate>,
    assigned: std::collections::HashSet<String>,
    assignment_overflow: bool,
    writes: std::collections::HashMap<String, u64>,
    write_overflow: bool,
    aliases: Vec<JsPrototypeAlias>,
    alias_overflow: bool,
}

impl TypeScriptScratch {
    fn reset(&mut self) {
        self.candidates.clear();
        self.assigned.clear();
        self.assignment_overflow = false;
        self.writes.clear();
        self.write_overflow = false;
        self.aliases.clear();
        self.alias_overflow = false;
    }
}

thread_local! {
    static TS_SCRATCH: std::cell::RefCell<TypeScriptScratch> = std::cell::RefCell::new(TypeScriptScratch::default());
}

impl TypeScript {
    fn metadata_with_documented(
        &self,
        mut node: Syntax<'_>,
        source: &str,
        name: &str,
        file: &FileContext,
        documented: BTreeMap<String, String>,
    ) -> SymbolMetadata {
        let mut param_types = BTreeMap::new();
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let mut p_cursor = parameter.walk();
                let name = parameter
                    .child_by_field_name("pattern")
                    .or_else(|| parameter.child_by_field_name("name"))
                    .or_else(|| (parameter.kind() == "identifier").then_some(parameter))
                    .or_else(|| {
                        parameter
                            .named_children(&mut p_cursor)
                            .find(|c| c.kind() == "identifier")
                    })
                    .filter(|node| node.kind() == "identifier")
                    .map(|node| text(node, source).trim().to_owned());
                let ty = parameter.child_by_field_name("type").map(|node| {
                    let mut cursor = node.walk();
                    let inner = if let Some(union_node) =
                        (node.kind() == "union_type").then_some(node).or_else(|| {
                            node.named_children(&mut cursor)
                                .find(|c| c.kind() == "union_type")
                        }) {
                        let mut ucursor = union_node.walk();
                        let non_null: Vec<_> = union_node
                            .named_children(&mut ucursor)
                            .filter(|c| {
                                let t = text(*c, source).trim();
                                !matches!(t, "null" | "undefined" | "void")
                                    && !matches!(
                                        c.kind(),
                                        "null_type" | "undefined_type" | "void_type"
                                    )
                            })
                            .collect();
                        if non_null.len() == 1 {
                            non_null[0]
                        } else {
                            node
                        }
                    } else {
                        node
                    };
                    text(inner, source)
                        .trim()
                        .trim_start_matches(':')
                        .trim()
                        .to_owned()
                });
                let ty =
                    ty.or_else(|| name.as_ref().and_then(|name| documented.get(name).cloned()));
                if let (Some(name), Some(ty)) = (name, ty) {
                    if !name.is_empty() && !ty.is_empty() {
                        param_types.insert(name, ty);
                    }
                }
            }
        }
        if !self.javascript && matches!(node.kind(), "arrow_function" | "function_expression") {
            let parameter = node
                .child_by_field_name("parameter")
                .or_else(|| node.child_by_field_name("parameters")?.named_child(0))
                .and_then(|p| {
                    if p.kind() == "identifier" {
                        Some(p)
                    } else {
                        p.child_by_field_name("name")
                            .or_else(|| p.child_by_field_name("pattern"))
                            .filter(|n| n.kind() == "identifier")
                    }
                });
            if let (Some(parameter), Some(element)) =
                (parameter, array_callback_element(node, source, file))
            {
                param_types.insert(text(parameter, source).to_owned(), element.to_owned());
            }
        }
        if matches!(node.kind(), "arrow_function" | "function_expression") {
            if let Some(event) = dom_event_callback_parameter(node, source, file) {
                param_types
                    .entry(event.to_owned())
                    .or_insert_with(|| "Event".to_owned());
            } else if let Some(event) = nitro_event_callback_parameter(node, source) {
                param_types
                    .entry(event.to_owned())
                    .or_insert_with(|| "H3Event".to_owned());
            }
        }
        let is_async = text(node, source).trim_start().starts_with("async ");
        let is_method_node = matches!(node.kind(), "method_definition" | "method_signature");
        let local_object = if self.javascript {
            if node.kind() == "pair" {
                node.parent().and_then(|object| same_object(object, source))
            } else {
                same_object_receiver(node, source)
            }
        } else {
            None
        };
        let lexical_this = (node.kind() == "arrow_function")
            .then(|| lexical_this_method(node))
            .flatten();
        let prototype_method = self
            .javascript
            .then(|| prototype_assignment(node, source))
            .flatten();
        let lexical_prototype = (self.javascript && node.kind() == "arrow_function")
            .then(|| lexical_prototype_owner(node, source))
            .flatten();
        let (receiver_type, is_method, is_static) = if is_method_node {
            let owner = method_owner(node);
            let receiver_type = owner
                .and_then(|o| o.child_by_field_name("name"))
                .map(|n| text(n, source).trim().to_owned());
            let is_static = (0..node.child_count()).any(|i| {
                node.child(i)
                    .is_some_and(|c| c.kind() == "static" || text(c, source).trim() == "static")
            });
            (receiver_type, Some(true), Some(is_static))
        } else if let Some((owner, _)) = prototype_method {
            (Some(owner.to_owned()), Some(true), Some(false))
        } else if let Some((method, class)) = lexical_this {
            let receiver_type = class
                .child_by_field_name("name")
                .map(|name| text(name, source).trim().to_owned());
            let is_static = (0..method.child_count()).any(|index| {
                method
                    .child(index)
                    .is_some_and(|child| child.kind() == "static")
            });
            (receiver_type, None, Some(is_static))
        } else if let Some(owner) = lexical_prototype {
            (Some(owner.to_owned()), None, Some(false))
        } else if node.kind() == "enum_assignment"
            || (node.kind() == "property_identifier"
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "enum_body"))
        {
            (None, None, Some(true))
        } else if node.kind() == "field_definition" {
            let is_static = (0..node.child_count()).any(|index| {
                node.child(index)
                    .is_some_and(|child| child.kind() == "static")
            });
            (None, None, Some(is_static))
        } else {
            (None, None, None)
        };
        let is_ts_without_body = matches!(
            node.kind(),
            "function_declaration" | "method_definition" | "method_signature"
        ) && node.child_by_field_name("body").is_none();
        let is_overload = is_ts_without_body;
        let is_stub = is_ts_without_body;
        let mut default_export = false;
        while let Some(parent) = node.parent() {
            if parent.kind() == "export_statement"
                && text(parent, source)
                    .trim_start()
                    .starts_with("export default")
            {
                default_export = true;
                break;
            }
            if matches!(parent.kind(), "program" | "module") {
                default_export = file.default_exports.contains(name);
                break;
            }
            if !matches!(
                parent.kind(),
                "variable_declarator"
                    | "lexical_declaration"
                    | "variable_declaration"
                    | "export_statement"
            ) {
                break;
            }
            node = parent;
        }
        SymbolMetadata {
            param_types,
            default_export,
            is_async,
            receiver_name: if local_object.is_some()
                || lexical_this.is_some()
                || lexical_prototype.is_some()
                || (is_method == Some(true) && node.kind() != "arrow_function")
            {
                Some("this".into())
            } else {
                None
            },
            receiver: local_object.map(|object| object_marker(object, source)),
            receiver_type,
            is_method,
            is_static,
            is_overload,
            is_stub,
            ..Default::default()
        }
    }
}

impl LanguageProfile for TypeScript {
    fn id(&self) -> &'static str {
        if self.javascript {
            "javascript"
        } else {
            "typescript"
        }
    }
    fn ast_search_capabilities(&self) -> AstSearchCapabilities {
        AstSearchCapabilities {
            declaration_kinds: &[
                "function_declaration",
                "generator_function_declaration",
                "function_signature",
                "class_declaration",
                "class",
                "interface_declaration",
                "method_signature",
                "enum_declaration",
            ],
            declaration_body_kinds: &[
                "statement_block",
                "class_body",
                "interface_body",
                "enum_body",
            ],
            declaration_completions: &[" {}"],
            attribute_kinds: &["decorator"],
            valid_examples: &[
                "function example() {}",
                "client.fetch($VALUE)",
                "@sealed class Example {}",
            ],
            diagnostic_hint: "Use a complete TypeScript or JavaScript declaration or expression.",
            ..AstSearchCapabilities::default()
        }
    }
    fn receiver(&self, name: &str, owner: &Node) -> bool {
        name == "this" && owner.details["receiver_name"].as_str() == Some("this")
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &[
            "package.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "pnpm-workspace.yaml",
            "yarn.lock",
            "tsconfig*.json",
        ]
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        package_metadata::extract_dependencies(filename, content)
    }
    fn is_stdlib(&self, module: &str) -> bool {
        const NODE_BUILTINS: &[&str] = &[
            "assert",
            "assert/strict",
            "async_hooks",
            "buffer",
            "child_process",
            "cluster",
            "console",
            "constants",
            "crypto",
            "dgram",
            "diagnostics_channel",
            "dns",
            "dns/promises",
            "domain",
            "events",
            "fs",
            "fs/promises",
            "http",
            "http2",
            "https",
            "inspector",
            "inspector/promises",
            "module",
            "net",
            "os",
            "path",
            "path/posix",
            "path/win32",
            "perf_hooks",
            "process",
            "punycode",
            "querystring",
            "readline",
            "readline/promises",
            "repl",
            "stream",
            "stream/consumers",
            "stream/promises",
            "stream/web",
            "string_decoder",
            "sys",
            "timers",
            "timers/promises",
            "tls",
            "trace_events",
            "tty",
            "url",
            "util",
            "util/types",
            "v8",
            "vm",
            "wasi",
            "worker_threads",
            "zlib",
            "test",
            "test/reporters",
        ];
        if module.starts_with("node:") {
            return true;
        }
        let specifier = module.strip_prefix("node:").unwrap_or(module);
        NODE_BUILTINS.contains(&specifier)
    }
    fn extensions(&self) -> &'static [&'static str] {
        if self.javascript {
            &["js", "jsx", "mjs", "cjs"]
        } else {
            &["ts", "tsx", "mts", "cts"]
        }
    }
    fn grammar(&self, path: &str) -> tree_sitter::Language {
        language(path, self.id())
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("javascript")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn symbol_with_source(&self, node: Syntax<'_>, source: &str) -> Option<&'static str> {
        if self.javascript && prototype_assignment(node, source).is_some() {
            return Some("method");
        }
        if !self.javascript {
            if node.kind() == "property_identifier"
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "enum_body")
            {
                return Some("field");
            }
            if node.kind() == "enum_assignment"
                && !node
                    .child_by_field_name("name")
                    .is_some_and(|name| name.kind() == "property_identifier")
            {
                return None;
            }
        }
        if self.javascript
            && node.kind() == "pair"
            && node
                .child_by_field_name("key")
                .is_some_and(|key| matches!(key.kind(), "property_identifier" | "identifier"))
            && node.child_by_field_name("value").is_some_and(|value| {
                !matches!(value.kind(), "arrow_function" | "function_expression")
            })
            && node
                .parent()
                .and_then(|object| same_object(object, source))
                .is_some()
        {
            return Some("field");
        }
        if node.kind() == "field_definition"
            && !node
                .child_by_field_name("property")
                .is_some_and(|property| property.kind() == "property_identifier")
        {
            return None;
        }
        self.symbol(node)
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        if self.javascript {
            if let Some((_, member)) = prototype_assignment(node, source) {
                return Some(member);
            }
        }
        if node.kind() == "property_identifier"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "enum_body")
        {
            return Some(text(node, source));
        }
        if node.kind() == "pair" {
            return field(node, source, "key");
        }
        if node.kind() == "field_definition" {
            return field(node, source, "property");
        }
        ast::symbol_name(node, source)
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "ts"
    }
    fn module_name(&self, path: &str) -> String {
        module_stem(path)
            .trim_end_matches("/index")
            .replace('/', ".")
    }
    fn extract_file(
        &self,
        path: &str,
        source: &str,
        module: &str,
        facts: &mut Facts,
    ) -> Result<()> {
        parse_file(self, path, source, module, facts)
    }
    fn extract_imports(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let (node, source) = (ctx.node, ctx.source);
        if let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") {
            if module.path.ends_with("/nuxt.config.ts") || module.path == "nuxt.config.ts" {
                if nuxt_module_declared(node, source, "radix-vue/nuxt") {
                    module.details["nuxt_radix_vue_enabled"] = serde_json::json!(true);
                }
                if let Some(enabled) = nuxt_default_components_enabled(node, source) {
                    module.details["nuxt_default_components_enabled"] = serde_json::json!(enabled);
                }
            }
        }
        if node.kind() == "import_statement" {
            let before = facts.references.len();
            let module =
                field(node, source, "source").map(|s| s.trim_matches(['\'', '"']).to_owned());
            fn ids(
                node: Syntax<'_>,
                source: &str,
                values: &mut Vec<(String, Option<String>, bool)>,
            ) {
                match node.kind() {
                    "import_specifier" => {
                        let name =
                            field(node, source, "name").unwrap_or_else(|| text(node, source));
                        values.push((
                            name.into(),
                            field(node, source, "alias")
                                .map(str::to_owned)
                                .or_else(|| Some(name.into())),
                            has_type_import_modifier(node),
                        ));
                    }
                    "namespace_import" => {
                        if let Some(n) = node.named_child(0) {
                            values.push(("*".into(), Some(text(n, source).into()), false));
                        }
                    }
                    "identifier" => {
                        values.push(("default".into(), Some(text(node, source).into()), false))
                    }
                    _ => {
                        let mut c = node.walk();
                        for n in node.named_children(&mut c) {
                            ids(n, source, values);
                        }
                    }
                }
            }
            let mut names = Vec::new();
            let mut type_only_aliases = Vec::new();
            let mut c = node.walk();
            for n in node.named_children(&mut c) {
                if n.kind() == "import_clause" {
                    ids(n, source, &mut names);
                }
            }
            if names.is_empty() {
                ctx.import(facts, "*".into(), None, module);
            } else {
                for (index, (name, alias, type_only)) in names.into_iter().enumerate() {
                    ctx.import(facts, name, alias, module.clone());
                    if type_only {
                        type_only_aliases.push(before + index);
                    }
                }
            }
            let statement_type_only = has_type_import_modifier(node);
            for (index, reference) in facts.references.iter_mut().enumerate().skip(before) {
                if statement_type_only || type_only_aliases.contains(&index) {
                    reference.receiver_hint = Some(ReceiverHint::TypeOnlyImport);
                }
            }
            return;
        }
        let mut add = |expression, alias, module| ctx.import(facts, expression, alias, module);
        if node.kind() == "export_statement" {
            if let Some(module) = field(node, source, "source")
                .map(|module| module.trim_matches(['\'', '"']).to_owned())
            {
                add(module.clone(), None, Some(module));
            }
        } else if node.kind() == "assignment_expression" && !require_is_shadowed(ctx) {
            if let Some(module) = node
                .child_by_field_name("right")
                .and_then(|right| required_module(right, source))
            {
                if field(node, source, "left").is_some_and(|left| {
                    left == "module.exports"
                        || left.starts_with("exports.")
                        || left.starts_with("module.exports.")
                }) {
                    add(module.clone(), None, Some(module));
                }
            }
        } else if node.kind() == "variable_declarator" {
            let Some(value) = node.child_by_field_name("value") else {
                return;
            };
            let Some(module) = required_module(value, source) else {
                return;
            };
            if require_is_shadowed(ctx) {
                return;
            }
            let Some(pattern) = node.child_by_field_name("name") else {
                add(module.clone(), None, Some(module));
                return;
            };
            if pattern.kind() == "identifier" {
                add("*".into(), Some(text(pattern, source).into()), Some(module));
                return;
            }

            let mut imports = Vec::new();
            collect_require_bindings(pattern, source, &mut imports);
            if imports.is_empty() {
                add(module.clone(), None, Some(module));
            } else {
                for (name, alias) in imports {
                    add(name, alias, Some(module.clone()));
                }
            }
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() == "member_expression"
            && field(ctx.node, ctx.source, "object") == Some("process")
            && field(ctx.node, ctx.source, "property") == Some("env")
        {
            facts.references.push(Reference {
                source: ctx.owner.into(),
                dynamic: false,
                expression: "process.env".into(),
                kind: "references".into(),
                line: ctx.line(),
                column: ctx.node.start_position().column,
                alias: None,
                module: None,
                receiver_hint: None,
            });
        }
        if ctx.node.kind() == "new_expression" {
            if let Some(constructor) = field(ctx.node, ctx.source, "constructor")
                .filter(|name| matches!(*name, "URL" | "Response" | "Request" | "Headers"))
            {
                facts.references.push(Reference {
                    source: ctx.owner.into(),
                    dynamic: false,
                    expression: constructor.into(),
                    kind: "calls".into(),
                    line: ctx.line(),
                    column: ctx.node.start_position().column,
                    alias: None,
                    module: None,
                    receiver_hint: None,
                });
            }
        }
        if matches!(ctx.node.kind(), "call_expression") {
            if required_module(ctx.node, ctx.source).is_some() {
                if require_is_shadowed(ctx) {
                    call(ctx, facts, false);
                } else if !ctx
                    .node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "variable_declarator")
                {
                    call(ctx, facts, true);
                }
            } else {
                let reference_count = facts.references.len();
                call(ctx, facts, true);
                if self.javascript {
                    if let Some(function) = ctx.node.child_by_field_name("function") {
                        if let Some(expression) = static_member_callee(function, ctx.source) {
                            if let Some(reference) = facts
                                .references
                                .get_mut(reference_count)
                                .filter(|reference| {
                                    reference.source == ctx.owner
                                        && reference.kind == "calls"
                                        && reference.line == ctx.line()
                                })
                            {
                                reference.expression = expression;
                                reference.dynamic = false;
                            }
                        }
                    }
                }
                if !self.javascript {
                    if let Some(reference) = facts.references.get_mut(reference_count) {
                        if reference.source == ctx.owner
                            && reference.kind == "calls"
                            && reference.line == ctx.line()
                            && reference.expression.contains("?.")
                        {
                            let normalized = reference.expression.replace("?.", ".");
                            if normalized.split('.').all(|part| {
                                !part.is_empty()
                                    && part
                                        .chars()
                                        .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                            }) {
                                reference.dynamic = false;
                            }
                        }
                    }
                }
                if let Some(function) = ctx
                    .node
                    .child_by_field_name("function")
                    .filter(|function| function.kind() == "member_expression")
                {
                    if let (Some(mut receiver), Some(member)) = (
                        function.child_by_field_name("object"),
                        function
                            .child_by_field_name("property")
                            .filter(|member| member.kind() == "property_identifier"),
                    ) {
                        for _ in 0..8 {
                            if receiver.kind() != "parenthesized_expression" {
                                break;
                            }
                            let Some(inner) = receiver.named_child(0) else {
                                break;
                            };
                            receiver = inner;
                        }
                        let mut curr_receiver = receiver;
                        let mut member_path = vec![text(member, ctx.source)];
                        while curr_receiver.kind() == "member_expression" {
                            if let Some(prop) = curr_receiver
                                .child_by_field_name("property")
                                .filter(|prop| prop.kind() == "property_identifier")
                            {
                                member_path.push(text(prop, ctx.source));
                            } else {
                                break;
                            }
                            let Some(obj) = curr_receiver.child_by_field_name("object") else {
                                break;
                            };
                            curr_receiver = obj;
                            for _ in 0..8 {
                                if curr_receiver.kind() != "parenthesized_expression" {
                                    break;
                                }
                                let Some(inner) = curr_receiver.named_child(0) else {
                                    break;
                                };
                                curr_receiver = inner;
                            }
                        }
                        member_path.reverse();
                        let full_member = member_path.join(".");
                        let hint =
                            match curr_receiver.kind() {
                                "call_expression" => curr_receiver
                                    .child_by_field_name("function")
                                    .map(|callee| ReceiverHint::CallResult {
                                        callee: text(callee, ctx.source).to_owned(),
                                        member: full_member,
                                    }),
                                "new_expression" => curr_receiver
                                    .child_by_field_name("constructor")
                                    .map(|callee| ReceiverHint::ConstructorResult {
                                        callee: text(callee, ctx.source).to_owned(),
                                        member: full_member,
                                    }),
                                "string" => Some(ReceiverHint::StringLiteral {
                                    member: full_member,
                                }),
                                _ => None,
                            };
                        if let Some(reference) = facts.references.last_mut() {
                            reference.receiver_hint = hint;
                        }
                    }
                }
            }
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        if browser_global_type(symbol) {
            return true;
        }
        if matches!(
            symbol,
            "Object.assign"
                | "Object.hasOwn"
                | "Object.defineProperty"
                | "Object.defineProperties"
                | "Object.keys"
                | "Object.values"
                | "Object.entries"
                | "Object.fromEntries"
                | "Object.create"
                | "Object.freeze"
                | "Object.seal"
                | "Object.getPrototypeOf"
                | "Object.getOwnPropertyDescriptor"
                | "Object.getOwnPropertyDescriptors"
                | "Object.getOwnPropertyNames"
                | "Object.getOwnPropertySymbols"
                | "Object.is"
                | "Number.isFinite"
                | "Number.isNaN"
                | "Number.isInteger"
                | "Number.isSafeInteger"
                | "Number.parseInt"
                | "Number.parseFloat"
                | "Reflect.apply"
                | "Reflect.construct"
                | "Reflect.defineProperty"
                | "Reflect.deleteProperty"
                | "Reflect.get"
                | "Reflect.getOwnPropertyDescriptor"
                | "Reflect.getPrototypeOf"
                | "Reflect.has"
                | "Reflect.isExtensible"
                | "Reflect.ownKeys"
                | "Reflect.preventExtensions"
                | "Reflect.set"
                | "Reflect.setPrototypeOf"
                | "crypto.getRandomValues"
                | "crypto.randomUUID"
                | "Promise.all"
                | "Date.now"
                | "CSS.escape"
                | "JSON.parse"
                | "JSON.stringify"
                | "Math.abs"
                | "Math.ceil"
                | "Math.floor"
                | "Math.round"
                | "Math.max"
                | "Math.min"
                | "Math.pow"
                | "Math.sqrt"
                | "Math.trunc"
                | "Math.random"
                | "Math.sign"
                | "console.log"
                | "console.error"
                | "console.warn"
                | "console.info"
                | "console.debug"
                | "console.trace"
                | "console.table"
                | "Buffer.from"
                | "Buffer.alloc"
                | "Buffer.concat"
                | "Buffer.isBuffer"
                | "process.env"
                | "process.cwd"
                | "process.nextTick"
                | "document.querySelector"
                | "document.querySelectorAll"
                | "document.getElementById"
                | "document.getElementsByClassName"
                | "document.getElementsByTagName"
                | "document.createElement"
                | "document.createTextNode"
                | "document.addEventListener"
                | "document.removeEventListener"
                | "window.addEventListener"
                | "window.removeEventListener"
                | "window.dispatchEvent"
                | "window.setTimeout"
                | "window.clearTimeout"
                | "window.setInterval"
                | "window.clearInterval"
                | "window.requestAnimationFrame"
                | "window.cancelAnimationFrame"
                | "localStorage.getItem"
                | "localStorage.setItem"
                | "localStorage.removeItem"
                | "localStorage.clear"
                | "sessionStorage.getItem"
                | "sessionStorage.setItem"
                | "sessionStorage.removeItem"
                | "sessionStorage.clear"
                | "Array.isArray"
                | "Array.from"
                | "Array.of"
                | "getComputedStyle"
                | "Alpine"
                | "Alpine.data"
                | "Alpine.store"
                | "Alpine.start"
                | "Alpine.plugin"
                | "Alpine.directive"
                | "Alpine.magic"
                | "Alpine.$data"
                | "window.Alpine"
        ) {
            return true;
        }
        matches!(
            symbol,
            "parseInt"
                | "parseFloat"
                | "encodeURIComponent"
                | "fetch"
                | "alert"
                | "confirm"
                | "prompt"
                | "String"
                | "Number"
                | "Reflect"
                | "crypto"
                | "Boolean"
                | "setTimeout"
                | "clearTimeout"
                | "setInterval"
                | "clearInterval"
                | "string"
                | "number"
                | "boolean"
                | "any"
                | "unknown"
                | "never"
                | "void"
                | "undefined"
                | "null"
                | "Promise"
                | "Date"
                | "CSS"
                | "Array"
                | "Record"
                | "Map"
                | "Set"
                | "Object"
                | "Function"
                | "Symbol"
                | "Error"
                | "Uint8Array"
                | "Partial"
                | "Required"
                | "Readonly"
                | "Pick"
                | "Omit"
                | "Exclude"
                | "Extract"
                | "NonNullable"
                | "ReturnType"
                | "InstanceType"
                | "Buffer"
                | "__dirname"
                | "__filename"
                | "clearImmediate"
                | "exports"
                | "global"
                | "globalThis"
                | "module"
                | "process"
                | "require"
                | "setImmediate"
        )
    }
    fn builtin_type(&self, name: &str) -> bool {
        if browser_global_type(name) {
            return true;
        }
        matches!(
            name,
            "string"
                | "number"
                | "boolean"
                | "String"
                | "Number"
                | "Boolean"
                | "Array"
                | "Map"
                | "Set"
                | "Object"
        )
    }

    fn builtin_generic(&self, receiver: &str) -> bool {
        matches!(receiver, "Array" | "Map" | "Set")
    }
    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        match receiver {
            "Web.Response" => matches!(
                member,
                "json" | "text" | "arrayBuffer" | "blob" | "formData" | "clone"
            ),
            "H3Event" => matches!(member, "headers" | "context" | "node"),
            "Document" => matches!(
                member,
                "querySelector"
                    | "querySelectorAll"
                    | "getElementById"
                    | "getElementsByClassName"
                    | "getElementsByTagName"
                    | "createElement"
                    | "createTextNode"
                    | "addEventListener"
                    | "removeEventListener"
                    | "body"
                    | "head"
                    | "documentElement"
            ),
            "ParentNode" | "DocumentFragment" => matches!(
                member,
                "querySelector" | "querySelectorAll" | "appendChild" | "removeChild"
            ),
            "ChildNode" => matches!(member, "appendChild" | "removeChild"),
            _ if typed_dom_receiver(receiver) == Some("Element") => matches!(
                member,
                "querySelector"
                    | "querySelectorAll"
                    | "getAttribute"
                    | "setAttribute"
                    | "removeAttribute"
                    | "closest"
                    | "matches"
                    | "addEventListener"
                    | "removeEventListener"
                    | "appendChild"
                    | "removeChild"
                    | "classList"
            ),
            "Window" => matches!(
                member,
                "addEventListener" | "removeEventListener" | "dispatchEvent"
            ),
            "Alpine" => matches!(
                member,
                "data" | "store" | "start" | "plugin" | "directive" | "magic" | "$data"
            ),
            "EventTarget" => matches!(
                member,
                "addEventListener" | "removeEventListener" | "dispatchEvent"
            ),
            "Event" | "CustomEvent" | "MouseEvent" | "KeyboardEvent" | "DragEvent"
            | "ClipboardEvent" | "FocusEvent" => matches!(
                member,
                "stopPropagation" | "stopImmediatePropagation" | "preventDefault"
            ),
            "Storage" => matches!(
                member,
                "getItem" | "setItem" | "removeItem" | "clear" | "key" | "length"
            ),
            "URLSearchParams" => matches!(
                member,
                "append"
                    | "delete"
                    | "get"
                    | "getAll"
                    | "has"
                    | "set"
                    | "sort"
                    | "forEach"
                    | "entries"
                    | "keys"
                    | "values"
            ),
            "Headers" => matches!(
                member,
                "append"
                    | "delete"
                    | "get"
                    | "getSetCookie"
                    | "has"
                    | "set"
                    | "forEach"
                    | "entries"
                    | "keys"
                    | "values"
            ),
            "DOMTokenList" => matches!(
                member,
                "add"
                    | "remove"
                    | "toggle"
                    | "contains"
                    | "replace"
                    | "supports"
                    | "entries"
                    | "forEach"
                    | "keys"
                    | "values"
            ),
            "URL" => matches!(
                member,
                "hash"
                    | "host"
                    | "hostname"
                    | "href"
                    | "origin"
                    | "password"
                    | "pathname"
                    | "port"
                    | "protocol"
                    | "search"
                    | "searchParams"
                    | "username"
                    | "toString"
                    | "toJSON"
            ),
            "string" | "String" => matches!(
                member,
                "charAt"
                    | "charCodeAt"
                    | "codePointAt"
                    | "concat"
                    | "endsWith"
                    | "includes"
                    | "indexOf"
                    | "lastIndexOf"
                    | "match"
                    | "matchAll"
                    | "normalize"
                    | "padEnd"
                    | "padStart"
                    | "repeat"
                    | "replace"
                    | "replaceAll"
                    | "search"
                    | "slice"
                    | "split"
                    | "startsWith"
                    | "substring"
                    | "toLowerCase"
                    | "toUpperCase"
                    | "trim"
                    | "trimEnd"
                    | "trimStart"
                    | "at"
            ),
            "Array" => matches!(
                member,
                "at" | "concat"
                    | "copyWithin"
                    | "entries"
                    | "every"
                    | "fill"
                    | "filter"
                    | "find"
                    | "findIndex"
                    | "findLast"
                    | "findLastIndex"
                    | "flat"
                    | "flatMap"
                    | "forEach"
                    | "includes"
                    | "indexOf"
                    | "join"
                    | "keys"
                    | "lastIndexOf"
                    | "map"
                    | "pop"
                    | "push"
                    | "reduce"
                    | "reduceRight"
                    | "reverse"
                    | "shift"
                    | "slice"
                    | "some"
                    | "sort"
                    | "splice"
                    | "unshift"
                    | "values"
            ),
            "Map" => matches!(
                member,
                "clear"
                    | "delete"
                    | "entries"
                    | "forEach"
                    | "get"
                    | "has"
                    | "keys"
                    | "set"
                    | "values"
            ),
            "Set" => matches!(
                member,
                "add" | "clear" | "delete" | "entries" | "forEach" | "has" | "keys" | "values"
            ),
            _ => false,
        }
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        self.normalize_import_with_root(None, owner, module)
    }

    fn normalize_import_with_root(
        &self,
        _root: Option<&Path>,
        owner: &str,
        module: &str,
    ) -> Option<ImportPath> {
        let path = Path::new(module);
        let module = if for_path(path).is_some_and(|p| p.family() == LanguageFamily("javascript")) {
            module_stem(module)
        } else {
            module
        };
        let module = module.trim_end_matches("/index");
        if module.starts_with("./") || module.starts_with("../") {
            Some(ImportPath {
                namespace: relative_namespace(owner, module)?,
                relative: true,
                symbol_path: false,
            })
        } else {
            Some(ImportPath::absolute(module.replace('/', ".")))
        }
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        self.is_stdlib(module).then_some("Node.js standard library")
    }
    fn prepare(&self, root: Syntax<'_>, source: &str) -> FileContext {
        let root_bindings = self.bindings(root, source);
        let mut file = FileContext {
            exports: value_flow::exports(root, source, &root_bindings),
            root_bindings: Some((root.id(), root_bindings)),
            ..Default::default()
        };
        let scope_fact_needs =
            scope_facts::classify_scope_fact_needs(root, source, self.javascript);
        let needs_dom_scope_facts = scope_fact_needs.dom_scope_facts;
        let needs_array_callback_scope_facts =
            !self.javascript && scope_fact_needs.array_callback_scope_facts;
        let needs_dom_array_receiver_facts =
            needs_array_callback_scope_facts && scope_fact_needs.dom_array_receiver_facts;
        let needs_full_scope_facts = needs_dom_scope_facts
            || needs_dom_array_receiver_facts
            || scope_fact_needs.commonjs_scope_facts;
        if needs_full_scope_facts {
            collect_scope_facts(root, source, &mut file, !self.javascript);
        }
        let needs_partial_scope_facts = (self.javascript
            && (needs_dom_scope_facts || needs_array_callback_scope_facts))
            || (!needs_full_scope_facts
                && needs_array_callback_scope_facts
                && !needs_dom_array_receiver_facts);
        if needs_partial_scope_facts {
            scope_facts::collect_array_callback_scope_facts(root, source, &mut file);
        }
        let mut c = root.walk();
        for node in root
            .named_children(&mut c)
            .filter(|n| n.kind() == "export_statement" && n.child_by_field_name("source").is_none())
        {
            if text(node, source)
                .trim_start()
                .starts_with("export default")
            {
                if let Some(value) = node
                    .child_by_field_name("value")
                    .filter(|n| n.kind() == "identifier")
                {
                    file.default_exports.insert(text(value, source).into());
                }
            }
            let mut c = node.walk();
            for clause in node
                .named_children(&mut c)
                .filter(|n| n.kind() == "export_clause")
            {
                let mut c = clause.walk();
                for specifier in clause.named_children(&mut c) {
                    if field(specifier, source, "alias") == Some("default") {
                        if let Some(name) = field(specifier, source, "name") {
                            file.default_exports.insert(name.into());
                        }
                    }
                }
            }
        }
        file
    }

    fn value_flow(&self, node: Syntax<'_>, source: &str) -> crate::core::semantic::ValueFlowFacts {
        value_flow::extract(node, source)
    }

    fn metadata(
        &self,
        node: Syntax<'_>,
        source: &str,
        name: &str,
        file: &FileContext,
    ) -> SymbolMetadata {
        self.metadata_with_documented(node, source, name, file, value_flow::jsdoc(node, source).0)
    }

    fn declaration(
        &self,
        node: Syntax<'_>,
        source: &str,
        name: &str,
        file: &FileContext,
    ) -> (SymbolMetadata, crate::core::semantic::ValueFlowFacts) {
        let (parameters, returned, _) = value_flow::jsdoc(node, source);
        let metadata = self.metadata_with_documented(node, source, name, file, parameters);
        let flow = value_flow::extract_with_return(node, source, || returned);
        (metadata, flow)
    }

    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let (node, source, id, line) = (ctx.node, ctx.source, ctx.owner, ctx.line());
        let mut c = node.walk();
        for child in node.named_children(&mut c) {
            if child.kind() == "class_heritage" {
                let mut h = child.walk();
                let clauses: Vec<_> = child.named_children(&mut h).collect();
                let has_clauses = clauses
                    .iter()
                    .any(|c| c.kind() == "extends_clause" || c.kind() == "implements_clause");
                if has_clauses {
                    for clause in clauses {
                        let kind = match clause.kind() {
                            "extends_clause" => "inherits",
                            "implements_clause" => "implements",
                            _ => continue,
                        };
                        let mut t = clause.walk();
                        for target in clause
                            .named_children(&mut t)
                            .filter(|n| n.kind() != "type_arguments")
                        {
                            relations::reference(
                                facts,
                                id,
                                relations::type_name(target, source),
                                kind,
                                line,
                            );
                        }
                    }
                } else {
                    for target in clauses {
                        relations::reference(
                            facts,
                            id,
                            relations::type_name(target, source),
                            "inherits",
                            line,
                        );
                    }
                }
            }
            if child.kind() == "extends_type_clause" {
                let mut t = child.walk();
                for target in child.named_children(&mut t) {
                    relations::reference(
                        facts,
                        id,
                        relations::type_name(target, source),
                        "inherits",
                        line,
                    );
                }
            }
        }

        relations::decorator_references(ctx, facts);
        routes::declaration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);

        if matches!(
            ctx.node.kind(),
            "function_declaration"
                | "function_signature"
                | "method_definition"
                | "method_signature"
                | "arrow_function"
                | "function"
        ) {
            if let Some(params) = ctx.node.child_by_field_name("parameters") {
                let mut c = params.walk();
                for param in params.named_children(&mut c) {
                    if let Some(ty) = param.child_by_field_name("type") {
                        relations::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
                    }
                }
            }
            if let Some(ret) = ctx.node.child_by_field_name("return_type") {
                relations::type_references(facts, ctx.owner, ret, ctx.source, ctx.line());
            }
        }
        if matches!(ctx.node.kind(), "property_signature" | "field_definition") {
            if let Some(ty) = ctx.node.child_by_field_name("type") {
                relations::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
            }
        }
        if ctx.node.kind() == "type_alias_declaration" {
            if let Some(val) = ctx.node.child_by_field_name("value") {
                relations::type_references(facts, ctx.owner, val, ctx.source, ctx.line());
            }
        }
    }

    fn extract_mutations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if !self.javascript && ctx.node.kind() == "identifier" {
            let runtime_value = ctx.node.parent().is_some_and(|parent| {
                (parent.kind() == "variable_declarator"
                    && parent.child_by_field_name("value") == Some(ctx.node))
                    || (parent.kind() == "assignment_expression"
                        && parent.child_by_field_name("right") == Some(ctx.node))
                    || (matches!(parent.kind(), "return_statement" | "arguments" | "array")
                        && (0..parent.named_child_count())
                            .any(|index| parent.named_child(index) == Some(ctx.node)))
            });
            if runtime_value {
                TS_SCRATCH.with(|s| {
                    s.borrow_mut().candidates.push(TsCandidate {
                        source: ctx.owner.into(),
                        expression: text(ctx.node, ctx.source).into(),
                        line: ctx.line(),
                        column: ctx.node.start_position().column,
                        is_member: false,
                    });
                });
            }
        }
        if !self.javascript && ctx.node.kind() == "member_expression" {
            let called = ctx.node.parent().is_some_and(|parent| {
                parent.kind() == "call_expression"
                    && parent.child_by_field_name("function") == Some(ctx.node)
            });
            if !called {
                if let (Some(object), Some(property)) = (
                    ctx.node
                        .child_by_field_name("object")
                        .filter(|object| object.kind() == "identifier"),
                    ctx.node
                        .child_by_field_name("property")
                        .filter(|property| property.kind() == "property_identifier"),
                ) {
                    TS_SCRATCH.with(|s| {
                        s.borrow_mut().candidates.push(TsCandidate {
                            source: ctx.owner.into(),
                            expression: format!(
                                "{}.{}",
                                text(object, ctx.source),
                                text(property, ctx.source)
                            ),
                            line: ctx.line(),
                            column: ctx.node.start_position().column,
                            is_member: true,
                        });
                    });
                }
            }
        }
        if self.javascript
            && (matches!(
                ctx.node.kind(),
                "assignment_expression" | "augmented_assignment_expression"
            ) || (ctx.node.kind() == "unary_expression"
                && ctx
                    .node
                    .child(0)
                    .is_some_and(|operator| operator.kind() == "delete")))
        {
            if let Some(left) = ctx
                .node
                .child_by_field_name("left")
                .or_else(|| ctx.node.child_by_field_name("argument"))
            {
                if facts
                    .nodes
                    .first()
                    .is_some_and(|node| node.kind == "module")
                {
                    TS_SCRATCH.with(|scratch| {
                        let mut s = scratch.borrow_mut();
                        if let Some(target) = (left.kind() == "identifier")
                            .then(|| text(left, ctx.source))
                            .or_else(|| direct_prototype_owner(left, ctx.source))
                        {
                            if !s.assigned.contains(target) {
                                if target.len() <= 128 && s.assigned.len() < 64 {
                                    s.assigned.insert(target.to_owned());
                                } else {
                                    s.assignment_overflow = true;
                                }
                            }
                        }
                        if let Some((owner, member)) = prototype_target(left, ctx.source) {
                            if !s.write_overflow {
                                let key = format!("{owner}.{member}");
                                if let Some(count) = s.writes.get_mut(&key) {
                                    *count = 2;
                                } else if s.writes.len() < 64 {
                                    s.writes.insert(key, 1);
                                } else {
                                    s.writes.clear();
                                    s.write_overflow = true;
                                }
                            }
                        }
                        if let Some((owner, member, implementation)) =
                            prototype_identifier_assignment(ctx.node, ctx.source)
                        {
                            if s.aliases.len() < 64
                                && owner.len() + member.len() + implementation.len() <= 192
                            {
                                s.aliases.push(JsPrototypeAlias {
                                    owner: owner.to_owned(),
                                    member: member.to_owned(),
                                    implementation: implementation.to_owned(),
                                    line: ctx.line(),
                                    column: ctx.node.start_position().column,
                                });
                            } else {
                                s.alias_overflow = true;
                            }
                        }
                    });
                }
            }
        }
        relations::mutation(
            ctx,
            facts,
            &["public_field_definition", "property_signature"],
            &[
                "assignment_expression",
                "augmented_assignment_expression",
                "update_expression",
            ],
            &["member_expression"],
        );
        if matches!(
            ctx.node.kind(),
            "public_field_definition" | "field_definition"
        ) && (0..ctx.node.child_count()).any(|index| {
            ctx.node
                .child(index)
                .is_some_and(|child| child.kind() == "static")
        }) {
            if let (Some(owner), Some(name)) = (
                facts.nodes.iter().find(|node| node.id == ctx.owner),
                ctx.node
                    .child_by_field_name("name")
                    .map(|name| text(name, ctx.source).to_owned()),
            ) {
                let qualname = format!("{}.{}", owner.qualname, name);
                let path = owner.path.clone();
                if let Some(field) = facts.nodes.iter_mut().find(|node| {
                    node.path == path && node.kind == "field" && node.qualname == qualname
                }) {
                    field.details["is_static"] = serde_json::json!(true);
                }
            }
        }
        let before = facts.references.len();
        relations::member_access(ctx, facts, &["member_expression"]);
        if let Some(reference) = facts.references.get_mut(before) {
            reference.column = ctx.node.start_position().column;
        }
    }
    fn extract_routes(
        &self,
        ctx: &SyntaxContext<'_, '_>,
        facts: &mut Facts,
        symbols: &HashMap<usize, String>,
    ) {
        if matches!(ctx.node.kind(), "call_expression" | "object") {
            routes::registration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset, symbols);
            routes::client_call(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);
        }
    }
    fn finish(&self, facts: &mut Facts) {
        if !self.javascript {
            let enums: HashSet<String> = facts
                .nodes
                .iter()
                .filter(|node| node.kind == "enum")
                .map(|node| node.name.clone())
                .collect();
            let imports: HashSet<String> = facts
                .references
                .iter()
                .filter(|reference| reference.kind == "imports")
                .filter_map(|reference| reference.alias.clone())
                .collect();
            let type_imports: HashSet<String> = facts
                .references
                .iter()
                .filter(|reference| {
                    reference.kind == "imports"
                        && matches!(
                            reference.receiver_hint.as_ref(),
                            Some(ReceiverHint::TypeOnlyImport)
                        )
                })
                .filter_map(|reference| reference.alias.clone())
                .collect();
            let candidates = TS_SCRATCH.with(|s| std::mem::take(&mut s.borrow_mut().candidates));
            for candidate in candidates {
                if !candidate.is_member {
                    if type_imports.contains(&candidate.expression) {
                        facts.references.push(Reference {
                            source: candidate.source,
                            dynamic: false,
                            expression: candidate.expression,
                            kind: "references".into(),
                            line: candidate.line,
                            column: candidate.column,
                            alias: None,
                            module: None,
                            receiver_hint: Some(ReceiverHint::TypeOnlyImport),
                        });
                    }
                } else {
                    let Some((receiver, _)) = candidate.expression.split_once('.') else {
                        continue;
                    };
                    if enums.contains(receiver) || imports.contains(receiver) {
                        let hint = if type_imports.contains(receiver) {
                            Some(ReceiverHint::TypeOnlyImport)
                        } else {
                            None
                        };
                        facts.references.push(Reference {
                            source: candidate.source,
                            dynamic: false,
                            expression: candidate.expression,
                            kind: "references".into(),
                            line: candidate.line,
                            column: candidate.column,
                            alias: None,
                            module: None,
                            receiver_hint: hint,
                        });
                    }
                }
            }
        }
        if self.javascript {
            let (
                writes,
                write_overflow,
                prototype_aliases,
                alias_overflow,
                assigned,
                assignment_overflow,
            ) = TS_SCRATCH.with(|s| {
                let mut b = s.borrow_mut();
                let writes = std::mem::take(&mut b.writes);
                let write_overflow = b.write_overflow;
                let prototype_aliases = std::mem::take(&mut b.aliases);
                let alias_overflow = b.alias_overflow;
                let assigned = std::mem::take(&mut b.assigned);
                let assignment_overflow = b.assignment_overflow;
                b.reset();
                (
                    writes,
                    write_overflow,
                    prototype_aliases,
                    alias_overflow,
                    assigned,
                    assignment_overflow,
                )
            });
            let Some(module) = facts.nodes.iter_mut().find(|node| node.kind == "module") else {
                return;
            };
            let module_id = module.id.clone();
            let module_qualname = module.qualname.clone();
            let rebound: std::collections::HashSet<String> = module.details["rebindings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect();
            let declarations: std::collections::HashSet<&str> = facts
                .edges
                .iter()
                .filter(|edge| {
                    edge.src == module_id
                        && edge.kind == "contains"
                        && matches!(
                            edge.evidence.as_str(),
                            "class_declaration" | "function_declaration"
                        )
                })
                .map(|edge| edge.dst.as_str())
                .collect();
            let mut direct_constructors: std::collections::HashMap<&str, Vec<&Node>> =
                std::collections::HashMap::new();
            for node in facts
                .nodes
                .iter()
                .filter(|node| declarations.contains(node.id.as_str()))
            {
                direct_constructors
                    .entry(node.qualname.as_str())
                    .or_default()
                    .push(node);
            }
            let parents: std::collections::HashMap<&str, &str> = facts
                .edges
                .iter()
                .filter(|edge| edge.kind == "contains")
                .map(|edge| (edge.dst.as_str(), edge.src.as_str()))
                .collect();
            let mut methods = std::collections::HashSet::new();
            let mut constructor_ids = std::collections::HashSet::new();
            let mut valid_aliases = Vec::new();
            for method in facts
                .nodes
                .iter()
                .filter(|node| node.details["prototype"] == true)
            {
                let Some(owner) = method.details["receiver_type"].as_str() else {
                    continue;
                };
                let key = format!("{owner}.{}", method.name);
                if write_overflow
                    || assignment_overflow
                    || writes.get(key.as_str()).copied() != Some(1)
                    || rebound.contains(owner)
                    || assigned.contains(owner)
                {
                    continue;
                }
                let qualname = format!("{module_qualname}.{owner}");
                let Some([constructor]) = direct_constructors
                    .get(qualname.as_str())
                    .map(Vec::as_slice)
                else {
                    continue;
                };
                if constructor.kind == "class"
                    && (constructor.line > method.line
                        || (constructor.line == method.line
                            && constructor.details["column"].as_u64().unwrap_or(0)
                                >= method.details["column"].as_u64().unwrap_or(0)))
                {
                    continue;
                }
                methods.insert(method.id.clone());
                if constructor.kind == "function" {
                    constructor_ids.insert(constructor.id.clone());
                }
            }
            if !write_overflow && !assignment_overflow && !alias_overflow {
                let mut target_counts = std::collections::HashMap::<&str, usize>::new();
                for alias in &prototype_aliases {
                    *target_counts
                        .entry(alias.implementation.as_str())
                        .or_default() += 1;
                }
                for alias in &prototype_aliases {
                    let key = format!("{}.{}", alias.owner, alias.member);
                    if writes.get(key.as_str()).copied() != Some(1)
                        || rebound.contains(&alias.owner)
                        || assigned.contains(&alias.owner)
                        || rebound.contains(&alias.implementation)
                        || assigned.contains(&alias.implementation)
                        || target_counts.get(alias.implementation.as_str()) != Some(&1)
                    {
                        continue;
                    }
                    let owner_qualname = format!("{module_qualname}.{}", alias.owner);
                    let implementation_qualname =
                        format!("{module_qualname}.{}", alias.implementation);
                    let (Some([constructor]), Some([function])) = (
                        direct_constructors
                            .get(owner_qualname.as_str())
                            .map(Vec::as_slice),
                        direct_constructors
                            .get(implementation_qualname.as_str())
                            .map(Vec::as_slice),
                    ) else {
                        continue;
                    };
                    if function.kind != "function"
                        || (
                            function.line,
                            function.details["column"].as_u64().unwrap_or(0),
                        ) >= (alias.line, alias.column as u64)
                        || (
                            constructor.line,
                            constructor.details["column"].as_u64().unwrap_or(0),
                        ) >= (alias.line, alias.column as u64)
                    {
                        continue;
                    }
                    methods.insert(function.id.clone());
                    if constructor.kind == "function" {
                        constructor_ids.insert(constructor.id.clone());
                    }
                    valid_aliases.push((
                        function.id.clone(),
                        alias.owner.clone(),
                        alias.member.clone(),
                        alias.line,
                        alias.column,
                    ));
                }
            }
            let valid_methods: std::collections::HashMap<String, String> = facts
                .nodes
                .iter()
                .filter(|node| methods.contains(&node.id))
                .filter_map(|node| {
                    node.details["receiver_type"]
                        .as_str()
                        .map(|owner| (node.id.clone(), owner.to_owned()))
                })
                .collect();
            for node in &mut facts.nodes {
                if let Some((_, owner, member, line, column)) =
                    valid_aliases.iter().find(|(id, ..)| *id == node.id)
                {
                    node.details["prototype"] = serde_json::json!(true);
                    node.details["prototype_member"] = serde_json::json!(member);
                    node.details["receiver_type"] = serde_json::json!(owner);
                    node.details["prototype_assignment_line"] = serde_json::json!(line);
                    node.details["prototype_assignment_column"] = serde_json::json!(column);
                }
                if methods.contains(&node.id) {
                    node.details["prototype_valid"] = serde_json::json!(true);
                }
                if constructor_ids.contains(&node.id) {
                    node.details["prototype_constructor"] = serde_json::json!(true);
                }
                if node.kind == "function" && node.details["receiver_name"] == "this" {
                    let receiver = node.details["receiver_type"].as_str();
                    let mut ancestor = node.id.as_str();
                    for _ in 0..16 {
                        let Some(parent) = parents.get(ancestor).copied() else {
                            break;
                        };
                        if valid_methods
                            .get(parent)
                            .is_some_and(|owner| Some(owner.as_str()) == receiver)
                        {
                            node.details["prototype_lexical_this"] = serde_json::json!(true);
                            break;
                        }
                        ancestor = parent;
                    }
                }
            }
        }
        relations::implicit_fields(facts);
    }
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&TYPESCRIPT, &JAVASCRIPT];

#[cfg(test)]
mod callback_parameter_tests {
    use super::{first_callback_parameter, language};

    fn find_arrow_function<'tree>(
        node: tree_sitter::Node<'tree>,
    ) -> Option<tree_sitter::Node<'tree>> {
        if node.kind() == "arrow_function" {
            return Some(node);
        }
        let mut cursor = node.walk();
        let arrow = node
            .named_children(&mut cursor)
            .find_map(find_arrow_function);
        arrow
    }

    #[test]
    fn first_callback_parameter_selects_the_typed_identifier() {
        let source = "const items = [1]; items.map((event: MouseEvent) => event);";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language("src/view.ts", "typescript"))
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let arrow = find_arrow_function(tree.root_node()).unwrap();
        let parameter = first_callback_parameter(arrow).unwrap();

        assert_eq!(parameter.utf8_text(source.as_bytes()).unwrap(), "event");
    }
}
