use super::*;
use crate::engine::ast::{relations, routes};
#[path = "python/computed_receivers.rs"]
mod computed_receivers;
#[path = "python/fastmcp.rs"]
mod fastmcp;
#[path = "python/lazy_exports.rs"]
mod lazy_exports;
#[path = "python/logging_factories.rs"]
mod logging_factories;

pub(crate) fn logging_factories(root: Syntax<'_>, source: &str, offset: usize) -> serde_json::Value {
    logging_factories::collect(root, source, offset)
}
#[path = "python/linker.rs"]
pub(crate) mod linker;
#[path = "python/manifest.rs"]
mod manifest;
#[path = "python/value_flow.rs"]
mod value_flow;
/// Performs language.
pub fn language() -> tree_sitter::Language {
    tree_sitter_python::language()
}
/// Performs kind.
pub fn kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" | "lambda" => Some("function"),
        "class_definition" => Some("class"),
        "type_alias_statement" => Some("type"),
        _ => None,
    }
}

fn class_method_owner(node: Syntax<'_>) -> Option<Syntax<'_>> {
    let mut parent = node.parent()?;
    if parent.kind() == "decorated_definition" {
        parent = parent.parent()?;
    }
    if parent.kind() != "block" {
        return None;
    }
    parent
        .parent()
        .filter(|owner| owner.kind() == "class_definition")
}

fn is_static_decorator(decorator: &str) -> bool {
    let decorator = decorator
        .trim()
        .strip_prefix('@')
        .unwrap_or(decorator.trim());
    decorator
        .split('(')
        .next()
        .unwrap_or_default()
        .trim()
        .rsplit('.')
        .next()
        == Some("staticmethod")
}

fn module_scope(node: Syntax<'_>) -> bool {
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        match ancestor.kind() {
            "function_definition" | "lambda" | "class_definition" => return false,
            "module" => return true,
            _ => parent = ancestor.parent(),
        }
    }
    false
}

fn typing_name(value: &str) -> &str {
    value
        .strip_prefix("typing.")
        .or_else(|| value.strip_prefix("typing_extensions."))
        .unwrap_or(value)
}

fn is_type_alias_rhs(node: Syntax<'_>, source: &str) -> bool {
    match node.kind() {
        "subscript" => true,
        "binary_operator" => field(node, source, "operator").is_some_and(|op| op == "|"),
        "identifier" => {
            let name = text(node, source);
            matches!(
                name,
                "int"
                    | "str"
                    | "float"
                    | "bool"
                    | "bytes"
                    | "dict"
                    | "list"
                    | "set"
                    | "tuple"
                    | "object"
                    | "None"
                    | "Any"
            ) || name.ends_with("Type")
                || name.ends_with("Dict")
                || name.ends_with("Value")
                || name.ends_with("Primitive")
                || name.ends_with("Mapping")
                || name.ends_with("Payload")
        }
        "attribute" => {
            let name = field(node, source, "attribute").unwrap_or_default();
            matches!(name, "Any" | "Type" | "None")
                || name.ends_with("Type")
                || name.ends_with("Dict")
                || name.ends_with("Value")
                || name.ends_with("Primitive")
                || name.ends_with("Mapping")
                || name.ends_with("Payload")
        }
        _ => false,
    }
}

fn type_alias(node: Syntax<'_>, source: &str) -> bool {
    if !matches!(node.kind(), "assignment" | "type_alias_statement") {
        return false;
    }
    if node.kind() == "type_alias_statement" {
        return module_scope(node);
    }
    if !node
        .child_by_field_name("left")
        .is_some_and(|left| left.kind() == "identifier")
    {
        return false;
    }
    if field(node, source, "type")
        .is_some_and(|annotation| matches!(typing_name(annotation), "TypeAlias" | "TypeAliasType"))
    {
        return module_scope(node);
    }
    if node
        .child_by_field_name("right")
        .filter(|right| right.kind() == "call")
        .and_then(|call| field(call, source, "function"))
        .is_some_and(|callee| {
            matches!(typing_name(callee), "TypeVar" | "NewType" | "TypeAliasType")
        })
        && module_scope(node)
    {
        return true;
    }
    if !module_scope(node) {
        return false;
    }
    let Some(left) = node.child_by_field_name("left") else {
        return false;
    };
    let left_name = text(left, source);
    if !left_name.starts_with(|c: char| c.is_ascii_uppercase()) {
        return false;
    }
    let Some(right) = node.child_by_field_name("right") else {
        return false;
    };
    is_type_alias_rhs(right, source)
}

