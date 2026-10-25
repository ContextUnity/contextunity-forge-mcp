use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

use super::projections::{Api, NamedBindingForm};
use super::semantic::Activation;

/// Bounded extraction operations; every API and keyword choice is input data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntaxRule {
    #[serde(skip)]
    pub pack: String,
    pub id: String,
    pub language: String,
    #[serde(default)]
    pub package_family: Option<String>,
    #[serde(default)]
    pub activation: Vec<Activation>,
    /// Compatibility labels only; admission and provenance use pack and rule identity.
    #[serde(default)]
    pub display: SyntaxDisplay,
    pub operation: SyntaxOperation,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntaxDisplay {
    pub builtin: Option<String>,
    pub autoimport: Option<String>,
    pub library: Option<String>,
    pub component: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SyntaxOperation {
    FileRoute {
        directory: String,
        extension: String,
        index: String,
        method: String,
    },
    FactoryMembers {
        callee: Api,
        argument: usize,
        #[serde(default)]
        shape: FactoryShape,
        #[serde(default)]
        configured_module: Option<String>,
        #[serde(default)]
        state_member: String,
        #[serde(default)]
        actions_member: String,
    },
    NamedBinding {
        domain: String,
        binding: NamedBindingForm,
    },
    RenderContext {
        package: String,
        module: String,
        export: String,
        target_argument: usize,
        context_argument: usize,
        argument_count: usize,
    },
    NamedRouteCall {
        module: String,
        export: String,
        argument: usize,
        expression_prefix: String,
    },
    ReceiverRegistration {
        module: String,
        constructors: Vec<String>,
        member: String,
        path_argument: usize,
        handler_argument: usize,
        name_keyword: String,
    },
    DecoratedRegistration {
        modules: Vec<String>,
        constructor: String,
        member: String,
        name_keyword: String,
        provider: String,
    },
    TemplateLoader {
        package: String,
        module: String,
        environment: String,
        loader: String,
        loader_keyword: String,
    },
    TemplateLibrary {
        directory: String,
        module: String,
        constructor: String,
        members: std::collections::BTreeMap<String, String>,
        name_keyword: String,
    },
    ObjectFactory {
        callees: Vec<String>,
        #[serde(default)]
        unshadowed_globals: Vec<String>,
    },
    ImplicitReceivers {
        names: Vec<String>,
    },
    ImplicitBuiltins {
        names: Vec<String>,
    },
    ReceiverType {
        receiver: ReceiverTypeRule,
    },
    ComponentEnvironment {
        package: String,
        config_file: String,
        config_callee: String,
        enabled_key: String,
        modules_key: String,
        directory: String,
        extension: String,
        virtual_module: String,
        autoimports: Vec<String>,
        builtins: Vec<String>,
        modules: std::collections::BTreeMap<String, ComponentModule>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        callable_imports: Option<CallableImports>,
    },
    ComponentOptions {
        module: String,
        export: String,
        components_key: String,
    },
    ComponentMacros {
        props: String,
        emits: String,
        slots: String,
        markers: Vec<String>,
    },
    RouteObject {
        collection: String,
        path_key: String,
        handler_key: String,
        method: String,
    },
    HttpClientCall {
        callee: String,
        receivers: Vec<String>,
        methods: std::collections::BTreeMap<String, String>,
        default_method: String,
        url_key: String,
        method_key: String,
        relative_prefixes: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_context: Option<HttpTargetContext>,
    },
    TemplateNamedRoute {
        tag: String,
        expression_prefix: String,
    },
    TemplateSymbols {
        requires_loader: bool,
        tags: Vec<String>,
        filters: Vec<String>,
        libraries: Vec<String>,
        library_routes: std::collections::BTreeMap<String, String>,
    },
    TemplateDialect {
        dialect: TemplateDialect,
    },
    HtmlRequestAttributes {
        request_node: String,
        context_node: String,
        namespace: String,
        context_label: String,
        prefixes: Vec<String>,
        methods: std::collections::BTreeMap<String, String>,
        contexts: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        values: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        value_dynamic_prefixes: Vec<String>,
    },
    HtmlScriptAttributes {
        event_prefixes: Vec<String>,
        object_attribute: String,
        event_scope: String,
        object_scope: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HttpTargetContext {
    pub request_owners: Vec<String>,
    pub route_owners: Vec<String>,
}

impl HttpTargetContext {
    fn validate(&self) -> Result<()> {
        for owners in [&self.request_owners, &self.route_owners] {
            let mut seen = std::collections::BTreeSet::new();
            ensure!(
                !owners.is_empty()
                    && owners.len() <= 128
                    && owners
                        .iter()
                        .all(|path| super::projections::confined_path(path, false)
                            && seen.insert(path)),
                "invalid HTTP target context owners"
            );
        }
        Ok(())
    }
}

fn normal_relative_dir(dir: &str) -> bool {
    !dir.starts_with('/')
        && dir
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

fn uppercase_methods<'a>(methods: impl IntoIterator<Item = &'a str>) -> bool {
    methods
        .into_iter()
        .all(|m| !m.is_empty() && m.chars().all(|ch| ch.is_ascii_uppercase()))
}

fn key_identifier(value: &str) -> bool {
    super::projections::identifier(value, false)
}

fn is_ident_like(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn validate_component_export(export: &ComponentExport) -> bool {
    super::semantic::normal_relative_path(&export.directory)
        && export.directory.len() <= 256
        && export.extension == ".ts"
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentModule {
    pub package: String,
    pub components: Vec<String>,
    #[serde(default)]
    pub exports: Vec<ComponentExport>,
    #[serde(default)]
    pub call_results: std::collections::BTreeMap<String, Vec<String>>,
}

impl ComponentModule {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.package.is_empty()
                && self
                    .components
                    .iter()
                    .all(|name| !name.is_empty() && !name.chars().any(char::is_whitespace)),
            "invalid component module"
        );
        ensure!(
            self.exports.len() <= 32 && self.call_results.len() <= 128,
            "component module result descriptors exceed limit"
        );
        ensure!(
            self.exports.iter().all(validate_component_export),
            "invalid component export path"
        );
        for (callee, members) in &self.call_results {
            let unique: std::collections::BTreeSet<_> = members.iter().collect();
            ensure!(
                key_identifier(callee)
                    && !members.is_empty()
                    && members.len() <= 128
                    && unique.len() == members.len()
                    && members.iter().all(|name| key_identifier(name)),
                "invalid component call result"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentExport {
    pub directory: String,
    pub extension: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallableImports {
    pub exports: Vec<ComponentExport>,
    pub aliases: Vec<String>,
    pub options_key: String,
    pub enabled_key: String,
    pub scan_key: String,
    pub unsupported_keys: Vec<String>,
}

impl CallableImports {
    fn validate(&self) -> Result<()> {
        let unique_exports: std::collections::BTreeSet<_> = self
            .exports
            .iter()
            .map(|export| (&export.directory, &export.extension))
            .collect();
        ensure!(
            !self.exports.is_empty()
                && self.exports.len() <= 16
                && unique_exports.len() == self.exports.len(),
            "invalid callable export selectors"
        );
        ensure!(
            self.exports.iter().all(validate_component_export),
            "invalid callable export path"
        );
        let aliases: std::collections::BTreeSet<_> = self.aliases.iter().collect();
        ensure!(
            self.aliases.len() <= 8
                && aliases.len() == self.aliases.len()
                && self.aliases.iter().all(|alias| !alias.is_empty()
                    && alias.len() <= 128
                    && alias.ends_with('/')
                    && !alias.contains('\\')
                    && !alias.chars().any(char::is_control)
                    && normal_relative_dir(alias.trim_end_matches('/'))),
            "invalid callable import aliases"
        );
        ensure!(
            key_identifier(&self.options_key)
                && key_identifier(&self.enabled_key)
                && key_identifier(&self.scan_key)
                && self.enabled_key != self.scan_key,
            "invalid callable import keys"
        );
        let unsupported: std::collections::BTreeSet<_> = self.unsupported_keys.iter().collect();
        ensure!(
            self.unsupported_keys.len() <= 16
                && unsupported.len() == self.unsupported_keys.len()
                && self.unsupported_keys.iter().all(|path| path.len() <= 128
                    && path.split('.').count() <= 4
                    && path.split('.').all(key_identifier)),
            "invalid unsupported callable import paths"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FactoryShape {
    #[default]
    Options,
    SetupReturn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiverTypeRule {
    pub name: String,
    pub module: Option<String>,
    pub export: Option<String>,
    pub members: Vec<String>,
    pub properties: std::collections::BTreeMap<String, String>,
    pub callback_callees: Vec<String>,
    pub generic: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argument_factories: Vec<ReceiverArgumentFactory>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiverArgumentFactory {
    pub callee: super::projections::Api,
    pub argument: usize,
    pub literals: Vec<String>,
}

impl ReceiverArgumentFactory {
    fn validate(&self, language: &str) -> Result<()> {
        ensure!(
            matches!(language, "javascript" | "typescript"),
            "argument factories require JavaScript or TypeScript"
        );
        let super::projections::Api::GlobalMember {
            receiver: global,
            member,
        } = &self.callee
        else {
            anyhow::bail!("argument factory requires global_member");
        };
        ensure!(
            self.argument < 4 && !self.literals.is_empty() && self.literals.len() <= 32,
            "invalid argument factory bounds"
        );
        let mut unique = std::collections::HashSet::new();
        ensure!(
            self.literals.iter().all(|literal| !literal.is_empty()
                && literal.len() <= 256
                && !literal.chars().any(char::is_control)
                && unique.insert(literal)),
            "invalid or duplicate argument factory literal"
        );
        ensure!(
            [global, member].iter().all(|name| !name.is_empty()
                && name.len() <= 4096
                && !name.chars().any(char::is_whitespace)
                && !name.chars().any(char::is_control)),
            "invalid argument factory API"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateDialect {
    pub directory: String,
    pub assignment: String,
    pub function: String,
    pub import: String,
    pub from: String,
    pub alias: String,
    pub load: String,
    pub include: String,
    pub extends: String,
    pub scope_open: Vec<String>,
    pub scope_close: Vec<String>,
}

impl SyntaxRule {
    pub(crate) fn key(&self) -> String {
        serde_json::to_string(&(&self.pack, &self.id)).expect("rule identity strings serialize")
    }
    pub(crate) fn activation_family(&self) -> Option<crate::engine::languages::LanguageFamily> {
        match self.package_family.as_deref() {
            Some(family) => crate::engine::languages::profiles()
                .map(|profile| profile.family())
                .find(|known| known.0 == family),
            None => crate::engine::languages::by_id(&self.language).map(|profile| profile.family()),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        for label in [
            &self.display.builtin,
            &self.display.autoimport,
            &self.display.library,
            &self.display.component,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                !label.is_empty() && label.len() <= 128 && !label.chars().any(char::is_control),
                "invalid syntax display label"
            );
        }
        ensure!(
            self.display.builtin.is_none()
                || matches!(
                    self.operation,
                    SyntaxOperation::ImplicitBuiltins { .. }
                        | SyntaxOperation::ImplicitReceivers { .. }
                        | SyntaxOperation::TemplateSymbols { .. }
                        | SyntaxOperation::ComponentMacros { .. }
                ),
            "builtin display requires symbol operation"
        );
        ensure!(
            self.display.autoimport.is_none() && self.display.component.is_none()
                || matches!(self.operation, SyntaxOperation::ComponentEnvironment { .. }),
            "component display requires component operation"
        );
        ensure!(
            self.display.library.is_none()
                || matches!(self.operation, SyntaxOperation::TemplateSymbols { .. }),
            "library display requires template symbols"
        );
        ensure!(
            self.package_family.as_deref().is_none_or(key_identifier),
            "invalid syntax package family"
        );
        ensure!(
            !self.id.trim().is_empty(),
            "syntax rule id must be nonempty"
        );
        ensure!(
            crate::engine::languages::profile_available(&self.language),
            "unsupported syntax language"
        );
        for Activation::Package { name } in &self.activation {
            ensure!(
                !name.trim().is_empty(),
                "syntax activation package must be nonempty"
            );
        }
        let strings: Vec<&str> = match &self.operation {
            SyntaxOperation::FileRoute {
                directory,
                extension,
                index,
                method,
            } => {
                ensure!(
                    self.language == "vue"
                        && directory
                            .strip_suffix('/')
                            .is_some_and(super::semantic::normal_relative_path)
                        && extension.starts_with('.')
                        && extension.len() > 1
                        && extension[1..].chars().all(|c| c.is_ascii_alphanumeric())
                        && key_identifier(index)
                        && method == "GET",
                    "invalid file route projection"
                );
                vec![]
            }
            SyntaxOperation::FactoryMembers {
                callee,
                argument,
                shape,
                configured_module,
                state_member,
                actions_member,
            } => {
                ensure!(
                    matches!(self.language.as_str(), "javascript" | "typescript")
                        && (matches!(callee, Api::Imported { .. })
                            || (matches!(callee, Api::Global { .. })
                                && configured_module.is_some()))
                        && *argument < 32
                        && match shape {
                            FactoryShape::Options =>
                                state_member != actions_member
                                    && super::projections::identifier(state_member, true)
                                    && super::projections::identifier(actions_member, true),
                            FactoryShape::SetupReturn =>
                                state_member.is_empty() && actions_member.is_empty(),
                        }
                        && configured_module
                            .as_ref()
                            .is_none_or(|name| !name.is_empty()
                                && name.len() <= 256
                                && !name.chars().any(char::is_whitespace)),
                    "invalid factory members projection"
                );
                callee.validate(&self.language)?;
                vec![]
            }
            SyntaxOperation::NamedBinding { domain, binding } => {
                ensure!(key_identifier(domain), "invalid named binding domain");
                binding.validate(&self.language)?;
                vec![]
            }
            SyntaxOperation::RenderContext {
                package,
                module,
                export,
                target_argument,
                context_argument,
                argument_count,
            } => {
                ensure!(
                    self.language == "python"
                        && *argument_count <= 32
                        && target_argument < argument_count
                        && context_argument < argument_count
                        && target_argument != context_argument,
                    "invalid render context operation"
                );
                vec![package, module, export]
            }
            SyntaxOperation::NamedRouteCall {
                module,
                export,
                argument,
                expression_prefix,
            } => {
                ensure!(
                    self.language == "python" && *argument < 32,
                    "invalid named route operation"
                );
                vec![module, export, expression_prefix]
            }
            SyntaxOperation::ReceiverRegistration {
                module,
                constructors,
                member,
                path_argument,
                handler_argument,
                name_keyword,
            } => {
                ensure!(
                    self.language == "python"
                        && !constructors.is_empty()
                        && *path_argument < 32
                        && *handler_argument < 32
                        && path_argument != handler_argument,
                    "invalid receiver registration operation"
                );
                std::iter::once(module.as_str())
                    .chain(constructors.iter().map(String::as_str))
                    .chain([member.as_str(), name_keyword.as_str()])
                    .collect()
            }
            SyntaxOperation::DecoratedRegistration {
                modules,
                constructor,
                member,
                name_keyword,
                provider,
            } => {
                ensure!(
                    self.language == "python" && !modules.is_empty(),
                    "invalid decorated registration operation"
                );
                modules
                    .iter()
                    .map(String::as_str)
                    .chain([
                        constructor.as_str(),
                        member.as_str(),
                        name_keyword.as_str(),
                        provider.as_str(),
                    ])
                    .collect()
            }
            SyntaxOperation::TemplateLoader {
                package,
                module,
                environment,
                loader,
                loader_keyword,
            } => {
                ensure!(
                    self.language == "python",
                    "invalid template loader language"
                );
                vec![package, module, environment, loader, loader_keyword]
            }
            SyntaxOperation::TemplateLibrary {
                directory,
                module,
                constructor,
                members,
                name_keyword,
            } => {
                ensure!(
                    self.language == "python"
                        && !members.is_empty()
                        && members
                            .values()
                            .all(|kind| matches!(kind.as_str(), "tag" | "filter")),
                    "invalid template library operation"
                );
                ensure!(
                    normal_relative_dir(directory),
                    "invalid template library directory"
                );
                std::iter::once(directory.as_str())
                    .chain([module.as_str(), constructor.as_str(), name_keyword.as_str()])
                    .chain(members.keys().map(String::as_str))
                    .collect()
            }
            SyntaxOperation::ReceiverType { receiver } => {
                ensure!(
                    receiver.argument_factories.len() <= 1,
                    "receiver permits one argument factory"
                );
                for factory in &receiver.argument_factories {
                    factory.validate(&self.language)?;
                }
                ensure!(
                    matches!(
                        self.language.as_str(),
                        "python" | "typescript" | "javascript" | "vue"
                    ) && receiver.module.is_some() == receiver.export.is_some(),
                    "invalid receiver type operation"
                );
                std::iter::once(receiver.name.as_str())
                    .chain(
                        receiver
                            .module
                            .iter()
                            .chain(receiver.export.iter())
                            .chain(receiver.members.iter())
                            .chain(receiver.properties.keys())
                            .chain(receiver.properties.values())
                            .chain(receiver.callback_callees.iter())
                            .map(String::as_str),
                    )
                    .collect()
            }
            SyntaxOperation::ImplicitBuiltins { names } => {
                ensure!(!names.is_empty(), "implicit builtin names must be nonempty");
                names.iter().map(String::as_str).collect()
            }
            SyntaxOperation::ImplicitReceivers { names } => {
                ensure!(
                    matches!(self.language.as_str(), "javascript" | "typescript")
                        && !names.is_empty()
                        && names.iter().all(|name| {
                            !name.is_empty()
                                && name.len() <= 256
                                && !name.chars().any(|ch| ch.is_whitespace() || ch.is_control())
                        })
                        && names
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == names.len(),
                    "invalid implicit receivers language"
                );
                names.iter().map(String::as_str).collect()
            }
            SyntaxOperation::ObjectFactory {
                callees,
                unshadowed_globals,
            } => {
                ensure!(
                    self.language == "javascript" && !callees.is_empty(),
                    "invalid object factory operation"
                );
                callees
                    .iter()
                    .chain(unshadowed_globals)
                    .map(String::as_str)
                    .collect()
            }
            SyntaxOperation::ComponentEnvironment {
                package,
                config_file,
                config_callee,
                enabled_key,
                modules_key,
                directory,
                extension,
                virtual_module,
                autoimports,
                builtins,
                modules,
                callable_imports,
            } => {
                ensure!(
                    matches!(self.language.as_str(), "typescript" | "javascript"),
                    "invalid component environment language"
                );
                ensure!(
                    !config_file.contains('/')
                        && normal_relative_dir(directory)
                        && extension.starts_with('.')
                        && !extension.contains('/'),
                    "invalid component environment paths"
                );
                for module in modules.values() {
                    module.validate()?;
                }
                if let Some(imports) = callable_imports {
                    imports.validate()?;
                }
                [
                    package,
                    config_file,
                    config_callee,
                    enabled_key,
                    modules_key,
                    directory,
                    extension,
                    virtual_module,
                ]
                .into_iter()
                .map(String::as_str)
                .chain(
                    autoimports
                        .iter()
                        .chain(builtins)
                        .chain(modules.keys())
                        .map(String::as_str),
                )
                .collect()
            }
            SyntaxOperation::RouteObject {
                collection,
                path_key,
                handler_key,
                method,
            } => {
                ensure!(
                    matches!(self.language.as_str(), "javascript" | "typescript"),
                    "invalid route object language"
                );
                vec![collection, path_key, handler_key, method]
            }
            SyntaxOperation::HttpClientCall {
                callee,
                receivers,
                methods,
                default_method,
                url_key,
                method_key,
                relative_prefixes,
                target_context,
            } => {
                if let Some(context) = target_context {
                    context.validate()?;
                }
                ensure!(
                    !callee.is_empty() || !receivers.is_empty(),
                    "HTTP client needs a callee or receivers"
                );
                ensure!(
                    uppercase_methods(
                        methods
                            .values()
                            .map(String::as_str)
                            .chain([default_method.as_str()])
                    ),
                    "invalid client method"
                );
                ensure!(
                    matches!(
                        self.language.as_str(),
                        "python" | "typescript" | "javascript"
                    ) && !relative_prefixes.is_empty(),
                    "invalid HTTP client language or URL prefixes"
                );
                receivers
                    .iter()
                    .chain(methods.keys())
                    .chain([url_key, method_key])
                    .chain(relative_prefixes.iter())
                    .chain((!callee.is_empty()).then_some(callee))
                    .map(String::as_str)
                    .collect()
            }
            SyntaxOperation::ComponentMacros {
                props,
                emits,
                slots,
                markers,
            } => {
                ensure!(self.language == "vue", "invalid component macros language");
                let names: Vec<_> = [props, emits, slots].into_iter().chain(markers).collect();
                ensure!(
                    names.iter().all(|name| !name.is_empty()
                        && name
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')))
                        && names
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == names.len(),
                    "invalid or ambiguous macro identities"
                );
                names.into_iter().map(String::as_str).collect()
            }
            SyntaxOperation::ComponentOptions {
                module,
                export,
                components_key,
            } => {
                ensure!(self.language == "vue", "invalid component options language");
                vec![module, export, components_key]
            }
            SyntaxOperation::TemplateNamedRoute {
                tag,
                expression_prefix,
            } => {
                ensure!(self.language == "html", "invalid template tag language");
                vec![tag, expression_prefix]
            }
            SyntaxOperation::TemplateDialect { dialect } => {
                ensure!(self.language == "html", "invalid template dialect language");
                ensure!(
                    normal_relative_dir(&dialect.directory),
                    "invalid template root directory"
                );
                [
                    &dialect.directory,
                    &dialect.assignment,
                    &dialect.function,
                    &dialect.import,
                    &dialect.from,
                    &dialect.alias,
                    &dialect.load,
                    &dialect.include,
                    &dialect.extends,
                ]
                .into_iter()
                .chain(dialect.scope_open.iter())
                .chain(dialect.scope_close.iter())
                .map(String::as_str)
                .collect()
            }
            SyntaxOperation::TemplateSymbols {
                tags,
                filters,
                libraries,
                library_routes,
                ..
            } => {
                ensure!(self.language == "html", "invalid template symbols language");
                let valid_names = tags
                    .iter()
                    .chain(filters)
                    .chain(libraries)
                    .all(|name| super::projections::identifier(name, false));
                ensure!(
                    valid_names
                        && tags.len() <= 512
                        && filters.len() <= 512
                        && libraries.len() <= 128
                        && library_routes.len() <= 128
                        && tags.iter().collect::<std::collections::BTreeSet<_>>().len()
                            == tags.len()
                        && filters
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == filters.len()
                        && libraries
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == libraries.len()
                        && library_routes.iter().all(|(tag, library)| {
                            super::projections::identifier(tag, false)
                                && super::projections::identifier(library, false)
                        }),
                    "invalid template symbol declarations"
                );
                tags.iter()
                    .chain(filters)
                    .chain(libraries)
                    .chain(library_routes.keys())
                    .chain(library_routes.values())
                    .map(String::as_str)
                    .collect()
            }
            SyntaxOperation::HtmlScriptAttributes {
                event_prefixes,
                object_attribute,
                event_scope,
                object_scope,
            } => {
                ensure!(
                    self.language == "html" && !event_prefixes.is_empty(),
                    "invalid HTML script attributes"
                );
                event_prefixes
                    .iter()
                    .map(String::as_str)
                    .chain([
                        object_attribute.as_str(),
                        event_scope.as_str(),
                        object_scope.as_str(),
                    ])
                    .collect()
            }
            SyntaxOperation::HtmlRequestAttributes {
                request_node,
                context_node,
                namespace,
                context_label,
                prefixes,
                methods,
                contexts,
                values,
                value_dynamic_prefixes,
            } => {
                ensure!(
                    self.language == "html" && !prefixes.is_empty() && !methods.is_empty(),
                    "invalid HTML request operation"
                );
                ensure!(
                    values.is_some() || value_dynamic_prefixes.is_empty(),
                    "HTML request value prefixes require a values attribute"
                );
                ensure!(
                    request_node != context_node
                        && [request_node, context_node, namespace]
                            .into_iter()
                            .all(|value| is_ident_like(value)),
                    "invalid HTML node role mapping"
                );
                ensure!(
                    !context_label.trim().is_empty()
                        && context_label.len() <= 128
                        && !context_label.contains(['\n', '\r']),
                    "invalid HTML context label"
                );
                ensure!(
                    uppercase_methods(methods.values().map(String::as_str)),
                    "invalid request method"
                );
                prefixes
                    .iter()
                    .chain(methods.keys())
                    .chain(contexts)
                    .chain(values.iter())
                    .chain(value_dynamic_prefixes)
                    .map(String::as_str)
                    .collect()
            }
        };
        ensure!(
            strings
                .into_iter()
                .all(|value| !value.is_empty() && !value.chars().any(char::is_whitespace)),
            "syntax identities must be nonempty identifier paths"
        );
        Ok(())
    }
}
