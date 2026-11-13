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
        Some(serde_yaml::Value::String(value)) if value == "default" => true,
        Some(serde_yaml::Value::Bool(value)) => value,
        Some(value) => {
            bail!(
                "ACDD_PROFILE_SCHEMA_INVALID: inherit must be 'default' or 'none' when specified, got {value:?}"
            )
        }
    };
    let commands_declared = mapping.contains_key(serde_yaml::Value::String("commands".to_owned()));
    let fields_document = document.clone();
    let mut profile = serde_yaml::from_value::<GateProfile>(document).map_err(|err| {
        anyhow::anyhow!(
            "ACDD_PROFILE_SCHEMA_INVALID: {err}\nSupported fields: version, inherit, gates, gate_templates, roles, commands, scope_args_by_pattern, policies, proofs, guidance, modules, contours, limits, git_commit_note, completed, contract_zero_exit_policies."
        )
    })?;
    mark_profile_explicit_fields(&mut profile, &fields_document);
    validate_layer_gate_uniqueness(&profile.gates, None)?;
    for template in profile.gate_templates.values() {
        validate_layer_gate_uniqueness(&template.gates, None)?;
    }
    Ok(ParsedProfile {
        profile,
        inherit_defaults,
        commands_declared,
    })
}

fn validate_layer_gate_uniqueness(gates: &[GateDef], parent_id: Option<&str>) -> Result<()> {
    let mut seen = std::collections::BTreeSet::<String>::new();
    for gate in gates {
        if !gate.id.is_empty() {
            let local_id = parent_id
                .map(|parent| local_gate_id(&gate.id, parent))
                .unwrap_or(gate.id.as_str());
            if !seen.insert(local_id.to_owned()) {
                bail!("ACDD_PROFILE_INVALID: duplicate gate id '{}'", gate.id);
            }
        }
        let child_parent = if gate.id.is_empty() {
            parent_id
        } else {
            Some(gate.id.as_str())
        };
        validate_layer_gate_uniqueness(&gate.gates, child_parent)?;
        if let Some(tasks) = &gate.tasks {
            validate_layer_gate_uniqueness(&tasks.gates, child_parent)?;
        }
    }
    Ok(())
}

fn mark_profile_explicit_fields(profile: &mut GateProfile, document: &serde_yaml::Value) {
    let Some(mapping) = document.as_mapping() else {
        return;
    };
    if let Some(gates) = mapping
        .get(serde_yaml::Value::String("gates".to_owned()))
        .and_then(serde_yaml::Value::as_sequence)
    {
        mark_gate_explicit_fields(&mut profile.gates, gates);
    }
    if let Some(templates) = mapping
        .get(serde_yaml::Value::String("gate_templates".to_owned()))
        .and_then(serde_yaml::Value::as_mapping)
    {
        for (name, template) in &mut profile.gate_templates {
            let Some(template_value) = templates.get(serde_yaml::Value::String(name.clone()))
            else {
                continue;
            };
            let Some(template_gates) = template_value
                .get("gates")
                .and_then(serde_yaml::Value::as_sequence)
            else {
                continue;
            };
            mark_gate_explicit_fields(&mut template.gates, template_gates);
        }
    }
}

