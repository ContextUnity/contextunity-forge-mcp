use super::*;

pub(super) fn phase_framework_projections<'a, 'file, F>(resolver: &mut FileResolver<'a, 'file, F>)
where
    'a: 'file,
    F: AsRef<Facts> + Sync,
{
    let input = resolver.input;
    let path = resolver.path;
    let profile = input.profiles.get(path).copied();
    let references = resolver.references;
    let dependency_registry = input.dependency_registry;
    let by_module = input.by_module;
    let module_nodes = input.module_nodes;
    let language_context = resolver.language_context;
    let html_templates = &resolver.linker.html_templates;
    let template_roots = &resolver.linker.template_roots;
    let graph = &mut resolver.graph;

    resolver.template_loads = if profile.is_some_and(|profile| profile.id() == "html") {
        references
            .iter()
            .filter(|reference| reference.kind == "template_loads")
            .collect()
    } else {
        Vec::new()
    };
    resolver.template_registry_rules = if profile.is_some_and(|profile| profile.id() == "html") {
        language_context
            .syntax_rules
            .iter()
            .filter(|rule| {
                rule.language == "html"
                    && matches!(
                        rule.operation,
                        languages::manifests::syntax::SyntaxOperation::NamedBinding {
                            binding: languages::manifests::projections::NamedBindingForm::TemplateReference { .. },
                            ..
                        }
                    )
            })
            .collect()
    } else {
        Vec::new()
    };
    resolver.template_rules = if profile.is_some_and(|profile| profile.id() == "html") {
        language_context
            .syntax_rules
            .iter()
            .filter(|rule| {
                let languages::manifests::syntax::SyntaxOperation::TemplateSymbols {
                    requires_loader,
                    ..
                } = &rule.operation
                else {
                    return false;
                };
                !*requires_loader || template_roots.admits(path, dependency_registry, &rule.pack)
            })
            .collect()
    } else {
        Vec::new()
    };

    for reference in references.iter().filter(|reference| {
        matches!(
            reference.kind.as_str(),
            "extends" | "includes" | "template_imports"
        )
    }) {
        let target_path = reference.expression.trim();
        let html_target = profile
            .filter(|profile| profile.id() == "html")
            .and_then(|_| {
                languages::html::local_template_target(path, target_path, language_context)
            });
        let candidate = if let Some(target) = html_target.as_deref() {
            by_module
                .get(target)
                .and_then(|modules| modules.iter().find(|node| node.kind == "module").copied())
        } else if profile.is_some_and(|profile| profile.id() == "html") {
            None
        } else {
            module_nodes
                .iter()
                .find(|node| node.path == target_path)
                .copied()
                .or_else(|| {
                    let suffix = format!("/{target_path}");
                    module_nodes
                        .iter()
                        .find(|node| node.path.ends_with(&suffix))
                        .copied()
                })
        };
        if let Some(target) = candidate {
            graph.edges.push(Edge {
                src: reference.source.clone(),
                dst: target.id.clone(),
                kind: reference.kind.as_str().into(),
                path: path.to_owned(),
                line: reference.line,
                evidence: format!("{path}: {} {}", reference.kind, target.path),
                confidence: "exact".into(),
            });
            graph.reference_coverage(
                path,
                reference,
                "resolved",
                format!("template target: {}", target.path).into(),
            );
        } else {
            graph.reference_coverage(
                path,
                reference,
                "unresolved",
                format!("template '{}' not found", reference.expression).into(),
            );
        }
    }

    for load in &resolver.template_loads {
        if let Some(rule) = resolver.template_rules.iter().find(|rule| {
            matches!(
                &rule.operation,
                languages::manifests::syntax::SyntaxOperation::TemplateSymbols { libraries, .. }
                    if libraries.iter().any(|library| library == &load.expression)
            )
        }) {
            let label = rule
                .display
                .library
                .as_deref()
                .unwrap_or("builtin:configured_template");
            graph.reference_coverage(
                path,
                load,
                "external",
                format!(
                    "{label}; rule:{}:{} {}",
                    rule.pack, rule.id, load.expression
                )
                .into(),
            );
            continue;
        }
        match html_templates.library(path, &load.expression, dependency_registry) {
            html::templates::Match::Unique { module, origin, .. } => {
                let provenance = origin
                    .map(|origin| format!("; rule:{}:{}", origin.pack, origin.rule))
                    .unwrap_or_default();
                graph.edges.push(Edge {
                    src: load.source.clone(),
                    dst: module.id.clone(),
                    kind: "imports".into(),
                    path: path.to_owned(),
                    line: load.line,
                    evidence: format!(
                        "template library {} from {}{provenance}",
                        load.expression, module.path
                    ),
                    confidence: "exact".into(),
                });
                graph.reference_coverage(
                    path,
                    load,
                    "resolved",
                    format!("template library: {}{provenance}", module.path).into(),
                );
            }
            outcome => graph.reference_coverage(
                path,
                load,
                if matches!(outcome, html::templates::Match::Ambiguous) {
                    "ambiguous"
                } else {
                    "unresolved"
                },
                format!(
                    "template library {} has no unique registered provider",
                    load.expression
                )
                .into(),
            ),
        }
    }
}
