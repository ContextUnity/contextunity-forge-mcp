use super::projections::{Api, AttachmentSource, EndpointAttachment, Slot};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticRule {
    pub(crate) id: String,
    pub(crate) namespace: String,
    pub(crate) scope: SemanticScope,
    #[serde(default)]
    pub(crate) activation: Vec<Activation>,
    pub(crate) operation: SemanticOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticScope {
    Project,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Activation {
    Package { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticCallee {
    Imported { module: String, export: String },
}

pub(crate) use crate::core::semantic::{SemanticRelation, SemanticTarget};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticOperation {
    SymbolAttachment {
        language: String,
        callee: Api,
        source: AttachmentSource,
        target: Slot,
        relation: SemanticRelation,
    },
    EndpointProjection {
        language: String,
        callee: Api,
        attachment: EndpointAttachment,
        url_keyword: String,
        method_keyword: String,
        default_method: String,
    },
    CallToFile {
        language: String,
        callee: SemanticCallee,
        argument: usize,
        #[serde(
            deserialize_with = "file_call_target",
            serialize_with = "serialize_call_target"
        )]
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    CallToSymbol {
        language: String,
        callee: SemanticCallee,
        argument: usize,
        #[serde(
            deserialize_with = "symbol_call_target",
            serialize_with = "serialize_call_target"
        )]
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    ClassAttributeToFile {
        language: String,
        attribute: String,
        #[serde(deserialize_with = "file_attribute_target")]
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    ClassAttributeToSymbol {
        language: String,
        attribute: String,
        #[serde(deserialize_with = "symbol_attribute_target")]
        target: SemanticTarget,
        relation: SemanticRelation,
    },
}

macro_rules! target_decoder {
    ($name:ident, $variant:ident, $message:literal) => {
        fn $name<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> std::result::Result<SemanticTarget, D::Error> {
            let target = SemanticTarget::deserialize(deserializer)?;
            if !matches!(target, SemanticTarget::$variant { .. }) {
                return Err(serde::de::Error::custom($message));
            }
            Ok(target)
        }
    };
}

target_decoder!(
    file_call_target,
    ProjectPathSuffix,
    "call_to_file requires a project_path_suffix target"
);
target_decoder!(
    symbol_call_target,
    ImportedSymbol,
    "call_to_symbol requires an imported_symbol target"
);
target_decoder!(
    file_attribute_target,
    ProjectPathSuffix,
    "class_attribute_to_file requires a project_path_suffix target"
);
target_decoder!(
    symbol_attribute_target,
    PythonSymbol,
    "class_attribute_to_symbol requires a python_symbol target"
);

fn serialize_call_target<S: serde::Serializer>(
    target: &SemanticTarget,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    if matches!(target, SemanticTarget::PythonSymbol { .. }) {
        return Err(serde::ser::Error::custom(
            "call operations cannot target python_symbol",
        ));
    }
    target.serialize(serializer)
}

/// A literal path must already be normal; never repair an uncertain identity.
pub(crate) fn normal_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && !matches!(segment, "." | "..")
                && !segment
                    .chars()
                    .any(|ch| ch.is_control() || matches!(ch, '\\' | ':' | '?' | '#'))
        })
}