fn is_inside_type_checking(mut curr: Syntax<'_>, source: &str) -> bool {
    while let Some(parent) = curr.parent() {
        if parent.kind() == "if_statement" {
            let in_consequence = parent
                .child_by_field_name("consequence")
                .is_some_and(|cons| cons.id() == curr.id());
            if in_consequence {
                if let Some(cond) = parent.child_by_field_name("condition") {
                    let cond_text = text(cond, source).trim();
                    let clean = cond_text.split('(').next().unwrap_or(cond_text).trim();
                    if clean == "TYPE_CHECKING"
                        || clean == "typing.TYPE_CHECKING"
                        || clean == "typing_extensions.TYPE_CHECKING"
                        || clean.ends_with(".TYPE_CHECKING")
                    {
                        return true;
                    }
                }
            }
        }
        curr = parent;
    }
    false
}

fn is_python_stub_body(node: Syntax<'_>, source: &str) -> bool {
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let mut cursor = body.walk();
    let mut non_trivial = Vec::new();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "comment" {
            continue;
        }
        if non_trivial.is_empty() && child.kind() == "expression_statement" {
            if let Some(first) = child.named_child(0) {
                if first.kind() == "string" {
                    continue;
                }
            }
        }
        non_trivial.push(child);
    }
    if non_trivial.is_empty() {
        return true;
    }
    non_trivial.iter().all(|stmt| {
        if stmt.kind() == "pass_statement" {
            return true;
        }
        if stmt.kind() == "expression_statement" {
            let t = text(*stmt, source).trim();
            if t == "..." || t == "Ellipsis" {
                return true;
            }
            if let Some(inner) = stmt.named_child(0) {
                if inner.kind() == "ellipsis" {
                    return true;
                }
            }
        }
        false
    })
}

