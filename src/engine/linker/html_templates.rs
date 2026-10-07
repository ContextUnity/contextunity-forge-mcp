use super::*;
use crate::core::semantic::{TemplateContextFact, ValueExpr, ValueFlowFacts};
use crate::core::typed_facts::FlowState;

pub(super) struct JinjaRootRegistry<'a> {
    by_root: HashMap<&'a str, Vec<&'a Node>>,
}

impl<'a> JinjaRootRegistry<'a> {
    pub(super) fn build<F: AsRef<Facts> + Sync>(
        all: &'a BTreeMap<String, F>,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Self {
        let mut by_root: HashMap<&str, Vec<&Node>> = HashMap::new();
        for (path, source) in all {
            if !dependencies.declares_for_path(LanguageFamily("python"), path, "jinja2") {
                continue;
            }
            for node in source.as_ref().nodes.iter().filter(|node| {
                node.kind == "template_loader_root"
                    && node.language == "python"
                    && node.details["provider"] == "jinja2"
            }) {
                if !source_imports_are_external(path, node, all, dependencies) {
                    continue;
                }
                by_root.entry(&node.name).or_default().push(node);
            }
        }
        Self { by_root }
    }

    pub(super) fn admits(
        &self,
        path: &str,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> bool {
        if !dependencies.declares_for_path(LanguageFamily("python"), path, "jinja2") {
            return false;
        }
        let Some(scope) =
            dependencies.nearest_manifest_scope_for_path(LanguageFamily("python"), path)
        else {
            return false;
        };
        let mut providers = 0;
        let mut prefix = path.rsplit_once('/').map(|(dir, _)| dir);
        while let Some(dir) = prefix {
            if let Some(nodes) = self.by_root.get(dir) {
                providers += nodes
                    .iter()
                    .filter(|node| {
                        dependencies
                            .nearest_manifest_scope_for_path(LanguageFamily("python"), &node.path)
                            == Some(scope)
                    })
                    .count();
            }
            prefix = dir.rsplit_once('/').map(|(parent, _)| parent);
        }
        providers == 1
    }
}

fn source_imports_are_external<F: AsRef<Facts> + Sync>(
    owner: &str,
    node: &Node,
    all: &BTreeMap<String, F>,
    dependencies: &languages::manifests::DependencyRegistry,
) -> bool {
    let Some(profile) = languages::by_id("python") else {
        return false;
    };
    let Some(scope) = dependencies.nearest_manifest_scope_for_path(LanguageFamily("python"), owner)
    else {
        return false;
    };
    let Some(imports) = node.details["import_modules"]
        .as_array()
        .filter(|imports| !imports.is_empty())
    else {
        return false;
    };
    for import in imports {
        let Some(module) = import.as_str() else {
            return false;
        };
        let Some(normalized) = profile
            .normalize_import(owner, module)
            .filter(|import| !import.relative)
        else {
            return false;
        };
        let stem = normalized.namespace.replace('.', "/");
        let src_root = if scope.is_empty() {
            "src".to_owned()
        } else {
            format!("{scope}/src")
        };
        let indexed_module_at = |directory: &str| {
            [".py", ".pyi", "/__init__.py", "/__init__.pyi"]
                .iter()
                .any(|suffix| {
                    let candidate = if directory.is_empty() {
                        format!("{stem}{suffix}")
                    } else {
                        format!("{directory}/{stem}{suffix}")
                    };
                    all.contains_key(&candidate)
                })
        };
        let mut directory = owner
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        loop {
            if indexed_module_at(directory) || (directory == scope && indexed_module_at(&src_root))
            {
                return false;
            }
            if directory == scope {
                break;
            }
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    true
}

struct Library<'a> {
    module: &'a Node,
    symbols: HashMap<&'static str, HashMap<String, Vec<&'a Node>>>,
}

pub(super) enum Match<'a> {
    Missing,
    Ambiguous,
    Unique { module: &'a Node, symbol: &'a Node },
}

pub(super) struct Registry<'a> {
    by_scope: HashMap<String, HashMap<String, Vec<Library<'a>>>>,
}