impl SemanticTarget {
    pub(crate) fn paths(&self, path: &str, language: &str) -> Vec<String> {
        let Self::ProjectPathSuffix {
            prefix,
            language: expected,
        } = self
        else {
            return Vec::new();
        };
        if language != expected || !prefix.strip_suffix('/').is_some_and(normal_relative_path) {
            return Vec::new();
        }
        std::iter::once(0)
            .chain(path.match_indices('/').map(|(offset, _)| offset + 1))
            .filter_map(|offset| path[offset..].strip_prefix(prefix))
            .filter(|literal| normal_relative_path(literal))
            .map(str::to_owned)
            .collect()
    }
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn validate_prefix(prefix: &str) -> Result<()> {
    ensure!(
        prefix.strip_suffix('/').is_some_and(normal_relative_path),
        "target.prefix needs normal relative directory segments ending in /"
    );
    Ok(())
}

fn validate_class_attribute(
    to_file: bool,
    language: &str,
    attribute: &str,
    target: &SemanticTarget,
    relation: &SemanticRelation,
) -> Result<()> {
    let kind = if to_file {
        "class_attribute_to_file"
    } else {
        "class_attribute_to_symbol"
    };
    ensure!(
        language == "python",
        "unsupported {kind} language {language}"
    );
    ensure!(identifier(attribute), "invalid class attribute");
    if to_file {
        ensure!(
            *relation == SemanticRelation::RendersTemplate,
            "class_attribute_to_file only supports renders_template"
        );
        let SemanticTarget::ProjectPathSuffix { prefix, language } = target else {
            anyhow::bail!("class_attribute_to_file requires project_path_suffix");
        };
        validate_prefix(prefix)?;
        ensure!(
            language == "html",
            "class_attribute_to_file requires html target language"
        );
    } else {
        ensure!(
            *relation == SemanticRelation::UsesModel,
            "class_attribute_to_symbol only supports uses_model"
        );
        let SemanticTarget::PythonSymbol { language } = target else {
            anyhow::bail!("class_attribute_to_symbol requires python_symbol");
        };
        ensure!(
            language == "python",
            "class_attribute_to_symbol requires python target language"
        );
    }
    Ok(())
}

impl SemanticRule {
    pub(crate) fn language(&self) -> &str {
        match &self.operation {
            SemanticOperation::SymbolAttachment { language, .. }
            | SemanticOperation::EndpointProjection { language, .. }
            | SemanticOperation::CallToFile { language, .. }
            | SemanticOperation::CallToSymbol { language, .. }
            | SemanticOperation::ClassAttributeToFile { language, .. }
            | SemanticOperation::ClassAttributeToSymbol { language, .. } => language,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        self.validate_fields()
            .with_context(|| format!("semantic rule {}", self.id))
    }

    fn validate_fields(&self) -> Result<()> {
        ensure!(!self.id.trim().is_empty(), "id must be nonempty");
        ensure!(
            !self.namespace.trim().is_empty(),
            "namespace must be nonempty"
        );
        for Activation::Package { name } in &self.activation {
            ensure!(!name.trim().is_empty(), "activation.name must be nonempty");
        }
        match &self.operation {
            SemanticOperation::SymbolAttachment {
                language,
                callee,
                source,
                target,
                relation,
            } => {
                ensure!(
                    language == "python"
                        && matches!(callee, Api::Imported { .. } | Api::ConstructedMember { .. }),
                    "invalid symbol attachment API"
                );
                callee.validate(language)?;
                target.validate(language)?;
                ensure!(
                    !matches!(target, Slot::KeywordMember { .. }),
                    "attachment target must be direct"
                );
                ensure!(
                    matches!(
                        relation,
                        SemanticRelation::UsesModel | SemanticRelation::References
                    ),
                    "invalid attachment relation"
                );
                if let AttachmentSource::Argument { index } = source {
                    ensure!(
                        *index < 32
                            && *relation == SemanticRelation::References
                            && !matches!(target, Slot::Argument { index: other } if other == index),
                        "invalid attachment source"
                    );
                }
            }
            SemanticOperation::EndpointProjection {
                language,
                callee,
                attachment,
                url_keyword,
                method_keyword,
                default_method,
            } => {
                ensure!(
                    language == "python"
                        && matches!(callee, Api::Imported { .. })
                        && matches!(attachment.callee, Api::Imported { .. }),
                    "endpoint constructors must be imported Python APIs"
                );
                callee.validate(language)?;
                attachment.callee.validate(language)?;
                ensure!(
                    identifier(&attachment.keyword)
                        && identifier(url_keyword)
                        && identifier(method_keyword)
                        && url_keyword != method_keyword
                        && super::projections::method(default_method),
                    "invalid endpoint slots or default method"
                );
            }
            SemanticOperation::CallToFile {
                language,
                callee,
                target,
                relation,
                ..
            }
            | SemanticOperation::CallToSymbol {
                language,
                callee,
                target,
                relation,
                ..
            } => {
                ensure!(
                    language == "python",
                    "unsupported call operation language {language}"
                );
                let SemanticCallee::Imported { module, export } = callee;
                ensure!(module.split('.').all(identifier), "invalid callee.module");
                ensure!(identifier(export), "invalid callee.export");
                match target {
                    SemanticTarget::ProjectPathSuffix { prefix, language } => {
                        ensure!(
                            *relation == SemanticRelation::RendersTemplate,
                            "call_to_file only supports renders_template"
                        );
                        validate_prefix(prefix)?;
                        ensure!(
                            language == "html",
                            "unsupported call_to_file target language {language}"
                        );
                    }
                    SemanticTarget::ImportedSymbol { language } => {
                        ensure!(
                            *relation == SemanticRelation::InjectsDependency,
                            "call_to_symbol only supports injects_dependency"
                        );
                        ensure!(
                            language == "python",
                            "unsupported call_to_symbol target language {language}"
                        );
                    }
                    SemanticTarget::PythonSymbol { .. } => {
                        anyhow::bail!("call operations cannot target python_symbol")
                    }
                }
            }
            SemanticOperation::ClassAttributeToFile {
                language,
                attribute,
                target,
                relation,
            }
            | SemanticOperation::ClassAttributeToSymbol {
                language,
                attribute,
                target,
                relation,
            } => {
                let to_file = matches!(
                    &self.operation,
                    SemanticOperation::ClassAttributeToFile { .. }
                );
                validate_class_attribute(to_file, language, attribute, target, relation)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::languages::manifests::semantic::{SemanticRule, normal_relative_path};

    #[test]
    fn typed_symbol_operation_is_decoded_and_validated() {
        assert!(normal_relative_path("app/providers.py"));
        assert!(!normal_relative_path("app/../providers.py"));

        let rule: SemanticRule = serde_json::from_value(serde_json::json!({
            "id": "fastapi-dependency",
            "namespace": "fastapi",
            "scope": "project",
            "activation": [{"kind": "package", "name": "fastapi"}],
            "operation": {
                "kind": "call_to_symbol",
                "language": "python",
                "callee": {
                    "kind": "imported",
                    "module": "fastapi",
                    "export": "Depends"
                },
                "argument": 0,
                "target": {
                    "kind": "imported_symbol",
                    "language": "python"
                },
                "relation": "injects_dependency"
            }
        }))
        .expect("typed symbol operations must deserialize");

        SemanticRule::validate(&rule).unwrap();
        let encoded = serde_json::to_value(&rule).unwrap();
        assert_eq!(encoded["operation"]["kind"], "call_to_symbol");

        let mut invalid = encoded;
        invalid["operation"]["target"]["language"] = serde_json::json!("ruby");
        let invalid: SemanticRule = serde_json::from_value(invalid).unwrap();
        assert!(SemanticRule::validate(&invalid).is_err());

        let wrong_target = serde_json::json!({
            "id": "wrong-target",
            "namespace": "fastapi",
            "scope": "project",
            "operation": {
                "kind": "call_to_symbol",
                "language": "python",
                "callee": {"kind": "imported", "module": "fastapi", "export": "Depends"},
                "argument": 0,
                "target": {"kind": "project_path_suffix", "prefix": "templates/", "language": "html"},
                "relation": "injects_dependency"
            }
        });
        assert!(serde_json::from_value::<SemanticRule>(wrong_target).is_err());
    }

    #[test]
    fn class_attribute_operations_round_trip_and_reject_wrong_targets() {
        for operation in [
            serde_json::json!({
                "kind": "class_attribute_to_file",
                "language": "python",
                "attribute": "template_name",
                "target": {"kind": "project_path_suffix", "prefix": "templates/", "language": "html"},
                "relation": "renders_template"
            }),
            serde_json::json!({
                "kind": "class_attribute_to_symbol",
                "language": "python",
                "attribute": "model",
                "target": {"kind": "python_symbol", "language": "python"},
                "relation": "uses_model"
            }),
        ] {
            let value = serde_json::json!({
                "id": "django-class-attribute",
                "namespace": "django",
                "scope": "project",
                "activation": [{"kind": "package", "name": "django"}],
                "operation": operation.clone()
            });
            let rule: SemanticRule = serde_json::from_value(value).unwrap();
            SemanticRule::validate(&rule).unwrap();
            let encoded = serde_json::to_value(&rule).unwrap();
            assert_eq!(encoded["operation"], operation);
        }

        let wrong_target = serde_json::json!({
            "id": "django-class-attribute",
            "namespace": "django",
            "scope": "project",
            "operation": {
                "kind": "class_attribute_to_symbol",
                "language": "python",
                "attribute": "model",
                "target": {"kind": "project_path_suffix", "prefix": "templates/", "language": "html"},
                "relation": "uses_model"
            }
        });
        assert!(serde_json::from_value::<SemanticRule>(wrong_target).is_err());

        let wrong_attribute = serde_json::json!({
            "id": "django-class-attribute",
            "namespace": "django",
            "scope": "project",
            "operation": {
                "kind": "class_attribute_to_file",
                "language": "python",
                "attribute": "template",
                "target": {"kind": "project_path_suffix", "prefix": "templates/", "language": "html"},
                "relation": "renders_template"
            }
        });
        let arbitrary_attribute: SemanticRule = serde_json::from_value(wrong_attribute).unwrap();
        SemanticRule::validate(&arbitrary_attribute).unwrap();
    }
}

use crate::core::semantic::{
    AttachmentRelation, EndpointRequestFact, ExtractionRuleOrigin, FactoryResultFact,
    NamedBindingFact, NamedBindingRole, NamedTargetKind, ObjectMember, SourcePosition,
    StaticString, SymbolAttachmentFact, SymbolOperand, TemplateContextFact, ValueFlowFacts,
};
use crate::engine::languages::{SyntaxContext, semantic::EffectiveSemanticRule};
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_semantic_rule<'rules, 'tree>(
    effective: &'rules EffectiveSemanticRule,
    ctx: &SyntaxContext<'_, 'tree>,
    language: &str,
    is_call: bool,
    decorated_calls: &[tree_sitter::Node<'tree>],
    api: impl Fn(tree_sitter::Node<'tree>, &'rules Api) -> bool,
    slot: impl Fn(tree_sitter::Node<'tree>, &Slot) -> Result<Option<tree_sitter::Node<'tree>>, ()>,
    keyword: impl Fn(tree_sitter::Node<'tree>, &str) -> Result<Option<tree_sitter::Node<'tree>>, ()>,
    literal: impl Fn(Option<tree_sitter::Node<'tree>>) -> StaticString,
    operand: impl Fn(Option<tree_sitter::Node<'tree>>) -> SymbolOperand,
    decorated_source: impl Fn() -> SymbolOperand,
    attached: impl Fn(tree_sitter::Node<'tree>, &'rules EndpointAttachment) -> bool,
    attachments: &mut Vec<SymbolAttachmentFact>,
    endpoints: &mut Vec<EndpointRequestFact>,
) {
    let position = |node: tree_sitter::Node<'_>| SourcePosition {
        line: node.start_position().row + ctx.offset + 1,
        column: node.start_position().column,
    };
    match &effective.rule.operation {
        SemanticOperation::SymbolAttachment {
            language: selected,
            callee,
            source,
            target,
            relation,
        } if selected == language => {
            let relation = match relation {
                SemanticRelation::UsesModel => AttachmentRelation::UsesModel,
                SemanticRelation::References => AttachmentRelation::References,
                _ => return,
            };
            let calls = match source {
                AttachmentSource::DecoratedCallable if !is_call => decorated_calls,
                AttachmentSource::Argument { .. } if is_call => std::slice::from_ref(&ctx.node),
                _ => return,
            };
            for &call in calls {
                if !api(call, callee) {
                    continue;
                }
                let target = match slot(call, target) {
                    Ok(None) => continue,
                    Ok(Some(node)) => operand(Some(node)),
                    Err(()) => SymbolOperand::Unknown,
                };
                let source = match source {
                    AttachmentSource::DecoratedCallable => decorated_source(),
                    AttachmentSource::Argument { index } => {
                        operand(slot(call, &Slot::Argument { index: *index }).ok().flatten())
                    }
                };
                attachments.push(SymbolAttachmentFact {
                    origin: ExtractionRuleOrigin::new(&effective.pack, &effective.rule.id),
                    position: position(call),
                    source,
                    target,
                    relation,
                });
            }
        }
        SemanticOperation::EndpointProjection {
            language: selected,
            callee,
            attachment,
            url_keyword,
            method_keyword,
            default_method,
        } if selected == language && is_call => {
            if !api(ctx.node, callee) || !attached(ctx.node, attachment) {
                return;
            }
            let url = literal(keyword(ctx.node, url_keyword).ok().flatten());
            let method =
                keyword(ctx.node, method_keyword).map(|node| node.map(|node| literal(Some(node))));
            let url = match url {
                StaticString::Literal { value }
                    if value.starts_with('/') && !value.starts_with("//") =>
                {
                    StaticString::Literal { value }
                }
                _ => StaticString::Unknown,
            };
            let method = match method {
                Ok(None) => StaticString::Literal {
                    value: default_method.to_owned(),
                },
                Ok(Some(StaticString::Literal { value })) => {
                    let value = value.to_ascii_uppercase();
                    if super::projections::method(&value) {
                        StaticString::Literal { value }
                    } else {
                        StaticString::Unknown
                    }
                }
                _ => StaticString::Unknown,
            };
            endpoints.push(EndpointRequestFact {
                origin: ExtractionRuleOrigin::new(&effective.pack, &effective.rule.id),
                position: position(ctx.node),
                url,
                method,
            });
        }
        _ => {}
    }
}

pub(crate) enum ProjectionPayload {
    Factory {
        binding: String,
        members: Vec<ObjectMember>,
    },
    Named {
        key: StaticString,
        target: Option<(SourcePosition, Option<String>)>,
    },
}

pub(crate) fn syntax_rule_api(rule: &super::syntax::SyntaxRule) -> Option<Result<&Api, &str>> {
    use super::{projections::NamedBindingForm, syntax::SyntaxOperation};
    match &rule.operation {
        SyntaxOperation::FactoryMembers { callee, .. }
        | SyntaxOperation::NamedBinding {
            binding:
                NamedBindingForm::CallDefinition { callee, .. }
                | NamedBindingForm::CallReference { callee, .. },
            ..
        } => Some(Ok(callee)),
        SyntaxOperation::NamedBinding {
            binding: NamedBindingForm::MemberReference { root },
            ..
        } => Some(Err(root)),
        _ => None,
    }
}

pub(crate) fn bind_receiver_type(
    rule: &super::syntax::SyntaxRule,
    position: SourcePosition,
    payload: ProjectionPayload,
    facts: &crate::core::models::Facts,
    flow: &mut ValueFlowFacts,
) {
    use super::syntax::SyntaxOperation;
    match (&rule.operation, payload) {
        (
            SyntaxOperation::FactoryMembers {
                configured_module, ..
            },
            ProjectionPayload::Factory { binding, members },
        ) => flow.push_factory_result(FactoryResultFact {
            origin: ExtractionRuleOrigin::new(&rule.pack, &rule.id),
            position,
            binding,
            configured_module: configured_module.clone(),
            members,
        }),
        (
            SyntaxOperation::NamedBinding { domain, binding },
            ProjectionPayload::Named { key, target },
        ) => flow.push_named_binding(emit_named_binding(
            rule, position, domain, binding, key, target, facts,
        )),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_named_binding(
    rule: &super::syntax::SyntaxRule,
    position: SourcePosition,
    domain: &str,
    binding: &super::projections::NamedBindingForm,
    key: StaticString,
    target: Option<(SourcePosition, Option<String>)>,
    facts: &crate::core::models::Facts,
) -> NamedBindingFact {
    let role =
        if let super::projections::NamedBindingForm::CallDefinition { target_kind, .. } = binding {
            let kind = match target_kind {
                super::projections::DefinitionKind::Object => NamedTargetKind::Object,
                super::projections::DefinitionKind::Callable => NamedTargetKind::Callable,
            };
            let operand = target.map_or(SymbolOperand::Unknown, |(position, name)| {
                if let Some(expression) = name {
                    return SymbolOperand::Name { expression };
                }
                let mut nodes = facts.nodes.iter().filter(|node| {
                    node.line == position.line
                        && node.details["column"].as_u64().unwrap_or(0) == position.column as u64
                        && match kind {
                            NamedTargetKind::Object => node.kind == "object",
                            NamedTargetKind::Callable => {
                                matches!(node.kind.as_str(), "function" | "method")
                            }
                            NamedTargetKind::ListenerScope => false,
                        }
                });
                let Some(node) = nodes.next() else {
                    return SymbolOperand::Unknown;
                };
                if nodes.next().is_some() {
                    SymbolOperand::Unknown
                } else {
                    SymbolOperand::Node {
                        id: node.id.clone(),
                    }
                }
            });
            NamedBindingRole::Definition {
                target: operand,
                target_kind: kind,
            }
        } else {
            NamedBindingRole::Reference
        };
    NamedBindingFact {
        origin: ExtractionRuleOrigin::new(&rule.pack, &rule.id),
        position,
        domain: domain.to_owned(),
        key,
        role,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_syntax_rule(
    rule: &super::syntax::SyntaxRule,
    is_call: bool,
    callee: &str,
    offset: usize,
    api_shape: impl Fn(&Api, &str) -> bool,
    outermost_member: impl FnOnce() -> bool,
    factory: impl FnOnce(usize, &str, &str) -> Option<(String, Vec<ObjectMember>, Vec<ObjectMember>)>,
    setup: impl FnOnce(usize) -> Option<(String, Vec<ObjectMember>)>,
    argument: impl Fn(&Slot) -> Option<(SourcePosition, Option<String>)>,
    literal: impl Fn(&Slot) -> Option<StaticString>,
) -> Option<ProjectionPayload> {
    use super::{projections::NamedBindingForm, syntax::SyntaxOperation};
    match &rule.operation {
        SyntaxOperation::FactoryMembers {
            callee: api,
            argument,
            state_member,
            actions_member,
            shape,
            ..
        } if is_call && api_shape(api, callee) => {
            let (binding, mut members) = if *shape == super::syntax::FactoryShape::SetupReturn {
                setup(*argument)?
            } else {
                let (binding, mut members, methods) =
                    factory(*argument, state_member, actions_member)?;
                if members.len() + methods.len() > 128
                    || methods
                        .iter()
                        .any(|method| members.iter().any(|field| field.name == method.name))
                {
                    return None;
                }
                members.extend(methods);
                (binding, members)
            };
            for member in &mut members {
                member.position.line += offset;
            }
            Some(ProjectionPayload::Factory { binding, members })
        }
        SyntaxOperation::NamedBinding {
            binding:
                NamedBindingForm::CallDefinition {
                    callee: api,
                    key,
                    target,
                    ..
                },
            ..
        } if is_call && api_shape(api, callee) => Some(ProjectionPayload::Named {
            key: literal(key)?,
            target: argument(target),
        }),
        SyntaxOperation::NamedBinding {
            binding:
                NamedBindingForm::CallReference {
                    callee: api, key, ..
                },
            ..
        } if is_call && api_shape(api, callee) => Some(ProjectionPayload::Named {
            key: literal(key)?,
            target: None,
        }),
        SyntaxOperation::NamedBinding {
            binding: NamedBindingForm::MemberReference { root },
            ..
        } if callee.split('.').count() >= 2
            && callee.split('.').next() == Some(root)
            && outermost_member() =>
        {
            Some(ProjectionPayload::Named {
                key: StaticString::Literal {
                    value: callee.split('.').nth(1).expect("member path").to_owned(),
                },
                target: None,
            })
        }
        _ => None,
    }
}

pub(crate) fn execute_reference_rule<'rules>(
    rule: &'rules super::syntax::SyntaxRule,
    position: SourcePosition,
    facts: &crate::core::models::Facts,
    api: impl Fn(&'rules Api) -> bool,
    key: impl Fn(&Slot) -> Result<Option<StaticString>, ()>,
) -> Option<NamedBindingFact> {
    let super::syntax::SyntaxOperation::NamedBinding {
        domain,
        binding:
            binding @ super::projections::NamedBindingForm::CallReference {
                callee,
                key: selected,
                ..
            },
    } = &rule.operation
    else {
        return None;
    };
    if !api(callee) {
        return None;
    }
    let key = match key(selected) {
        Ok(None) => return None,
        Ok(Some(value)) => value,
        Err(()) => StaticString::Unknown,
    };
    Some(emit_named_binding(
        rule, position, domain, binding, key, None, facts,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn project_context_key(
    rules: &[super::syntax::SyntaxRule],
    candidates: &[usize],
    argument_count: usize,
    alias: &str,
    position: SourcePosition,
    context_shape: impl Fn(usize) -> bool,
    literal: impl Fn(usize) -> Option<String>,
    imported: impl Fn(&str, &str) -> bool,
    context_keys: impl Fn(usize) -> Option<Vec<String>>,
) -> Option<TemplateContextFact> {
    use super::syntax::SyntaxOperation;
    let (rule, target_index, context_index) = candidates.iter().find_map(|&index| {
        let rule = &rules[index];
        let SyntaxOperation::RenderContext {
            module,
            export,
            target_argument,
            context_argument,
            argument_count: expected,
            ..
        } = &rule.operation
        else {
            return None;
        };
        (argument_count == *expected && imported(module, export)).then_some((
            rule,
            *target_argument,
            *context_argument,
        ))
    })?;
    let target = literal(target_index)?;
    if target.is_empty() || target.len() > 256 || !context_shape(context_index) {
        return None;
    }
    let keys = context_keys(context_index)?;
    if keys.is_empty() || keys.len() > 32 {
        return None;
    }
    let mut unique = std::collections::HashSet::new();
    if keys.iter().any(|key| {
        key.is_empty()
            || key.len() > 128
            || !key.chars().enumerate().all(|(index, ch)| {
                ch == '_'
                    || if index == 0 {
                        ch.is_alphabetic()
                    } else {
                        ch.is_alphanumeric()
                    }
            })
            || !unique.insert(key)
    }) {
        return None;
    }
    Some(TemplateContextFact {
        origin: Some(ExtractionRuleOrigin::new(&rule.pack, &rule.id)),
        import_alias: alias.to_owned(),
        target,
        target_path: None,
        keys,
        position,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_class_attribute_rule(
    effective: &EffectiveSemanticRule,
    language: &str,
    attribute: &str,
    owner: &str,
    position: SourcePosition,
    expression: &str,
    literal: impl FnOnce() -> Option<String>,
    symbol: impl FnOnce() -> Option<String>,
    facts: &mut crate::core::models::Facts,
) {
    use crate::core::semantic::SemanticFactRole;
    let (identity, role, target) = match &effective.rule.operation {
        SemanticOperation::ClassAttributeToFile {
            language: selected,
            attribute: expected,
            target,
            relation,
        } if selected == language && expected == attribute => (
            literal()
                .filter(|value| normal_relative_path(value))
                .unwrap_or_default(),
            SemanticFactRole::ClassAttributeToFile {
                attribute: attribute.to_owned(),
                relation: *relation,
            },
            target,
        ),
        SemanticOperation::ClassAttributeToSymbol {
            language: selected,
            attribute: expected,
            target,
            relation,
        } if selected == language && expected == attribute => (
            symbol().unwrap_or_default(),
            SemanticFactRole::ClassAttributeToSymbol {
                attribute: attribute.to_owned(),
                relation: *relation,
            },
            target,
        ),
        _ => return,
    };
    crate::engine::languages::semantic::push(
        facts,
        effective.fact(
            owner,
            position.line,
            position.column,
            identity,
            expression.to_owned(),
            target,
            role,
        ),
    );
}

pub(crate) fn execute_registry_rule(
    rule: &super::syntax::SyntaxRule,
    position: SourcePosition,
    member: Option<&str>,
    key: StaticString,
    receiver: impl FnOnce() -> SymbolOperand,
    target: impl FnOnce() -> SymbolOperand,
) -> Option<NamedBindingFact> {
    let super::syntax::SyntaxOperation::NamedBinding {
        domain,
        binding:
            super::projections::NamedBindingForm::AssignmentDefinition {
                member: expected, ..
            },
    } = &rule.operation
    else {
        return None;
    };
    if member != Some(expected) {
        return None;
    }
    Some(NamedBindingFact {
        origin: ExtractionRuleOrigin::new(&rule.pack, &rule.id),
        position,
        domain: domain.clone(),
        key,
        role: NamedBindingRole::RegistryDefinition {
            receiver: receiver(),
            target: target(),
        },
    })
}

pub(crate) fn execute_named_route_rule(
    rule: &super::syntax::SyntaxRule,
    owner: &str,
    position: SourcePosition,
    prove: impl FnOnce(&str, &str, usize) -> Option<String>,
    facts: &mut crate::core::models::Facts,
) {
    let super::syntax::SyntaxOperation::NamedRouteCall {
        module,
        export,
        argument,
        expression_prefix,
    } = &rule.operation
    else {
        return;
    };
    let Some(name) = prove(module, export, *argument).filter(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    }) else {
        return;
    };
    facts.references.push(crate::core::models::Reference {
        source: owner.to_owned(),
        dynamic: false,
        expression: format!("{expression_prefix}{name}"),
        kind: "references_route".into(),
        line: position.line,
        column: position.column,
        alias: None,
        module: None,
        receiver_hint: Some(crate::core::models::ReceiverHint::NamedRoute {
            origin: ExtractionRuleOrigin::new(&rule.pack, &rule.id),
            key: name,
        }),
    });
}

pub(crate) fn execute_configuration_rule(
    rule: &super::syntax::SyntaxRule,
    path: &str,
    enabled: impl FnOnce(&str, &str) -> Option<bool>,
    declared: impl Fn(&str, &str, &str) -> bool,
    callable: impl FnOnce(
        &str,
        &super::syntax::CallableImports,
    ) -> Option<crate::core::semantic::CallableImportConfiguration>,
) -> Option<crate::core::semantic::ComponentConfigurationFact> {
    let super::syntax::SyntaxOperation::ComponentEnvironment {
        config_file,
        config_callee,
        enabled_key,
        modules_key,
        modules,
        callable_imports,
        ..
    } = &rule.operation
    else {
        return None;
    };
    if path.rsplit('/').next() != Some(config_file.as_str()) {
        return None;
    }
    Some(crate::core::semantic::ComponentConfigurationFact {
        origin: ExtractionRuleOrigin::new(&rule.pack, &rule.id),
        enabled: enabled(config_callee, enabled_key)?,
        modules: modules
            .keys()
            .filter(|name| declared(config_callee, modules_key, name))
            .cloned()
            .collect(),
        callable_imports: callable_imports
            .as_ref()
            .and_then(|descriptor| callable(config_callee, descriptor)),
    })
}

pub(crate) fn object_factory_matches(rule: &super::syntax::SyntaxRule, callee: &str) -> bool {
    matches!(&rule.operation, super::syntax::SyntaxOperation::ObjectFactory { callees, .. }
        if callees.iter().any(|configured| callee == configured || (configured.starts_with('.') && callee.ends_with(configured))))
}

pub(crate) fn object_factory_guard(rule: &super::syntax::SyntaxRule, binding: &str) -> bool {
    matches!(&rule.operation, super::syntax::SyntaxOperation::ObjectFactory { unshadowed_globals, .. }
        if unshadowed_globals.iter().any(|name| name == binding))
}

pub(crate) fn execute_object_factory_rule(
    rule: &super::syntax::SyntaxRule,
    callee: &str,
) -> Option<crate::core::semantic::ObjectOwnershipFact> {
    object_factory_matches(rule, callee).then(|| {
        let binding = callee.split('.').next().unwrap_or("");
        crate::core::semantic::ObjectOwnershipFact {
            origin: ExtractionRuleOrigin::new(&rule.pack, &rule.id),
            guard: object_factory_guard(rule, binding).then(|| binding.to_owned()),
        }
    })
}

pub(crate) fn projection_object_matches(
    rule: &super::syntax::SyntaxRule,
    api: impl Fn(&Api) -> bool,
    target: impl FnOnce(&Slot) -> bool,
    members: impl FnOnce(usize, &str, &str) -> bool,
) -> bool {
    use super::{
        projections::{DefinitionKind, NamedBindingForm},
        syntax::SyntaxOperation,
    };
    match &rule.operation {
        SyntaxOperation::NamedBinding {
            binding:
                NamedBindingForm::CallDefinition {
                    callee,
                    target: slot,
                    target_kind: DefinitionKind::Object,
                    ..
                },
            ..
        } => api(callee) && target(slot),
        SyntaxOperation::FactoryMembers {
            callee,
            argument,
            state_member,
            actions_member,
            ..
        } => api(callee) && members(*argument, state_member, actions_member),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_call_rules<'source>(
    ctx: &crate::engine::languages::SyntaxContext<'source, '_>,
    facts: &mut crate::core::models::Facts,
    rules: &crate::engine::languages::LanguageContext,
    indices: &[usize],
    language: &str,
    alias: &str,
    candidate: impl Fn(&str) -> bool,
    argument_value: impl Fn(usize) -> Option<&'source str>,
    imported: impl Fn(&str, &str) -> bool,
) {
    let Some(module_owner) = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .map(|node| node.id.clone())
    else {
        return;
    };
    let position = ctx.node.start_position();
    let call_line = position.row + ctx.offset + 1;
    let mut visible_local_import_owners = None;
    for &index in indices {
        let effective = &rules.semantic_rules[index];
        let (SemanticOperation::CallToFile {
            language: expected,
            callee,
            argument,
            target,
            relation,
        }
        | SemanticOperation::CallToSymbol {
            language: expected,
            callee,
            argument,
            target,
            relation,
        }) = &effective.rule.operation
        else {
            continue;
        };
        let SemanticCallee::Imported { module, export } = callee;
        if expected != language || !candidate(module) {
            continue;
        }
        let Some(literal) =
            argument_value(*argument).filter(|literal| normal_relative_path(literal))
        else {
            continue;
        };
        let Some(import_owner) = facts
            .references
            .iter()
            .find(|reference| {
                reference.kind == "imports"
                    && reference.alias.as_deref().unwrap_or(&reference.expression) == alias
                    && reference.module.as_deref() == Some(module)
                    && reference.expression == *export
                    && import_scope_visible_from_call(
                        &reference.source,
                        &module_owner,
                        ctx.owner,
                        call_line,
                        &mut visible_local_import_owners,
                        facts,
                    )
            })
            .map(|reference| reference.source.clone())
        else {
            continue;
        };
        if !imported(module, export) {
            continue;
        }
        crate::engine::languages::semantic::push(
            facts,
            effective.fact(
                ctx.owner,
                position.row + ctx.offset + 1,
                position.column,
                literal.to_owned(),
                crate::engine::languages::text(ctx.node, ctx.source).to_owned(),
                target,
                crate::engine::languages::semantic::SemanticFactRole::CallToFile {
                    import_owner,
                    alias: alias.to_owned(),
                    module: module.clone(),
                    export: export.clone(),
                    relation: *relation,
                },
            ),
        );
    }
}

pub(crate) fn semantic_rule_apis(rule: &SemanticRule) -> [Option<&Api>; 2] {
    match &rule.operation {
        SemanticOperation::SymbolAttachment { callee, .. } => [Some(callee), None],
        SemanticOperation::EndpointProjection {
            callee, attachment, ..
        } => [Some(callee), Some(&attachment.callee)],
        _ => [None, None],
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedRules {
    pub(crate) render_rules: Vec<usize>,
    pub(crate) loader_rules: Vec<usize>,
    pub(crate) named_route_rules: Vec<usize>,
    pub(crate) named_route_members: hashbrown::HashMap<String, Vec<usize>>,
    pub(crate) decorated_registration_rules: Vec<usize>,
    pub(crate) class_attributes: hashbrown::HashMap<String, Vec<usize>>,
    pub(crate) function_semantic: Vec<usize>,
    pub(crate) call_semantic: Vec<usize>,
    pub(crate) call_to_file: Vec<usize>,
    pub(crate) named_references: Vec<usize>,
    pub(crate) registry_definitions: Vec<usize>,
    pub(crate) semantic_constructors: Vec<usize>,
    pub(crate) syntax_constructors: Vec<usize>,
}

pub(crate) fn prepare_projection_rules(
    rules: &crate::engine::languages::LanguageContext,
) -> std::collections::HashMap<String, PreparedRules> {
    use super::{projections::NamedBindingForm, syntax::SyntaxOperation};
    let mut compiled = std::collections::HashMap::<String, PreparedRules>::new();
    for (index, effective) in rules
        .semantic_rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.active)
    {
        match &effective.rule.operation {
            SemanticOperation::ClassAttributeToFile {
                language,
                attribute,
                ..
            }
            | SemanticOperation::ClassAttributeToSymbol {
                language,
                attribute,
                ..
            } => {
                compiled
                    .entry(language.clone())
                    .or_default()
                    .class_attributes
                    .entry(attribute.clone())
                    .or_default()
                    .push(index);
            }
            SemanticOperation::CallToFile { language, .. }
            | SemanticOperation::CallToSymbol { language, .. } => {
                compiled
                    .entry(language.clone())
                    .or_default()
                    .call_to_file
                    .push(index);
            }
            SemanticOperation::SymbolAttachment {
                language,
                callee,
                source,
                ..
            } => {
                let prepared = compiled.entry(language.clone()).or_default();
                match source {
                    AttachmentSource::DecoratedCallable => prepared.function_semantic.push(index),
                    AttachmentSource::Argument { .. } => prepared.call_semantic.push(index),
                }
                if matches!(callee, Api::ConstructedMember { .. }) {
                    prepared.semantic_constructors.push(index);
                }
            }
            SemanticOperation::EndpointProjection {
                language,
                callee,
                attachment,
                ..
            } => {
                let prepared = compiled.entry(language.clone()).or_default();
                prepared.call_semantic.push(index);
                if [callee, &attachment.callee]
                    .iter()
                    .any(|api| matches!(api, Api::ConstructedMember { .. }))
                {
                    prepared.semantic_constructors.push(index);
                }
            }
        }
    }
    for (index, rule) in rules.syntax_rules.iter().enumerate() {
        let prepared = compiled.entry(rule.language.clone()).or_default();
        match &rule.operation {
            SyntaxOperation::RenderContext { .. } => prepared.render_rules.push(index),
            SyntaxOperation::TemplateLoader { .. } => prepared.loader_rules.push(index),
            SyntaxOperation::NamedRouteCall { export, .. } => {
                prepared.named_route_rules.push(index);
                prepared
                    .named_route_members
                    .entry(export.clone())
                    .or_default()
                    .push(index);
            }
            SyntaxOperation::DecoratedRegistration { .. } => {
                prepared.decorated_registration_rules.push(index)
            }
            SyntaxOperation::NamedBinding {
                binding: NamedBindingForm::CallReference { callee, .. },
                ..
            } => {
                prepared.named_references.push(index);
                if matches!(callee, Api::ConstructedMember { .. }) {
                    prepared.syntax_constructors.push(index);
                }
            }
            SyntaxOperation::NamedBinding {
                binding: NamedBindingForm::AssignmentDefinition { .. },
                ..
            } => {
                prepared.registry_definitions.push(index);
            }
            _ => {}
        }
    }
    compiled
}

pub(crate) fn imported_rule_candidates(
    rules: &[super::syntax::SyntaxRule],
    indices: &[usize],
    imports: &std::collections::HashMap<(&str, &str), Vec<(&str, usize)>>,
) -> std::collections::HashMap<String, Vec<usize>> {
    use super::syntax::SyntaxOperation;
    let mut candidates = std::collections::HashMap::<String, Vec<usize>>::new();
    for (index, rule) in indexed_rules(rules, indices.iter().copied()) {
        let (module, export) = match &rule.operation {
            SyntaxOperation::RenderContext { module, export, .. }
            | SyntaxOperation::NamedRouteCall { module, export, .. } => (module, export),
            SyntaxOperation::TemplateLoader {
                module,
                environment,
                ..
            } => (module, environment),
            _ => continue,
        };
        for &(alias, _) in imports
            .get(&(module.as_str(), export.as_str()))
            .into_iter()
            .flatten()
        {
            candidates.entry(alias.to_owned()).or_default().push(index);
        }
    }
    candidates
}

pub(crate) fn indexed_rules<T, I>(rules: &[T], indices: I) -> impl Iterator<Item = (usize, &T)>
where
    I: IntoIterator<Item = usize>,
{
    indices.into_iter().map(move |index| (index, &rules[index]))
}

use crate::core::models::{Facts, Node, ReceiverHint};
use crate::core::semantic::ValueExpr;
use crate::core::typed_facts::FlowStore;

fn has_name(node: &Node, field: &str, name: &str) -> bool {
    match field {
        "bindings" => node.binding_names().any(|binding| binding == name),
        "rebindings" => node.rebinding_names().any(|binding| binding == name),
        _ => node.details[field]
            .as_array()
            .is_some_and(|names| names.iter().any(|value| value.as_str() == Some(name))),
    }
}

pub(crate) fn lexical_scopes<'a>(owner: &'a Node, facts: &'a Facts) -> Vec<&'a Node> {
    let mut scopes: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| {
            node.id == owner.id
                || node.kind == "module"
                || (owner
                    .qualname
                    .strip_prefix(&node.qualname)
                    .is_some_and(|suffix| suffix.starts_with('.'))
                    && node.has_binding_names())
        })
        .collect();
    scopes.sort_unstable_by_key(|node| std::cmp::Reverse(node.qualname.len()));
    scopes
}

fn import_scope_visible_from_call(
    source: &str,
    module_owner: &str,
    call_owner: &str,
    call_line: usize,
    visible_local_import_owners: &mut Option<Vec<String>>,
    facts: &Facts,
) -> bool {
    if source == module_owner || source == call_owner {
        return true;
    }
    if source.starts_with("module:") {
        return false;
    }
    visible_local_import_owners
        .get_or_insert_with(|| visible_local_import_owners_for_call(call_owner, call_line, facts))
        .iter()
        .any(|owner| owner.as_str() == source)
}

fn visible_local_import_owners_for_call(
    call_owner: &str,
    call_line: usize,
    facts: &Facts,
) -> Vec<String> {
    let Some(call_scope) = facts.nodes.iter().find(|node| node.id == call_owner) else {
        return Vec::new();
    };
    let call_qualname = call_scope.qualname.as_str();
    facts
        .nodes
        .iter()
        .filter(|scope| {
            scope.id != call_owner
                && matches!(scope.kind.as_str(), "function" | "method")
                && scope.line <= call_line
                && call_line <= scope.end_line
                && scope.has_binding_names()
                && call_qualname
                    .strip_prefix(&scope.qualname)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        })
        .map(|scope| scope.id.clone())
        .collect()
}

fn import_binds(reference: &crate::core::models::Reference, scope: &Node, name: &str) -> bool {
    reference.source == scope.id
        && reference.kind == "imports"
        && reference.alias.as_deref().unwrap_or(&reference.expression) == name
}

pub(crate) fn unshadowed(scopes: &[&Node], name: &str, facts: &Facts) -> bool {
    !scopes.iter().any(|scope| {
        has_name(scope, "bindings", name)
            || has_name(scope, "rebindings", name)
            || facts
                .references
                .iter()
                .any(|reference| import_binds(reference, scope, name))
    })
}

pub(crate) fn unique_initializer<'a>(
    flow: &'a ValueFlowFacts,
    name: &str,
) -> Option<(
    &'a crate::core::semantic::BindingFact,
    &'a crate::core::semantic::BindingFact,
)> {
    let writes: Vec<_> = flow
        .bindings
        .iter()
        .filter(|write| write.name == name)
        .collect();
    let [first, second] = writes.as_slice() else {
        return None;
    };
    if first.conditional || second.conditional {
        return None;
    }
    let (declaration, initializer) = match (&first.value, &second.value) {
        (ValueExpr::Unknown, value) if !matches!(value, ValueExpr::Unknown) => (*first, *second),
        (value, ValueExpr::Unknown) if !matches!(value, ValueExpr::Unknown) => (*second, *first),
        _ => return None,
    };
    (declaration.position < initializer.position).then_some((declaration, initializer))
}

pub(crate) fn api_proven(
    api: &Api,
    callee: &str,
    owner: &Node,
    position: SourcePosition,
    facts: &Facts,
    flows: &FlowStore,
    file_import_mask: u64,
) -> bool {
    let scopes = lexical_scopes(owner, facts);
    match api {
        Api::Global { name } => callee == name && unshadowed(&scopes, name, facts),
        Api::GlobalMember { receiver, member } => {
            let Some(actual) = callee
                .strip_suffix(member.as_str())
                .and_then(|head| head.strip_suffix('.'))
            else {
                return false;
            };
            let root = receiver.split('.').next().expect("validated receiver");
            if !unshadowed(&scopes, root, facts) {
                return false;
            }
            if actual == receiver {
                return true;
            }
            if actual.contains('.') {
                return false;
            }
            let Some(scope) = scopes
                .iter()
                .find(|scope| has_name(scope, "bindings", actual))
            else {
                return false;
            };
            let Some(flow) = flows.get(scope) else {
                return false;
            };
            let Some((_, binding)) = unique_initializer(flow, actual) else {
                return false;
            };
            if binding.position >= position {
                return false;
            }
            match &binding.value {
                ValueExpr::Alias { name } => name == receiver,
                ValueExpr::Field {
                    receiver: base,
                    member,
                } => {
                    receiver
                        .strip_prefix(base.as_str())
                        .and_then(|tail| tail.strip_prefix('.'))
                        == Some(member.as_str())
                }
                _ => false,
            }
        }
        Api::Imported { module, export } => {
            let required_mask = crate::engine::linker::interner::package_mask(module);
            if file_import_mask & required_mask != required_mask {
                return false;
            }
            let (alias, member) = callee
                .split_once('.')
                .map_or((callee, None), |(alias, member)| (alias, Some(member)));
            let Some(scope) = scopes.iter().find(|scope| {
                has_name(scope, "bindings", alias)
                    || facts
                        .references
                        .iter()
                        .any(|reference| import_binds(reference, scope, alias))
            }) else {
                return false;
            };
            if has_name(scope, "rebindings", alias) {
                return false;
            }
            let mut imports = facts
                .references
                .iter()
                .filter(|reference| import_binds(reference, scope, alias));
            let Some(import) = imports.next() else {
                return false;
            };
            imports.next().is_none()
                && import.module.as_deref() == Some(module)
                && !matches!(import.receiver_hint, Some(ReceiverHint::TypeOnlyImport))
                && match member {
                    None => import.expression == *export,
                    Some(member) => import.expression == "*" && member == export,
                }
        }
        Api::ConstructedMember { .. } => false,
    }
}