/// Represents python data.
pub struct Python;
/// Shared python language profile.
pub static PYTHON: Python = Python;
impl LanguageProfile for Python {
    fn bindings(&self, node: Syntax<'_>, source: &str) -> ast::ScopeBindings {
        let mut bindings = ast::scope_bindings(node, source);
        if node.kind() == "module" {
            let aliases = value_flow::unique_module_aliases(node, source);
            bindings.rebindings.retain(|name| !aliases.contains(name));
        }
        bindings
    }
    fn builtin_member(&self, receiver: &str, member: &str) -> bool {
        builtin_member(receiver, member)
    }
    fn builtin_generic(&self, receiver: &str) -> bool {
        matches!(
            receiver,
            "dict"
                | "Dict"
                | "list"
                | "List"
                | "set"
                | "Set"
                | "tuple"
                | "Tuple"
                | "Mapping"
                | "MutableMapping"
                | "Sequence"
                | "Iterable"
        )
    }
    fn id(&self) -> &'static str {
        "python"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["py", "pyi"]
    }
    fn grammar(&self, _path: &str) -> tree_sitter::Language {
        language()
    }
    fn family(&self) -> LanguageFamily {
        LanguageFamily("python")
    }
    fn symbol_kind(&self, k: &str) -> Option<&'static str> {
        kind(k)
    }
    fn symbol(&self, node: Syntax<'_>) -> Option<&'static str> {
        if node.kind() == "function_definition" && class_method_owner(node).is_some() {
            Some("method")
        } else {
            self.symbol_kind(node.kind())
        }
    }
    fn symbol_with_source(&self, node: Syntax<'_>, source: &str) -> Option<&'static str> {
        if matches!(node.kind(), "assignment" | "type_alias_statement") {
            type_alias(node, source).then_some("type")
        } else {
            self.symbol(node)
        }
    }
    fn symbol_name<'a>(&self, node: Syntax<'_>, source: &'a str) -> Option<&'a str> {
        if node.kind() == "assignment" {
            return field(node, source, "left");
        }
        if node.kind() == "type_alias_statement" {
            let mut name = node.named_child(0)?;
            while matches!(name.kind(), "type" | "generic_type") {
                name = name.named_child(0)?;
            }
            return (name.kind() == "identifier").then(|| text(name, source));
        }
        ast::symbol_name(node, source)
    }
    fn node_prefix(&self, kind: &str) -> &'static str {
        match kind {
            "class" => "class",
            "type" => "type",
            _ => "py",
        }
    }
    fn module_name(&self, path: &str) -> String {
        module_stem(path)
            .trim_end_matches("/__init__")
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
    fn prepare(&self, root: Syntax<'_>, source: &str) -> FileContext {
        FileContext {
            lazy_exports: lazy_exports::extract(root, source),
            ..FileContext::default()
        }
    }
    fn extract_imports(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        let (node, source) = (ctx.node, ctx.source);
        let statement = text(node, source);
        let mut add = |expression, alias, module| ctx.import(facts, expression, alias, module);
        match node.kind() {
            "import_from_statement" => {
                let module = field(node, source, "module_name").unwrap_or("").to_owned();
                let mut c = node.walk();
                for child in node.children_by_field_name("name", &mut c) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    add(
                        name.into(),
                        field(child, source, "alias")
                            .map(str::to_owned)
                            .or_else(|| Some(name.into())),
                        Some(module.clone()),
                    );
                }
                if statement.contains('*') {
                    add("*".into(), Some("*".into()), Some(module));
                }
            }
            "import_statement"
                if statement.starts_with("import ")
                    && !statement.contains("from ")
                    && !statement.contains('"')
                    && !statement.contains('\'') =>
            {
                let mut c = node.walk();
                for child in node.children_by_field_name("name", &mut c) {
                    let name = field(child, source, "name").unwrap_or_else(|| text(child, source));
                    add(
                        name.into(),
                        field(child, source, "alias")
                            .map(str::to_owned)
                            .or_else(|| Some(name.split('.').next().unwrap_or(name).into())),
                        Some(name.into()),
                    );
                }
            }
            _ => {}
        }
    }
    fn extract_calls(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if matches!(ctx.node.kind(), "call") {
            call(ctx, facts, false);
            if let Some(hint) = computed_receivers::receiver_hint(ctx.node, ctx.source) {
                if let Some(reference) = facts.references.last_mut() {
                    reference.receiver_hint = Some(hint);
                }
            }
        }
    }
    fn external_import(&self, module: &str) -> Option<&'static str> {
        let root = module.split('.').next().unwrap_or(module);
        if is_python_stdlib(root) {
            Some("Python standard library")
        } else {
            None
        }
    }
    fn manifest_filenames(&self) -> &'static [&'static str] {
        &[
            "pyproject.toml",
            "requirements*.txt",
            "setup.cfg",
            "Pipfile",
        ]
    }
    fn extract_manifest_dependencies(&self, filename: &str, content: &str) -> Vec<String> {
        manifest::dependencies(filename, content)
    }
    fn is_stdlib(&self, module: &str) -> bool {
        is_python_stdlib(module.split('.').next().unwrap_or(module))
    }
    fn builtin(&self, symbol: &str) -> bool {
        matches!(
            symbol,
            "print"
                | "len"
                | "isinstance"
                | "issubclass"
                | "str"
                | "int"
                | "float"
                | "bool"
                | "list"
                | "dict"
                | "set"
                | "tuple"
                | "super"
                | "getattr"
                | "setattr"
                | "hasattr"
                | "delattr"
                | "type"
                | "repr"
                | "open"
                | "iter"
                | "next"
                | "any"
                | "all"
                | "min"
                | "max"
                | "sum"
                | "enumerate"
                | "zip"
                | "sorted"
                | "reversed"
                | "abs"
                | "round"
                | "id"
                | "hash"
                | "callable"
                | "dir"
                | "vars"
                | "help"
                | "range"
                | "frozenset"
                | "bytes"
                | "bytearray"
                | "memoryview"
                | "complex"
                | "slice"
                | "object"
                | "classmethod"
                | "staticmethod"
                | "property"
                | "filter"
                | "map"
                | "format"
                | "pow"
                | "divmod"
                | "bin"
                | "hex"
                | "oct"
                | "ord"
                | "chr"
                | "breakpoint"
                | "compile"
                | "eval"
                | "exec"
                | "BaseException"
                | "Exception"
                | "ArithmeticError"
                | "BufferError"
                | "LookupError"
                | "AssertionError"
                | "AttributeError"
                | "EOFError"
                | "FloatingPointError"
                | "GeneratorExit"
                | "ImportError"
                | "ModuleNotFoundError"
                | "IndexError"
                | "KeyError"
                | "KeyboardInterrupt"
                | "MemoryError"
                | "NameError"
                | "NotImplementedError"
                | "OSError"
                | "OverflowError"
                | "RecursionError"
                | "ReferenceError"
                | "RuntimeError"
                | "StopIteration"
                | "StopAsyncIteration"
                | "SyntaxError"
                | "IndentationError"
                | "TabError"
                | "SystemError"
                | "SystemExit"
                | "TypeError"
                | "UnboundLocalError"
                | "UnicodeError"
                | "UnicodeEncodeError"
                | "UnicodeDecodeError"
                | "UnicodeTranslateError"
                | "ValueError"
                | "ZeroDivisionError"
                | "EnvironmentError"
                | "IOError"
                | "BlockingIOError"
                | "ChildProcessError"
                | "ConnectionError"
                | "BrokenPipeError"
                | "ConnectionAbortedError"
                | "ConnectionRefusedError"
                | "ConnectionResetError"
                | "FileExistsError"
                | "FileNotFoundError"
                | "InterruptedError"
                | "IsADirectoryError"
                | "NotADirectoryError"
                | "PermissionError"
                | "ProcessLookupError"
                | "TimeoutError"
                | "Warning"
                | "UserWarning"
                | "DeprecationWarning"
                | "PendingDeprecationWarning"
                | "SyntaxWarning"
                | "RuntimeWarning"
                | "FutureWarning"
                | "ImportWarning"
                | "UnicodeWarning"
                | "BytesWarning"
                | "ResourceWarning"
                | "Any"
                | "Optional"
                | "Union"
                | "List"
                | "Dict"
                | "Set"
                | "Tuple"
                | "Callable"
                | "Iterator"
                | "Generator"
                | "Sequence"
                | "Mapping"
                | "Literal"
                | "TypeVar"
                | "Generic"
                | "Annotated"
                | "Protocol"
        )
    }
    fn builtin_type(&self, name: &str) -> bool {
        matches!(
            name,
            "str" | "int" | "float" | "bool" | "bytes" | "dict" | "list" | "set" | "tuple"
        )
    }

    fn normalize_import(&self, owner: &str, module: &str) -> Option<ImportPath> {
        if module.starts_with('.') {
            let count = module.bytes().take_while(|b| *b == b'.').count();
            let path = format!(
                "{}{}",
                "../".repeat(count - 1),
                module[count..].replace('.', "/")
            );
            Some(ImportPath {
                namespace: relative_namespace(owner, &path)?,
                relative: true,
                symbol_path: false,
            })
        } else {
            Some(ImportPath::absolute(module.into()))
        }
    }
    fn doc_comment(&self, node: Syntax<'_>, source: &str) -> String {
        if let Some(first) = node
            .child_by_field_name("body")
            .and_then(|b| b.named_child(0))
        {
            if first.kind() == "expression_statement"
                && first.named_child(0).is_some_and(|n| n.kind() == "string")
            {
                return text(first, source).into();
            }
        }
        ast::doc_comment(node, source)
    }
    fn receiver(&self, name: &str, owner: &Node) -> bool {
        matches!(name, "self" | "cls") && owner.details["receiver_name"] == name
    }
    fn value_flow(&self, node: Syntax<'_>, source: &str) -> crate::core::semantic::ValueFlowFacts {
        value_flow::extract(node, source)
    }
    fn metadata(
        &self,
        node: Syntax<'_>,
        source: &str,
        _name: &str,
        _file: &FileContext,
    ) -> SymbolMetadata {
        let decorators = node
            .parent()
            .filter(|p| p.kind() == "decorated_definition")
            .map(|p| {
                let mut c = p.walk();
                p.named_children(&mut c)
                    .filter(|n| n.kind() == "decorator")
                    .map(|n| text(n, source).to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let class_owner = class_method_owner(node);
        let receiver_name = if class_owner.is_some()
            && !decorators
                .iter()
                .any(|decorator| is_static_decorator(decorator))
        {
            node.child_by_field_name("parameters")
                .and_then(|p| p.named_child(0))
                .and_then(|p| {
                    if p.kind() == "identifier" {
                        Some(text(p, source))
                    } else {
                        p.named_child(0)
                            .filter(|n| n.kind() == "identifier")
                            .map(|n| text(n, source))
                    }
                })
                .map(str::to_owned)
                .filter(|name| matches!(name.as_str(), "self" | "cls"))
        } else {
            None
        };
        let receiver_type = class_owner
            .and_then(|owner| field(owner, source, "name"))
            .map(str::to_owned);
        let is_method = class_owner.is_some().then_some(true);
        let is_static = class_owner.is_some().then_some(receiver_name.is_none());
        let mut param_types = std::collections::BTreeMap::new();
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                if !matches!(
                    parameter.kind(),
                    "typed_parameter" | "typed_default_parameter"
                ) {
                    continue;
                }
                let Some(annotation) = field(parameter, source, "type") else {
                    continue;
                };
                let Some(name) = parameter
                    .child_by_field_name("name")
                    .or_else(|| parameter.named_child(0))
                else {
                    continue;
                };
                if name.kind() != "identifier" {
                    continue;
                }
                let annotation = annotation.trim();
                let annotation = annotation
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
                    .or_else(|| {
                        annotation
                            .strip_prefix('"')
                            .and_then(|value| value.strip_suffix('"'))
                    })
                    .unwrap_or(annotation);
                param_types.insert(text(name, source).to_owned(), annotation.to_owned());
            }
        }
        let is_overload = decorators.iter().any(|decorator| {
            let d = decorator.trim_start_matches('@').trim();
            let name = d.split('(').next().unwrap_or(d).trim();
            name == "overload"
                || name == "typing.overload"
                || name == "typing_extensions.overload"
                || name.ends_with(".overload")
        });
        let is_stub = is_overload
            || is_inside_type_checking(node, source)
            || (node.kind() == "function_definition" && is_python_stub_body(node, source));
        SymbolMetadata {
            receiver_name,
            decorators,
            bases: field(node, source, "superclasses").map(str::to_owned),
            receiver_type,
            is_method,
            is_static,
            param_types,
            is_async: text(node, source).trim_start().starts_with("async "),
            is_overload,
            is_stub,
            ..Default::default()
        }
    }
    fn extract_relations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        if let Some(bases) = ctx.node.child_by_field_name("superclasses") {
            let mut c = bases.walk();
            for base in bases
                .named_children(&mut c)
                .filter(|n| n.kind() != "keyword_argument")
            {
                relations::reference(
                    facts,
                    ctx.owner,
                    relations::type_name(base, ctx.source),
                    "inherits",
                    ctx.line(),
                );
            }
        }
        if ctx.node.kind() == "function_definition" {
            if let Some(parameters) = ctx.node.child_by_field_name("parameters") {
                let mut c = parameters.walk();
                for param in parameters.named_children(&mut c) {
                    if let Some(ty) = param.child_by_field_name("type") {
                        relations::type_references(facts, ctx.owner, ty, ctx.source, ctx.line());
                    }
                }
            }
            if let Some(ret) = ctx.node.child_by_field_name("return_type") {
                relations::type_references(facts, ctx.owner, ret, ctx.source, ctx.line());
            }
        }
        relations::decorator_references(ctx, facts);
        fastmcp::decorator_candidates(ctx, facts);
        routes::declaration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset);
    }
    fn extract_mutations(&self, ctx: &SyntaxContext<'_, '_>, facts: &mut Facts) {
        relations::mutation(
            ctx,
            facts,
            &[],
            &["assignment", "augmented_assignment"],
            &["attribute"],
        );
        relations::member_access(ctx, facts, &["attribute"]);
    }
    fn extract_routes(
        &self,
        ctx: &SyntaxContext<'_, '_>,
        facts: &mut Facts,
        symbols: &HashMap<usize, String>,
    ) {
        if ctx.node.kind() == "call" {
            routes::registration(ctx.node, ctx.source, ctx.owner, facts, ctx.offset, symbols);
        }
        fastmcp::module_assignment(ctx, facts);
    }
    fn finish(&self, facts: &mut Facts) {
        relations::implicit_fields(facts);
        fastmcp::registrations(facts);
    }
    fn prepare_pattern(&self, pattern: &mut String) -> bool {
        let partial = pattern.trim_end().ends_with(':');
        if partial {
            pattern.push_str("\n    __FORGE_META_BODY\n");
        }
        partial
    }
}

