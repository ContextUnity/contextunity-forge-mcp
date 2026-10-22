use super::projections::{Api, AttachmentSource, EndpointAttachment, Slot};
use anyhow::{ensure, Context, Result};
use serde::ser::SerializeStruct;
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticTarget {
    ProjectPathSuffix { prefix: String, language: String },
    ImportedSymbol { language: String },
    PythonSymbol { language: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticRelation {
    RendersTemplate,
    InjectsDependency,
    UsesModel,
    References,
}

impl SemanticRelation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RendersTemplate => "renders_template",
            Self::InjectsDependency => "injects_dependency",
            Self::UsesModel => "uses_model",
            Self::References => "references",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    // This carrier remains the Python extractor's existing call-rule shape.
    // The target projection selects the public operation kind (`call_to_file`
    // or `call_to_symbol`) without adding a parallel extraction path.
    CallToFile {
        language: String,
        callee: SemanticCallee,
        argument: usize,
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    ClassAttributeToFile {
        language: String,
        attribute: String,
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    ClassAttributeToSymbol {
        language: String,
        attribute: String,
        target: SemanticTarget,
        relation: SemanticRelation,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SemanticOperationWire {
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
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    CallToSymbol {
        language: String,
        callee: SemanticCallee,
        argument: usize,
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    ClassAttributeToFile {
        language: String,
        attribute: String,
        target: SemanticTarget,
        relation: SemanticRelation,
    },
    ClassAttributeToSymbol {
        language: String,
        attribute: String,
        target: SemanticTarget,
        relation: SemanticRelation,
    },
}

impl<'de> Deserialize<'de> for SemanticOperation {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match SemanticOperationWire::deserialize(deserializer)? {
            SemanticOperationWire::SymbolAttachment {
                language,
                callee,
                source,
                target,
                relation,
            } => Ok(Self::SymbolAttachment {
                language,
                callee,
                source,
                target,
                relation,
            }),
            SemanticOperationWire::EndpointProjection {
                language,
                callee,
                attachment,
                url_keyword,
                method_keyword,
                default_method,
            } => Ok(Self::EndpointProjection {
                language,
                callee,
                attachment,
                url_keyword,
                method_keyword,
                default_method,
            }),
            SemanticOperationWire::CallToFile {
                language,
                callee,
                argument,
                target,
                relation,
            } => {
                if !matches!(target, SemanticTarget::ProjectPathSuffix { .. }) {
                    return Err(serde::de::Error::custom(
                        "call_to_file requires a project_path_suffix target",
                    ));
                }
                Ok(Self::CallToFile {
                    language,
                    callee,
                    argument,
                    target,
                    relation,
                })
            }
            SemanticOperationWire::CallToSymbol {
                language,
                callee,
                argument,
                target,
                relation,
            } => {
                if !matches!(target, SemanticTarget::ImportedSymbol { .. }) {
                    return Err(serde::de::Error::custom(
                        "call_to_symbol requires an imported_symbol target",
                    ));
                }
                Ok(Self::CallToFile {
                    language,
                    callee,
                    argument,
                    target,
                    relation,
                })
            }
            SemanticOperationWire::ClassAttributeToFile {
                language,
                attribute,
                target,
                relation,
            } => {
                if !matches!(target, SemanticTarget::ProjectPathSuffix { .. }) {
                    return Err(serde::de::Error::custom(
                        "class_attribute_to_file requires a project_path_suffix target",
                    ));
                }
                Ok(Self::ClassAttributeToFile {
                    language,
                    attribute,
                    target,
                    relation,
                })
            }
            SemanticOperationWire::ClassAttributeToSymbol {
                language,
                attribute,
                target,
                relation,
            } => {
                if !matches!(target, SemanticTarget::PythonSymbol { .. }) {
                    return Err(serde::de::Error::custom(
                        "class_attribute_to_symbol requires a python_symbol target",
                    ));
                }
                Ok(Self::ClassAttributeToSymbol {
                    language,
                    attribute,
                    target,
                    relation,
                })
            }
        }
    }
}

impl Serialize for SemanticOperation {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let field_count = match self {
            Self::SymbolAttachment { .. } => 6,
            Self::EndpointProjection { .. } => 7,
            Self::CallToFile { .. } => 6,
            Self::ClassAttributeToFile { .. } | Self::ClassAttributeToSymbol { .. } => 5,
        };
        let mut operation = serializer.serialize_struct("SemanticOperation", field_count)?;
        match self {
            Self::SymbolAttachment {
                language,
                callee,
                source,
                target,
                relation,
            } => {
                operation.serialize_field("kind", "symbol_attachment")?;
                operation.serialize_field("language", language)?;
                operation.serialize_field("callee", callee)?;
                operation.serialize_field("source", source)?;
                operation.serialize_field("target", target)?;
                operation.serialize_field("relation", relation)?;
            }
            Self::EndpointProjection {
                language,
                callee,
                attachment,
                url_keyword,
                method_keyword,
                default_method,
            } => {
                operation.serialize_field("kind", "endpoint_projection")?;
                operation.serialize_field("language", language)?;
                operation.serialize_field("callee", callee)?;
                operation.serialize_field("attachment", attachment)?;
                operation.serialize_field("url_keyword", url_keyword)?;
                operation.serialize_field("method_keyword", method_keyword)?;
                operation.serialize_field("default_method", default_method)?;
            }
            Self::CallToFile {
                language,
                callee,
                argument,
                target,
                relation,
            } => {
                let kind = match target {
                    SemanticTarget::ProjectPathSuffix { .. } => "call_to_file",
                    SemanticTarget::ImportedSymbol { .. } => "call_to_symbol",
                    SemanticTarget::PythonSymbol { .. } => {
                        return Err(serde::ser::Error::custom(
                            "call operations cannot target python_symbol",
                        ));
                    }
                };
                operation.serialize_field("kind", kind)?;
                operation.serialize_field("language", language)?;
                operation.serialize_field("callee", callee)?;
                operation.serialize_field("argument", argument)?;
                operation.serialize_field("target", target)?;
                operation.serialize_field("relation", relation)?;
            }
            Self::ClassAttributeToFile {
                language,
                attribute,
                target,
                relation,
            } => {
                operation.serialize_field("kind", "class_attribute_to_file")?;
                operation.serialize_field("language", language)?;
                operation.serialize_field("attribute", attribute)?;
                operation.serialize_field("target", target)?;
                operation.serialize_field("relation", relation)?;
            }
            Self::ClassAttributeToSymbol {
                language,
                attribute,
                target,
                relation,
            } => {
                operation.serialize_field("kind", "class_attribute_to_symbol")?;
                operation.serialize_field("language", language)?;
                operation.serialize_field("attribute", attribute)?;
                operation.serialize_field("target", target)?;
                operation.serialize_field("relation", relation)?;
            }
        }
        operation.end()
    }
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

impl SemanticRule {
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
                        ensure!(
                            prefix.strip_suffix('/').is_some_and(normal_relative_path),
                            "target.prefix needs normal relative directory segments ending in /"
                        );
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
            } => {
                ensure!(
                    language == "python",
                    "unsupported class_attribute_to_file language {language}"
                );
                ensure!(identifier(attribute), "invalid class attribute");
                ensure!(
                    *relation == SemanticRelation::RendersTemplate,
                    "class_attribute_to_file only supports renders_template"
                );
                let SemanticTarget::ProjectPathSuffix { prefix, language } = target else {
                    anyhow::bail!("class_attribute_to_file requires project_path_suffix");
                };
                ensure!(
                    prefix.strip_suffix('/').is_some_and(normal_relative_path),
                    "target.prefix needs normal relative directory segments ending in /"
                );
                ensure!(
                    language == "html",
                    "class_attribute_to_file requires html target language"
                );
            }
            SemanticOperation::ClassAttributeToSymbol {
                language,
                attribute,
                target,
                relation,
            } => {
                ensure!(
                    language == "python",
                    "unsupported class_attribute_to_symbol language {language}"
                );
                ensure!(identifier(attribute), "invalid class attribute");
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::languages::manifests::semantic::{normal_relative_path, SemanticRule};

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
