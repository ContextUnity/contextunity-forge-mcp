use super::*;
use crate::engine::ast::{relations, routes};
use std::path::PathBuf;
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

fn resolve_export_target(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(conds) => {
            conds.get("import")
                .or_else(|| conds.get("default"))
                .or_else(|| conds.get("types"))
                .or_else(|| conds.get("require"))
                .or_else(|| conds.get("node"))
                .and_then(resolve_export_target)
        }
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

fn resolve_package_json_exports(root: Option<&Path>, owner: &str, module: &str) -> Option<ImportPath> {
    let (pkg_name, subpath) = if module.starts_with('@') {
        let mut parts = module.splitn(3, '/');
        let scope = parts.next()?;
        let name = parts.next()?;
        let rest = parts.next().unwrap_or("");
        (format!("{scope}/{name}"), if rest.is_empty() { ".".to_string() } else { format!("./{rest}") })
    } else if !module.starts_with('.') && !module.starts_with('/') {
        let (name, rest) = module.split_once('/').unwrap_or((module, ""));
        (name.to_string(), if rest.is_empty() { ".".to_string() } else { format!("./{rest}") })
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
        let nm_candidate = dir.join("node_modules").join(&pkg_name).join("package.json");
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
        target_path.strip_prefix(r).unwrap_or(&target_path).to_string_lossy().replace('\\', "/")
    } else {
        target_path.to_string_lossy().replace('\\', "/")
    };
    let target_mod = module_stem(&target_str).trim_end_matches("/index").replace('/', ".");

    Some(ImportPath {
        namespace: target_mod,
        relative: false,
        symbol_path: false,
    })
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
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(ctx.node.kind(), "call_expression") {
            call(ctx, facts, true);
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        matches!(
            symbol,
            "parseInt"
                | "parseFloat"
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
        )
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
        module.starts_with("node:").then_some("Node.js built-in module")
    }
    fn prepare(&self, root: Syntax<'_>, source: &str) -> FileContext {
        let mut file = FileContext::default();
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
    fn metadata(
        &self,
        mut node: Syntax<'_>,
        source: &str,
        name: &str,
        file: &FileContext,
    ) -> SymbolMetadata {
        let is_async = text(node, source).trim_start().starts_with("async ");
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
            default_export,
            is_async,
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
