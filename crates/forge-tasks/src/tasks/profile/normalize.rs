use super::*;

pub(super) fn gate_is_hierarchical(gate: &GateDef) -> bool {
    gate.command.is_some()
        || gate.use_template.is_some()
        || gate.tasks.is_some()
        || !gate.gates.is_empty()
        || gate.child_gates().any(gate_is_hierarchical)
}

pub(super) fn gate_has_named_id(gate: &GateDef) -> bool {
    !gate.id.is_empty() || gate.child_gates().any(gate_has_named_id)
}

pub(super) fn local_gate_reference<'a>(reference: &'a str, path: &str) -> &'a str {
    path.rsplit('.')
        .next()
        .and_then(|parent| reference.strip_prefix(&format!("{parent}.")))
        .unwrap_or(reference)
}

pub(super) fn normalize_gate_condition(condition: &str) -> Option<&'static str> {
    match condition {
        "code" => Some("code"),
        "doc" | "docs" => Some("doc"),
        "db_migration" | "db" => Some("db_migration"),
        "seam" => Some("seam"),
        "public_api" => Some("public_api"),
        _ => None,
    }
}

pub(super) fn validate_gate_condition_syntax(condition: &str) -> bool {
    let parts: Vec<&str> = condition
        .split([',', '+', '&'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return false;
    }
    parts
        .into_iter()
        .all(|part| normalize_gate_condition(part).is_some())
}

pub(super) fn merge_yaml(base: &mut serde_yaml::Value, override_value: serde_yaml::Value) {
    if let (serde_yaml::Value::Mapping(base), serde_yaml::Value::Mapping(overrides)) =
        (&mut *base, &override_value)
    {
        for (key, value) in overrides {
            merge_yaml(
                base.entry(key.clone()).or_insert(serde_yaml::Value::Null),
                value.clone(),
            );
        }
    } else {
        *base = override_value;
    }
}

pub(super) fn parse_profile_document(yaml_str: &str) -> Result<ParsedProfile> {
    let is_empty_document = yaml_str.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with('#') || line == "---"
    });
    let mut document: serde_yaml::Value = if is_empty_document {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str(yaml_str).map_err(|err| {
            anyhow::anyhow!(
                "ACDD_PROFILE_SCHEMA_INVALID: {err}\nSupported fields: version, inherit, gates, gate_templates, roles, commands, scope_args_by_pattern, policies, proofs, guidance, modules, contours, limits, git_commit_note, completed, contract_zero_exit_policies."
            )
        })?
    };
    let mapping = document
        .as_mapping_mut()
        .context("ACDD_PROFILE_SCHEMA_INVALID: profile document must be a YAML mapping")?;
    let inherit = mapping.remove(serde_yaml::Value::String("inherit".to_owned()));
    let inherit_defaults = match inherit {
        None => true,
        Some(serde_yaml::Value::String(value)) if value == "none" => false,
        Some(value) => {
            bail!(
                "ACDD_PROFILE_SCHEMA_INVALID: inherit must be 'none' when specified, got {value:?}"
            )
        }
    };
    let commands_declared = mapping.contains_key(serde_yaml::Value::String("commands".to_owned()));
    let mut profile = serde_yaml::from_value::<GateProfile>(document).map_err(|err| {
        anyhow::anyhow!(
            "ACDD_PROFILE_SCHEMA_INVALID: {err}\nSupported fields: version, inherit, gates, gate_templates, roles, commands, scope_args_by_pattern, policies, proofs, guidance, modules, contours, limits, git_commit_note, completed, contract_zero_exit_policies."
        )
    })?;
    let profile_persistence = profile.persistence;
    for gate in &mut profile.gates {
        normalize_gate(gate, profile_persistence, false);
    }
    for template in profile.gate_templates.values_mut() {
        for gate in &mut template.gates {
            normalize_gate(gate, profile_persistence, false);
        }
    }
    Ok(ParsedProfile {
        profile,
        inherit_defaults,
        commands_declared,
    })
}

fn normalize_gate(
    gate: &mut GateDef,
    profile_persistence: Option<PersistenceAction>,
    is_task_gate: bool,
) {
    normalize_gate_hierarchy(None, gate, profile_persistence, is_task_gate);
}

fn infer_gate_proof(gate: &mut GateDef, _is_task_gate: bool) {
    if gate.proof != ProofDef::Unspecified {
        return;
    }
    if gate.receipt {
        gate.proof = ProofDef::Kind(ProofKind::Delivery);
    } else if gate.expected_failure.is_some() {
        gate.proof = ProofDef::Kind(ProofKind::Contract);
    } else if gate.command.is_some() {
        gate.proof = ProofDef::Kind(ProofKind::Command);
    } else if gate.policy.is_some() || gate.contours.is_some() {
        gate.proof = ProofDef::Kind(ProofKind::Review);
    } else {
        gate.proof = ProofDef::Kind(ProofKind::None);
    }
}

