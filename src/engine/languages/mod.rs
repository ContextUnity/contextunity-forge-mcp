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
use crate::engine::linker::traits::{LanguageLinker, GENERIC_LINKER};
use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};
use tree_sitter::{Node as Syntax, Parser};
#[cfg(any(feature = "lang-java", feature = "lang-kotlin"))]
pub(crate) mod build_manifest;
/// Implements manifests support.
pub mod manifests;
#[path = "python/receivers.rs"]
pub(crate) mod python_receivers;
#[cfg(any(feature = "lang-html", feature = "lang-vue"))]
#[path = "support/template.rs"]
pub(crate) mod template;
#[cfg(any(feature = "lang-python", feature = "lang-rust"))]
pub(crate) mod toml_manifest;
#[path = "typescript/linker_bindings.rs"]
pub(crate) mod typescript_bindings;

pub(crate) const MAX_VALUE_FLOW_TYPE_DEPTH: usize = 8;

thread_local! {
    static THREAD_LOCAL_PARSERS: std::cell::RefCell<std::collections::HashMap<&'static str, tree_sitter::Parser>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Executes a closure with a warm, thread-local Tree-sitter parser configured for the grammar.
pub fn with_warm_parser<R>(
    lang_key: &'static str,
    grammar: tree_sitter::Language,
    f: impl FnOnce(&mut tree_sitter::Parser) -> R,
) -> Result<R> {
    let parser = THREAD_LOCAL_PARSERS.with(|cell| -> Result<tree_sitter::Parser> {
        let mut map = cell.borrow_mut();
        if let Some(mut p) = map.remove(lang_key) {
            p.reset();
            Ok(p)
        } else {
            let mut p = tree_sitter::Parser::new();
            p.set_language(&grammar)?;
            Ok(p)
        }
    })?;

    struct ReturnGuard {
        lang_key: &'static str,
        parser: Option<tree_sitter::Parser>,
    }
    impl Drop for ReturnGuard {
        fn drop(&mut self) {
            if let Some(p) = self.parser.take() {
                THREAD_LOCAL_PARSERS.with(|cell| {
                    if let Ok(mut map) = cell.try_borrow_mut() {
                        map.insert(self.lang_key, p);
                    }
                });
            }
        }
    }

    let mut guard = ReturnGuard {
        lang_key,
        parser: Some(parser),
    };

    let result = f(guard.parser.as_mut().expect("parser present"));
    Ok(result)
}

pub(crate) fn source_start(node: Syntax<'_>) -> crate::core::semantic::SourcePosition {
    let position = node.start_position();
    crate::core::semantic::SourcePosition {
        line: position.row + 1,
        column: position.column,
    }
}

pub(crate) fn source_end(node: Syntax<'_>) -> crate::core::semantic::SourcePosition {
    let position = node.end_position();
    crate::core::semantic::SourcePosition {
        line: position.row + 1,
        column: position.column,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
/// Import namespace shared by language profiles that can resolve one another.
pub struct LanguageFamily(pub &'static str);

/// Normalized module namespace and import interpretation for the linker.
pub struct ImportPath {
    /// Canonical namespace used for module lookup.
    pub namespace: String,
    /// Whether the import is relative to its owning module.
    pub relative: bool,
    /// Whether the namespace includes an imported symbol suffix.
    pub symbol_path: bool,
}
impl ImportPath {
    /// Creates an absolute module import with no symbol suffix.
    pub fn absolute(namespace: String) -> Self {
        Self {
            namespace,
            relative: false,
            symbol_path: false,
        }
    }
}

#[derive(Default)]
/// Per-file extraction state shared by hooks of one language profile.
pub struct FileContext {
    /// Export bindings discovered during extraction.
    pub exports: Vec<crate::core::semantic::ExportBinding>,
    /// Deferred export bindings resolved during linking.
    pub lazy_exports: Vec<LazyExport>,
    /// Names exported as defaults.
    pub default_exports: HashSet<String>,
    /// Package namespace when the source belongs to a package.
    pub package: Option<String>,
    /// Scope identifiers where a local binding shadows CommonJS `require`.
    pub shadowed_require_scopes: Arc<HashSet<usize>>,
    /// Scope identifiers where a source binding shadows the standard Array type.
    pub shadowed_array_type_scopes: Arc<HashSet<usize>>,
    /// Lexical scopes where a binding or assignment masks a DOM global.
    pub shadowed_dom_global_scopes: Arc<HashMap<usize, HashSet<String>>>,
    /// Exact DOM receiver parameter annotations indexed by function syntax node.
    pub dom_listener_receivers: Arc<HashMap<usize, HashMap<String, String>>>,
    /// Direct class field DOM annotations; duplicate or unknown fields have no type.
    pub dom_listener_class_fields: Arc<HashMap<usize, HashMap<String, Option<String>>>>,
    /// Source positions of lexical receiver writes within each enclosing scope.
    pub dom_listener_local_writes: Arc<HashMap<usize, HashMap<String, Vec<usize>>>>,
    /// Exact direct lexical declarations used to bound DOM receiver factory flow.
    pub dom_listener_local_declarations:
        Arc<HashMap<usize, HashMap<String, DomListenerLocalDeclaration>>>,
    /// Parameter and catch bindings that shadow DOM receiver names in their scope.
    pub dom_listener_parameter_bindings: Arc<HashMap<usize, HashSet<String>>>,
    /// Function scopes and names whose parameters are reassigned or rebound.
    pub rebound_function_parameters: Arc<HashMap<usize, HashSet<String>>>,
}

#[derive(Clone, Debug, Default)]
/// A bounded expression shape retained for DOM listener receiver inference.
pub enum DomListenerReceiverExpression {
    /// An identifier read at a source position.
    Identifier {
        /// Identifier spelling.
        name: String,
        /// Byte offset of the identifier read.
        position: usize,
    },
    /// A field on lexical `this`.
    ThisField {
        /// Field spelling.
        name: String,
        /// Syntax identifier of the containing class.
        class_scope: usize,
        /// Whether the containing method is static.
        static_method: bool,
    },
    /// A call to a member on a recursively described receiver.
    MemberCall {
        /// Receiver expression for the member call.
        receiver: Box<DomListenerReceiverExpression>,
        /// Member spelling.
        method: String,
        /// Exact type argument spellings, if supplied.
        type_arguments: Vec<String>,
        /// Whether the call has one or more explicit arguments.
        has_arguments: bool,
    },
    /// A parenthesized receiver expression.
    Parenthesized(Box<DomListenerReceiverExpression>),
    /// Any syntax outside the bounded receiver expression grammar.
    #[default]
    Unsupported,
}

#[derive(Clone, Debug, Default)]
/// A lexical binding summary used by DOM listener receiver inference.
pub struct DomListenerLocalDeclaration {
    /// Number of same-name declarations in the lexical block.
    pub declaration_count: usize,
    /// Whether the unique declaration uses `const`.
    pub is_const: bool,
    /// End byte of the declaration statement.
    pub statement_end: usize,
    /// Bounded initializer shape for the unique declaration, when present.
    pub initializer: Option<DomListenerReceiverExpression>,
    /// Ancestor scope identifiers at the declaration site.
    pub scope_chain: Arc<Vec<usize>>,
}

#[derive(serde::Serialize)]
/// Export that refers to another module or member and resolves during linking.
pub struct LazyExport {
    /// Exported name in the current module.
    pub name: String,
    /// Module that supplies the binding.
    pub module: String,
    /// Named member, or the module itself when absent.
    pub member: Option<String>,
    /// Whether the module path is relative to the current module.
    pub relative_to_module: bool,
}
#[derive(Default)]
/// Compact semantic metadata projected into an indexed symbol.
pub struct SymbolMetadata {
    /// Optional return type value.
    pub return_type: Option<crate::core::semantic::TypeExpr>,
    /// The param types value.
    pub param_types: BTreeMap<String, String>,
    /// Optional receiver name value.
    pub receiver_name: Option<String>,
    /// The decorators value.
    pub decorators: Vec<String>,
    /// Optional bases value.
    pub bases: Option<String>,
    /// Optional receiver value.
    pub receiver: Option<String>,
    /// Optional receiver type value.
    pub receiver_type: Option<String>,
    /// Whether method applies.
    pub is_method: Option<bool>,
    /// Whether static applies.
    pub is_static: Option<bool>,
    /// Whether default export applies.
    pub default_export: bool,
    /// Whether async applies.
    pub is_async: bool,
    /// Whether overload applies.
    pub is_overload: bool,
    /// Whether stub applies.
    pub is_stub: bool,
    /// Whether a declaration is guarded by source control flow in its owner scope.
    pub is_conditional: bool,
}

/// Borrowed syntax and source context passed to extraction hooks.
pub struct SyntaxContext<'a, 'tree> {
    /// The node value.
    pub node: Syntax<'tree>,
    /// The source value.
    pub source: &'a str,
    /// The owner value.
    pub owner: &'a str,
    /// The offset value.
    pub offset: usize,
    /// The shadowed require scopes value.
    pub shadowed_require_scopes: &'a HashSet<usize>,
}
impl SyntaxContext<'_, '_> {
    /// Returns the one-based source line after applying the file offset.
    pub fn line(&self) -> usize {
        self.node.start_position().row + self.offset + 1
    }
    /// Appends an import reference at this syntax node to the file facts.
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
            column: self.node.start_position().column,
            alias,
            module,
            receiver_hint: None,
        });
    }
}

/// Supplies syntax extraction and normalization rules for one language.
///
/// Profiles are shared as `Send + Sync` statics. Default hooks keep extraction
/// bounded to a source file; cross-file resolution belongs to `LanguageLinker`.
pub trait LanguageProfile: Send + Sync {
    /// Returns the stable language identifier used in indexed nodes.
    fn id(&self) -> &'static str;
    /// Lists manifest filenames recognized by this profile.
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &[]
    }
    /// Extracts dependency names from a recognized manifest file.
    fn extract_manifest_dependencies(&self, _filename: &str, _content: &str) -> Vec<String> {
        Vec::new()
    }
    /// Reports whether a module belongs to this language's standard library.
    fn is_stdlib(&self, _module: &str) -> bool {
        false
    }
    /// Returns the namespace family used for cross-profile import matching.
    fn family(&self) -> LanguageFamily {
        LanguageFamily(self.id())
    }
    /// Returns source file extensions accepted by this profile.
    fn extensions(&self) -> &'static [&'static str];
    /// Selects the Tree-sitter grammar for a source path.
    fn grammar(&self, path: &str) -> tree_sitter::Language;
    /// Creates a Tree-sitter parser configured for the source path.
    ///
    /// # Errors
    ///
    /// Returns an error when Tree-sitter rejects the grammar.
    fn create_parser(&self, path: &str) -> Result<Parser> {
        let mut parser = Parser::new();
        parser.set_language(&self.grammar(path))?;
        Ok(parser)
    }
    /// Maps a grammar node kind to an indexed symbol category.
    fn symbol_kind(&self, kind: &str) -> Option<&'static str>;
    /// Classifies a syntax node as an indexed symbol when applicable.
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        self.symbol_kind(node.kind())
    }
    /// Classifies a syntax node using its source text when needed.
    fn symbol_with_source(&self, node: Syntax<'_>, _source: &str) -> Option<&'static str> {
        self.symbol(node)
    }
    /// Returns the identifier prefix for a symbol category.
    fn node_prefix(&self, kind: &str) -> &'static str;
    /// Derives a module name from a workspace-relative path.
    fn module_name(&self, path: &str) -> String {
        module_stem(path).replace('/', ".")
    }
    /// Derives a module name when source content can affect its namespace.
    fn module_name_for_source(&self, path: &str, _source: &str) -> String {
        self.module_name(path)
    }
    /// Reports whether a declaration in one file is visible to a sibling file.
    fn sibling_accessible(&self, _path_a: &str, _path_b: &str) -> bool {
        false
    }
    /// Normalizes an import expression against its owning source path.
    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath>;
    /// Normalizes an import when workspace root context is available.
    fn normalize_import_with_root(
        &self,
        _root: Option<&Path>,
        owner: &str,
        module: &str,
    ) -> Option<ImportPath> {
        self.normalize_import(owner, module)
    }
    /// Classifies a module as an external dependency when known.
    fn external_import(&self, _module: &str) -> Option<&'static str> {
        None
    }
    /// Reports whether a name is a language builtin.
    fn builtin(&self, _name: &str) -> bool {
        false
    }
    /// Reports whether a name is a builtin type.
    fn builtin_type(&self, _name: &str) -> bool {
        false
    }
    /// Reports whether a member is builtin on a receiver type.
    fn builtin_member(&self, _receiver: &str, _member: &str) -> bool {
        false
    }
    /// Reports whether a receiver names a builtin generic type.
    fn builtin_generic(&self, _receiver: &str) -> bool {
        false
    }
    /// Extracts documentation text attached to a syntax node.
    fn doc_comment(&self, node: Syntax<'_>, source: &str) -> String {
        ast::doc_comment(node, source)
    }
    /// Extracts a symbol name borrowed from the source text.
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        ast::symbol_name(node, source)
    }
    /// Initializes per-file extraction state before walking the syntax tree.
    fn prepare(&self, _root: Syntax<'_>, _source: &str) -> FileContext {
        FileContext::default()
    }
    /// Projects bounded semantic metadata for an indexed declaration.
    fn metadata(
        &self,
        _node: Syntax<'_>,
        _source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        SymbolMetadata::default()
    }
    /// Reports whether a syntax node marks a test declaration.
    fn test_attribute(&self, _node: Syntax<'_>, _source: &str) -> bool {
        false
    }
    /// Reports whether this grammar needs class-scoped binding extraction.
    fn class_scope(&self) -> bool {
        false
    }
    /// Extracts local scope bindings from a syntax node.
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        ast::scope_bindings(node, source)
    }
    /// Extracts bounded local value-flow facts from a syntax node.
    fn value_flow(
        &self,
        _node: Syntax<'_>,
        _source: &str,
    ) -> crate::core::semantic::ValueFlowFacts {
        crate::core::semantic::ValueFlowFacts::default()
    }
    /// Reports whether a reference can use local value-flow bindings.
    fn value_binding_applies(&self, _reference: &Reference) -> bool {
        true
    }
    /// Reports whether a name denotes the receiver of its owning method.
    fn receiver(&self, name: &str, _owner: &Node) -> bool {
        name == "self" || name == "this" || name == "cls"
    }
    /// Appends import references found at the current syntax node.
    fn extract_imports(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    /// Appends structural relations found at the current syntax node.
    fn extract_relations(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    /// Appends call references found at the current syntax node.
    fn extract_calls(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    /// Appends mutation references found at the current syntax node.
    fn extract_mutations(&self, _ctx: &SyntaxContext<'_, '_>, _facts: &mut Facts) {}
    /// Appends route relationships found at the current syntax node.
    fn extract_routes(
        &self,
        _ctx: &SyntaxContext<'_, '_>,
        _facts: &mut Facts,
        _symbols: &HashMap<usize, String>,
    ) {
    }
    /// Finalizes extracted facts after the syntax walk.
    fn finish(&self, _facts: &mut Facts) {}
    /// Rewrites a search pattern before language-specific matching.
    fn prepare_pattern(&self, _pattern: &mut String) -> bool {
        false
    }
    /// Reports whether a syntax kind only wraps a searchable pattern.
    fn pattern_wrapper(&self, kind: &str) -> bool {
        matches!(
            kind,
            "module" | "program" | "source_file" | "expression_statement"
        )
    }
    /// Extracts symbols and references from a source file into bounded facts.
    ///
    /// # Errors
    ///
    /// Returns an error if the grammar cannot parse or extract the file.
    fn extract_file(&self, path: &str, source: &str, module: &str, facts: &mut Facts)
        -> Result<()>;
}

include!(concat!(env!("OUT_DIR"), "/language_profiles.rs"));

fn registry() -> impl Iterator<Item = &'static dyn LanguageProfile> + Clone {
    PROFILE_GROUPS
        .iter()
        .flat_map(|group| group.iter().copied())
}
/// Performs validate profiles.
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
/// Performs profiles.
pub fn profiles() -> impl Iterator<Item = &'static dyn LanguageProfile> + Clone {
    static VALIDATED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    VALIDATED
        .get_or_init(|| validate_profiles(registry()).expect("invalid compiled language registry"));
    registry()
}
/// Performs by id.
pub fn by_id(id: &str) -> Option<&'static dyn LanguageProfile> {
    profiles().find(|p| p.id() == id)
}
/// Performs linker for.
pub fn linker_for(id: &str) -> &'static dyn LanguageLinker {
    #[cfg(feature = "lang-rust")]
    if id == "rust" {
        return &rust::linker::RUST_LINKER;
    }
    #[cfg(feature = "lang-typescript")]
    if matches!(id, "typescript" | "javascript" | "vue") {
        return &typescript::linker::TYPESCRIPT_LINKER;
    }
    #[cfg(feature = "lang-python")]
    if id == "python" {
        return &python::linker::PYTHON_LINKER;
    }
    let _ = id;
    &GENERIC_LINKER
}
/// Performs require.
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
/// Performs for path.
pub fn for_path(path: &Path) -> Option<&'static dyn LanguageProfile> {
    let extension = path.extension()?.to_str()?;
    profiles().find(|p| p.extensions().contains(&extension))
}
/// Performs module stem.
pub fn module_stem(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(p, _)| p)
}
/// Performs workspace path.
pub fn workspace_path(path: &str) -> (&str, &str) {
    match path.split_once('/') {
        Some((head, tail)) if head.starts_with('[') && head.ends_with(']') => (head, tail),
        _ => ("", path),
    }
}
/// Performs relative namespace.
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
fn parser_slot(profile_id: &str, path: &str) -> Option<&'static str> {
    Some(match profile_id {
        "python" => "python",
        "rust" => "rust",
        "html" => "html",
        "vue" => "vue",
        "javascript" => "javascript",
        "typescript" if path.ends_with(".tsx") => "typescript-tsx",
        "typescript"
            if path.ends_with(".js")
                || path.ends_with(".jsx")
                || path.ends_with(".mjs")
                || path.ends_with(".cjs") =>
        {
            "javascript"
        }
        "typescript" => "typescript",
        _ => return None,
    })
}

