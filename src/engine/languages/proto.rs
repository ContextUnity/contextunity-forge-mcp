use super::*;
use tree_sitter::Language;

extern "C" {
    fn tree_sitter_proto() -> Language;
}

/// Performs language.
pub fn language() -> Language {
    unsafe { tree_sitter_proto() }
}

/// Performs kind.
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "message" => Some("class"),
        "service" => Some("service"),
        "rpc" => Some("function"),
        "enum" => Some("enum"),
        _ => None,
    }
}

fn parse_proto_package(source: &str) -> Option<&str> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("package") {
            let pkg = rest.trim().trim_end_matches(';').trim();
            if !pkg.is_empty() {
                return Some(pkg);
            }
        }
    }
    None
}

fn is_proto_primitive(name: &str) -> bool {
    matches!(
        name,
        "double"
            | "float"
            | "int32"
            | "int64"
            | "uint32"
            | "uint64"
            | "sint32"
            | "sint64"
            | "fixed32"
            | "fixed64"
            | "sfixed32"
            | "sfixed64"
            | "bool"
            | "string"
            | "bytes"
    )
}

/// Represents proto data.
pub struct Proto;
/// Shared proto language profile.
pub static PROTO: Proto = Proto;
impl LanguageProfile for Proto {
    fn id(&self) -> &'static str {
        "proto"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["proto"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        language()
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("proto")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        match kind {
            "class" => "message",
            "service" => "service",
            "function" => "rpc",
            "enum" => "enum",
            _ => "proto",
        }
    }
    fn module_name_for_source(&self, path: &str, source: &str) -> String {
        parse_proto_package(source)
            .map(str::to_owned)
            .unwrap_or_else(|| self.module_name(path))
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
        if node.kind() == "import" {
            let mut p = field(node, source, "path");
            if p.is_none() {
                let mut c = node.walk();
                p = node
                    .named_children(&mut c)
                    .find(|n| n.kind() == "string")
                    .map(|n| text(n, source));
            }
            if let Some(path) = p {
                let path = path.trim_matches(['"', '\'', '`']);
                add(
                    path.into(),
                    Some(path.rsplit('/').next().unwrap_or(path).into()),
                    Some(path.into()),
                );
            }
        }
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        Some(ImportPath {
            namespace: relative_namespace(owner, module.trim_end_matches(".proto"))?,
            relative: true,
            symbol_path: false,
        })
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        let mut c = node.walk();
        let name = node
            .named_children(&mut c)
            .find(|n| {
                matches!(
                    n.kind(),
                    "message_name" | "service_name" | "rpc_name" | "enum_name"
                )
            })
            .map(|n| text(n, source));
        name
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if ctx.node.kind() == "rpc" {
            let mut c = ctx.node.walk();
            for ch in ctx
                .node
                .named_children(&mut c)
                .filter(|ch| ch.kind() == "message_or_enum_type")
            {
                let expression = text(ch, ctx.source).trim();
                if !expression.is_empty() {
                    facts.references.push(Reference {
                        source: ctx.owner.into(),
                        dynamic: false,
                        expression: expression.into(),
                        kind: "references".into(),
                        line: ctx.line(),
                        column: ctx.node.start_position().column,
                        alias: None,
                        module: expression.rsplit_once('.').map(|(m, _)| m.into()),
                        receiver_hint: None,
                    });
                }
            }
        } else if ctx.node.kind() == "field" {
            let mut c = ctx.node.walk();
            for ch in ctx.node.named_children(&mut c) {
                if matches!(ch.kind(), "type" | "message_or_enum_type") {
                    let expression = text(ch, ctx.source).trim();
                    if !expression.is_empty() && !is_proto_primitive(expression) {
                        facts.references.push(Reference {
                            source: ctx.owner.into(),
                            dynamic: false,
                            expression: expression.into(),
                            kind: "references".into(),
                            line: ctx.line(),
                            column: ctx.node.start_position().column,
                            alias: None,
                            module: expression.rsplit_once('.').map(|(m, _)| m.into()),
                            receiver_hint: None,
                        });
                    }
                }
            }
        }
    }
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&PROTO];
