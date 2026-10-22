//! Strict storage adapter for facts shared by existing language and linker owners.
use super::{
    manifests::{SemanticOperation, SemanticRule},
    Facts,
};
use serde::{Deserialize, Serialize};

pub(crate) use super::manifests::semantic::{SemanticRelation, SemanticTarget};

#[derive(Clone, Debug)]
pub(crate) struct EffectiveSemanticRule {
    pub(crate) pack: String,
    pub(crate) rule: SemanticRule,
    pub(crate) scope: String,
    pub(crate) active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticIdentity {
    pub(crate) scope: String,
    pub(crate) namespace: String,
    pub(crate) target: SemanticTarget,
    pub(crate) path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticOccurrence {
    pub(crate) owner_id: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SemanticFactRole {
    FileProvider {
        target: String,
    },
    CallToFile {
        import_owner: String,
        alias: String,
        module: String,
        export: String,
        relation: SemanticRelation,
    },
    ClassAttributeToFile {
        attribute: String,
        relation: SemanticRelation,
    },
    ClassAttributeToSymbol {
        attribute: String,
        relation: SemanticRelation,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticFact {
    pub(crate) pack: String,
    pub(crate) rule: String,
    pub(crate) identity: SemanticIdentity,
    pub(crate) occurrence: SemanticOccurrence,
    pub(crate) expression: String,
    pub(crate) role: SemanticFactRole,
}

impl EffectiveSemanticRule {
    pub(crate) fn fact(
        &self,
        owner: &str,
        line: usize,
        column: usize,
        path: String,
        expression: String,
        role: SemanticFactRole,
    ) -> SemanticFact {
        let target = match &self.rule.operation {
            SemanticOperation::CallToFile { target, .. }
            | SemanticOperation::ClassAttributeToFile { target, .. }
            | SemanticOperation::ClassAttributeToSymbol { target, .. } => target.clone(),
            SemanticOperation::SymbolAttachment { .. }
            | SemanticOperation::EndpointProjection { .. } => SemanticTarget::ImportedSymbol {
                language: "python".into(),
            },
        };
        SemanticFact {
            pack: self.pack.clone(),
            rule: self.rule.id.clone(),
            identity: SemanticIdentity {
                scope: self.scope.clone(),
                namespace: self.rule.namespace.clone(),
                target,
                path,
            },
            occurrence: SemanticOccurrence {
                owner_id: owner.to_owned(),
                line,
                column,
            },
            expression,
            role,
        }
    }
}

/// Project effective rules onto actual module identities, including empty files.
pub(crate) fn file_providers(facts: &mut Facts, context: &super::LanguageContext) {
    let Some(module) = facts.nodes.iter().find(|node| node.kind == "module") else {
        return;
    };
    let (id, path, language) = (
        module.id.clone(),
        module.path.clone(),
        module.language.clone(),
    );
    for effective in context.semantic_rules.iter().filter(|rule| rule.active) {
        let target = match &effective.rule.operation {
            SemanticOperation::CallToFile { target, .. }
            | SemanticOperation::ClassAttributeToFile { target, .. } => target,
            SemanticOperation::ClassAttributeToSymbol { .. }
            | SemanticOperation::SymbolAttachment { .. }
            | SemanticOperation::EndpointProjection { .. } => continue,
        };
        for literal in target.paths(&path, &language) {
            push(
                facts,
                effective.fact(
                    &id,
                    1,
                    0,
                    literal,
                    path.clone(),
                    SemanticFactRole::FileProvider { target: id.clone() },
                ),
            );
        }
    }
}

pub(crate) fn push(facts: &mut Facts, fact: SemanticFact) {
    if matches!(&fact.identity.target, SemanticTarget::ImportedSymbol { .. }) {
        return;
    }
    let key = dependency_key(&fact.identity);
    let occurrence = fact.occurrence.clone();
    if let Some(owner) = facts
        .nodes
        .iter_mut()
        .find(|node| node.id == occurrence.owner_id)
    {
        let slot = &mut owner.details["semantic_facts"];
        if slot.is_null() {
            *slot = serde_json::json!([]);
        }
        if let Some(values) = slot.as_array_mut() {
            values.push(serde_json::to_value(fact).expect("semantic DTO serializes"));
            facts.references.push(super::Reference {
                source: occurrence.owner_id,
                expression: key,
                kind: "semantic_key".into(),
                line: occurrence.line,
                column: occurrence.column,
                alias: None,
                module: None,
                dynamic: false,
                receiver_hint: None,
            });
        }
    }
}

pub(crate) fn decode(node: &super::Node) -> anyhow::Result<Vec<SemanticFact>> {
    match node.details.get("semantic_facts") {
        None => Ok(Vec::new()),
        Some(value) => Ok(serde_json::from_value(value.clone())?),
    }
}

pub(crate) fn dependency_key(identity: &SemanticIdentity) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "semantic:{:x}",
        Sha256::digest(serde_json::to_vec(identity).expect("identity serializes"))
    )
}