fn mark_gate_explicit_fields(gates: &mut [GateDef], source: &[serde_yaml::Value]) {
    for (gate, value) in gates.iter_mut().zip(source) {
        let Some(mapping) = value.as_mapping() else {
            continue;
        };
        gate.explicit_fields.insert("parsed".to_owned());
        for (field, aliases) in [
            ("role", &["role", "agent"][..]),
            ("command", &["command"][..]),
            ("gates", &["gates"][..]),
            ("tasks", &["tasks"][..]),
            ("use", &["use"][..]),
            ("when", &["when"][..]),
            ("proof", &["proof"][..]),
            ("tools", &["tools"][..]),
            ("steps", &["steps"][..]),
            ("guidance", &["guidance"][..]),
            ("policy", &["policy"][..]),
            ("symbols", &["symbols"][..]),
            ("reject_routing", &["reject_routing", "loop"][..]),
            ("expected_failure", &["expected_failure"][..]),
            ("max_attempts", &["max_attempts"][..]),
            ("receipt", &["receipt"][..]),
            (
                "absorb_working_tree",
                &["absorb_working_tree", "absorb"][..],
            ),
            ("independent_from", &["independent_from"][..]),
            ("receipt_review", &["receipt_review"][..]),
            ("reject_to", &["reject_to"][..]),
            ("on_fail", &["on_fail", "fail"][..]),
            ("on_success", &["on_success", "pass"][..]),
            ("review_sources", &["review_sources", "need"][..]),
            ("contours", &["contours"][..]),
            ("sha_snapshot", &["sha_snapshot"][..]),
            ("auto_commit", &["auto_commit"][..]),
        ] {
            if aliases
                .iter()
                .any(|alias| mapping.contains_key(serde_yaml::Value::String((*alias).to_owned())))
            {
                gate.explicit_fields.insert(field.to_owned());
            }
        }
        if gate.proof.is_unspecified() {
            gate.explicit_fields.remove("proof");
        }

        if let Some(expected_failure) = mapping
            .get(serde_yaml::Value::String("expected_failure".to_owned()))
            .and_then(serde_yaml::Value::as_mapping)
        {
            for member in ["exit_codes", "stderr_contains"] {
                if expected_failure.contains_key(serde_yaml::Value::String(member.to_owned())) {
                    gate.explicit_fields
                        .insert(format!("expected_failure.{member}"));
                }
            }
        }

        if let Some((persistence_key, persistence_value)) =
            ["persistence", "save"].iter().find_map(|key| {
                mapping
                    .get(serde_yaml::Value::String((*key).to_owned()))
                    .map(|value| (*key, value))
            })
        {
            gate.explicit_fields.insert("persistence".to_owned());
            if let Some(policy) = persistence_value.as_mapping() {
                for member in ["pre", "post", "immutable", "git_commit_note"] {
                    if policy.contains_key(serde_yaml::Value::String(member.to_owned())) {
                        gate.explicit_fields.insert(format!("persistence.{member}"));
                    }
                }
            } else if persistence_key == "save" || persistence_key == "persistence" {
                // Scalar aliases specify the post action in PersistencePolicy's deserializer.
                gate.explicit_fields.insert("persistence.post".to_owned());
            }
        }

        if let Some(children) = mapping
            .get(serde_yaml::Value::String("gates".to_owned()))
            .and_then(serde_yaml::Value::as_sequence)
        {
            mark_gate_explicit_fields(&mut gate.gates, children);
        }
        if let Some(task_block) = mapping.get(serde_yaml::Value::String("tasks".to_owned())) {
            if let Some(children) = task_block
                .get("gates")
                .and_then(serde_yaml::Value::as_sequence)
            {
                if let Some(tasks) = &mut gate.tasks {
                    mark_gate_explicit_fields(&mut tasks.gates, children);
                }
            }
        }
    }
}

fn normalize_gate(
    gate: &mut GateDef,
    profile_persistence: Option<PersistenceAction>,
    proofs: &BTreeMap<String, ProofCatalogDef>,
    is_task_gate: bool,
) {
    normalize_gate_hierarchy(None, gate, profile_persistence, proofs, is_task_gate);
}

fn infer_gate_proof(gate: &mut GateDef, _is_task_gate: bool) {
    let explicitly_configured = gate.explicit_fields.contains("proof")
        || (!gate.proof.is_unspecified() && !gate.explicit_fields.contains("proof_inferred"));
    if explicitly_configured {
        return;
    }
    gate.proof = if gate.receipt {
        ProofDef::Kind(ProofKind::Delivery)
    } else if gate.expected_failure.is_some() {
        ProofDef::Kind(ProofKind::Contract)
    } else if gate.command.is_some() {
        ProofDef::Kind(ProofKind::Command)
    } else if gate.policy.is_some() || gate.contours.is_some() {
        ProofDef::Kind(ProofKind::Review)
    } else {
        ProofDef::Kind(ProofKind::None)
    };
    gate.explicit_fields.insert("proof_inferred".to_owned());
}

pub(crate) fn combine_gate_conditions(parent: Option<&str>, child: Option<&str>) -> Option<String> {
    match (parent, child) {
        (Some(parent), Some(child)) => {
            let mut parts = Vec::new();
            for part in parent.split('&').chain(child.split('&')).map(str::trim) {
                if !part.is_empty() && !parts.contains(&part) {
                    parts.push(part);
                }
            }
            Some(parts.join(" & "))
        }
        (Some(condition), _) | (_, Some(condition)) => Some(condition.to_owned()),
        (None, None) => None,
    }
}