pub(crate) fn combine_gate_conditions(parent: Option<&str>, child: Option<&str>) -> Option<String> {
    match (parent, child) {
        (Some(parent), Some(child)) if parent != child => Some(format!("{parent} & {child}")),
        (Some(condition), _) | (_, Some(condition)) => Some(condition.to_owned()),
        (None, None) => None,
    }
}

fn normalize_gate_hierarchy(
    parent_id: Option<&str>,
    gate: &mut GateDef,
    profile_persistence: Option<PersistenceAction>,
    is_task_gate: bool,
) {
    infer_gate_proof(gate, is_task_gate);
    if let Some(parent) = parent_id {
        if !parent.is_empty() && !gate.id.is_empty() && !gate.id.starts_with(&format!("{parent}."))
        {
            gate.id = format!("{parent}.{}", gate.id);
        }
    }
    if gate.persistence.post == Some(PersistenceAction::Snapshot) {
        gate.sha_snapshot = true;
    } else if gate.sha_snapshot && gate.persistence.post.is_none() {
        gate.persistence.post = Some(PersistenceAction::Snapshot);
    }
    if gate_is_delivery(gate) {
        if !gate.auto_commit {
            if gate.persistence.post == Some(PersistenceAction::Commit) {
                gate.persistence.post = None;
            }
        } else if gate.persistence.post.is_none() {
            if let Some(prof_p) = profile_persistence {
                gate.persistence.post = Some(prof_p);
            } else if gate.auto_commit {
                gate.persistence.post = Some(PersistenceAction::Commit);
            }
        }
        gate.auto_commit = gate.persistence.post == Some(PersistenceAction::Commit);
    } else {
        gate.auto_commit = false;
    }
    let my_id = if gate.id.is_empty() {
        parent_id.unwrap_or("")
    } else {
        &gate.id
    };
    for child in &mut gate.gates {
        normalize_gate_hierarchy(Some(my_id), child, profile_persistence, is_task_gate);
    }
    if let Some(tasks) = &mut gate.tasks {
        for child in &mut tasks.gates {
            child.when = combine_gate_conditions(tasks.when.as_deref(), child.when.as_deref());
            normalize_gate_hierarchy(Some(my_id), child, profile_persistence, true);
        }
    }
}

pub(super) fn merge_profile_document(base: &mut GateProfile, document: ParsedProfile) {
    if !document.inherit_defaults {
        *base = document.profile;
        return;
    }
    if document.commands_declared && document.profile.commands.is_empty() {
        base.commands.clear();
        if document.profile.gate_templates.is_empty() {
            base.gate_templates.clear();
        }
    }
    base.merge_override(document.profile);
}

impl GateProfile {
    /// Merge overrides on top of this base profile. Gates replace as a whole sequence.
    pub fn merge_override(&mut self, other: GateProfile) {
        if other.version.is_some() {
            self.version = other.version;
        }
        if !other.contract_zero_exit_policies.is_empty() {
            self.contract_zero_exit_policies = other.contract_zero_exit_policies;
        }
        self.proofs.extend(other.proofs);
        self.guidance.extend(other.guidance);
        self.gate_templates.extend(other.gate_templates);
        if other.limits != ResourceLimits::default() {
            self.limits = other.limits;
        }
        if other.persistence.is_some() {
            self.persistence = other.persistence;
        }
        if other.git_commit_note.is_some() {
            self.git_commit_note = other.git_commit_note;
        }
        if other.modules != SubsystemRegistry::default() {
            self.modules = other.modules;
        }
        for (key, role) in other.roles {
            if let Some(current) = self.roles.get_mut(&key) {
                current.merge_override(role);
            } else {
                self.roles.insert(key, role);
            }
        }
        self.commands.extend(other.commands);
        self.scope_args_by_pattern
            .extend(other.scope_args_by_pattern);
        for (key, value) in other.policies {
            let current = self.policies.entry(key).or_default();
            if value.description.is_some() {
                current.description = value.description;
            }
            for (setting, value) in value.settings {
                merge_yaml(current.settings.entry(setting).or_default(), value);
            }
        }
        if !other.gates.is_empty() {
            self.gates = other.gates;
        }
        if !other.completed.tools.is_empty() {
            self.completed.tools = other.completed.tools;
        }
        if !other.completed.steps.is_empty() {
            self.completed.steps = other.completed.steps;
        }
        if !other.polish_scope.is_empty() {
            self.polish_scope = other.polish_scope;
        }
    }
}