impl<'a> Registry<'a> {
    pub(super) fn build<F: AsRef<Facts> + Sync>(
        all: &'a BTreeMap<String, F>,
        flows: Option<&HashMap<&str, &FlowState>>,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Self {
        let mut by_scope: HashMap<String, HashMap<String, Vec<Library<'a>>>> = HashMap::new();
        for (path, source) in all {
            let Some(library_name) = path
                .rsplit_once("/templatetags/")
                .map(|(_, name)| name)
                .or_else(|| path.strip_prefix("templatetags/"))
                .and_then(|name| name.strip_suffix(".py"))
                .filter(|name| !name.contains('/') && languages::html::valid_template_name(name))
            else {
                continue;
            };
            let Some(scope) =
                dependencies.nearest_manifest_scope_for_path(LanguageFamily("python"), path)
            else {
                continue;
            };
            if !dependencies.declares_for_path(LanguageFamily("python"), path, "django") {
                continue;
            }
            let facts = source.as_ref();
            let Some(module) = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module" && node.language == "python")
            else {
                continue;
            };
            let typed_flow = flows
                .and_then(|flows| flows.get(module.id.as_str()))
                .and_then(|state| match state {
                    FlowState::Valid { facts, .. } => Some(facts.as_ref()),
                    FlowState::Invalid(_) => None,
                });
            let public_flow = if typed_flow.is_none() {
                ValueFlowFacts::deserialize_for_scope(&module.details["value_flow"], false).ok()
            } else {
                None
            };
            let Some(flow) = typed_flow.or(public_flow.as_ref()) else {
                continue;
            };
            let mut symbols: HashMap<&'static str, HashMap<String, Vec<&Node>>> = HashMap::new();
            for function in facts.nodes.iter().filter(|node| node.kind == "function") {
                if !function
                    .qualname
                    .rsplit_once('.')
                    .is_some_and(|(owner, name)| owner == module.qualname && name == function.name)
                {
                    continue;
                }
                let Some(decorators) = function.details["decorators"].as_array() else {
                    continue;
                };
                for decorator in decorators.iter().filter_map(|value| value.as_str()) {
                    let Some((receiver, kind, name)) = registration(decorator, &function.name)
                    else {
                        continue;
                    };
                    if verified_library_instance(facts, module, flow, receiver, function.line) {
                        symbols
                            .entry(kind)
                            .or_default()
                            .entry(name)
                            .or_default()
                            .push(function);
                    }
                }
            }
            by_scope
                .entry(scope.to_owned())
                .or_default()
                .entry(library_name.to_owned())
                .or_default()
                .push(Library { module, symbols });
        }
        Self { by_scope }
    }

    pub(super) fn library(
        &self,
        owner: &str,
        name: &str,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Match<'a> {
        let Some(libraries) = self.libraries(owner, name, dependencies) else {
            return Match::Missing;
        };
        if libraries.len() != 1 {
            return Match::Ambiguous;
        }
        Match::Unique {
            module: libraries[0].module,
            symbol: libraries[0].module,
        }
    }

    pub(super) fn symbol(
        &self,
        owner: &str,
        loads: &[&Reference],
        reference: &Reference,
        kind: &str,
        name: &str,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Match<'a> {
        let mut selected = None;
        for load in loads.iter().copied().filter(|load| {
            load.line < reference.line
                && load
                    .alias
                    .as_deref()
                    .is_none_or(|selected| selected == name)
        }) {
            let Some(libraries) = self.libraries(owner, &load.expression, dependencies) else {
                continue;
            };
            if libraries.len() != 1 {
                return Match::Ambiguous;
            }
            let library = &libraries[0];
            let Some(providers) = library
                .symbols
                .get(kind)
                .and_then(|symbols| symbols.get(name))
            else {
                continue;
            };
            if providers.len() != 1 {
                return Match::Ambiguous;
            }
            let provider = providers[0];
            if selected.is_some_and(|(existing, _): (&Node, &Node)| existing.id != provider.id) {
                return Match::Ambiguous;
            }
            selected = Some((provider, library.module));
        }
        match selected {
            Some((symbol, module)) => Match::Unique { module, symbol },
            None => Match::Missing,
        }
    }

    fn libraries(
        &self,
        owner: &str,
        name: &str,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Option<&[Library<'a>]> {
        if !dependencies.declares_for_path(LanguageFamily("python"), owner, "django") {
            return None;
        }
        let scope =
            dependencies.nearest_manifest_scope_for_path(LanguageFamily("python"), owner)?;
        self.by_scope.get(scope)?.get(name).map(Vec::as_slice)
    }
}

struct TemplateFile<'a> {
    module: &'a Node,
    variables: HashMap<&'a str, Vec<&'a Node>>,
    macros: HashMap<&'a str, Vec<&'a Node>>,
    imports: Vec<&'a Reference>,
}

pub(super) struct ExpressionRegistry<'a> {
    files: HashMap<&'a str, TemplateFile<'a>>,
}