fn normalize_gate_hierarchy(
    parent_id: Option<&str>,
    gate: &mut GateDef,
    profile_persistence: Option<PersistenceAction>,
    proofs: &BTreeMap<String, ProofCatalogDef>,
    is_task_gate: bool,
) {
    infer_gate_proof(gate, is_task_gate);
    if let Some(name) = gate.proof.catalog_name().map(str::to_owned) {
        if let Some(schema) = proofs.get(&name).and_then(|proof| proof.schema.as_ref()) {
            gate.proof = ProofDef::ResolvedNamed {
                name,
                schema: Box::new(schema.clone()),
            };
        }
    }
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
        normalize_gate_hierarchy(
            Some(my_id),
            child,
            profile_persistence,
            proofs,
            is_task_gate,
        );
    }
    if let Some(tasks) = &mut gate.tasks {
        let task_condition = tasks.when.clone();
        for child in &mut tasks.gates {
            let declared_condition = declared_gate_condition(child);
            child.when =
                combine_gate_conditions(task_condition.as_deref(), declared_condition.as_deref());
            normalize_gate_hierarchy(Some(my_id), child, profile_persistence, proofs, true);
        }
    }
}

pub(super) fn resolve_proof_schema<'a>(
    proof: &'a ProofDef,
    proofs: &'a BTreeMap<String, ProofCatalogDef>,
) -> Option<&'a SchemaDef> {
    match proof {
        ProofDef::Scheme { scheme } => Some(scheme),
        ProofDef::Named(name) => proofs.get(name)?.schema.as_ref(),
        ProofDef::ResolvedNamed { schema, .. } => Some(schema),
        ProofDef::Kind(_) | ProofDef::Unspecified => None,
    }
}

pub(super) fn merge_profile_document(base: &mut GateProfile, document: ParsedProfile) {
    if !document.inherit_defaults {
        *base = document.profile;
        normalize_profile(base);
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

pub(super) fn normalize_profile(profile: &mut GateProfile) {
    let persistence = profile.persistence;
    let proofs = &profile.proofs;
    for gate in &mut profile.gates {
        normalize_gate(gate, persistence, proofs, false);
    }
    for template in profile.gate_templates.values_mut() {
        for gate in &mut template.gates {
            normalize_gate(gate, persistence, proofs, false);
        }
    }
}

impl GateProfile {
    /// Merge overrides on top of this base profile.
    pub fn merge_override(&mut self, other: GateProfile) {
        if other.version.is_some() {
            self.version = other.version;
        }
        if !other.contract_zero_exit_policies.is_empty() {
            self.contract_zero_exit_policies = other.contract_zero_exit_policies;
        }
        self.proofs.extend(other.proofs);
        self.guidance.extend(other.guidance);
        for (name, template) in other.gate_templates {
            if let Some(current) = self.gate_templates.get_mut(&name) {
                if template.command.is_some() {
                    current.command = template.command;
                }
                if !template.guidance.is_empty() {
                    current.guidance = template.guidance;
                }
                if template.max_attempts.is_some() {
                    current.max_attempts = template.max_attempts;
                }
                merge_gate_lists(&mut current.gates, template.gates, None);
            } else {
                self.gate_templates.insert(name, template);
            }
        }
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
        merge_gate_lists(&mut self.gates, other.gates, None);
        if !other.completed.tools.is_empty() {
            self.completed.tools = other.completed.tools;
        }
        if !other.completed.steps.is_empty() {
            self.completed.steps = other.completed.steps;
        }
        if !other.polish_scope.is_empty() {
            self.polish_scope = other.polish_scope;
        }
        normalize_profile(self);
    }
}

fn local_gate_id<'a>(id: &'a str, parent_id: &str) -> &'a str {
    id.strip_prefix(&format!("{parent_id}."))
        .unwrap_or_else(|| {
            parent_id
                .rsplit('.')
                .next()
                .and_then(|parent| id.strip_prefix(&format!("{parent}.")))
                .unwrap_or(id)
        })
}

const DECLARED_WHEN_CAPTURED: &str = "declared_when_captured";

fn declared_gate_condition(gate: &mut GateDef) -> Option<String> {
    if !gate.explicit_fields.contains(DECLARED_WHEN_CAPTURED) {
        gate.declared_when = gate.when.clone();
        gate.explicit_fields
            .insert(DECLARED_WHEN_CAPTURED.to_owned());
    }
    gate.declared_when.clone()
}

