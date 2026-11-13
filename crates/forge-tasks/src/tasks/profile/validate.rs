use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl GateProfile {
    /// Resolve profile-supplied scoped command arguments for one project-relative path.
    ///
    /// The `{file_stem}` token expands to the path's filename without its extension.
    pub fn scope_args_for_path(&self, path: &str) -> Option<String> {
        let scope_args = self
            .scope_args_by_pattern
            .iter()
            .filter(|(pattern, _)| glob_pattern_matches(pattern, path))
            .max_by_key(|(pattern, _)| {
                let literal_bytes = pattern
                    .bytes()
                    .filter(|byte| !matches!(*byte, b'*' | b'?' | b'{' | b'}' | b','))
                    .count();
                (literal_bytes, pattern.len())
            })
            .map(|(_, scope_args)| scope_args.as_str())?;
        if !scope_args.contains("{file_stem}") {
            return Some(scope_args.to_owned());
        }
        let file_stem = Path::new(path).file_stem()?.to_str()?;
        if forge_core::fs::contains_shell_metacharacters(file_stem) || file_stem.contains(' ') {
            return None;
        }
        Some(scope_args.replace("{file_stem}", file_stem))
    }

    /// Resolve scoped command arguments for one project-relative path within a specific command.
    ///
    /// The most specific matching pattern wins; command-local patterns win ties.
    pub fn scope_args_for_command_path(&self, command: &CommandDef, path: &str) -> Option<String> {
        let command_patterns = match command {
            CommandDef::Detailed {
                scope_args_by_pattern,
                ..
            } => Some(scope_args_by_pattern),
            _ => None,
        };
        let scope_args = self
            .scope_args_by_pattern
            .iter()
            .map(|entry| (entry, false))
            .chain(
                command_patterns
                    .into_iter()
                    .flat_map(|patterns| patterns.iter())
                    .map(|entry| (entry, true)),
            )
            .filter(|((pattern, _), _)| glob_pattern_matches(pattern, path))
            .max_by_key(|((pattern, _), local)| {
                let literal_bytes = pattern
                    .bytes()
                    .filter(|byte| !matches!(*byte, b'*' | b'?' | b'{' | b'}' | b','))
                    .count();
                (literal_bytes, pattern.len(), *local)
            })
            .map(|((_, scope_args), _)| scope_args.as_str())?;
        if !scope_args.contains("{file_stem}") {
            return Some(scope_args.to_owned());
        }
        let file_stem = Path::new(path).file_stem()?.to_str()?;
        if forge_core::fs::contains_shell_metacharacters(file_stem) || file_stem.contains(' ') {
            return None;
        }
        Some(scope_args.replace("{file_stem}", file_stem))
    }

    /// Return all executable leaf gates across root gates and nested containers.
    pub fn executable_gates(&self) -> Vec<&GateDef> {
        self.executable_gate_paths()
            .into_iter()
            .map(|(_, gate)| gate)
            .collect()
    }

    /// Return executable leaves with their qualified paths, including template leaves.
    pub fn executable_gate_paths(&self) -> Vec<(String, &GateDef)> {
        fn collect<'a>(
            profile: &'a GateProfile,
            gates: &'a [GateDef],
            parent: &str,
            out: &mut Vec<(String, &'a GateDef)>,
            stack: &mut Vec<String>,
        ) {
            for gate in gates {
                let path = if gate.id.is_empty() {
                    parent.to_owned()
                } else if parent.is_empty() || gate.id.contains('.') {
                    gate.id.clone()
                } else {
                    format!("{parent}.{}", gate.id)
                };
                if !gate.gates.is_empty() || gate.tasks.is_some() || gate.use_template.is_some() {
                    collect(profile, &gate.gates, &path, out, stack);
                    if let Some(tasks) = &gate.tasks {
                        collect(profile, &tasks.gates, &path, out, stack);
                    }
                    if let Some(name) = &gate.use_template {
                        if !stack.contains(name) {
                            if let Some(template) = profile.gate_templates.get(name) {
                                stack.push(name.clone());
                                collect(profile, &template.gates, &path, out, stack);
                                stack.pop();
                            }
                        }
                    }
                } else {
                    out.push((path, gate));
                }
            }
        }
        let has_task_container = self.gates.iter().any(|g| g.tasks.is_some());
        let mut result = Vec::new();
        for gate in &self.gates {
            if has_task_container && gate.tasks.is_none() {
                continue;
            }
            collect(
                self,
                std::slice::from_ref(gate),
                "",
                &mut result,
                &mut Vec::new(),
            );
        }
        result
    }

    /// Return the gate identifier at a runtime index.
    pub fn gate_id(&self, index: usize) -> &str {
        self.executable_gates()
            .get(index)
            .map(|g| g.id.as_str())
            .unwrap_or("completed")
    }
    /// Return a gate's runtime index by its persisted identifier or leaf alias.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        let gates = self.executable_gate_paths();
        if let Some(pos) = gates.iter().position(|(path, _)| path == id) {
            return Some(pos);
        }
        if id.contains('.') {
            return None;
        }
        let mut matches = gates
            .iter()
            .enumerate()
            .filter(|(_, (path, _))| path.rsplit('.').next() == Some(id));
        let (index, _) = matches.next()?;
        matches.next().is_none().then_some(index)
    }
    /// Return a gate by its persisted identifier or leaf alias.
    pub fn by_stage(&self, stage: &str) -> Option<&GateDef> {
        self.executable_gates().get(self.index_of(stage)?).copied()
    }
    /// Return a reference to a gate by its identifier or leaf alias.
    pub fn find_gate(&self, id: &str) -> Option<&GateDef> {
        fn search<'a>(gates: &'a [GateDef], id: &str) -> Option<&'a GateDef> {
            for gate in gates {
                if gate.id == id || gate.id.rsplit('.').next() == Some(id) {
                    return Some(gate);
                }
                if let Some(found) = search(&gate.gates, id) {
                    return Some(found);
                }
                if let Some(tasks) = &gate.tasks {
                    if let Some(found) = search(&tasks.gates, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        search(&self.gates, id)
    }
    /// Return a mutable reference to a gate by its identifier or leaf alias.
    pub fn find_gate_mut(&mut self, id: &str) -> Option<&mut GateDef> {
        fn search<'a>(gates: &'a mut [GateDef], id: &str) -> Option<&'a mut GateDef> {
            for gate in gates {
                if gate.id == id || gate.id.rsplit('.').next() == Some(id) {
                    return Some(gate);
                }
                if let Some(found) = search(&mut gate.gates, id) {
                    return Some(found);
                }
                if let Some(tasks) = &mut gate.tasks {
                    if let Some(found) = search(&mut tasks.gates, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        search(&mut self.gates, id)
    }
    /// Return the definition of done criteria for subtasks, derived from `guidance["subtask.dod"]`
    /// or `guidance["subtask_dod"]`.
    pub fn subtask_dod_criteria(&self) -> Vec<String> {
        if let Some(entry) = self
            .guidance
            .get("subtask.dod")
            .or_else(|| self.guidance.get("subtask_dod"))
        {
            if !entry.criteria().is_empty() {
                return entry.criteria().to_vec();
            } else if !entry.text().is_empty() {
                return vec![entry.text().to_owned()];
            }
        }
        vec![]
    }
    /// Return the review guidance criteria, derived from `guidance["review.policy"]`
    /// or `guidance["review_policy"]`.
    pub fn review_policy_criteria(&self) -> Vec<String> {
        if let Some(entry) = self
            .guidance
            .get("review.policy")
            .or_else(|| self.guidance.get("review_policy"))
        {
            if !entry.criteria().is_empty() {
                return entry.criteria().to_vec();
            } else if !entry.text().is_empty() {
                return vec![entry.text().to_owned()];
            }
        }
        vec![]
    }
    /// Return proof policies that permit a zero red exit code for contract proof.
    pub fn contract_zero_exit_policies(&self) -> Vec<String> {
        if !self.contract_zero_exit_policies.is_empty() {
            return self.contract_zero_exit_policies.clone();
        }
        vec![
            "direct-proof".to_string(),
            "deferred-final-test".to_string(),
        ]
    }
    /// Return the role name and role definition assigned to a gate.
    pub fn role_for_stage(&self, stage: &str) -> Option<(&str, &RoleDef)> {
        let role_name = self.by_stage(stage)?.role.as_deref()?;
        Some((role_name, self.roles.get(role_name)?))
    }
    /// Return a configured contour set.
    ///
    /// Resolves lenses from `self.guidance` matching `contour.<name>` when `reference`
    /// is `guidance.contours` (or `None`).
    pub fn contour_set(&self, reference: Option<&str>) -> Option<IndexMap<String, ContourDef>> {
        let reference = reference.unwrap_or("guidance.contours");
        if reference != "guidance.contours" && reference != "guidance.contour" {
            return None;
        }
        let mut set = IndexMap::new();
        // Insert standard review contours in canonical order first if defined
        for canonical in crate::tasks::gates::REVIEW_CONTOURS {
            let key = format!("contour.{canonical}");
            if let Some(entry) = self.guidance.get(&key) {
                set.insert(
                    canonical.to_string(),
                    ContourDef {
                        description: entry.text().to_owned(),
                        criteria: if entry.criteria().is_empty() {
                            vec![entry.text().to_owned()]
                        } else {
                            entry.criteria().to_vec()
                        },
                    },
                );
            }
        }
        // Then any additional custom contours declared under guidance (contour.<name>)
        for (key, entry) in &self.guidance {
            if let Some(name) = key.strip_prefix("contour.") {
                if !set.contains_key(name) && !name.trim().is_empty() {
                    set.insert(
                        name.to_string(),
                        ContourDef {
                            description: entry.text().to_owned(),
                            criteria: if entry.criteria().is_empty() {
                                vec![entry.text().to_owned()]
                            } else {
                                entry.criteria().to_vec()
                            },
                        },
                    );
                }
            }
        }
        if set.is_empty() {
            None
        } else {
            Some(set)
        }
    }

    /// Resolve a catalog key for a guidance target and replacement, supporting short values (e.g. "hitl" -> "pro" resolves to "hitl.pro").
    fn resolve_catalog_key(&self, target_or_slot: &str, replacement: &str) -> Option<&str> {
        if let Some((k, _)) = self.guidance.get_key_value(replacement) {
            return Some(k.as_str());
        }
        let slot_prefix = target_or_slot.split('.').next().unwrap_or(target_or_slot);
        let candidate = format!("{slot_prefix}.{replacement}");
        if let Some((k, _)) = self.guidance.get_key_value(&candidate) {
            return Some(k.as_str());
        }
        None
    }

    /// Validate manifest guidance overrides against the guidance catalog.
    pub fn validate_guidance_bindings(
        &self,
        bindings: &BTreeMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut declared_gate_guidance = BTreeSet::new();
        fn collect_gate_guidance(gate: &GateDef, out: &mut BTreeSet<String>) {
            out.extend(gate.guidance.iter().cloned());
            for child in gate.child_gates() {
                collect_gate_guidance(child, out);
            }
        }
        for gate in &self.gates {
            collect_gate_guidance(gate, &mut declared_gate_guidance);
        }

        let has_hitl_guidance = self.guidance.keys().any(|k| k.starts_with("hitl."));

        for (target, replacement) in bindings {
            let target_known = declared_gate_guidance.contains(target)
                || self.guidance.contains_key(target)
                || (target == "hitl" && has_hitl_guidance);
            if !target_known {
                bail!(
                    "ACDD_GUIDANCE_SLOT_INVALID: manifest guidance binding '{target}: {replacement}' targets undeclared guidance key '{target}'"
                );
            }
            if self.resolve_catalog_key(target, replacement).is_none() {
                bail!(
                    "ACDD_GUIDANCE_SLOT_INVALID: manifest guidance binding '{target}: {replacement}' references missing catalog entry '{replacement}'"
                );
            }
        }
        Ok(Vec::new())
    }

    /// Resolve the guidance entries declared by one gate using manifest bindings and defaults.
    pub fn resolve_gate_guidance(
        &self,
        gate: &GateDef,
        bindings: &BTreeMap<String, String>,
    ) -> Vec<(String, String)> {
        gate.guidance
            .iter()
            .filter_map(|requested| {
                let resolved_entry = if let Some(replacement) = bindings.get(requested) {
                    self.resolve_catalog_key(requested, replacement)
                        .and_then(|k| self.guidance.get(k))
                } else if requested == "hitl" || requested.starts_with("hitl.") {
                    if let Some(replacement) = bindings.get("hitl") {
                        self.resolve_catalog_key("hitl", replacement)
                            .and_then(|k| self.guidance.get(k))
                    } else {
                        self.guidance
                            .get(requested)
                            .or_else(|| self.guidance.get("hitl.pro"))
                    }
                } else {
                    self.guidance.get(requested)
                };

                let text = resolved_entry
                    .or_else(|| self.guidance.get(requested))
                    .map(GuidanceEntry::rendered_text)?;
                Some((requested.clone(), text))
            })
            .collect()
    }

    /// Validate the fully merged profile before it becomes active.
    pub fn validate(&self) -> Result<()> {
        if self.limits.diagnostic_inline_bytes == 0
            || self.limits.diagnostic_inline_bytes > forge_core::response::MAX_OUTPUT_BYTES
            || self.limits.max_command_timeout_seconds == 0
        {
            bail!(
                "ACDD_PROFILE_SCHEMA_INVALID: diagnostic_inline_bytes must be 1..={} and max_command_timeout_seconds must be positive",
                forge_core::response::MAX_OUTPUT_BYTES
            );
        }
        self.modules.validate()?;
        self.validate_catalogs()?;
        if self.gates.is_empty() {
            bail!("ACDD_PROFILE_INVALID: gates must not be empty");
        }
        if self.version.is_some_and(|version| version >= 2)
            || self.gates.iter().any(gate_is_hierarchical)
        {
            return self.validate_hierarchical();
        }
        let mut seen = BTreeMap::<&str, usize>::new();
        for (index, gate) in self.gates.iter().enumerate() {
            if gate.id.is_empty()
                || !gate
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                bail!(
                    "ACDD_PROFILE_INVALID: gate id '{}' must be a non-empty token",
                    gate.id
                );
            }
            if seen.insert(gate.id.as_str(), index).is_some() {
                bail!("ACDD_PROFILE_INVALID: duplicate gate id '{}'", gate.id);
            }
            if let Some(attempts) = gate.max_attempts {
                if attempts == 0 {
                    bail!(
                        "GATE_INVALID_CONFIGURATION: gate '{}' max_attempts must be positive",
                        gate.id
                    );
                }
            }
            if let Some(role) = &gate.role {
                if !self.roles.contains_key(role) {
                    bail!(
                        "ACDD_PROFILE_INVALID: gate '{}' references undefined role '{role}'",
                        gate.id
                    );
                }
            }
            if let Some(set) = &gate.contours {
                if self.contour_set(Some(set)).is_none() {
                    bail!(
                        "ACDD_PROFILE_INVALID: gate '{}' references undefined contour set '{set}'",
                        gate.id
                    );
                }
            }
            validate_gate_proof_definition(gate, self)?;
            if gate.receipt && gate.proof.kind() != Some(ProofKind::Delivery) {
                bail!("GATE_INVALID_CONFIGURATION: gate '{}' has receipt:true, which requires proof:delivery", gate.id);
            }
            if matches!(gate.proof.kind(), Some(ProofKind::Command)) && self.commands.is_empty() {
                bail!("ACDD_PROFILE_INVALID: command gate '{}' requires a non-empty commands registry", gate.id);
            }
            for (field, reference) in [
                ("reject_to", gate.reject_to.as_deref()),
                ("independent_from", gate.independent_from.as_deref()),
                ("receipt_review", gate.receipt_review.as_deref()),
            ] {
                if let Some(reference) = reference {
                    let prior = seen.get(reference).copied().filter(|prior| *prior < index);
                    if prior.is_none() {
                        bail!("ACDD_PROFILE_INVALID: gate '{}' {field} must reference a strictly prior gate", gate.id);
                    }
                }
            }
            for (field, reference) in [
                ("on_fail", gate.on_fail.as_deref()),
                ("on_success", gate.on_success.as_deref()),
            ] {
                if let Some(reference) = reference {
                    let valid = seen.contains_key(reference)
                        || self.gates.iter().any(|g| g.id == reference)
                        || self.hierarchical_gate_reference_count(reference, &gate.id) == 1;
                    if !valid {
                        bail!("ACDD_PROFILE_INVALID: gate '{}' {field} references undefined gate '{reference}'", gate.id);
                    }
                }
            }
            for source in &gate.review_sources {
                let prior = seen
                    .get(source.as_str())
                    .copied()
                    .filter(|prior| *prior < index);
                let Some(prior) = prior else {
                    bail!("ACDD_PROFILE_INVALID: gate '{}' review_sources must reference strictly prior review gates", gate.id);
                };
                if self.gates[prior].proof.kind() != Some(ProofKind::Review) {
                    bail!("ACDD_PROFILE_INVALID: gate '{}' review source '{source}' is not a review gate", gate.id);
                }
            }
            if gate.proof.kind() == Some(ProofKind::Delivery) {
                if gate.review_sources.is_empty() {
                    bail!("ACDD_PROFILE_INVALID: delivery gate '{}' requires at least one review_sources entry", gate.id);
                }
                if let Some(review) = gate.receipt_review.as_deref() {
                    let prior = seen.get(review).copied().filter(|prior| *prior < index)
                        .context(format!("ACDD_PROFILE_INVALID: delivery gate '{}' receipt_review must reference a strictly prior gate", gate.id))?;
                    if self.gates[prior].proof.kind() != Some(ProofKind::Review) {
                        bail!("ACDD_PROFILE_INVALID: delivery gate '{}' receipt_review '{review}' is not a review gate", gate.id);
                    }
                    if !gate.review_sources.iter().any(|source| source == review) {
                        bail!("ACDD_PROFILE_INVALID: delivery gate '{}' receipt_review must be included in review_sources", gate.id);
                    }
                }
            }
        }
        let deliveries: Vec<_> = self
            .gates
            .iter()
            .enumerate()
            .filter(|(_, gate)| gate.proof.kind() == Some(ProofKind::Delivery))
            .collect();
        if deliveries.len() != 1 || deliveries[0].0 + 1 != self.gates.len() {
            bail!("ACDD_PROFILE_INVALID: exactly one terminal delivery gate is required");
        }
        self.validate_commands()?;
        self.validate_contours()?;
        Ok(())
    }

    fn validate_catalogs(&self) -> Result<()> {
        if self
            .scope_args_by_pattern
            .iter()
            .chain(
                self.commands
                    .values()
                    .filter_map(|command| match command {
                        CommandDef::Detailed {
                            scope_args_by_pattern,
                            ..
                        } => Some(scope_args_by_pattern),
                        _ => None,
                    })
                    .flat_map(|patterns| patterns.iter()),
            )
            .any(|(pattern, scope_args)| pattern.trim().is_empty() || scope_args.trim().is_empty())
        {
            bail!("ACDD_PROFILE_SCHEMA_INVALID: scope_args_by_pattern patterns and arguments must not be empty");
        }
        for (name, policy) in &self.policies {
            if name.trim().is_empty() {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: policies contains an empty name");
            }
            policy.validate(&format!("policies.{name}"))?;
        }
        for (name, proof) in &self.proofs {
            if name.trim().is_empty() || proof.schema.is_none() {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: proofs.{name} requires a schema");
            }
            proof
                .schema
                .as_ref()
                .unwrap()
                .validate_definition(&format!("proofs.{name}.schema"))?;
        }
        if self.limits.max_metadata_bytes == 0
            || self.limits.max_metadata_bytes > forge_core::config::MAX_METADATA_BYTES
        {
            bail!(
                "ACDD_PROFILE_SCHEMA_INVALID: limits.max_metadata_bytes must be 1..={}",
                forge_core::config::MAX_METADATA_BYTES
            );
        }
        if self.limits.max_trace_summary_bytes == 0
            || self.limits.max_trace_summary_bytes > forge_core::config::MAX_METADATA_BYTES
        {
            bail!(
                "ACDD_PROFILE_SCHEMA_INVALID: limits.max_trace_summary_bytes must be 1..={}",
                forge_core::config::MAX_METADATA_BYTES
            );
        }
        if let Some(policy) = &self.git_commit_note {
            if policy.limits.max_bytes == 0
                || policy.limits.max_bytes > forge_core::config::MAX_METADATA_BYTES
            {
                bail!(
                    "ACDD_PROFILE_SCHEMA_INVALID: git_commit_note.limits.max_bytes must be 1..={}",
                    forge_core::config::MAX_METADATA_BYTES
                );
            }
            if policy
                .format
                .as_deref()
                .is_some_and(|format| !matches!(format, "markdown" | "json"))
            {
                bail!(
                    "ACDD_PROFILE_SCHEMA_INVALID: git_commit_note.format must be markdown or json"
                );
            }
        }
        Ok(())
    }

    /// Ensure the embedded profile keeps commands declarative and language-dispatched.
    pub fn validate_language_agnostic_defaults(&self) -> Result<()> {
        for (name, command) in &self.commands {
            if !matches!(command, CommandDef::Pattern(_)) {
                bail!(
                    "ACDD_PROFILE_INVALID: embedded command '{name}' must use by_pattern dispatch"
                );
            }
        }
        Ok(())
    }

    fn validate_hierarchical(&self) -> Result<()> {
        if !self.gates.iter().any(gate_has_named_id) {
            bail!("GATE_INVALID_CONFIGURATION: hierarchical profile must declare at least one named gate");
        }
        self.validate_commands()?;
        self.validate_contours()?;

        let declares_delivery = self.gates.iter().any(|g| {
            gate_is_delivery(g)
                || g.gates.iter().any(gate_is_delivery)
                || g.tasks
                    .as_ref()
                    .is_some_and(|t| t.gates.iter().any(gate_is_delivery))
        });
        let has_tasks = self.gates.iter().any(|g| g.tasks.is_some());
        let require_terminal_delivery = declares_delivery
            || (has_tasks && self.persistence != Some(PersistenceAction::Logfile));
        self.validate_gate_sequence(&self.gates, "gates", require_terminal_delivery)?;
        for (name, template) in &self.gate_templates {
            if name.trim().is_empty() || template.gates.is_empty() {
                bail!(
                    "ACDD_PROFILE_SCHEMA_INVALID: gate_templates.{name} requires at least one gate"
                );
            }
            if template
                .command
                .as_ref()
                .is_some_and(|command| !self.commands.contains_key(command))
            {
                bail!("GATE_INVALID_CONFIGURATION: gate template '{name}' references an undefined command");
            }
            self.validate_guidance_refs(&template.guidance, &format!("gate template '{name}'"))?;
            if template.max_attempts.is_some_and(|attempts| attempts == 0) {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: gate_templates.{name}.max_attempts must be positive");
            }
            self.validate_gate_sequence(&template.gates, &format!("templates.{name}"), false)?;
        }
        self.validate_template_cycles()?;
        Ok(())
    }

    fn validate_commands(&self) -> Result<()> {
        for (name, command) in &self.commands {
            if name.trim().is_empty() || !command.is_valid() {
                bail!(
                    "ACDD_PROFILE_INVALID: commands registry names and commands must be non-empty"
                );
            }
            if let Some(output) = command.output() {
                output
                    .validate()
                    .with_context(|| format!("ACDD_PROFILE_INVALID: commands.{name}.output"))?;
            }
        }
        Ok(())
    }

    fn validate_contours(&self) -> Result<()> {
        for (key, entry) in &self.guidance {
            if let Some(name) = key.strip_prefix("contour.") {
                if name.trim().is_empty()
                    || entry.text().trim().is_empty()
                    || (matches!(entry, GuidanceEntry::Structured { .. })
                        && (entry.criteria().is_empty()
                            || entry.criteria().iter().any(|c| c.trim().is_empty())))
                {
                    bail!("ACDD_PROFILE_INVALID: contour '{name}' requires a description and operational criteria");
                }
            }
        }
        Ok(())
    }

    fn validate_guidance_refs(&self, requested: &[String], owner: &str) -> Result<()> {
        for key in requested {
            if !self.guidance.contains_key(key) {
                if key == "hitl" && self.guidance.keys().any(|k| k.starts_with("hitl.")) {
                    continue;
                }
                bail!("GATE_INVALID_CONFIGURATION: {owner} references undefined guidance catalog entry '{key}'");
            }
        }
        Ok(())
    }

    fn hierarchical_gate_reference_count(&self, reference: &str, owner: &str) -> usize {
        fn collect(gates: &[GateDef], parent: &str, paths: &mut Vec<String>) {
            for gate in gates {
                let path = if gate.id.is_empty() {
                    parent.to_owned()
                } else if parent.is_empty() || gate.id.contains('.') {
                    gate.id.clone()
                } else {
                    format!("{parent}.{}", gate.id)
                };
                if !gate.id.is_empty() {
                    paths.push(path.clone());
                }
                collect(&gate.gates, &path, paths);
                if let Some(tasks) = &gate.tasks {
                    collect(&tasks.gates, &path, paths);
                }
            }
        }

        let mut paths = Vec::new();
        collect(&self.gates, "", &mut paths);
        let mut namespace = owner.rsplit_once('.').map(|(parent, _)| parent);
        while let Some(parent) = namespace {
            let qualified = format!("{parent}.{reference}");
            if paths.contains(&qualified) {
                return 1;
            }
            namespace = parent.rsplit_once('.').map(|(parent, _)| parent);
        }
        paths
            .iter()
            .filter(|path| {
                path.as_str() == reference
                    || path
                        .strip_suffix(reference)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
            .count()
    }

    fn validate_gate_sequence(
        &self,
        gates: &[GateDef],
        path: &str,
        require_terminal_delivery: bool,
    ) -> Result<()> {
        let mut prior = BTreeMap::<&str, &GateDef>::new();
        for gate in gates {
            if !gate.id.is_empty() && prior.contains_key(gate.id.as_str()) {
                bail!(
                    "GATE_INVALID_CONFIGURATION: duplicate gate id '{}' in {path}",
                    gate.id
                );
            }
            self.validate_gate_fields(gate)?;
            self.validate_ordered_refs(gate, &prior, path)?;
            if !gate.id.is_empty() {
                prior.insert(&gate.id, gate);
                let leaf = gate.id.rsplit('.').next().unwrap_or(&gate.id);
                if leaf != gate.id {
                    prior.entry(leaf).or_insert(gate);
                }
            }
        }

        let deliveries: Vec<_> = gates
            .iter()
            .enumerate()
            .filter(|(_, gate)| gate_is_delivery(gate))
            .collect();
        let has_terminal_delivery = gates.last().is_some_and(|g| {
            gate_is_delivery(g)
                || g.gates.last().is_some_and(gate_is_delivery)
                || g.tasks
                    .as_ref()
                    .and_then(|t| t.gates.last())
                    .is_some_and(gate_is_delivery)
        });
        if (require_terminal_delivery && !has_terminal_delivery)
            || deliveries.len() > 1
            || (deliveries.len() == 1 && deliveries[0].0 + 1 != gates.len())
        {
            bail!("ACDD_PROFILE_INVALID: {path} must contain one terminal delivery gate when delivery is declared");
        }

        for gate in gates {
            let child_path = if gate.id.is_empty() {
                path.to_owned()
            } else {
                format!("{path}.{}", gate.id)
            };
            if !gate.gates.is_empty() {
                self.validate_gate_sequence(&gate.gates, &child_path, false)?;
            }
            if let Some(tasks) = &gate.tasks {
                self.validate_gate_sequence(&tasks.gates, &child_path, false)?;
            }
        }
        Ok(())
    }

    fn validate_ordered_refs(
        &self,
        gate: &GateDef,
        prior: &BTreeMap<&str, &GateDef>,
        path: &str,
    ) -> Result<()> {
        for (field, reference) in [
            ("reject_to", gate.reject_to.as_deref()),
            ("independent_from", gate.independent_from.as_deref()),
            ("receipt_review", gate.receipt_review.as_deref()),
        ] {
            if let Some(reference) = reference {
                let local = local_gate_reference(reference, path);
                let Some(prior_gate) = prior.get(local) else {
                    bail!("ACDD_PROFILE_INVALID: gate '{}' {field} must reference a strictly prior gate in {path}", gate.id);
                };
                if field == "receipt_review" && prior_gate.proof.kind() != Some(ProofKind::Review) {
                    bail!("ACDD_PROFILE_INVALID: gate '{}' receipt_review '{reference}' is not a review gate", gate.id);
                }
            }
        }
        for (field, reference) in [
            ("on_fail", gate.on_fail.as_deref()),
            ("on_success", gate.on_success.as_deref()),
        ] {
            if let Some(reference) = reference {
                let local = local_gate_reference(reference, path);
                if prior.contains_key(local) {
                    continue;
                }
                match self.hierarchical_gate_reference_count(reference, &gate.id) {
                    0 => bail!(
                        "ACDD_PROFILE_INVALID: gate '{}' {field} references undefined gate '{reference}' in {path}",
                        gate.id
                    ),
                    1 => {}
                    _ => bail!(
                        "ACDD_PROFILE_INVALID: gate '{}' {field} reference '{reference}' is ambiguous in the profile",
                        gate.id
                    ),
                }
            }
        }
        if let Some(routing) = &gate.reject_routing {
            for (field, reference) in [
                ("scope", routing.scope.as_deref()),
                ("symbols", routing.symbols.as_deref()),
            ] {
                if let Some(reference) = reference {
                    let local = local_gate_reference(reference, path);
                    if prior.get(local).is_none() {
                        match self.hierarchical_gate_reference_count(reference, &gate.id) {
                            1 => {}
                            0 => bail!("GATE_INVALID_CONFIGURATION: gate '{}' reject_routing.{field} references undefined gate '{reference}' in {path}", gate.id),
                            _ => bail!("GATE_INVALID_CONFIGURATION: gate '{}' reject_routing.{field} reference '{reference}' is ambiguous in the profile", gate.id),
                        }
                    }
                }
            }
        }
        for source in &gate.review_sources {
            let local = local_gate_reference(source, path);
            let Some(prior_gate) = prior.get(local) else {
                bail!("ACDD_PROFILE_INVALID: gate '{}' review_sources must reference strictly prior review gates in {path}", gate.id);
            };
            if prior_gate.proof.kind() != Some(ProofKind::Review) {
                bail!(
                    "ACDD_PROFILE_INVALID: gate '{}' review source '{source}' is not a review gate",
                    gate.id
                );
            }
        }
        if gate.proof.kind() == Some(ProofKind::Delivery) {
            if let Some(review) = gate.receipt_review.as_deref() {
                let local = local_gate_reference(review, path);
                let prior_gate = prior.get(local).context(format!(
                    "ACDD_PROFILE_INVALID: delivery gate '{}' receipt_review must reference a strictly prior gate in {path}",
                    gate.id
                ))?;
                if prior_gate.proof.kind() != Some(ProofKind::Review) {
                    bail!("ACDD_PROFILE_INVALID: delivery gate '{}' receipt_review '{review}' is not a review gate", gate.id);
                }
                if !gate
                    .review_sources
                    .iter()
                    .any(|source| source == review || local_gate_reference(source, path) == local)
                {
                    bail!("ACDD_PROFILE_INVALID: delivery gate '{}' receipt_review must be included in review_sources", gate.id);
                }
            }
        }
        Ok(())
    }

    fn validate_gate_fields(&self, gate: &GateDef) -> Result<()> {
        let has_container_fields =
            !gate.gates.is_empty() || gate.tasks.is_some() || gate.use_template.is_some();
        if gate.role.is_some() && has_container_fields {
            bail!(
                "GATE_INVALID_CONFIGURATION: agent gate '{}' cannot also declare container fields",
                gate.id
            );
        }
        if gate.role.is_none() && gate.command.is_some() && has_container_fields {
            bail!("GATE_INVALID_CONFIGURATION: command gate '{}' cannot also declare container fields", gate.id);
        }
        if gate.id.is_empty() && gate.driver() != Some(GateDriver::Container) {
            bail!("GATE_INVALID_CONFIGURATION: non-container gate requires an id");
        }
        if !gate.id.is_empty()
            && !gate.id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
            })
        {
            bail!(
                "GATE_INVALID_CONFIGURATION: gate id '{}' must be a non-empty token",
                gate.id
            );
        }
        // The language-neutral embedded profile leaves the milestone verifier
        // unbound until a repository overlay supplies its registered command.
        // Its explicit command proof keeps the placeholder distinct from an
        // agent gate while allowing the default catalog to remain empty.
        let unbound_command_placeholder = gate.role.is_none()
            && gate.command.is_none()
            && !has_container_fields
            && gate.proof.kind() == Some(ProofKind::Command);
        let Some(driver) = gate
            .driver()
            .or_else(|| unbound_command_placeholder.then_some(GateDriver::Command))
        else {
            bail!("GATE_INVALID_CONFIGURATION: gate '{}' must declare role, command, gates, tasks, or use", gate.id);
        };
        if let Some(attempts) = gate.max_attempts {
            if attempts == 0 {
                bail!(
                    "GATE_INVALID_CONFIGURATION: gate '{}' max_attempts must be positive",
                    gate.id
                );
            }
        }
        if let Some(role) = &gate.role {
            if !self.roles.contains_key(role) {
                bail!(
                    "GATE_INVALID_CONFIGURATION: gate '{}' references undefined role '{role}'",
                    gate.id
                );
            }
        }
        if let Some(command) = &gate.command {
            if !self.commands.contains_key(command) {
                bail!("GATE_INVALID_CONFIGURATION: gate '{}' references undefined command '{command}'", gate.id);
            }
        }
        if let Some(policy) = &gate.policy {
            if !self.policies.contains_key(policy) {
                bail!(
                    "GATE_INVALID_CONFIGURATION: gate '{}' references undefined policy '{policy}'",
                    gate.id
                );
            }
        }
        if let Some(contours) = &gate.contours {
            if self.contour_set(Some(contours)).is_none() {
                bail!("GATE_INVALID_CONFIGURATION: gate '{}' references undefined contour set '{contours}'", gate.id);
            }
        }
        self.validate_guidance_refs(&gate.guidance, &format!("gate '{}'", gate.id))?;
        if gate
            .when
            .as_deref()
            .is_some_and(|condition| !validate_gate_condition_syntax(condition))
        {
            bail!("GATE_INVALID_CONFIGURATION: gate '{}' has unsupported when condition '{}'; expected code, doc, db_migration, seam, or public_api", gate.id, gate.when.as_deref().unwrap_or_default());
        }
        for symbol in &gate.symbols {
            super::super::validate_symbol_ref(symbol).with_context(|| {
                format!(
                    "GATE_INVALID_CONFIGURATION: gate '{}' has invalid symbol boundary '{symbol}'",
                    gate.id
                )
            })?;
        }
        if let Some(template) = &gate.use_template {
            if !self.gate_templates.contains_key(template) {
                bail!("GATE_INVALID_CONFIGURATION: gate '{}' references undefined gate template '{template}'", gate.id);
            }
        }
        if let Some(routing) = &gate.reject_routing {
            for (field, target) in [
                ("scope", routing.scope.as_deref()),
                ("symbols", routing.symbols.as_deref()),
            ] {
                if let Some(target) = target {
                    if target.trim().is_empty() {
                        bail!("GATE_INVALID_CONFIGURATION: gate '{}' reject_routing.{field} must not be empty", gate.id);
                    }
                }
            }
        }
        validate_gate_proof_definition(gate, self)?;
        if gate.receipt && gate.proof.kind() != Some(ProofKind::Delivery) {
            bail!("GATE_INVALID_CONFIGURATION: gate '{}' has receipt:true, which requires proof:delivery", gate.id);
        }
        if let Some(expected) = &gate.expected_failure {
            if expected.exit_codes.is_empty() || expected.exit_codes.contains(&0) {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: gate '{}' expected_failure.exit_codes must contain non-zero values", gate.id);
            }
        }
        if driver == GateDriver::Command && gate.command.is_none() && !unbound_command_placeholder {
            bail!(
                "GATE_INVALID_CONFIGURATION: command gate '{}' requires command",
                gate.id
            );
        }
        Ok(())
    }

    fn validate_template_cycles(&self) -> Result<()> {
        fn visit(
            profile: &GateProfile,
            name: &str,
            visiting: &mut BTreeSet<String>,
            visited: &mut BTreeSet<String>,
        ) -> Result<()> {
            if visited.contains(name) {
                return Ok(());
            }
            if !visiting.insert(name.to_owned()) {
                bail!("GATE_INVALID_CONFIGURATION: gate template cycle includes '{name}'");
            }
            let template = profile.gate_templates.get(name).context(format!(
                "GATE_INVALID_CONFIGURATION: unknown gate template '{name}'"
            ))?;
            fn dependencies(gates: &[GateDef], out: &mut BTreeSet<String>) {
                for gate in gates {
                    if let Some(template) = &gate.use_template {
                        out.insert(template.clone());
                    }
                    dependencies(&gate.gates, out);
                    if let Some(tasks) = &gate.tasks {
                        dependencies(&tasks.gates, out);
                    }
                }
            }
            let mut dependencies_found = BTreeSet::new();
            dependencies(&template.gates, &mut dependencies_found);
            for dependency in dependencies_found {
                visit(profile, &dependency, visiting, visited)?;
            }
            visiting.remove(name);
            visited.insert(name.to_owned());
            Ok(())
        }

        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for name in self.gate_templates.keys() {
            visit(self, name, &mut visiting, &mut visited)?;
        }
        Ok(())
    }
}

fn validate_gate_proof_definition(gate: &GateDef, profile: &GateProfile) -> Result<()> {
    gate.proof.validate_scheme()?;
    if let Some(name) = gate.proof.catalog_name() {
        let catalog = profile.proofs.get(name).with_context(|| {
            format!("ACDD_PROFILE_SCHEMA_INVALID: gate '{}' references undefined proof catalog entry '{name}'", gate.id)
        })?;
        let schema = catalog.schema.as_ref().with_context(|| {
            format!("ACDD_PROFILE_SCHEMA_INVALID: proofs.{name} requires a schema")
        })?;
        schema.validate_definition(&format!("proofs.{name}.schema"))?;
    }
    Ok(())
}

impl CommandDef {
    /// Return the executable shell command.
    pub fn command(&self) -> &str {
        match self {
            Self::Script(s) => s,
            Self::Detailed { command, .. } => command,
            Self::Pattern(patterns) => patterns
                .by_pattern
                .get("**/*.rs")
                .or_else(|| patterns.by_pattern.values().next())
                .map(String::as_str)
                .unwrap_or_default(),
        }
    }

    /// Return the declared output rules, if present.
    pub fn output(&self) -> Option<&CommandOutputSpec> {
        match self {
            Self::Detailed { output, .. } => output.as_ref(),
            Self::Pattern(patterns) => patterns.output.as_ref(),
            _ => None,
        }
    }

    /// Resolve a matching command for one project-relative path.
    pub fn command_for_path(&self, path: &str) -> Option<&str> {
        let Self::Pattern(patterns) = self else {
            return Some(self.command());
        };
        patterns
            .by_pattern
            .iter()
            .filter(|(pattern, _)| glob_pattern_matches(pattern, path))
            .max_by_key(|(pattern, _)| pattern.len())
            .map(|(_, command)| command.as_str())
    }

    /// Resolve matching commands against a collection of candidate paths.
    pub fn commands_for_paths<'a>(&'a self, paths: &[impl AsRef<str>]) -> Vec<&'a str> {
        let Self::Pattern(patterns) = self else {
            return vec![self.command()];
        };
        let mut matched_commands = BTreeSet::new();
        for path in paths {
            let path_str = path.as_ref();
            for (pattern, command) in &patterns.by_pattern {
                if glob_pattern_matches(pattern, path_str) {
                    matched_commands.insert(command.as_str());
                }
            }
        }
        matched_commands.into_iter().collect()
    }

    /// Resolve every distinct command for repository root matching actual files.
    pub fn commands_for_root(&self, root: &Path) -> Vec<&str> {
        match self {
            Self::Script(cmd) => vec![cmd.as_str()],
            Self::Detailed { command, .. } => vec![command.as_str()],
            Self::Pattern(patterns) => {
                let paths = discover_root_files(root);
                let mut matched = BTreeSet::new();
                for (pattern, cmd) in &patterns.by_pattern {
                    if paths.iter().any(|path| glob_pattern_matches(pattern, path)) {
                        matched.insert(cmd.as_str());
                    }
                }
                matched.into_iter().collect()
            }
        }
    }

    fn is_valid(&self) -> bool {
        match self {
            Self::Script(command) => !command.trim().is_empty(),
            Self::Detailed {
                command,
                timeout_seconds,
                ..
            } => !command.trim().is_empty() && timeout_seconds.is_none_or(|timeout| timeout > 0),
            Self::Pattern(patterns) => {
                !patterns.by_pattern.is_empty()
                    && patterns.timeout_seconds.is_none_or(|timeout| timeout > 0)
                    && patterns.by_pattern.iter().all(|(pattern, command)| {
                        !pattern.trim().is_empty() && !command.trim().is_empty()
                    })
            }
        }
    }
}

fn discover_root_files(root: &Path) -> Vec<String> {
    if let Ok(out) = crate::git::git_command(
        root,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    ) {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let paths: Vec<String> = stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect();
            if !paths.is_empty() {
                return paths;
            }
        }
    }
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if file_type.is_dir() {
                if !forge_core::config::DEFAULT_IGNORED_NAMES.contains(&name_str.as_ref()) {
                    stack.push(path);
                }
            } else if file_type.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    files.push(rel.to_string_lossy().into_owned());
                }
            }
        }
    }
    files
}
