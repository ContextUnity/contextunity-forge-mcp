use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Component, Path, PathBuf}, sync::{Arc, OnceLock}};

const DEFAULT_PROFILE_YAML: &str = include_str!("acdd.default.yaml");

/// Specification of an agent execution model within a role.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    /// Model identifier offered for this role.
    pub model: String,
    /// Optional reasoning effort recommendation.
    #[serde(default)]
    pub reasoning: Option<String>,
}

/// Execution mode selected for an ACDD role.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoleExecutionMode {
    /// Run the role as a separate agent.
    #[default]
    Subagent,
    /// Run the role inline in the current agent.
    Inline,
}

/// Definition of an agent role in an ACDD workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RoleDef {
    /// Requested execution mode; omitted values effectively use `Subagent`.
    #[serde(default)]
    pub mode: Option<RoleExecutionMode>,
    /// Whether to return review rejections to the same worker; defaults to true.
    #[serde(default)]
    pub reuse_on_reject: Option<bool>,
    /// Ordered model recommendations, with the first entry as the primary model.
    #[serde(default)]
    pub models: Vec<ModelSpec>,
    /// Free-form advisory guidance for the role.
    #[serde(default)]
    pub recommendation: Option<String>,
}

impl RoleDef {
    /// Return the configured execution mode or its default.
    pub fn effective_mode(&self) -> RoleExecutionMode {
        self.mode.unwrap_or_default()
    }

    /// Return whether rejection handling should reuse the prior worker.
    pub fn effective_reuse_on_reject(&self) -> bool {
        self.reuse_on_reject.unwrap_or(true)
    }

    fn merge_override(&mut self, other: RoleDef) {
        if other.mode.is_some() { self.mode = other.mode; }
        if other.reuse_on_reject.is_some() { self.reuse_on_reject = other.reuse_on_reject; }
        if !other.models.is_empty() { self.models = other.models; }
        if other.recommendation.is_some() { self.recommendation = other.recommendation; }
    }
}

/// Closed set of executable gate proof behaviors.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProofKind {
    /// Contract proof containing the seam test and red exit code.
    Contract,
    /// Command proof containing test command and result counts.
    Command,
    /// Review proof containing a decision and configured contour evidence.
    Review,
    /// Delivery proof recording the task delivery result.
    Delivery,
    /// Empty proof payload for a gate with no typed evidence.
    None,
}

/// Recursive data schema for `proof: { scheme: ... }` evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SchemaDef {
    /// JSON value type accepted by this schema node.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Named child schemas for object properties.
    #[serde(default)]
    pub properties: IndexMap<String, SchemaDef>,
    /// Object properties that must be present.
    #[serde(default)]
    pub required: Vec<String>,
    /// Whether object properties absent from `properties` are accepted.
    #[serde(default, rename = "additionalProperties")]
    pub additional_properties: Option<bool>,
    /// Schema applied to every array element.
    #[serde(default)]
    pub items: Option<Box<SchemaDef>>,
    /// Optional set of accepted values.
    #[serde(default, rename = "enum")]
    pub enum_values: Vec<serde_json::Value>,
}

impl SchemaDef {
    fn validate_definition(&self, path: &str) -> Result<()> {
        match self.schema_type.as_str() {
            "object" => {
                if self.items.is_some() { bail!("ACDD_PROFILE_SCHEMA_INVALID: {path}.items is only valid for arrays"); }
                for required in &self.required {
                    if !self.properties.contains_key(required) {
                        bail!("ACDD_PROFILE_SCHEMA_INVALID: {path}.required references undefined property '{required}'");
                    }
                }
                for (name, schema) in self.properties.iter() {
                    schema.validate_definition(&format!("{path}.properties.{name}"))?;
                }
            }
            "array" => {
                if !self.properties.is_empty() || !self.required.is_empty() {
                    bail!("ACDD_PROFILE_SCHEMA_INVALID: {path} array cannot declare object properties");
                }
                self.items.as_ref().context(format!("ACDD_PROFILE_SCHEMA_INVALID: {path}.items is required for arrays"))?
                    .validate_definition(&format!("{path}.items"))?;
            }
            "string" | "integer" | "number" | "boolean" | "null" => {
                if !self.properties.is_empty() || !self.required.is_empty() || self.items.is_some() {
                    bail!("ACDD_PROFILE_SCHEMA_INVALID: {path} scalar schema has container fields");
                }
            }
            other => bail!("ACDD_PROFILE_SCHEMA_INVALID: {path}.type '{other}' is unsupported"),
        }
        Ok(())
    }