fn task_container_condition(gate: &GateDef) -> Option<&str> {
    if let Some(condition) = gate.tasks.as_ref().and_then(|tasks| tasks.when.as_deref()) {
        return Some(condition);
    }
    if gate.explicit_fields.contains(DECLARED_WHEN_CAPTURED) {
        gate.declared_when.as_deref()
    } else {
        gate.when.as_deref()
    }
}

fn matching_task_container(base: &[GateDef], override_gate: &GateDef) -> Option<usize> {
    let candidates = base
        .iter()
        .enumerate()
        .filter(|(_, gate)| gate.id.is_empty() && gate.tasks.is_some())
        .collect::<Vec<_>>();
    let override_condition = task_container_condition(override_gate);

    if let Some(condition) = override_condition {
        let matching_condition = candidates
            .iter()
            .find(|(_, gate)| task_container_condition(gate) == Some(condition))
            .map(|(index, _)| *index);
        matching_condition.or_else(|| {
            if candidates.len() == 1 {
                candidates.first().map(|(index, _)| *index)
            } else {
                None
            }
        })
    } else if candidates.len() == 1 {
        candidates.first().map(|(index, _)| *index)
    } else {
        let mut unconditioned = candidates
            .into_iter()
            .filter(|(_, gate)| task_container_condition(gate).is_none());
        let first = unconditioned.next().map(|(index, _)| index);
        if unconditioned.next().is_none() {
            first
        } else {
            None
        }
    }
}

fn merge_gate_lists(base: &mut Vec<GateDef>, overrides: Vec<GateDef>, parent_id: Option<&str>) {
    for override_gate in overrides {
        let matching_id = base.iter().position(|gate| {
            !gate.id.is_empty()
                && !override_gate.id.is_empty()
                && (gate.id == override_gate.id
                    || parent_id.is_some_and(|parent| {
                        local_gate_id(&gate.id, parent) == local_gate_id(&override_gate.id, parent)
                    }))
        });
        let matching = matching_id.or_else(|| {
            (override_gate.id.is_empty() && override_gate.tasks.is_some())
                .then(|| matching_task_container(base, &override_gate))
                .flatten()
        });
        if let Some(gate) = matching.and_then(|index| base.get_mut(index)) {
            gate.merge_override(override_gate);
        } else {
            base.push(override_gate);
        }
    }
}

