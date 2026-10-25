//! Strict storage adapter for facts shared by existing language and linker owners.
use super::{
    manifests::{SemanticOperation, SemanticRule},
    Facts,
};

pub(crate) use super::manifests::semantic::{SemanticRelation, SemanticTarget};

#[derive(Clone, Debug)]
pub(crate) struct EffectiveSemanticRule {
    pub(crate) pack: String,
    pub(crate) rule: SemanticRule,
    pub(crate) scope: String,
    pub(crate) active: bool,
}

pub(crate) use crate::core::semantic::{
    SemanticFact, SemanticFactRole, SemanticIdentity, SemanticOccurrence,
};

impl EffectiveSemanticRule {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fact(
        &self,
        owner: &str,
        line: usize,
        column: usize,
        path: String,
        expression: String,
        target: &SemanticTarget,
        role: SemanticFactRole,
    ) -> SemanticFact {
        SemanticFact {
            pack: self.pack.clone(),
            rule: self.rule.id.clone(),
            identity: SemanticIdentity {
                scope: self.scope.clone(),
                namespace: self.rule.namespace.clone(),
                target: target.clone(),
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
    let prepared = context.file_provider_rules_for_language(&language);
    let indices = prepared.into_iter().flatten().copied().chain(
        0..if prepared.is_none() {
            context.semantic_rules.len()
        } else {
            0
        },
    );
    for index in indices {
        let effective = &context.semantic_rules[index];
        if !effective.active {
            continue;
        }
        let target = match &effective.rule.operation {
            SemanticOperation::CallToFile { target, .. }
            | SemanticOperation::ClassAttributeToFile { target, .. } => target,
            SemanticOperation::CallToSymbol { .. }
            | SemanticOperation::ClassAttributeToSymbol { .. }
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
                    target,
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
    if facts.semantic.push(&mut facts.nodes, fact) {
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
