use super::*;
use crate::core::models::ReceiverHint;
use crate::engine::ast::{relations, routes};
use std::path::PathBuf;
#[path = "typescript/linker.rs"]
pub(crate) mod linker;
#[path = "typescript/commonjs.rs"]
mod commonjs;
pub(crate) use commonjs::commonjs_bindings;
#[path = "typescript/value_flow.rs"]
mod value_flow;
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
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration"
        | "generator_function_declaration"
        | "arrow_function"
        | "function_expression" => Some("function"),
        "method_definition" | "method_signature" => Some("method"),
        "class_declaration" | "class" => Some("class"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}

fn method_owner<'tree>(node: Syntax<'tree>) -> Option<Syntax<'tree>> {
    let mut current = node.parent()?;
    while matches!(
        current.kind(),
        "class_body" | "interface_body" | "object_type"
    ) {
        if let Some(owner) = current.parent() {
            if matches!(
                owner.kind(),
                "class_declaration" | "class" | "interface_declaration"
            ) {
                return Some(owner);
            }
        }
        current = current.parent()?;
    }
    None
}

fn resolve_export_target(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(conds) => conds
            .get("import")
            .or_else(|| conds.get("default"))
            .or_else(|| conds.get("types"))
            .or_else(|| conds.get("require"))
            .or_else(|| conds.get("node"))
            .and_then(resolve_export_target),
        _ => None,
    }
}