impl GateDef {
    fn merge_override(&mut self, other: GateDef) {
        let explicit_fields = other.explicit_fields.clone();
        let base_proof_explicit = self.explicit_fields.contains("proof")
            || (!self.proof.is_unspecified() && !self.explicit_fields.contains("proof_inferred"));
        let override_proof_explicit = explicit_fields.contains("proof")
            || (!other.proof.is_unspecified() && !explicit_fields.contains("proof_inferred"));
        let inference_inputs_changed = (other.command.is_some() && self.command != other.command)
            || (other.expected_failure.is_some()
                && self.expected_failure != other.expected_failure)
            || (other.policy.is_some() && self.policy != other.policy)
            || (other.contours.is_some() && self.contours != other.contours)
            || ((explicit_fields.contains("receipt") || other.receipt)
                && self.receipt != other.receipt);

        if !other.id.is_empty() && (self.id.is_empty() || other.id.contains('.')) {
            self.id = other.id;
        }
        if other.command.is_some() {
            self.command = other.command;
        }
        if other.role.is_some() {
            self.role = other.role;
        }
        if !other.guidance.is_empty() || explicit_fields.contains("guidance") {
            self.guidance = other.guidance;
        }
        if !other.steps.is_empty() || explicit_fields.contains("steps") {
            self.steps = other.steps;
        }
        if !other.symbols.is_empty() || explicit_fields.contains("symbols") {
            self.symbols = other.symbols;
        }
        if other.policy.is_some() {
            self.policy = other.policy;
        }
        if other.contours.is_some() {
            self.contours = other.contours;
        }
        if override_proof_explicit {
            self.proof = other.proof;
            self.explicit_fields.insert("proof".to_owned());
            self.explicit_fields.remove("proof_inferred");
        }
        if let Some(override_failure) = other.expected_failure {
            let failure = self
                .expected_failure
                .get_or_insert_with(ExpectedFailure::default);
            if explicit_fields.contains("expected_failure.exit_codes")
                || !override_failure.exit_codes.is_empty()
            {
                failure.exit_codes = override_failure.exit_codes;
            }
            if explicit_fields.contains("expected_failure.stderr_contains")
                || override_failure.stderr_contains.is_some()
            {
                failure.stderr_contains = override_failure.stderr_contains;
            }
        }
        if other.on_success.is_some() {
            self.on_success = other.on_success;
        }
        if other.on_fail.is_some() {
            self.on_fail = other.on_fail;
        }
        let override_when_is_explicit = explicit_fields.contains("when")
            || (!explicit_fields.contains(DECLARED_WHEN_CAPTURED) && other.when.is_some());
        if override_when_is_explicit {
            self.declared_when = if other.explicit_fields.contains(DECLARED_WHEN_CAPTURED) {
                other.declared_when.clone()
            } else {
                other.when.clone()
            };
            self.when = other.when;
            self.explicit_fields
                .insert(DECLARED_WHEN_CAPTURED.to_owned());
        }
        if other.max_attempts.is_some() {
            self.max_attempts = other.max_attempts;
        }
        if other.independent_from.is_some() {
            self.independent_from = other.independent_from;
        }
        if other.reject_to.is_some() {
            self.reject_to = other.reject_to;
        }
        if other.receipt_review.is_some() {
            self.receipt_review = other.receipt_review;
        }
        if !other.review_sources.is_empty() || explicit_fields.contains("review_sources") {
            self.review_sources = other.review_sources;
        }
        if !other.tools.is_empty() || explicit_fields.contains("tools") {
            self.tools = other.tools;
        }
        merge_persistence_policy(&mut self.persistence, other.persistence, &explicit_fields);
        if explicit_fields.contains("sha_snapshot") || other.sha_snapshot {
            self.sha_snapshot = other.sha_snapshot;
        }
        if explicit_fields.contains("auto_commit")
            || (!explicit_fields.contains("parsed") && other.auto_commit)
        {
            self.auto_commit = other.auto_commit;
        }
        if explicit_fields.contains("absorb_working_tree") || other.absorb_working_tree {
            self.absorb_working_tree = other.absorb_working_tree;
        }
        if explicit_fields.contains("receipt") || other.receipt {
            self.receipt = other.receipt;
        }
        if other.use_template.is_some() {
            self.use_template = other.use_template;
        }
        if let Some(override_routing) = other.reject_routing {
            let routing = self
                .reject_routing
                .get_or_insert_with(RejectRouting::default);
            if override_routing.scope.is_some() {
                routing.scope = override_routing.scope;
            }
            if override_routing.symbols.is_some() {
                routing.symbols = override_routing.symbols;
            }
        }
        let child_parent = (!self.id.is_empty()).then_some(self.id.as_str());
        merge_gate_lists(&mut self.gates, other.gates, child_parent);
        match (self.tasks.as_mut(), other.tasks) {
            (Some(base_tasks), Some(override_tasks)) => {
                if override_tasks.when.is_some() {
                    base_tasks.when = override_tasks.when;
                }
                merge_gate_lists(&mut base_tasks.gates, override_tasks.gates, child_parent);
            }
            (None, Some(tasks)) => self.tasks = Some(tasks),
            _ => {}
        }
        if inference_inputs_changed && !base_proof_explicit && !override_proof_explicit {
            self.proof = ProofDef::Unspecified;
            self.explicit_fields.remove("proof_inferred");
        }
        self.explicit_fields.extend(
            explicit_fields
                .into_iter()
                .filter(|field| field != "proof_inferred"),
        );
    }
}

fn merge_persistence_policy(
    base: &mut PersistencePolicy,
    override_policy: PersistencePolicy,
    explicit_fields: &BTreeSet<String>,
) {
    let field_was_supplied =
        |field: &str| explicit_fields.contains(&format!("persistence.{field}"));

    if field_was_supplied("pre") || override_policy.pre.is_some() {
        base.pre = override_policy.pre;
    }
    if field_was_supplied("post") || override_policy.post.is_some() {
        base.post = override_policy.post;
    }
    if field_was_supplied("immutable") || override_policy.immutable {
        base.immutable = override_policy.immutable;
    }
    if field_was_supplied("git_commit_note") || override_policy.git_commit_note.is_some() {
        base.git_commit_note = override_policy.git_commit_note;
    }
}
