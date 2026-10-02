use super::*;
/// Performs language.
pub fn language() -> tree_sitter::Language {
    tree_sitter_go::language()
}
/// Performs kind.
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration" => Some("function"),
        "method_declaration" => Some("method"),
        "type_spec" => Some("type"),
        _ => None,
    }
}

fn parse_package_name(source: &str) -> Option<&str> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("package") {
            let pkg = rest.split_whitespace().next()?.trim_matches(';');
            if !pkg.is_empty() {
                return Some(pkg);
            }
        }
    }
    None
}

fn dependency_path(line: &str) -> Option<String> {
    let candidate = line.split_whitespace().next()?;
    let path = candidate.trim_matches(['\'', '"']);
    (path.contains('.') || path.contains('/')).then(|| path.to_owned())
}

/// Language profile for go source files.
pub struct Go;
/// Shared go language profile.
pub static GO: Go = Go;
impl LanguageProfile for Go {
    fn id(&self) -> &'static str {
        "go"
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &["go.mod"]
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        if filename != "go.mod" {
            return Vec::new();
        }
        let mut dependencies = Vec::new();
        let mut in_require_block = false;
        for line in content.lines() {
            let line = line.split_once("//").map_or(line, |(before, _)| before).trim();
            if line.is_empty() {
                continue;
            }
            if in_require_block {
                if line.starts_with(')') {
                    in_require_block = false;
                } else if let Some(module) = dependency_path(line) {
                    dependencies.push(module);
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("require") {
                if !rest
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_whitespace() || character == '(')
                {
                    continue;
                }
                let rest = rest.trim();
                if rest.starts_with('(') {
                    in_require_block = true;
                } else if let Some(module) = dependency_path(rest) {
                    dependencies.push(module);
                }
            }
        }
        dependencies.sort();
        dependencies.dedup();
        dependencies
    }
    fn is_stdlib(&self, module: &str) -> bool {
        const STANDARD_PACKAGES: &[&str] = &[
            "archive", "bufio", "bytes", "cmp", "compress", "container", "context",
            "crypto", "database", "debug", "embed", "encoding", "errors", "expvar",
            "flag", "fmt", "go", "hash", "html", "image", "index", "io", "iter",
            "log", "maps", "math", "mime", "net", "os", "path", "plugin", "reflect",
            "regexp", "runtime", "slices", "sort", "strconv", "strings", "sync",
            "syscall", "testing", "text", "time", "unicode", "unique", "unsafe",
        ];
        let root = module.split('/').next().unwrap_or(module);
        STANDARD_PACKAGES.contains(&root)
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["go"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        language()
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("go")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn node_prefix(&self, _kind: &str) -> &'static str {
        "go"
    }
    fn module_name_for_source(&self, path: &str, source: &str) -> String {
        let pkg = parse_package_name(source);
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        match (dir, pkg) {
            ("", Some(pkg)) => pkg.to_string(),
            ("", None) => module_stem(path).to_string(),
            (dir, Some(pkg)) => {
                let dir_mod = dir.replace('/', ".");
                if dir.ends_with(pkg) {
                    dir_mod
                } else {
                    format!("{dir_mod}.{pkg}")
                }
            }
            (dir, None) => dir.replace('/', "."),
        }
    }
    fn module_name(&self, path: &str) -> String {
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        if dir.is_empty() {
            module_stem(path).to_string()
        } else {
            dir.replace('/', ".")
        }
    }
    fn sibling_accessible(&self, path_a: &str, path_b: &str) -> bool {
        let dir_a = path_a.rsplit_once('/').map_or("", |(d, _)| d);
        let dir_b = path_b.rsplit_once('/').map_or("", |(d, _)| d);
        dir_a == dir_b
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
        if node.kind() == "import_spec" {
            if let Some(path) = field(node, source, "path") {
                let path = path.trim_matches(['"', '`']);
                add(
                    path.into(),
                    Some(
                        field(node, source, "name")
                            .unwrap_or(path.rsplit('/').next().unwrap_or(path))
                            .into(),
                    ),
                    Some(path.into()),
                );
            }
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(ctx.node.kind(), "call_expression") {
            call(ctx, facts, false);
        }
    }
    fn builtin(&self, symbol: &str) -> bool {
        matches!(
            symbol,
            "make"
                | "new"
                | "len"
                | "cap"
                | "append"
                | "copy"
                | "delete"
                | "recover"
                | "close"
                | "panic"
                | "print"
                | "println"
                | "clear"
                | "min"
                | "max"
                | "complex"
                | "real"
                | "imag"
        )
    }

    fn normalize_import(&self, _owner: &str, module: &str) -> Option<ImportPath> {
        Some(ImportPath::absolute(module.replace('/', ".")))
    }
    fn metadata(
        &self,
        node: Syntax<'_>,
        source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        if node.kind() == "method_declaration" {
            let receiver = field(node, source, "receiver").map(str::to_owned);
            let param = node
                .child_by_field_name("receiver")
                .and_then(|recv| recv.named_child(0));
            let receiver_type = param
                .and_then(|p| p.child_by_field_name("type"))
                .map(|ty| text(ty, source).trim().trim_start_matches('*').to_owned());
            let receiver_name = param
                .and_then(|p| p.child_by_field_name("name"))
                .map(|n| text(n, source).trim().to_owned());
            return SymbolMetadata {
                receiver,
                receiver_name,
                receiver_type,
                is_method: Some(true),
                is_static: Some(false),
                ..Default::default()
            };
        }
        SymbolMetadata {
            receiver: field(node, source, "receiver").map(str::to_owned),
            ..Default::default()
        }
    }
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&GO];