    /// Validate a JSON value and report the exact nested evidence path.
    pub fn validate_value(&self, value: &serde_json::Value, path: &str) -> Result<()> {
        let matches = match self.schema_type.as_str() {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => false,
        };
        if !matches { bail!("TASK_EVIDENCE_INVALID: {path} must be {}", self.schema_type); }
        if !self.enum_values.is_empty() && !self.enum_values.contains(value) {
            bail!("TASK_EVIDENCE_INVALID: {path} is not an allowed value");
        }
        if let Some(object) = value.as_object() {
            for required in &self.required {
                if !object.contains_key(required) { bail!("TASK_EVIDENCE_INVALID: {path}.{required} is required"); }
            }
            for (name, item) in object {
                if let Some(schema) = self.properties.get(name) {
                    schema.validate_value(item, &format!("{path}.{name}"))?;
                } else if self.additional_properties == Some(false) {
                    bail!("TASK_EVIDENCE_INVALID: {path}.{name} is not allowed");
                }
            }
        }
        if let (Some(items), Some(array)) = (&self.items, value.as_array()) {
            for (index, item) in array.iter().enumerate() {
                items.validate_value(item, &format!("{path}[{index}]"))?;
            }
        }
        Ok(())
    }
}

/// A proof is either a built-in closed behavior or a recursively validated schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ProofDef {
    /// One of the built-in proof behaviors.
    Kind(ProofKind),
    /// Recursive evidence schema under `proof: { scheme: ... }`.
    Scheme {
        /// Schema used to validate evidence at this gate.
        scheme: SchemaDef,
    },
}

impl ProofDef {
    /// Return the built-in proof kind, or `None` for a schema proof.
    pub fn kind(&self) -> Option<ProofKind> {
        match self { Self::Kind(kind) => Some(*kind), Self::Scheme { .. } => None }
    }
    /// Validate the schema definition for a scheme proof.
    pub fn validate_scheme(&self) -> Result<()> {
        if let Self::Scheme { scheme } = self { scheme.validate_definition("proof.scheme")?; }
        Ok(())
    }
}

/// Operational criteria attached to one review contour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContourDef {
    /// Short purpose statement shown to reviewers.
    pub description: String,
    /// Concrete checks required for this contour.
    pub criteria: Vec<String>,
}

/// A registered command available to command proof gates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CommandDef {
    /// Shell command registered directly by its command name.
    Script(String),
    /// Shell command with optional explanatory text.
    Detailed {
        /// Executable shell command.
        command: String,
        /// Optional explanation shown with the command.
        #[serde(default)]
        description: Option<String>,
    },
}
impl CommandDef {
    /// Return the executable shell command.
    pub fn command(&self) -> &str { match self { Self::Script(s) => s, Self::Detailed { command, .. } => command } }
}

/// Definition of an ACDD lifecycle gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GateDef {
    /// Stable stage identifier persisted in each task.
    pub id: String,
    /// Role assigned to this gate, if any.
    #[serde(default)]
    pub role: Option<String>,
    /// Closed proof behavior accepted at this gate.
    pub proof: ProofDef,
    /// Tools available or recommended for this gate.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Ordered operational steps for this gate.
    #[serde(default)]
    pub steps: Vec<String>,
    /// Prior gate whose worker must be independent from this gate's worker.
    #[serde(default)]
    pub independent_from: Option<String>,
    /// Prior review gate used to form the durable receipt.
    #[serde(default)]
    pub receipt_review: Option<String>,
    /// Prior gate to rewind to when this gate is rejected.
    #[serde(default)]
    pub reject_to: Option<String>,
    /// Prior review gates whose evidence is included in delivery.
    #[serde(default)]
    pub review_sources: Vec<String>,
    /// Named contour set required for review proofs.
    #[serde(default)]
    pub contours: Option<String>,
    /// Whether this gate captures a candidate source snapshot.
    #[serde(default)]
    pub sha_snapshot: bool,
    /// Whether delivery creates a commit from the candidate snapshot.
    #[serde(default = "default_auto_commit")]
    pub auto_commit: bool,
}
fn default_auto_commit() -> bool { true }