pub(crate) fn builtin_member(receiver: &str, member: &str) -> bool {
    match receiver {
        "str" => matches!(
            member,
            "capitalize"
                | "casefold"
                | "center"
                | "count"
                | "encode"
                | "endswith"
                | "expandtabs"
                | "find"
                | "format"
                | "format_map"
                | "index"
                | "isalnum"
                | "isalpha"
                | "isascii"
                | "isdecimal"
                | "isdigit"
                | "isidentifier"
                | "islower"
                | "isnumeric"
                | "isprintable"
                | "isspace"
                | "istitle"
                | "isupper"
                | "join"
                | "ljust"
                | "lower"
                | "lstrip"
                | "maketrans"
                | "partition"
                | "removeprefix"
                | "removesuffix"
                | "replace"
                | "rfind"
                | "rindex"
                | "rjust"
                | "rpartition"
                | "rsplit"
                | "rstrip"
                | "split"
                | "splitlines"
                | "startswith"
                | "strip"
                | "swapcase"
                | "title"
                | "translate"
                | "upper"
                | "zfill"
        ),
        "list" => matches!(
            member,
            "append"
                | "clear"
                | "copy"
                | "count"
                | "extend"
                | "index"
                | "insert"
                | "pop"
                | "remove"
                | "reverse"
                | "sort"
        ),
        "dict" | "Dict" | "Mapping" | "MutableMapping" => matches!(
            member,
            "clear"
                | "copy"
                | "fromkeys"
                | "get"
                | "items"
                | "keys"
                | "pop"
                | "popitem"
                | "setdefault"
                | "update"
                | "values"
        ),
        "Logger" | "LoggerAdapter" | "logging.Logger" | "logging.LoggerAdapter" => matches!(
            member,
            "info"
                | "warning"
                | "error"
                | "debug"
                | "critical"
                | "exception"
                | "log"
                | "isEnabledFor"
                | "setLevel"
                | "addHandler"
                | "removeHandler"
        ),
        "set" => matches!(
            member,
            "add"
                | "clear"
                | "copy"
                | "difference"
                | "difference_update"
                | "discard"
                | "intersection"
                | "intersection_update"
                | "isdisjoint"
                | "issubset"
                | "issuperset"
                | "pop"
                | "remove"
                | "symmetric_difference"
                | "symmetric_difference_update"
                | "union"
                | "update"
        ),
        "frozenset" => matches!(
            member,
            "copy"
                | "difference"
                | "intersection"
                | "isdisjoint"
                | "issubset"
                | "issuperset"
                | "symmetric_difference"
                | "union"
        ),
        "tuple" => matches!(member, "count" | "index"),
        "int" => matches!(
            member,
            "as_integer_ratio"
                | "bit_count"
                | "bit_length"
                | "conjugate"
                | "from_bytes"
                | "to_bytes"
        ),
        "float" => matches!(
            member,
            "as_integer_ratio" | "conjugate" | "fromhex" | "hex" | "is_integer"
        ),
        _ => false,
    }
}

