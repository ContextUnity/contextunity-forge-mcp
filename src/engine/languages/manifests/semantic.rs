use anyhow::{ensure, Context, Result};
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
pub(crate) enum FileProjection {
    ProjectPathSuffix { prefix: String, language: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticRelation {
    RendersTemplate,
}

impl SemanticRelation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RendersTemplate => "renders_template",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticOperation {
    CallToFile {
        language: String,
        callee: SemanticCallee,
        argument: usize,
        target: FileProjection,
        relation: SemanticRelation,
    },
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

impl FileProjection {
    pub(crate) fn paths(&self, path: &str, language: &str) -> Vec<String> {
        let Self::ProjectPathSuffix {
            prefix,
            language: expected,
        } = self;
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
        let SemanticOperation::CallToFile {
            language,
            callee,
            target,
            ..
        } = &self.operation;
        ensure!(
            language == "python",
            "unsupported call_to_file language {language}"
        );
        let SemanticCallee::Imported { module, export } = callee;
        ensure!(module.split('.').all(identifier), "invalid callee.module");
        ensure!(identifier(export), "invalid callee.export");
        let FileProjection::ProjectPathSuffix { prefix, language } = target;
        ensure!(
            prefix.strip_suffix('/').is_some_and(normal_relative_path),
            "target.prefix needs normal relative directory segments ending in /"
        );
        ensure!(
            language == "html",
            "unsupported call_to_file target language {language}"
        );
        Ok(())
    }
}
