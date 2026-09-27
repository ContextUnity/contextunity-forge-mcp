#[cfg(any(feature = "lang-c", feature = "lang-cpp"))]
#[path = "support/c_family.rs"]
mod c_family;
#[cfg(any(
    feature = "lang-c",
    feature = "lang-cpp",
    feature = "lang-csharp",
    feature = "lang-java",
    feature = "lang-kotlin",
    feature = "lang-php",
    feature = "lang-ruby"
))]
#[path = "support/mod.rs"]
mod syntax;
use crate::core::models::{Facts, Node, Reference};
use crate::engine::ast::{self, field, text};
use anyhow::{Context, Result};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use tree_sitter::{Node as Syntax, Parser};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LanguageFamily(pub &'static str);

pub struct ImportPath {
    pub namespace: String,
    pub relative: bool,
    pub symbol_path: bool,
}
impl ImportPath {
    pub fn absolute(namespace: String) -> Self {
        Self {
            namespace,
            relative: false,
            symbol_path: false,
        }
    }
}

#[derive(Default)]
pub struct FileContext {
    pub default_exports: HashSet<String>,
    pub package: Option<String>,
}
#[derive(Default)]
pub struct SymbolMetadata {
    pub receiver_name: Option<String>,
    pub decorators: Vec<String>,
    pub bases: Option<String>,
    pub receiver: Option<String>,
    pub default_export: bool,
    pub is_async: bool,
}

pub struct SyntaxContext<'a, 'tree> {
    pub node: Syntax<'tree>,
    pub source: &'a str,
    pub owner: &'a str,
    pub offset: usize,
}
impl SyntaxContext<'_, '_> {
    pub fn line(&self) -> usize {
        self.node.start_position().row + self.offset + 1
    }
    pub fn import(
        &self,
        facts: &mut Facts,
        expression: String,
        alias: Option<String>,
        module: Option<String>,
    ) {
        facts.references.push(Reference {
            source: self.owner.into(),
            dynamic: false,
            expression,
            kind: "imports".into(),
            line: self.line(),
            alias,
            module,
        });
    }
}

pub trait LanguageProfile: Send + Sync {
    fn id(&self) -> &'static str;
    fn family(&self) -> LanguageFamily {
        LanguageFamily(self.id())
    }
    fn extensions(&self) -> &'static [&'static str];
    fn grammar(&self, path: &str) -> tree_sitter::Language;
    fn create_parser(&self, path: &str) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&self.grammar(path))?;
        Ok(parser)
    }
    fn symbol_kind(&self, kind: &str) -> Option<&'static str>;
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        self.symbol_kind(node.kind())
    }
    fn node_prefix(&self, kind: &str) -> &'static str;
    fn module_name(&self, path: &str) -> String {
        module_stem(path).replace('/', ".")
    }
    fn module_name_for_source(&self, path: &str, _source: &str) -> String {
        self.module_name(path)
    }
    fn sibling_accessible(&self, _path_a: &str, _path_b: &str) -> bool {
        false
    }
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath>;
    fn normalize_import_with_root(&self, _root: Option<&Path>, owner: &str, module: &str) -> Option<ImportPath> {
        self.normalize_import(owner, module)
    }
    fn builtin(&self, _name: &str) -> bool {
        false
    }
    fn doc_comment(&self, node: Syntax<'_>, source: &str) -> String {
        ast::doc_comment(node, source)
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        ast::symbol_name(node, source)
    }
    fn prepare(&self, _root: Syntax<'_>, _source: &str) -> FileContext {
        FileContext::default()
    }
    fn metadata(
        &self,
        _node: Syntax<'_>,
        _source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        SymbolMetadata::default()
    }
    fn test_attribute(&self, _node: Syntax<'_>, _source: &str) -> bool {
        false
    }
    fn class_scope(&self) -> bool {
        false
    }
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        ast::scope_bindings(node, source)
    }
    fn value_binding_applies(&self, _reference: &Reference) -> bool {
        true
    }
    fn receiver(&self, name: &str, _owner: &Node) -> bool {
        name == "self" || name == "this" || name == "cls"
    }
    fn extract_imports(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    fn extract_relations(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    fn extract_calls(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    fn extract_mutations(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    fn extract_routes(
        &self,
        _ctx: &SyntaxContext<'_, '_>,
        _facts: &mut Facts,
        _symbols: &HashMap<usize, String>,
    ) {
    }
    fn finish(&self, _facts: &mut Facts) {}
    fn prepare_pattern(&self, _pattern: &mut String) -> bool {
        false
    }
    fn extract_file(&self, path: &str, source: &str, module: &str, facts: &mut Facts)
        -> Result<()>;
}

include!(concat!(env!("OUT_DIR"), "/language_profiles.rs"));

fn registry() -> impl Iterator<Item = &'static dyn LanguageProfile> + Clone {
    PROFILE_GROUPS
        .iter()
        .flat_map(|group| group.iter().copied())
}
pub fn validate_profiles(
    profiles: impl IntoIterator<Item = &'static dyn LanguageProfile>,
) -> Result<()> {
    let mut ids = HashSet::new();
    let mut extensions = HashSet::new();
    for profile in profiles {
        anyhow::ensure!(
            !profile.id().is_empty() && ids.insert(profile.id()),
            "duplicate or empty language profile id: {}",
            profile.id()
        );
        anyhow::ensure!(
            !profile.extensions().is_empty(),
            "profile {} has no extensions",
            profile.id()
        );
        for extension in profile.extensions() {
            anyhow::ensure!(
                !extension.is_empty()
                    && extension.bytes().all(|b| b.is_ascii_alphanumeric())
                    && extensions.insert(*extension),
                "duplicate or invalid language extension: {extension}"
            );
        }
    }
    Ok(())
}
pub fn profiles() -> impl Iterator<Item = &'static dyn LanguageProfile> + Clone {
    static VALIDATED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    VALIDATED
        .get_or_init(|| validate_profiles(registry()).expect("invalid compiled language registry"));
    registry()
}
pub fn by_id(id: &str) -> Option<&'static dyn LanguageProfile> {
    profiles().find(|p| p.id() == id)
}
pub fn require(id: &str) -> Result<&'static dyn LanguageProfile> {
    if let Some(profile) = by_id(id) {
        return Ok(profile);
    }
    let provider = if id == "javascript" { "typescript" } else { id };
    let feature = format!("lang-{provider}");
    let compiled = profiles().map(|p| p.id()).collect::<Vec<_>>().join(", ");
    let compiled = if compiled.is_empty() {
        "none (document-only build)"
    } else {
        &compiled
    };
    if AVAILABLE_LANGUAGE_FEATURES.contains(&feature.as_str()) {
        anyhow::bail!(
            "AST language '{id}' is disabled; rebuild with --features {feature} or all-languages; compiled languages: {compiled}"
        );
    }
    anyhow::bail!("unknown AST language '{id}'; compiled languages: {compiled}");
}
pub fn for_path(path: &Path) -> Option<&'static dyn LanguageProfile> {
    let extension = path.extension()?.to_str()?;
    profiles().find(|p| p.extensions().contains(&extension))
}
pub fn module_stem(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(p, _)| p)
}
pub fn workspace_path(path: &str) -> (&str, &str) {
    match path.split_once('/') {
        Some((head, tail)) if head.starts_with('[') && head.ends_with(']') => (head, tail),
        _ => ("", path),
    }
}
pub fn relative_namespace(owner: &str, module: &str) -> Option<String> {
    let (_, owner) = workspace_path(owner);
    let mut parts: Vec<_> = owner
        .rsplit_once('/')
        .map_or("", |(p, _)| p)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    for part in module.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("."))
}
pub fn parse_file(
    profile: &dyn LanguageProfile,
    path: &str,
    source: &str,
    module: &str,
    facts: &mut Facts,
) -> Result<()> {
    let tree = profile
        .create_parser(path)?
        .parse(source, None)
        .context("Tree-sitter parse cancelled")?;
    ast::extract_tree(profile, tree.root_node(), path, source, module, facts);
    Ok(())
}
pub fn call(ctx: &SyntaxContext<'_, '_>, facts: &mut Facts, dynamic_imports: bool) {
    let Some(callee) =
        field(ctx.node, ctx.source, "function").or_else(|| field(ctx.node, ctx.source, "macro"))
    else {
        return;
    };
    let dynamic = dynamic_imports && matches!(callee, "import" | "require");
    let import_path = if dynamic {
        ctx.node
            .child_by_field_name("arguments")
            .and_then(|n| n.named_child(0))
            .filter(|n| n.kind() == "string")
            .map(|n| text(n, ctx.source).trim_matches(['\'', '"']).to_owned())
    } else {
        None
    };
    facts.references.push(Reference {
        source: ctx.owner.into(),
        dynamic: !callee
            .chars()
            .all(|c| c.is_alphanumeric() || "_.:!".contains(c)),
        expression: import_path
            .clone()
            .unwrap_or_else(|| ast::bounded_expression(callee)),
        kind: if dynamic { "imports" } else { "calls" }.into(),
        line: ctx.line(),
        alias: None,
        module: import_path,
    });
}

pub fn module_name(path: &str) -> String {
    for_path(Path::new(path)).map_or_else(
        || module_stem(path).replace('/', "."),
        |p| p.module_name(path),
    )
}