fn parse_tree(
    profile: &dyn LanguageProfile,
    path: &str,
    source: &str,
) -> Result<tree_sitter::Tree> {
    let grammar = profile.grammar(path);
    if let Some(slot) = parser_slot(profile.id(), path) {
        return with_warm_parser(slot, grammar, |parser| parser.parse(source, None))?
            .context("Tree-sitter parse cancelled");
    }
    profile
        .create_parser(path)?
        .parse(source, None)
        .context("Tree-sitter parse cancelled")
}

/// Parses one source file into indexed facts.
pub fn parse_file(
    profile: &dyn LanguageProfile,
    path: &str,
    source: &str,
    module: &str,
    facts: &mut Facts,
) -> Result<()> {
    let tree = parse_tree(profile, path, source)?;
    ast::extract_tree(profile, tree.root_node(), path, source, module, facts);
    Ok(())
}
pub(crate) fn parse_file_typed(
    profile: &dyn LanguageProfile,
    path: &str,
    source: &str,
    module: &str,
    typed: &mut crate::core::typed_facts::TypedFacts,
) -> Result<()> {
    let tree = parse_tree(profile, path, source)?;
    ast::extract_tree_with_flows(
        profile,
        tree.root_node(),
        path,
        source,
        module,
        &format!("module:{path}"),
        &mut typed.facts,
        Some(&mut typed.flows),
    );
    Ok(())
}
/// Performs call.
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
        column: ctx.node.start_position().column,
        alias: None,
        module: import_path,
        receiver_hint: None,
    });
}

/// Performs module name.
pub fn module_name(path: &str) -> String {
    for_path(Path::new(path)).map_or_else(
        || module_stem(path).replace('/', "."),
        |p| p.module_name(path),
    )
}