impl<'a> ExpressionRegistry<'a> {
    pub(super) fn build<F: AsRef<Facts> + Sync>(all: &'a BTreeMap<String, F>) -> Self {
        let mut files = HashMap::new();
        for (path, source) in all {
            let facts = source.as_ref();
            let Some(module) = facts
                .nodes
                .iter()
                .find(|node| node.kind == "module" && node.language == "html")
            else {
                continue;
            };
            let mut variables: HashMap<&str, Vec<&Node>> = HashMap::new();
            let mut macros: HashMap<&str, Vec<&Node>> = HashMap::new();
            for node in &facts.nodes {
                match (
                    node.kind.as_str(),
                    node.details["template_binding"].as_str(),
                ) {
                    ("variable", Some("variable")) => {
                        variables.entry(&node.name).or_default().push(node)
                    }
                    ("function", Some("function")) => {
                        macros.entry(&node.name).or_default().push(node)
                    }
                    _ => {}
                }
            }
            let imports = facts
                .references
                .iter()
                .filter(|reference| reference.kind == "template_bindings")
                .collect();
            files.insert(
                path.as_str(),
                TemplateFile {
                    module,
                    variables,
                    macros,
                    imports,
                },
            );
        }
        Self { files }
    }

    pub(super) fn resolve(&self, owner: &str, reference: &Reference) -> Match<'a> {
        let Some(file) = self.files.get(owner) else {
            return Match::Missing;
        };
        if let Some(name) = reference.expression.strip_prefix("template.variable.") {
            let Some(bindings) = file.variables.get(name) else {
                return Match::Missing;
            };
            let mut latest: Option<&Node> = None;
            for node in bindings
                .iter()
                .copied()
                .filter(|node| node.line < reference.line)
            {
                if latest.is_some_and(|previous| previous.line == node.line) {
                    return Match::Ambiguous;
                }
                if latest.is_none_or(|previous| previous.line < node.line) {
                    latest = Some(node);
                }
            }
            return latest.map_or(Match::Missing, |symbol| Match::Unique {
                module: file.module,
                symbol,
            });
        }
        let Some(callee) = reference.expression.strip_prefix("template.macro.") else {
            return Match::Missing;
        };
        let (alias, selected_name) = callee
            .split_once('.')
            .map_or((callee, None), |(alias, name)| (alias, Some(name)));
        let mut selected: Option<(&Node, &Node)> = None;
        if selected_name.is_none() {
            if let Some(macros) = file.macros.get(alias) {
                for symbol in macros
                    .iter()
                    .copied()
                    .filter(|node| node.line < reference.line)
                {
                    if selected.is_some() {
                        return Match::Ambiguous;
                    }
                    selected = Some((file.module, symbol));
                }
            }
        }
        for import in
            file.imports.iter().copied().filter(|import| {
                import.line < reference.line && import.alias.as_deref() == Some(alias)
            })
        {
            let Some(target) = import
                .module
                .as_deref()
                .and_then(|target| languages::html::local_template_target(owner, target))
            else {
                continue;
            };
            let Some(provider) = self.files.get(target.as_str()) else {
                continue;
            };
            let macro_name = match selected_name {
                Some(name) if import.expression == "*" => name,
                None if import.expression != "*" => import.expression.as_str(),
                _ => continue,
            };
            let Some(macros) = provider.macros.get(macro_name) else {
                continue;
            };
            if macros.len() != 1 {
                return Match::Ambiguous;
            }
            let symbol = macros[0];
            if selected.is_some_and(|(_, existing)| existing.id != symbol.id) {
                return Match::Ambiguous;
            }
            selected = Some((provider.module, symbol));
        }
        selected.map_or(Match::Missing, |(module, symbol)| Match::Unique {
            module,
            symbol,
        })
    }
}

