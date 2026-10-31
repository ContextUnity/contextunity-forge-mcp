use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

/// Finite API identities; spellings and receiver alternatives are manifest data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Api {
    Imported {
        module: String,
        export: String,
    },
    ConstructedMember {
        module: String,
        constructors: Vec<String>,
        member: String,
    },
    Global {
        name: String,
    },
    GlobalMember {
        receiver: String,
        member: String,
    },
}

/// AST slot specification for framework arguments and keywords.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Slot {
    /// Positional argument by index.
    Argument {
        /// Positional index.
        index: usize,
    },
    /// Keyword argument by name.
    Keyword {
        /// Keyword name.
        name: String,
    },
    /// Keyword member access.
    KeywordMember {
        /// Keyword name.
        name: String,
        /// Member name.
        member: String,
    },
}

/// Source of an endpoint attachment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachmentSource {
    /// Attached to a decorated callable.
    DecoratedCallable,
    /// Attached to an argument by index.
    Argument {
        /// Positional argument index.
        index: usize,
    },
}

/// Endpoint attachment configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointAttachment {
    /// Callee API.
    pub callee: Api,
    /// Keyword name.
    pub keyword: String,
}

/// Definition kind for named bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    /// Object value definition.
    Object,
    /// Callable function or method definition.
    Callable,
}

/// Named provider scope specification.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedProviderScope {
    /// Workspace identifier.
    pub workspace: String,
    /// Project identifier.
    pub project: String,
    /// Package identifier.
    pub package: String,
}

/// Imported receiver type specification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedReceiverType {
    /// Module name.
    pub module: String,
    /// Export name.
    pub export: String,
}

/// Template symbol kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSymbolKind {
    /// Filter function.
    Filter,
}

/// A canonical confined path, optionally in an explicitly named workspace.
pub(crate) fn confined_path(value: &str, allow_root: bool) -> bool {
    if value.is_empty() {
        return allow_root;
    }
    if value.len() > 512 || value.contains('\\') || value.chars().any(char::is_control) {
        return false;
    }
    let (workspace, path) = crate::engine::languages::workspace_path(value);
    if !workspace.is_empty()
        && (!workspace.starts_with('[')
            || !workspace.ends_with(']')
            || !identifier(&workspace[1..workspace.len() - 1], false))
    {
        return false;
    }
    (allow_root && path.is_empty())
        || (!path.is_empty()
            && path.split('/').all(|part| {
                !part.is_empty() && !matches!(part, "." | "..") && !part.contains(['[', ']', ':'])
            }))
}

/// Named binding projection form definitions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NamedBindingForm {
    CallDefinition {
        callee: Api,
        key: Slot,
        target: Slot,
        target_kind: DefinitionKind,
    },
    CallReference {
        callee: Api,
        key: Slot,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        provider_scopes: Vec<NamedProviderScope>,
    },
    AssignmentDefinition {
        receiver_type: ImportedReceiverType,
        member: String,
        template_root: String,
    },
    TemplateReference {
        symbol_kind: TemplateSymbolKind,
    },
    MemberReference {
        root: String,
    },
    AttributeDefinition {
        prefix: String,
        scope: String,
    },
}

pub(crate) fn identifier(value: &str, javascript: bool) -> bool {
    let mut chars = value.chars();
    !value.is_empty()
        && value.len() <= 128
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || (javascript && c == '$'))
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || (javascript && c == '$'))
}

pub(crate) fn method(value: &str) -> bool {
    matches!(
        value,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" | "CONNECT" | "TRACE"
    )
}