/// Settings and guidance for completed tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CompletedGate {
    /// Tools shown after all configured gates pass.
    #[serde(default)] pub tools: Vec<String>,
    /// Steps shown after all configured gates pass.
    #[serde(default)] pub steps: Vec<String>,
}

/// A complete declarative ACDD workflow profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct GateProfile {
    /// Definition of done applied to every subtask.
    #[serde(default)] pub subtask_dod: Vec<String>,
    /// Review guidance shown to reviewers.
    #[serde(default)] pub review_policy: Vec<String>,
    /// Contract proof policies that permit a zero red exit code.
    #[serde(default)] pub contract_zero_exit_policies: Vec<String>,
    /// Available agent roles and their model guidance.
    #[serde(default)] pub roles: BTreeMap<String, RoleDef>,
    /// Registered shell commands usable by command proof gates.
    #[serde(default)] pub commands: BTreeMap<String, CommandDef>,
    /// Hierarchical workflow policy values.
    #[serde(default)] pub policies: BTreeMap<String, serde_yaml::Value>,
    /// Named ordered sets of review contours.
    #[serde(default)] pub contours: IndexMap<String, IndexMap<String, ContourDef>>,
    /// Ordered workflow gates.
    #[serde(default)] pub gates: Vec<GateDef>,
    /// Guidance displayed after completion.
    #[serde(default)] pub completed: CompletedGate,
}

impl GateProfile {
    /// Return the gate identifier at a runtime index.
    pub fn gate_id(&self, index: usize) -> &str { self.gates.get(index).map(|g| g.id.as_str()).unwrap_or("completed") }
    /// Return a gate's runtime index by its persisted identifier.
    pub fn index_of(&self, id: &str) -> Option<usize> { self.gates.iter().position(|g| g.id == id) }
    /// Return a gate by its persisted identifier.
    pub fn by_stage(&self, stage: &str) -> Option<&GateDef> { self.gates.iter().find(|g| g.id == stage) }
    /// Return the role name and role definition assigned to a gate.
    pub fn role_for_stage(&self, stage: &str) -> Option<(&str, &RoleDef)> {
        let role_name = self.by_stage(stage)?.role.as_deref()?;
        Some((role_name, self.roles.get(role_name)?))
    }
    /// Return a configured contour set, defaulting to `standard`.
    pub fn contour_set(&self, name: Option<&str>) -> Option<&IndexMap<String, ContourDef>> {
        self.contours.get(name.unwrap_or("standard"))
    }