pub(super) struct RenderProvider<'a> {
    pub(super) source: &'a Node,
    pub(super) context: TemplateContextFact,
}

pub(super) struct RenderContextRegistry<'a> {
    providers: Vec<RenderProvider<'a>>,
    by_template: HashMap<String, Vec<usize>>,
    by_provider: HashMap<&'a str, Vec<(usize, &'a Node)>>,
}

impl<'a> RenderContextRegistry<'a> {
    pub(super) fn build<F: AsRef<Facts> + Sync>(
        all: &'a BTreeMap<String, F>,
        flows: Option<&HashMap<&str, &FlowState>>,
        semantic_context: &semantic_context::Context<'a>,
        dependencies: &languages::manifests::DependencyRegistry,
    ) -> Self {
        let modules: HashMap<&str, &Node> = all
            .iter()
            .filter_map(|(path, source)| {
                source
                    .as_ref()
                    .nodes
                    .iter()
                    .find(|node| node.kind == "module" && node.language == "html")
                    .map(|node| (path.as_str(), node))
            })
            .collect();
        let mut includes: HashMap<&str, Vec<String>> = HashMap::new();
        for (path, source) in all {
            if !modules.contains_key(path.as_str()) {
                continue;
            }
            for reference in source
                .as_ref()
                .references
                .iter()
                .filter(|reference| reference.kind == "includes")
            {
                if let Some(target) =
                    languages::html::local_template_target(path, &reference.expression)
                {
                    if modules.contains_key(target.as_str()) {
                        includes.entry(path).or_default().push(target);
                    }
                }
            }
        }
        let mut providers = Vec::new();
        let mut by_template: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_provider: HashMap<&str, Vec<(usize, &Node)>> = HashMap::new();
        for (path, source) in all {
            if !dependencies.declares_for_path(LanguageFamily("python"), path, "django") {
                continue;
            }
            let Some((render_module, render_symbol)) = languages::linker_for("python")
                .framework_manifest(
                    dependencies,
                    LanguageFamily("python"),
                    path,
                    "django",
                    "django",
                )
                .and_then(|manifest| manifest.receiver_target("template_context"))
                .and_then(|receiver| receiver.rsplit_once('.'))
            else {
                continue;
            };
            let Some(import_owner) = source
                .as_ref()
                .nodes
                .iter()
                .find(|node| node.kind == "module" && node.language == "python")
            else {
                continue;
            };
            let Some(scope) =
                dependencies.nearest_manifest_scope_for_path(LanguageFamily("python"), path)
            else {
                continue;
            };
            let workspace = languages::workspace_path(path).0;
            for node in source.as_ref().nodes.iter().filter(|node| {
                node.language == "python"
                    && matches!(node.kind.as_str(), "module" | "function" | "method")
            }) {
                let typed = flows
                    .and_then(|flows| flows.get(node.id.as_str()))
                    .and_then(|state| match state {
                        FlowState::Valid { facts, .. } => Some(facts.as_ref()),
                        FlowState::Invalid(_) => None,
                    });
                let public = if typed.is_none() {
                    ValueFlowFacts::deserialize_for_scope(&node.details["value_flow"], false).ok()
                } else {
                    None
                };
                let Some(flow) = typed.or(public.as_ref()) else {
                    continue;
                };
                for context in &flow.template_contexts {
                    if !semantic_context.exact_external_import(
                        import_owner,
                        &context.import_alias,
                        render_module,
                        render_symbol,
                        context.position,
                    ) {
                        continue;
                    }
                    let Some(target) =
                        languages::html::local_template_target(path, &context.target)
                    else {
                        continue;
                    };
                    if languages::workspace_path(&target).0 != workspace
                        || dependencies
                            .nearest_manifest_scope_for_path(LanguageFamily("python"), &target)
                            != Some(scope)
                    {
                        continue;
                    }
                    let Some(_) = modules.get(target.as_str()) else {
                        continue;
                    };
                    let index = providers.len();
                    providers.push(RenderProvider {
                        source: node,
                        context: context.clone(),
                    });
                    let mut visited = HashSet::new();
                    let mut frontier = vec![(target, 0usize)];
                    while let Some((current, depth)) = frontier.pop() {
                        if !visited.insert(current.clone()) || depth > 16 {
                            continue;
                        }
                        if languages::workspace_path(&current).0 != workspace
                            || dependencies
                                .nearest_manifest_scope_for_path(LanguageFamily("python"), &current)
                                != Some(scope)
                        {
                            continue;
                        }
                        let Some(module) = modules.get(current.as_str()).copied() else {
                            continue;
                        };
                        by_template.entry(current.clone()).or_default().push(index);
                        by_provider
                            .entry(path.as_str())
                            .or_default()
                            .push((index, module));
                        if let Some(children) = includes.get(current.as_str()) {
                            frontier
                                .extend(children.iter().cloned().map(|child| (child, depth + 1)));
                        }
                    }
                }
            }
        }
        Self {
            providers,
            by_template,
            by_provider,
        }
    }