fn is_python_stdlib(pkg: &str) -> bool {
    matches!(
        pkg,
        "__future__"
            | "abc"
            | "aifc"
            | "antigravity"
            | "argparse"
            | "array"
            | "ast"
            | "asynchat"
            | "asyncio"
            | "asyncore"
            | "atexit"
            | "audioop"
            | "base64"
            | "bdb"
            | "binascii"
            | "binhex"
            | "bisect"
            | "builtins"
            | "bz2"
            | "calendar"
            | "cgi"
            | "cgitb"
            | "chunk"
            | "cmath"
            | "cmd"
            | "code"
            | "codecs"
            | "codeop"
            | "collections"
            | "colorsys"
            | "compileall"
            | "concurrent"
            | "configparser"
            | "contextlib"
            | "contextvars"
            | "copy"
            | "copyreg"
            | "cProfile"
            | "crypt"
            | "csv"
            | "ctypes"
            | "curses"
            | "dataclasses"
            | "datetime"
            | "dbm"
            | "decimal"
            | "difflib"
            | "dis"
            | "distutils"
            | "doctest"
            | "email"
            | "encodings"
            | "enum"
            | "errno"
            | "faulthandler"
            | "fcntl"
            | "filecmp"
            | "fileinput"
            | "fnmatch"
            | "fractions"
            | "ftplib"
            | "functools"
            | "gc"
            | "getopt"
            | "getpass"
            | "gettext"
            | "glob"
            | "graphlib"
            | "grp"
            | "gzip"
            | "hashlib"
            | "heapq"
            | "hmac"
            | "html"
            | "http"
            | "idlelib"
            | "imaplib"
            | "imghdr"
            | "imp"
            | "importlib"
            | "inspect"
            | "io"
            | "ipaddress"
            | "itertools"
            | "json"
            | "keyword"
            | "lib2to3"
            | "linecache"
            | "locale"
            | "logging"
            | "lzma"
            | "mailbox"
            | "mailcap"
            | "marshal"
            | "math"
            | "mimetypes"
            | "mmap"
            | "modulefinder"
            | "msilib"
            | "msvcrt"
            | "multiprocessing"
            | "netrc"
            | "nis"
            | "nntplib"
            | "numbers"
            | "operator"
            | "optparse"
            | "os"
            | "ossaudiodev"
            | "parser"
            | "pathlib"
            | "pdb"
            | "pickle"
            | "pickletools"
            | "pipes"
            | "pkgutil"
            | "platform"
            | "plistlib"
            | "poplib"
            | "posix"
            | "posixpath"
            | "pprint"
            | "profile"
            | "pstats"
            | "pty"
            | "pwd"
            | "py_compile"
            | "pyclbr"
            | "pydoc"
            | "queue"
            | "quopri"
            | "random"
            | "re"
            | "readline"
            | "reprlib"
            | "resource"
            | "rlcompleter"
            | "runpy"
            | "sched"
            | "secrets"
            | "select"
            | "selectors"
            | "shelve"
            | "shlex"
            | "shutil"
            | "signal"
            | "site"
            | "smtpd"
            | "smtplib"
            | "sndhdr"
            | "socket"
            | "socketserver"
            | "spwd"
            | "sqlite3"
            | "sre_compile"
            | "sre_constants"
            | "sre_parse"
            | "ssl"
            | "stat"
            | "statistics"
            | "string"
            | "stringprep"
            | "struct"
            | "subprocess"
            | "sunau"
            | "symbol"
            | "symtable"
            | "sys"
            | "sysconfig"
            | "syslog"
            | "tabnanny"
            | "tarfile"
            | "telnetlib"
            | "tempfile"
            | "termios"
            | "test"
            | "textwrap"
            | "threading"
            | "time"
            | "timeit"
            | "tkinter"
            | "token"
            | "tokenize"
            | "tomllib"
            | "trace"
            | "traceback"
            | "tracemalloc"
            | "tty"
            | "turtle"
            | "turtledemo"
            | "types"
            | "typing"
            | "typing_extensions"
            | "unicodedata"
            | "unittest"
            | "urllib"
            | "uu"
            | "uuid"
            | "venv"
            | "warnings"
            | "wave"
            | "weakref"
            | "webbrowser"
            | "winreg"
            | "winsound"
            | "wsgiref"
            | "xdrlib"
            | "xml"
            | "xmlrpc"
            | "zipapp"
            | "zipfile"
            | "zipimport"
            | "zlib"
            | "_thread"
    )
}

/// Language profiles provided by this module.
pub static PROFILES: &[&dyn LanguageProfile] = &[&PYTHON];