    /// Validate the fully merged profile before it becomes active.
    pub fn validate(&self) -> Result<()> {
        if self.gates.is_empty() { bail!("ACDD_PROFILE_INVALID: gates must not be empty"); }
        let mut seen = BTreeMap::<&str, usize>::new();
        for (index, gate) in self.gates.iter().enumerate() {
            if gate.id.is_empty() || !gate.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                bail!("ACDD_PROFILE_INVALID: gate id '{}' must be a non-empty token", gate.id);
            }
            if seen.insert(gate.id.as_str(), index).is_some() { bail!("ACDD_PROFILE_INVALID: duplicate gate id '{}'", gate.id); }
            if let Some(role) = &gate.role {
                if !self.roles.contains_key(role) { bail!("ACDD_PROFILE_INVALID: gate '{}' references undefined role '{role}'", gate.id); }
            }
            if let Some(set) = &gate.contours {
                if self.contours.get(set).is_none() { bail!("ACDD_PROFILE_INVALID: gate '{}' references undefined contour set '{set}'", gate.id); }
            }
            gate.proof.validate_scheme()?;
            if matches!(gate.proof.kind(), Some(ProofKind::Command)) && self.commands.is_empty() {
                bail!("ACDD_PROFILE_INVALID: command gate '{}' requires a non-empty commands registry", gate.id);
            }
            for (field, reference) in [("reject_to", gate.reject_to.as_deref()), ("independent_from", gate.independent_from.as_deref()), ("receipt_review", gate.receipt_review.as_deref())] {
                if let Some(reference) = reference {
                    let prior = seen.get(reference).copied().filter(|prior| *prior < index);
                    if prior.is_none() { bail!("ACDD_PROFILE_INVALID: gate '{}' {field} must reference a strictly prior gate", gate.id); }
                }
            }
            for source in &gate.review_sources {
                let prior = seen.get(source.as_str()).copied().filter(|prior| *prior < index);
                let Some(prior) = prior else { bail!("ACDD_PROFILE_INVALID: gate '{}' review_sources must reference strictly prior review gates", gate.id); };
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
        let deliveries: Vec<_> = self.gates.iter().enumerate().filter(|(_, gate)| gate.proof.kind() == Some(ProofKind::Delivery)).collect();
        if deliveries.len() != 1 || deliveries[0].0 + 1 != self.gates.len() {
            bail!("ACDD_PROFILE_INVALID: exactly one terminal delivery gate is required");
        }
        for (name, command) in &self.commands {
            if name.trim().is_empty() || command.command().trim().is_empty() {
                bail!("ACDD_PROFILE_INVALID: commands registry names and commands must be non-empty");
            }
        }
        for (set_name, contours) in self.contours.iter() {
            for (name, contour) in contours.iter() {
                if name.trim().is_empty() || contour.description.trim().is_empty() || contour.criteria.is_empty() || contour.criteria.iter().any(|criterion| criterion.trim().is_empty()) {
                    bail!("ACDD_PROFILE_INVALID: contour '{set_name}.{name}' requires a description and operational criteria");
                }
            }
        }
        Ok(())
    }

    /// Merge overrides on top of this base profile. Gates replace as a whole sequence.
    pub fn merge_override(&mut self, other: GateProfile) {
        if !other.subtask_dod.is_empty() { self.subtask_dod = other.subtask_dod; }
        if !other.review_policy.is_empty() { self.review_policy = other.review_policy; }
        if !other.contract_zero_exit_policies.is_empty() { self.contract_zero_exit_policies = other.contract_zero_exit_policies; }
        for (key, role) in other.roles {
            if let Some(current) = self.roles.get_mut(&key) { current.merge_override(role); }
            else { self.roles.insert(key, role); }
        }
        self.commands.extend(other.commands);
        for (key, value) in other.policies { merge_yaml(self.policies.entry(key).or_default(), value); }
        self.contours.extend(other.contours);
        if !other.gates.is_empty() { self.gates = other.gates; }
        if !other.completed.tools.is_empty() { self.completed.tools = other.completed.tools; }
        if !other.completed.steps.is_empty() { self.completed.steps = other.completed.steps; }
    }
}

fn merge_yaml(base: &mut serde_yaml::Value, override_value: serde_yaml::Value) {
    if let (serde_yaml::Value::Mapping(base), serde_yaml::Value::Mapping(overrides)) = (&mut *base, &override_value) {
        for (key, value) in overrides {
            merge_yaml(base.entry(key.clone()).or_insert(serde_yaml::Value::Null), value.clone());
        }
    } else { *base = override_value; }
}

/// Parse an ACDD profile YAML document. Validation is applied after overrides merge.
pub fn parse_profile(yaml_str: &str) -> Result<GateProfile> {
    serde_yaml::from_str(yaml_str).map_err(|err| anyhow::anyhow!(
        "ACDD_PROFILE_SCHEMA_INVALID: {err}\nSupported fields: gates, roles, commands, policies, contours, completed, subtask_dod, review_policy, contract_zero_exit_policies."
    ))
}

static COMPILED_PROFILE: OnceLock<Arc<GateProfile>> = OnceLock::new();
/// Compile and return the embedded default workflow profile.
pub fn compiled() -> Arc<GateProfile> {
    COMPILED_PROFILE.get_or_init(|| {
        let profile = parse_profile(DEFAULT_PROFILE_YAML).expect("embedded acdd.default.yaml must be valid");
        profile.validate().expect("embedded acdd.default.yaml must compile");
        Arc::new(profile)
    }).clone()
}

/// Split a pinned profile reference into a relative path and optional SHA prefix.
pub fn parse_profile_reference(reference: &str, require_pin: bool) -> Result<(PathBuf, Option<String>)> {
    let (path, pin) = match reference.rsplit_once(':') {
        Some((path, pin)) if !path.is_empty() && !pin.is_empty() => (path, Some(pin.to_ascii_lowercase())),
        _ => (reference, None),
    };
    if require_pin && pin.is_none() { bail!("ACDD_PROFILE_PIN_REQUIRED: use '<path>:<sha256_prefix>'"); }
    if let Some(pin) = &pin {
        if pin.len() < 7 || !pin.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("ACDD_PROFILE_PIN_INVALID: SHA-256 prefix must contain at least 7 hexadecimal characters");
        }
    }
    let path = Path::new(path);
    if path.is_absolute() || path.components().any(|component| !matches!(component, Component::Normal(_))) {
        bail!("ACDD_PROFILE_PATH_INVALID: profile path must be relative and confined to the workspace");
    }
    Ok((path.to_path_buf(), pin))
}