impl Api {
    pub(crate) fn validate(&self, language: &str) -> Result<()> {
        let js = matches!(language, "javascript" | "typescript");
        match self {
            Self::Imported { module, export } => {
                ensure!(
                    !module.is_empty()
                        && module.len() <= 128
                        && !module.chars().any(char::is_control),
                    "invalid API module"
                );
                ensure!(identifier(export, js), "invalid API export");
                if language == "python" {
                    ensure!(
                        module.split('.').all(|s| identifier(s, false)),
                        "invalid Python module"
                    );
                }
            }
            Self::ConstructedMember {
                module,
                constructors,
                member,
            } => {
                ensure!(
                    language == "python"
                        && module.len() <= 128
                        && module.split('.').all(|s| identifier(s, false)),
                    "invalid constructed API module"
                );
                ensure!(
                    !constructors.is_empty()
                        && constructors.len() <= 32
                        && constructors.iter().all(|s| identifier(s, false)),
                    "invalid constructors"
                );
                ensure!(identifier(member, false), "invalid API member");
            }
            Self::Global { name } => ensure!(js && identifier(name, true), "invalid global API"),
            Self::GlobalMember { receiver, member } => {
                ensure!(
                    js && (1..=2).contains(&receiver.split('.').count())
                        && receiver.split('.').all(|s| identifier(s, true))
                        && identifier(member, true),
                    "invalid global member API"
                );
            }
        }
        Ok(())
    }
}

impl Slot {
    pub(crate) fn validate(&self, language: &str) -> Result<()> {
        match self {
            Self::Argument { index } => ensure!(*index < 32, "argument index exceeds bound"),
            Self::Keyword { name } => ensure!(
                language == "python" && identifier(name, false),
                "invalid keyword slot"
            ),
            Self::KeywordMember { name, member } => ensure!(
                language == "python" && identifier(name, false) && identifier(member, false),
                "invalid keyword member slot"
            ),
        }
        Ok(())
    }
}

impl NamedBindingForm {
    pub(crate) fn validate(&self, language: &str) -> Result<()> {
        match self {
            Self::CallDefinition {
                callee,
                key,
                target,
                ..
            } => {
                ensure!(
                    matches!(language, "javascript" | "typescript"),
                    "invalid named call language"
                );
                callee.validate(language)?;
                key.validate(language)?;
                target.validate(language)?;
                ensure!(key != target, "definition key and target slots must differ");
            }
            Self::CallReference {
                callee,
                key,
                provider_scopes,
            } => {
                ensure!(
                    matches!(language, "python" | "javascript" | "typescript"),
                    "invalid named call language"
                );
                callee.validate(language)?;
                key.validate(language)?;
                ensure!(
                    provider_scopes.len() <= 16,
                    "too many named provider scopes"
                );
                let mut seen = std::collections::BTreeSet::new();
                for provider in provider_scopes {
                    ensure!(
                        (provider.workspace.is_empty() || identifier(&provider.workspace, false))
                            && confined_path(&provider.project, true)
                            && !provider.project.starts_with('[')
                            && !provider.package.is_empty()
                            && provider.package.len() <= 128
                            && !provider.package.chars().any(char::is_whitespace)
                            && !provider.package.chars().any(char::is_control)
                            && seen.insert((
                                &provider.workspace,
                                &provider.project,
                                &provider.package
                            )),
                        "invalid or duplicate named provider scope"
                    );
                }
            }
            Self::AssignmentDefinition {
                receiver_type,
                member,
                template_root,
            } => {
                ensure!(language == "python", "invalid registry assignment language");
                Api::Imported {
                    module: receiver_type.module.clone(),
                    export: receiver_type.export.clone(),
                }
                .validate(language)?;
                ensure!(
                    identifier(member, false)
                        && confined_path(template_root, false)
                        && !template_root.starts_with('['),
                    "invalid registry assignment policy"
                );
            }
            Self::TemplateReference { .. } => {
                ensure!(language == "html", "invalid template reference language")
            }
            Self::MemberReference { root } => ensure!(
                matches!(language, "javascript" | "typescript") && identifier(root, true),
                "invalid named member root"
            ),
            Self::AttributeDefinition { prefix, scope } => ensure!(
                language == "html"
                    && !prefix.is_empty()
                    && prefix.len() <= 128
                    && !prefix.chars().any(char::is_control)
                    && scope == "event_handler",
                "invalid named attribute definition"
            ),
        }
        Ok(())
    }
}