fn resolve_exports_field(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    match exports {
        serde_json::Value::String(s) if subpath == "." => Some(s.clone()),
        serde_json::Value::Object(map) => {
            if let Some(target) = map.get(subpath) {
                resolve_export_target(target)
            } else if subpath == "." {
                resolve_export_target(exports)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn resolve_package_json_exports(
    root: Option<&Path>,
    owner: &str,
    module: &str,
) -> Option<ImportPath> {
    let (pkg_name, subpath) = if module.starts_with('@') {
        let mut parts = module.splitn(3, '/');
        let scope = parts.next()?;
        let name = parts.next()?;
        let rest = parts.next().unwrap_or("");
        (
            format!("{scope}/{name}"),
            if rest.is_empty() {
                ".".to_string()
            } else {
                format!("./{rest}")
            },
        )
    } else if !module.starts_with('.') && !module.starts_with('/') {
        let (name, rest) = module.split_once('/').unwrap_or((module, ""));
        (
            name.to_string(),
            if rest.is_empty() {
                ".".to_string()
            } else {
                format!("./{rest}")
            },
        )
    } else {
        return None;
    };

    let owner_path = match root {
        Some(r) if !Path::new(owner).is_absolute() => r.join(owner),
        _ => PathBuf::from(owner),
    };
    let mut current_dir = owner_path.parent();
    let mut found_pkg_json: Option<(std::path::PathBuf, serde_json::Value)> = None;

    while let Some(dir) = current_dir {
        let nm_candidate = dir
            .join("node_modules")
            .join(&pkg_name)
            .join("package.json");
        if nm_candidate.exists() {
            if let Ok(content) = std::fs::read_to_string(&nm_candidate) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    found_pkg_json = Some((nm_candidate.parent().unwrap().to_path_buf(), v));
                    break;
                }
            }
        }
        let local_pkg = dir.join("package.json");
        if local_pkg.exists() {
            if let Ok(content) = std::fs::read_to_string(&local_pkg) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if v.get("name").and_then(|n| n.as_str()) == Some(&pkg_name) {
                        found_pkg_json = Some((dir.to_path_buf(), v));
                        break;
                    }
                }
            }
        }
        if let Some(r) = root {
            if dir == r {
                break;
            }
        }
        current_dir = dir.parent();
    }

    if found_pkg_json.is_none() {
        let root_dir = root.unwrap_or_else(|| Path::new("."));
        let simple_name = pkg_name.rsplit('/').next().unwrap_or(&pkg_name);
        for prefix in ["packages", "crates", "libs", "modules"] {
            let p = root_dir.join(prefix);
            let candidate = p.join(simple_name).join("package.json");
            if candidate.exists() {
                if let Ok(content) = std::fs::read_to_string(&candidate) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                        if v.get("name").and_then(|n| n.as_str()) == Some(&pkg_name) {
                            found_pkg_json = Some((candidate.parent().unwrap().to_path_buf(), v));
                            break;
                        }
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(&p) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("package.json");
                    if candidate.exists() {
                        if let Ok(content) = std::fs::read_to_string(&candidate) {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                                if v.get("name").and_then(|n| n.as_str()) == Some(&pkg_name) {
                                    found_pkg_json = Some((entry.path(), v));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if found_pkg_json.is_some() {
                break;
            }
        }
    }

    let (pkg_dir, pkg_json) = found_pkg_json?;
    let exports = pkg_json.get("exports")?;
    let target_file_rel = resolve_exports_field(exports, &subpath)?;
    let clean_target = target_file_rel.trim_start_matches("./");
    let target_path = pkg_dir.join(clean_target);
    let target_str = if let Some(r) = root {
        target_path
            .strip_prefix(r)
            .unwrap_or(&target_path)
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        target_path.to_string_lossy().replace('\\', "/")
    };
    let target_mod = module_stem(&target_str)
        .trim_end_matches("/index")
        .replace('/', ".");

    Some(ImportPath {
        namespace: target_mod,
        relative: false,
        symbol_path: false,
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

fn pattern_binds_require(pattern: Syntax<'_>, source: &str) -> bool {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            text(pattern, source).trim() == "require"
        }
        "pair_pattern" => pattern
            .child_by_field_name("value")
            .is_some_and(|value| pattern_binds_require(value, source)),
        "assignment_pattern" => pattern
            .child_by_field_name("left")
            .is_some_and(|left| pattern_binds_require(left, source)),
        "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern"
        | "required_parameter" | "optional_parameter" | "formal_parameter" => {
            let candidate = pattern
                .child_by_field_name("name")
                .or_else(|| pattern.child_by_field_name("pattern"));
            if let Some(candidate) = candidate {
                return pattern_binds_require(candidate, source);
            }
            let mut cursor = pattern.walk();
            let binds = pattern
                .named_children(&mut cursor)
                .any(|child| pattern_binds_require(child, source));
            binds
        }
        _ => false,
    }
}

fn nearest_binding_scope(node: Syntax<'_>, var_scoped: bool) -> Option<Syntax<'_>> {
    let mut current = Some(node);
    while let Some(scope) = current {
        if var_scoped {
            if matches!(
                scope.kind(),
                "program"
                    | "source_file"
                    | "function_declaration"
                    | "generator_function_declaration"
                    | "function_expression"
                    | "generator_function"
                    | "generator_function_expression"
                    | "arrow_function"
                    | "method_definition"
            ) {
                return Some(scope);
            }
        } else if matches!(
            scope.kind(),
            "statement_block"
                | "catch_clause"
                | "for_statement"
                | "for_in_statement"
                | "program"
                | "source_file"
        ) {
            return Some(scope);
        }
        current = scope.parent();
    }
    None
}

fn collect_shadowed_require_scopes(root: Syntax<'_>, source: &str, file: &mut FileContext) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let (binding, var_scoped) = match node.kind() {
            "variable_declarator" => (
                node.child_by_field_name("name")
                    .is_some_and(|name| pattern_binds_require(name, source)),
                node.parent()
                    .is_some_and(|parent| parent.kind() == "variable_declaration"),
            ),
            "formal_parameters" | "required_parameter" | "optional_parameter"
            | "formal_parameter" => (pattern_binds_require(node, source), true),
            "arrow_function" => (
                node.child_by_field_name("parameter")
                    .is_some_and(|parameter| pattern_binds_require(parameter, source)),
                true,
            ),
            "function_expression" | "generator_function" | "generator_function_expression" => (
                field(node, source, "name").is_some_and(|name| name.trim() == "require"),
                true,
            ),
            "function_declaration" | "generator_function_declaration" | "class_declaration" => (
                field(node, source, "name").is_some_and(|name| name.trim() == "require"),
                false,
            ),
            "import_statement" => (import_binds_require(node, source), false),
            "catch_clause" => (
                node.child_by_field_name("parameter")
                    .is_some_and(|pattern| pattern_binds_require(pattern, source)),
                false,
            ),
            _ => (false, false),
        };
        if binding {
            if let Some(scope) = nearest_binding_scope(node, var_scoped) {
                std::sync::Arc::make_mut(&mut file.shadowed_require_scopes).insert(scope.id());
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
}

fn import_binds_require(node: Syntax<'_>, source: &str) -> bool {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        match current.kind() {
            "import_clause" | "namespace_import" => {
                let mut cursor = current.walk();
                if current.named_children(&mut cursor).any(|child| {
                    child.kind() == "identifier" && text(child, source).trim() == "require"
                }) {
                    return true;
                }
            }
            "import_specifier" => {
                let local =
                    field(current, source, "alias").or_else(|| field(current, source, "name"));
                if local.is_some_and(|name| name.trim() == "require") {
                    return true;
                }
            }
            _ => {}
        }
        let mut cursor = current.walk();
        stack.extend(current.named_children(&mut cursor));
    }
    false
}

pub struct TypeScript {
    pub javascript: bool,
}
pub static TYPESCRIPT: TypeScript = TypeScript { javascript: false };
pub static JAVASCRIPT: TypeScript = TypeScript { javascript: true };
impl LanguageProfile for TypeScript {
    fn id(&self) -> &'static str {
        if self.javascript {
            "javascript"
        } else {
            "typescript"
        }
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &["package.json"]
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        if filename != "package.json" {
            return Vec::new();
        }
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(content) else {
            return Vec::new();
        };
        ["dependencies", "devDependencies", "peerDependencies"]
            .into_iter()
            .filter_map(|section| manifest.get(section)?.as_object())
            .flat_map(|dependencies| dependencies.keys().cloned())
            .collect()
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
        let mut add = |expression, alias, module| ctx.import(facts, expression, alias, module);
        if node.kind() == "import_statement" {
            let module =
                field(node, source, "source").map(|s| s.trim_matches(['\'', '"']).to_owned());
            fn ids(node: Syntax<'_>, source: &str, values: &mut Vec<(String, Option<String>)>) {
                match node.kind() {
                    "import_specifier" => {
                        let name =
                            field(node, source, "name").unwrap_or_else(|| text(node, source));
                        values.push((
                            name.into(),
                            field(node, source, "alias")
                                .map(str::to_owned)
                                .or_else(|| Some(name.into())),
                        ));
                    }
                    "namespace_import" => {
                        if let Some(n) = node.named_child(0) {
                            values.push(("*".into(), Some(text(n, source).into())));
                        }
                    }
                    "identifier" => {
                        values.push(("default".into(), Some(text(node, source).into())))
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
            let mut c = node.walk();
            for n in node.named_children(&mut c) {
                if n.kind() == "import_clause" {
                    ids(n, source, &mut names);
                }
            }
            if names.is_empty() {
                add("*".into(), None, module);
            } else {
                for (name, alias) in names {
                    add(name, alias, module.clone());
                }
            }
        } else if node.kind() == "export_statement" {
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
                call(ctx, facts, true);
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
                        let hint =
                            match receiver.kind() {
                                "call_expression" => {
                                    receiver.child_by_field_name("function").map(|callee| {
                                        ReceiverHint::CallResult {
                                            callee: text(callee, ctx.source).to_owned(),
                                            member: text(member, ctx.source).to_owned(),
                                        }
                                    })
                                }
                                "new_expression" => receiver
                                    .child_by_field_name("constructor")
                                    .map(|callee| ReceiverHint::ConstructorResult {
                                        callee: text(callee, ctx.source).to_owned(),
                                        member: text(member, ctx.source).to_owned(),
                                    }),
                                "string" => Some(ReceiverHint::StringLiteral {
                                    member: text(member, ctx.source).to_owned(),
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
        if matches!(
            symbol,
            "Object.assign"
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
        ) {
            return true;
        }
        matches!(
            symbol,
            "parseInt"
                | "parseFloat"
                | "fetch"
                | "alert"
                | "confirm"
                | "prompt"
                | "String"
                | "Number"
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
                | "Element"
                | "HTMLElement"
                | "HTMLDivElement"
                | "HTMLInputElement"
                | "HTMLButtonElement"
                | "HTMLAnchorElement"
                | "Document"
                | "Window"
                | "Event"
                | "CustomEvent"
                | "MouseEvent"
                | "KeyboardEvent"
                | "Response"
                | "Request"
                | "Headers"
                | "URL"
                | "URLSearchParams"
                | "FormData"
                | "Blob"
                | "File"
        )
    }

    fn builtin_generic(&self, receiver: &str) -> bool {
        matches!(receiver, "Array" | "Map" | "Set")
    }
    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        match receiver {
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
        root: Option<&Path>,
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
        } else if let Some(resolved) = resolve_package_json_exports(root, owner, module) {
            Some(resolved)
        } else {
            Some(ImportPath::absolute(module.replace('/', ".")))
        }
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        self.is_stdlib(module).then_some("Node.js standard library")
    }
    fn prepare(&self, root: Syntax<'_>, source: &str) -> FileContext {
        let mut file = FileContext {
            exports: value_flow::exports(root, source),
            ..Default::default()
        };
        collect_shadowed_require_scopes(root, source, &mut file);
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
        mut node: Syntax<'_>,
        source: &str,
        name: &str,
        file: &FileContext,
    ) -> SymbolMetadata {
        let mut param_types = BTreeMap::new();
        let documented = value_flow::jsdoc(node, source).0;
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                let name = parameter
                    .child_by_field_name("pattern")
                    .or_else(|| parameter.child_by_field_name("name"))
                    .or_else(|| (parameter.kind() == "identifier").then_some(parameter))
                    .filter(|node| node.kind() == "identifier")
                    .map(|node| text(node, source).trim().to_owned());
                let ty = parameter.child_by_field_name("type").map(|node| {
                    text(node, source)
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
        let is_async = text(node, source).trim_start().starts_with("async ");
        let is_method_node = matches!(node.kind(), "method_definition" | "method_signature");
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
            receiver_name: if is_method == Some(true) && is_static != Some(true) {
                Some("this".into())
            } else {
                None
            },
            receiver_type,
            is_method,
            is_static,
            is_overload,
            is_stub,
            ..Default::default()
        }
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
        relations::member_access(ctx, facts, &["member_expression"]);
    }
    fn extract_routes(
        &self,
        ctx: &SyntaxContext<'_, '_>,
        facts: &mut Facts,
        symbols: &HashMap<usize, String>,
    ) {
        if matches!(ctx.node.kind(), "call_expression" | "object") {
            routes::registration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset, symbols);
        }
    }
    fn finish(&self, facts: &mut Facts) {
        relations::implicit_fields(facts);
    }
}

pub static PROFILES: &[&dyn LanguageProfile] = &[&TYPESCRIPT, &JAVASCRIPT];