fn load_profile_path(root: &Path, reference: &str, require_pin: bool) -> Result<GateProfile> {
    let (relative, pin) = parse_profile_reference(reference, require_pin)?;
    let path = crate::core::tasks::confined_path(root, &relative.to_string_lossy())?;
    let contents = std::fs::read(&path).with_context(|| format!("ACDD_PROFILE_NOT_FOUND: {}", path.display()))?;
    if let Some(prefix) = pin {
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(&contents));
        if !digest.starts_with(&prefix) { bail!("TASK_PROFILE_TAMPERED: profile SHA-256 does not match pinned prefix '{prefix}'"); }
    }
    let text = std::str::from_utf8(&contents).context("ACDD_PROFILE_SCHEMA_INVALID: profile must be UTF-8 YAML")?;
    parse_profile(text).with_context(|| format!("invalid profile in {}", path.display()))
}

/// Load one pinned task or milestone override above the workspace base profile.
pub fn load_pinned(root: &Path, reference: &str) -> Result<Arc<GateProfile>> {
    let base = load_for_workspace(root)?;
    load_pinned_over(root, reference, &base)
}

/// Load one pinned task or milestone override above an already-selected workspace base profile.
pub fn load_pinned_over(
    root: &Path,
    reference: &str,
    base_profile: &GateProfile,
) -> Result<Arc<GateProfile>> {
    let mut base = base_profile.clone();
    base.merge_override(load_profile_path(root, reference, true)?);
    base.validate()?;
    Ok(Arc::new(base))
}

/// Load a task profile override, then milestone override, then workspace profile.
pub fn load_for_task(root: &Path, task_profile: Option<&str>, milestone_profile: Option<&str>) -> Result<Arc<GateProfile>> {
    match task_profile.or(milestone_profile) {
        Some(reference) => load_pinned(root, reference),
        None => load_for_workspace(root),
    }
}

/// Resolve workspace profile links and repository defaults, then validate the merged profile.
pub fn load_for_workspace(root: &Path) -> Result<Arc<GateProfile>> {
    let mut base = (*compiled()).clone();
    let forge_config = root.join("forge-mcp.yaml");
    if forge_config.is_file() {
        let text = std::fs::read_to_string(&forge_config).with_context(|| format!("failed to read {}", forge_config.display()))?;
        let value: serde_yaml::Value = serde_yaml::from_str(&text).with_context(|| format!("invalid YAML in {}", forge_config.display()))?;
        if let Some(reference) = value.get("acdd_profile") {
            let reference = reference.as_str().context("acdd_profile in forge-mcp.yaml must be a relative file path string")?;
            base.merge_override(load_profile_path(root, reference, false)?);
            base.validate()?;
            return Ok(Arc::new(base));
        }
    }
    let repository_profile =
        crate::core::tasks::confined_path(root, ".forge/acdd/profile.yaml")?;
    if repository_profile.is_file() {
        let text = std::fs::read_to_string(&repository_profile).with_context(|| format!("failed to read {}", repository_profile.display()))?;
        base.merge_override(parse_profile(&text).with_context(|| format!("invalid profile in {}", repository_profile.display()))?);
    }
    base.validate()?;
    Ok(Arc::new(base))
}