    pub(super) fn providers(&self, path: &str) -> impl Iterator<Item = &RenderProvider<'a>> {
        self.by_template
            .get(path)
            .into_iter()
            .flatten()
            .map(|index| &self.providers[*index])
    }

    pub(super) fn targets(
        &self,
        path: &str,
    ) -> impl Iterator<Item = (&RenderProvider<'a>, &'a Node)> {
        self.by_provider
            .get(path)
            .into_iter()
            .flatten()
            .map(|(index, module)| (&self.providers[*index], *module))
    }
}

fn registration<'a>(
    decorator: &'a str,
    default_name: &str,
) -> Option<(&'a str, &'static str, String)> {
    let declaration = decorator.trim().strip_prefix('@')?;
    let (callee, arguments) = if let Some((callee, arguments)) = declaration.split_once('(') {
        (callee.trim(), Some(arguments.strip_suffix(')')?.trim()))
    } else {
        (declaration.trim(), None)
    };
    let (receiver, method) = callee.rsplit_once('.')?;
    if !languages::html::valid_template_name(receiver) {
        return None;
    }
    let kind = match method {
        "filter" => "filter",
        "simple_tag" | "inclusion_tag" => "tag",
        _ => return None,
    };
    let name = match arguments {
        None | Some("") => default_name,
        Some(arguments) => {
            let (key, literal) = arguments.split_once('=')?;
            if key.trim() != "name" {
                return None;
            }
            let literal = literal.trim();
            let quote = literal.chars().next()?;
            if !matches!(quote, '\'' | '"') || !literal.ends_with(quote) || literal.len() < 2 {
                return None;
            }
            &literal[1..literal.len() - 1]
        }
    };
    languages::html::valid_template_name(name).then(|| (receiver, kind, name.to_owned()))
}

fn verified_library_instance(
    facts: &Facts,
    module: &Node,
    flow: &ValueFlowFacts,
    receiver: &str,
    function_line: usize,
) -> bool {
    let Some(latest) = flow
        .bindings
        .iter()
        .filter(|binding| binding.name == receiver && binding.position.line < function_line)
        .max_by_key(|binding| binding.position)
    else {
        return false;
    };
    if latest.conditional {
        return false;
    }
    let ValueExpr::Call { callee } = &latest.value else {
        return false;
    };
    let (alias, expected_symbol, expected_module) = match callee.rsplit_once('.') {
        Some((alias, "Library")) if languages::html::valid_template_name(alias) => {
            (alias, "template", "django")
        }
        None if callee == "Library" => ("Library", "Library", "django.template"),
        _ => return false,
    };
    let mut imports = facts.references.iter().filter(|reference| {
        reference.kind == "imports"
            && reference.source == module.id
            && reference.alias.as_deref() == Some(alias)
            && reference.line < latest.position.line
    });
    let Some(import) = imports.next() else {
        return false;
    };
    if imports.next().is_some()
        || import.expression != expected_symbol
        || import.module.as_deref() != Some(expected_module)
    {
        return false;
    }
    !flow.bindings.iter().any(|binding| {
        binding.name == alias
            && binding.position.line > import.line
            && binding.position.line < latest.position.line
    })
}
