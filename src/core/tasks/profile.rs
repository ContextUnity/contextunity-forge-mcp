use super::catalog::SubsystemRegistry;
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::{Arc, OnceLock},
};

const DEFAULT_PROFILE_YAML: &str = include_str!("acdd.default.yaml");

/// Specification of an agent execution model within a role.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelSpec {
    /// Model identifier offered for this role.
    pub model: String,
    /// Optional reasoning effort recommendation.
    #[serde(default)]
    pub reasoning: Option<String>,
}

impl ModelSpec {
    /// Format model with optional reasoning (e.g. "model:reasoning" or "model").
    pub fn formatted(&self) -> String {
        match &self.reasoning {
            Some(reasoning) => format!("{}:{}", self.model, reasoning),
            None => self.model.clone(),
        }
    }
}

impl<'de> Deserialize<'de> for ModelSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct MapRepr {
            model: String,
            #[serde(default)]
            reasoning: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Str(String),
            Map(MapRepr),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Str(s) => {
                let s = s.trim();
                if s.is_empty() {
                    return Err(serde::de::Error::custom("model identifier cannot be empty"));
                }
                if let Some((model, reasoning)) = s.split_once(':') {
                    let model = model.trim();
                    let reasoning = reasoning.trim();
                    if model.is_empty() || reasoning.is_empty() {
                        return Err(serde::de::Error::custom(
                            "model string with colon requires non-empty model and reasoning (or use map format)",
                        ));
                    }
                    Ok(ModelSpec {
                        model: model.to_owned(),
                        reasoning: Some(reasoning.to_owned()),
                    })
                } else {
                    Ok(ModelSpec {
                        model: s.to_owned(),
                        reasoning: None,
                    })
                }
            }
            Repr::Map(MapRepr { model, reasoning }) => {
                let model = model.trim();
                if model.is_empty() {
                    return Err(serde::de::Error::custom("model identifier cannot be empty"));
                }
                let reasoning = reasoning
                    .map(|r| r.trim().to_owned())
                    .filter(|r| !r.is_empty());
                Ok(ModelSpec {
                    model: model.to_owned(),
                    reasoning,
                })
            }
        }
    }
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
    pub guidance: Option<String>,
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
        if other.mode.is_some() {
            self.mode = other.mode;
        }
        if other.reuse_on_reject.is_some() {
            self.reuse_on_reject = other.reuse_on_reject;
        }
        if !other.models.is_empty() {
            self.models = other.models;
        }
        if other.guidance.is_some() {
            self.guidance = other.guidance;
        }
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
    /// Joint production seam proof for cross-task integration.
    SeamTest,
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
    /// Optional exact value accepted by this node.
    #[serde(default, rename = "const", skip_serializing_if = "Option::is_none")]
    pub const_value: Option<serde_json::Value>,
}

impl SchemaDef {
    fn validate_definition(&self, path: &str) -> Result<()> {
        match self.schema_type.as_str() {
            "object" => {
                if self.items.is_some() {
                    bail!("ACDD_PROFILE_SCHEMA_INVALID: {path}.items is only valid for arrays");
                }
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
                self.items
                    .as_ref()
                    .context(format!(
                        "ACDD_PROFILE_SCHEMA_INVALID: {path}.items is required for arrays"
                    ))?
                    .validate_definition(&format!("{path}.items"))?;
            }
            "string" | "integer" | "number" | "boolean" | "null" => {
                if !self.properties.is_empty() || !self.required.is_empty() || self.items.is_some()
                {
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
        if !matches {
            bail!("TASK_EVIDENCE_INVALID: {path} must be {}", self.schema_type);
        }
        if !self.enum_values.is_empty() && !self.enum_values.contains(value) {
            bail!("TASK_EVIDENCE_INVALID: {path} is not an allowed value");
        }
        if self
            .const_value
            .as_ref()
            .is_some_and(|expected| expected != value)
        {
            bail!("TASK_EVIDENCE_INVALID: {path} does not match its required constant");
        }
        if let Some(object) = value.as_object() {
            for required in &self.required {
                if !object.contains_key(required) {
                    bail!("TASK_EVIDENCE_INVALID: {path}.{required} is required");
                }
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
        scheme: Box<SchemaDef>,
    },
}

impl ProofDef {
    /// Return the built-in proof kind, or `None` for a schema proof.
    pub fn kind(&self) -> Option<ProofKind> {
        match self {
            Self::Kind(kind) => Some(*kind),
            Self::Scheme { .. } => None,
        }
    }
    /// Validate the schema definition for a scheme proof.
    pub fn validate_scheme(&self) -> Result<()> {
        if let Self::Scheme { scheme } = self {
            scheme.validate_definition("proof.scheme")?;
        }
        Ok(())
    }
}

impl Default for ProofDef {
    fn default() -> Self {
        Self::Kind(ProofKind::None)
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

/// Declarative policy configuration retained as typed key/value settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PolicyDef {
    /// Human-readable policy purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Policy-specific settings interpreted by the owning gate.
    #[serde(flatten)]
    pub settings: BTreeMap<String, serde_yaml::Value>,
}

impl PolicyDef {
    fn validate(&self, path: &str) -> Result<()> {
        for (name, value) in &self.settings {
            validate_policy_setting(name, value, &format!("{path}.{name}"))?;
        }
        Ok(())
    }
}

fn validate_policy_setting(name: &str, value: &serde_yaml::Value, path: &str) -> Result<()> {
    if matches!(
        name,
        "atomic_receipt"
            | "audit_extended_scope"
            | "immutable"
            | "independent_worker"
            | "require_all_contours"
            | "require_zero_exit"
            | "restore_on_reject"
    ) && !value.is_bool()
    {
        bail!("ACDD_PROFILE_SCHEMA_INVALID: {path} must be a boolean");
    }
    if matches!(
        name,
        "max_bytes" | "max_log_bytes" | "max_metadata_bytes" | "max_trace_summary_bytes"
    ) {
        let limit = value.as_u64().context(format!(
            "ACDD_PROFILE_SCHEMA_INVALID: {path} must be a positive integer"
        ))?;
        if limit == 0 || limit > 2048 && name != "max_log_bytes" {
            bail!("ACDD_PROFILE_SCHEMA_INVALID: {path} must be within its supported byte limit");
        }
    }
    if let Some(mapping) = value.as_mapping() {
        for (child_name, child_value) in mapping {
            if let Some(child_name) = child_name.as_str() {
                validate_policy_setting(child_name, child_value, &format!("{path}.{child_name}"))?;
            }
        }
    }
    Ok(())
}

/// Stable diagnostic description paired with an ACDD error identifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticDef {
    /// Human-readable description of the diagnostic category.
    pub description: String,
}

/// Declarative schema entry for a named proof kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProofCatalogDef {
    /// Purpose of the proof kind.
    #[serde(default)]
    pub description: Option<String>,
    /// Recursive schema accepted for this proof payload.
    #[serde(default)]
    pub schema: Option<SchemaDef>,
}

/// A gate rejection destination for scope and public-symbol findings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RejectRouting {
    /// Gate that receives unadmitted path or implementation findings.
    #[serde(default)]
    pub scope: Option<String>,
    /// Gate that receives unadmitted public-interface findings.
    #[serde(default)]
    pub symbols: Option<String>,
}

/// A slot binding resolved through the profile guidance catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuidanceSlotBinding {
    /// Declared gate guidance slot.
    pub slot: String,
    /// Selected catalog value for the slot.
    pub value: String,
}

/// Commands selected declaratively from source path patterns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PatternCommands {
    /// Glob patterns mapped to executable command strings.
    pub by_pattern: BTreeMap<String, String>,
    /// Optional explanation shared by all pattern-matched commands.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional per-command timeout in seconds.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

/// Supported persistence hooks surrounding one gate transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PersistencePolicy {
    /// Snapshot or commit captured before a gate starts.
    pub pre: Option<PersistenceAction>,
    /// Snapshot or commit created after a gate passes.
    pub post: Option<PersistenceAction>,
    /// Whether the working tree must remain identical to the reviewed snapshot.
    pub immutable: bool,
    /// Whether a rejected gate restores its pre-gate snapshot.
    pub restore_on_reject: bool,
    /// Named telemetry or command policy attached as a Git note.
    pub git_commit_note: Option<String>,
}

/// A persistence action supported by the gate profile schema.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceAction {
    /// Capture an immutable tree snapshot.
    Snapshot,
    /// Create a Git commit.
    Commit,
}

/// Resource bounds for profile-controlled metadata and trace excerpts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceLimits {
    /// Maximum size of custom task and gate metadata.
    pub max_metadata_bytes: usize,
    /// Maximum size of persisted trace summaries.
    pub max_trace_summary_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_metadata_bytes: 2048,
            max_trace_summary_bytes: 2048,
        }
    }
}

/// Bounds and format for a commit note written by the delivery engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct GitCommitNotePolicy {
    /// Maximum serialized note size; defaults to two KiB.
    pub limits: GitCommitNoteLimits,
    /// Output format used by the note renderer.
    pub format: Option<String>,
    /// Optional named telemetry sections.
    pub sections: BTreeMap<String, bool>,
    /// Optional telemetry renderer configuration used by the v2 profile shape.
    pub telemetry: Option<GitCommitNoteTelemetry>,
}

/// Formatting and section switches for commit-note telemetry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct GitCommitNoteTelemetry {
    /// Output format used by the telemetry renderer.
    pub format: Option<String>,
    /// Telemetry sections included by the renderer.
    pub sections: Vec<BTreeMap<String, bool>>,
}

/// Git note byte limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GitCommitNoteLimits {
    /// Maximum bytes in one rendered Git commit note.
    pub max_bytes: usize,
}

impl Default for GitCommitNoteLimits {
    fn default() -> Self {
        Self { max_bytes: 2048 }
    }
}

/// Architect controls for ordering tasks into deterministic parallel waves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DagOptimizationPolicy {
    /// Whether synthesis may reorder tasks while preserving dependency edges.
    pub enabled: bool,
    /// Wave construction strategy.
    pub strategy: DagOptimizationStrategy,
    /// Place upfront contract tasks before implementation tasks.
    pub prioritize_contracts: bool,
    /// Extract shared helpers before scheduling their consumers.
    pub shared_helpers_first: bool,
    /// Optional upper bound on tasks in one parallel wave.
    pub max_parallel_tasks: Option<u32>,
}

impl Default for DagOptimizationPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: DagOptimizationStrategy::Topological,
            prioritize_contracts: true,
            shared_helpers_first: true,
            max_parallel_tasks: None,
        }
    }
}

/// Supported architect wave optimization strategies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DagOptimizationStrategy {
    /// Preserve dependency order and group disjoint tasks into waves.
    #[default]
    Topological,
}

/// Failure signatures expected from a red-seam or conditional command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ExpectedFailure {
    /// Accepted non-zero process exit codes.
    pub exit_codes: Vec<i32>,
    /// Optional output substring distinguishing a runtime failure from setup errors.
    pub stderr_contains: Option<String>,
}

/// Nested gates used by a task container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TaskGateBlock {
    /// Ordered gate definitions applied to each task.
    pub gates: Vec<GateDef>,
}

/// Named reusable subgraph of gates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct GateTemplate {
    /// Gates instantiated by this template.
    pub gates: Vec<GateDef>,
    /// Optional command override supplied by the parent gate.
    pub command: Option<String>,
    /// Guidance slots attached to the template.
    pub guidance: Vec<String>,
    /// Maximum retry count for this template.
    pub max_attempts: Option<u32>,
}

/// Conditional fallback subgraph for a failed verification gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct GateFallback {
    /// Reusable gate template to invoke.
    #[serde(rename = "use")]
    pub use_template: Option<String>,
    /// Optional command override.
    pub command: Option<String>,
    /// Guidance slots attached to the fallback.
    pub guidance: Vec<String>,
    /// Maximum fallback attempts.
    pub max_attempts: Option<u32>,
}

/// Selected guidance catalog entry for one parameterized gate slot.
pub fn resolve_guidance_slot<'a>(
    gate_slots: &[String],
    catalog: &'a BTreeMap<String, String>,
    slot: &str,
    value: Option<&str>,
    defaults: &BTreeMap<String, String>,
) -> Option<&'a str> {
    if !gate_slots
        .iter()
        .any(|declared| declared == slot || declared.starts_with(&format!("{slot}.")))
    {
        return None;
    }
    let selected = value
        .map(|value| {
            if catalog.contains_key(value) {
                value.to_owned()
            } else {
                format!("{slot}.{value}")
            }
        })
        .or_else(|| defaults.get(slot).cloned());
    selected
        .as_deref()
        .and_then(|key| catalog.get(key))
        .or_else(|| catalog.get(slot))
        .map(String::as_str)
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
        /// Optional execution timeout in seconds.
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    /// Commands selected declaratively from source path patterns.
    Pattern(PatternCommands),
}
impl CommandDef {
    /// Return the executable shell command.
    pub fn command(&self) -> &str {
        match self {
            Self::Script(s) => s,
            Self::Detailed { command, .. } => command,
            Self::Pattern(patterns) => patterns
                .by_pattern
                .values()
                .next()
                .map(String::as_str)
                .unwrap_or_default(),
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

fn glob_pattern_matches(pattern: &str, path: &str) -> bool {
    if let Some(open) = pattern.find('{') {
        if let Some(close) = pattern[open + 1..].find('}') {
            let close = open + 1 + close;
            let prefix = &pattern[..open];
            let suffix = &pattern[close + 1..];
            return pattern[open + 1..close]
                .split(',')
                .any(|choice| glob_pattern_matches(&format!("{prefix}{choice}{suffix}"), path));
        }
    }
    glob_match_bytes(pattern.as_bytes(), path.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], path: &[u8]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    match pattern[0] {
        b'*' if pattern.get(1) == Some(&b'*') => {
            let rest = &pattern[2..];
            if rest.first() == Some(&b'/') && glob_match_bytes(&rest[1..], path) {
                return true;
            }
            (0..=path.len()).any(|index| glob_match_bytes(rest, &path[index..]))
        }
        b'*' => (0..=path.len()).any(|index| glob_match_bytes(&pattern[1..], &path[index..])),
        b'?' => !path.is_empty() && glob_match_bytes(&pattern[1..], &path[1..]),
        byte => path.first() == Some(&byte) && glob_match_bytes(&pattern[1..], &path[1..]),
    }
}

/// Definition of an ACDD lifecycle gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GateDef {
    /// Stable stage identifier persisted in each task.
    #[serde(default)]
    pub id: String,
    /// Role assigned to this gate, if any.
    #[serde(default)]
    pub role: Option<String>,
    /// Named command executed when this is a command gate or an agent side effect.
    #[serde(default)]
    pub command: Option<String>,
    /// Child gates executed by this container gate.
    #[serde(default)]
    pub gates: Vec<GateDef>,
    /// Task lifecycle subgraph attached to a task container.
    #[serde(default)]
    pub tasks: Option<TaskGateBlock>,
    /// Reusable subgraph referenced by this container gate.
    #[serde(default, rename = "use")]
    pub use_template: Option<String>,
    /// Optional predicate used to select a conditional gate.
    #[serde(default)]
    pub when: Option<String>,
    /// Closed proof behavior accepted at this gate.
    #[serde(default)]
    pub proof: ProofDef,
    /// Tools available or recommended for this gate.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Ordered operational steps for this gate.
    #[serde(default)]
    pub steps: Vec<String>,
    /// Guidance catalog slots requested by this gate.
    #[serde(default)]
    pub guidance: Vec<String>,
    /// Named machine policy enforced by this gate.
    #[serde(default)]
    pub policy: Option<String>,
    /// Public-interface symbols admitted for this gate or task boundary.
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Explicit path and symbol review re-routing.
    #[serde(default)]
    pub reject_routing: Option<RejectRouting>,
    /// Snapshot, commit, immutability, and rejection behavior.
    #[serde(default)]
    pub persistence: PersistencePolicy,
    /// Typed failure signature expected from a red seam or conditional command.
    #[serde(default)]
    pub expected_failure: Option<ExpectedFailure>,
    /// Reusable recovery subgraph for a failed gate.
    #[serde(default)]
    pub fallback: Option<GateFallback>,
    /// Optional nested autonomous delivery settings.
    #[serde(default)]
    pub deliver: Option<serde_yaml::Value>,
    /// Maximum retry count for conditional execution.
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// Whether this gate creates a delivery receipt.
    #[serde(default)]
    pub receipt: bool,
    /// Whether handoff may absorb remaining working-tree polish.
    #[serde(default)]
    pub absorb_working_tree: bool,
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

/// Runtime driver selected by the polymorphic gate field matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDriver {
    /// Human or agent role owns gate execution.
    Agent,
    /// A named command owns gate execution.
    Command,
    /// Child gates, task gates, or a reusable template own execution.
    Container,
}

impl GateDef {
    /// Resolve a gate's driver with role taking precedence over command.
    pub fn driver(&self) -> Option<GateDriver> {
        if self.role.is_some() {
            Some(GateDriver::Agent)
        } else if self.command.is_some() {
            Some(GateDriver::Command)
        } else if !self.gates.is_empty()
            || self.tasks.is_some()
            || self.use_template.is_some()
            || (self.receipt && self.proof.kind() == Some(ProofKind::Delivery))
        {
            Some(GateDriver::Container)
        } else {
            None
        }
    }

    /// Return every child gate directly declared by this node.
    pub fn child_gates(&self) -> impl Iterator<Item = &GateDef> {
        self.gates
            .iter()
            .chain(self.tasks.iter().flat_map(|tasks| tasks.gates.iter()))
    }
}

fn gate_is_hierarchical(gate: &GateDef) -> bool {
    gate.command.is_some()
        || gate.use_template.is_some()
        || gate.tasks.is_some()
        || !gate.gates.is_empty()
        || gate.child_gates().any(gate_is_hierarchical)
}

fn gate_has_named_id(gate: &GateDef) -> bool {
    !gate.id.is_empty() || gate.child_gates().any(gate_has_named_id)
}

fn local_gate_reference<'a>(reference: &'a str, path: &str) -> &'a str {
    path.rsplit('.')
        .next()
        .and_then(|parent| reference.strip_prefix(&format!("{parent}.")))
        .unwrap_or(reference)
}

fn normalize_gate_condition(condition: &str) -> Option<&'static str> {
    match condition {
        "code" => Some("code"),
        "doc" | "docs" => Some("doc"),
        "db_migration" | "db" => Some("db_migration"),
        "seam" => Some("seam"),
        "public_api" => Some("public_api"),
        _ => None,
    }
}

fn known_error_code(name: &str) -> bool {
    matches!(
        name,
        "CONTRACT_MATRIX_MISMATCH"
            | "DEPENDENCY_MUTATED"
            | "FEATURE_DISABLED"
            | "FEATURE_REQUIRED_BY_PROFILE"
            | "GATE_INVALID_CONFIGURATION"
            | "MILESTONE_DAG_CYCLE"
            | "MILESTONE_PREFLIGHT_FAILED"
            | "TASK_CANDIDATE_MISMATCH"
            | "TASK_CANDIDATE_OUT_OF_SCOPE"
            | "TASK_SCOPE_INVALID"
            | "TASK_SCOPE_PROTECTED"
            | "TASK_SEAM_DISCONNECTED"
            | "TASK_SEAM_STALE"
            | "UNADMITTED_SYMBOL_EXPANSION"
    )
}

fn collect_guidance_slots(gate: &GateDef, profile: &GateProfile, slots: &mut BTreeSet<String>) {
    for guidance in &gate.guidance {
        if profile.guidance_slots.contains_key(guidance) {
            slots.insert(guidance.clone());
        }
    }
    for child in gate.child_gates() {
        collect_guidance_slots(child, profile, slots);
    }
}
fn default_auto_commit() -> bool {
    true
}

/// Settings and guidance for completed tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CompletedGate {
    /// Tools shown after all configured gates pass.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Steps shown after all configured gates pass.
    #[serde(default)]
    pub steps: Vec<String>,
}

/// A complete declarative ACDD workflow profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct GateProfile {
    /// Schema generation used by profile authors.
    #[serde(default)]
    pub version: Option<u32>,
    /// Definition of done applied to every subtask.
    #[serde(default)]
    pub subtask_dod: Vec<String>,
    /// Review guidance shown to reviewers.
    #[serde(default)]
    pub review_policy: Vec<String>,
    /// Contract proof policies that permit a zero red exit code.
    #[serde(default)]
    pub contract_zero_exit_policies: Vec<String>,
    /// Stable shared ACDD diagnostic catalogue.
    #[serde(default)]
    pub error_codes: BTreeMap<String, DiagnosticDef>,
    /// Default key selected for each declared guidance slot.
    #[serde(default)]
    pub guidance_slots: BTreeMap<String, String>,
    /// Named recursive evidence schemas.
    #[serde(default)]
    pub proofs: BTreeMap<String, ProofCatalogDef>,
    /// Named guidance entries, including dotted slot keys such as `hitl.pro`.
    #[serde(default)]
    pub guidance: BTreeMap<String, String>,
    /// Reusable hierarchical gate subgraphs.
    #[serde(default)]
    pub gate_templates: BTreeMap<String, GateTemplate>,
    /// Global limits for custom metadata and trace summaries.
    #[serde(default)]
    pub limits: ResourceLimits,
    /// Git commit note rendering and byte limit policy.
    #[serde(default)]
    pub git_commit_note: Option<GitCommitNotePolicy>,
    /// Optional dynamic MCP subsystem selection for profile consumers.
    #[serde(default)]
    pub modules: SubsystemRegistry,
    /// Architect-controlled task wave optimization.
    #[serde(default)]
    pub dag_optimization: DagOptimizationPolicy,
    /// Available agent roles and their model guidance.
    #[serde(default)]
    pub roles: BTreeMap<String, RoleDef>,
    /// Registered shell commands usable by command proof gates.
    #[serde(default)]
    pub commands: BTreeMap<String, CommandDef>,
    /// Hierarchical workflow policy values.
    #[serde(default)]
    pub policies: BTreeMap<String, PolicyDef>,
    /// Named ordered sets of review contours.
    #[serde(default)]
    pub contours: IndexMap<String, IndexMap<String, ContourDef>>,
    /// Ordered workflow gates.
    #[serde(default)]
    pub gates: Vec<GateDef>,
    /// Guidance displayed after completion.
    #[serde(default)]
    pub completed: CompletedGate,
}

impl GateProfile {
    /// Return the gate identifier at a runtime index.
    pub fn gate_id(&self, index: usize) -> &str {
        self.gates
            .get(index)
            .map(|g| g.id.as_str())
            .unwrap_or("completed")
    }
    /// Return a gate's runtime index by its persisted identifier.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.gates.iter().position(|g| g.id == id)
    }
    /// Return a gate by its persisted identifier.
    pub fn by_stage(&self, stage: &str) -> Option<&GateDef> {
        self.gates.iter().find(|g| g.id == stage)
    }
    /// Return the role name and role definition assigned to a gate.
    pub fn role_for_stage(&self, stage: &str) -> Option<(&str, &RoleDef)> {
        let role_name = self.by_stage(stage)?.role.as_deref()?;
        Some((role_name, self.roles.get(role_name)?))
    }
    /// Return a configured contour set, defaulting to `standard`.
    pub fn contour_set(&self, name: Option<&str>) -> Option<&IndexMap<String, ContourDef>> {
        self.contours.get(name.unwrap_or("standard"))
    }

    /// Resolve manifest guidance bindings and return fallback warnings for unbound slots.
    pub fn validate_guidance_bindings(
        &self,
        bindings: &BTreeMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut declared = BTreeSet::new();
        for gate in &self.gates {
            collect_guidance_slots(gate, self, &mut declared);
        }
        let unknown: Vec<_> = bindings
            .keys()
            .filter(|slot| !declared.contains(*slot))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            bail!(
                "ACDD_GUIDANCE_SLOT_INVALID: manifest binds undeclared guidance slot(s): {}",
                unknown.join(", ")
            );
        }
        let mut warnings = Vec::new();
        for slot in declared {
            let binding = bindings.get(&slot).map(String::as_str);
            let default = self.guidance_slots.get(&slot).map(String::as_str);
            let resolved = binding.or(default);
            if binding.is_none() {
                warnings.push(format!(
                    "ACDD_GUIDANCE_SLOT_UNBOUND: slot '{slot}' uses profile default{}",
                    default.map(|key| format!(" '{key}'")).unwrap_or_default()
                ));
            }
            let Some(key) = resolved else {
                continue;
            };
            let catalog_key = if self.guidance.contains_key(key) {
                key.to_owned()
            } else {
                format!("{slot}.{key}")
            };
            if !self.guidance.contains_key(&catalog_key) {
                bail!("ACDD_GUIDANCE_SLOT_INVALID: guidance slot '{slot}' references missing catalog entry '{key}'");
            }
        }
        Ok(warnings)
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
                let resolved = if self.guidance.contains_key(requested) {
                    self.guidance.get(requested).map(String::as_str)
                } else {
                    resolve_guidance_slot(
                        &gate.guidance,
                        &self.guidance,
                        requested,
                        bindings.get(requested).map(String::as_str),
                        &self.guidance_slots,
                    )
                }?;
                Some((requested.clone(), resolved.to_owned()))
            })
            .collect()
    }

    /// Validate the fully merged profile before it becomes active.
    pub fn validate(&self) -> Result<()> {
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
            if let Some(role) = &gate.role {
                if !self.roles.contains_key(role) {
                    bail!(
                        "ACDD_PROFILE_INVALID: gate '{}' references undefined role '{role}'",
                        gate.id
                    );
                }
            }
            if let Some(set) = &gate.contours {
                if self.contours.get(set).is_none() {
                    bail!(
                        "ACDD_PROFILE_INVALID: gate '{}' references undefined contour set '{set}'",
                        gate.id
                    );
                }
            }
            gate.proof.validate_scheme()?;
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
        for (name, command) in &self.commands {
            if name.trim().is_empty() || !command.is_valid() {
                bail!(
                    "ACDD_PROFILE_INVALID: commands registry names and commands must be non-empty"
                );
            }
        }
        for (set_name, contours) in self.contours.iter() {
            for (name, contour) in contours.iter() {
                if name.trim().is_empty()
                    || contour.description.trim().is_empty()
                    || contour.criteria.is_empty()
                    || contour
                        .criteria
                        .iter()
                        .any(|criterion| criterion.trim().is_empty())
                {
                    bail!("ACDD_PROFILE_INVALID: contour '{set_name}.{name}' requires a description and operational criteria");
                }
            }
        }
        Ok(())
    }

    fn validate_catalogs(&self) -> Result<()> {
        for (name, policy) in &self.policies {
            if name.trim().is_empty() {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: policies contains an empty name");
            }
            policy.validate(&format!("policies.{name}"))?;
        }
        for (name, diagnostic) in &self.error_codes {
            if !known_error_code(name) {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: unknown error code '{name}'");
            }
            if diagnostic.description.trim().is_empty() {
                bail!(
                    "ACDD_PROFILE_SCHEMA_INVALID: error_codes.{name}.description must not be empty"
                );
            }
        }
        for (slot, key) in &self.guidance_slots {
            if slot.trim().is_empty() || !self.guidance.contains_key(key) {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: guidance_slots.{slot} references missing guidance '{key}'");
            }
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
        if self.limits.max_metadata_bytes == 0 || self.limits.max_metadata_bytes > 2048 {
            bail!("ACDD_PROFILE_SCHEMA_INVALID: limits.max_metadata_bytes must be 1..=2048");
        }
        if self.limits.max_trace_summary_bytes == 0 || self.limits.max_trace_summary_bytes > 2048 {
            bail!("ACDD_PROFILE_SCHEMA_INVALID: limits.max_trace_summary_bytes must be 1..=2048");
        }
        if let Some(policy) = &self.git_commit_note {
            if policy.limits.max_bytes == 0 || policy.limits.max_bytes > 2048 {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: git_commit_note.limits.max_bytes must be 1..=2048");
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
        if self
            .dag_optimization
            .max_parallel_tasks
            .is_some_and(|count| count == 0)
        {
            bail!(
                "ACDD_PROFILE_SCHEMA_INVALID: dag_optimization.max_parallel_tasks must be positive"
            );
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

        self.validate_gate_sequence(&self.gates, "gates", true)?;
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
        }
        Ok(())
    }

    fn validate_contours(&self) -> Result<()> {
        for (set_name, contours) in self.contours.iter() {
            for (name, contour) in contours.iter() {
                if name.trim().is_empty()
                    || contour.description.trim().is_empty()
                    || contour.criteria.is_empty()
                    || contour
                        .criteria
                        .iter()
                        .any(|criterion| criterion.trim().is_empty())
                {
                    bail!("ACDD_PROFILE_INVALID: contour '{set_name}.{name}' requires a description and operational criteria");
                }
            }
        }
        Ok(())
    }

    fn validate_guidance_refs(&self, requested: &[String], owner: &str) -> Result<()> {
        for key in requested {
            if !self.guidance.contains_key(key) && !self.guidance_slots.contains_key(key) {
                bail!("GATE_INVALID_CONFIGURATION: {owner} references undefined guidance slot or catalog entry '{key}'");
            }
        }
        Ok(())
    }

    fn hierarchical_gate_reference_count(&self, reference: &str) -> usize {
        fn collect(gates: &[GateDef], parent: &str, paths: &mut Vec<String>) {
            for gate in gates {
                let path = if gate.id.is_empty() {
                    parent.to_owned()
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
        collect(&self.gates, "gates", &mut paths);
        for (name, template) in &self.gate_templates {
            collect(&template.gates, &format!("templates.{name}"), &mut paths);
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
            }
        }

        let deliveries: Vec<_> = gates
            .iter()
            .enumerate()
            .filter(|(_, gate)| gate.proof.kind() == Some(ProofKind::Delivery))
            .collect();
        if (require_terminal_delivery && deliveries.len() != 1)
            || deliveries
                .iter()
                .any(|(index, _)| *index + 1 != gates.len())
            || deliveries.len() > 1
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
        if let Some(routing) = &gate.reject_routing {
            for (field, reference) in [
                ("scope", routing.scope.as_deref()),
                ("symbols", routing.symbols.as_deref()),
            ] {
                if let Some(reference) = reference {
                    let local = local_gate_reference(reference, path);
                    if prior.get(local).is_none() {
                        match self.hierarchical_gate_reference_count(reference) {
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
            && !gate
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            bail!(
                "GATE_INVALID_CONFIGURATION: gate id '{}' must be a non-empty token",
                gate.id
            );
        }
        let Some(driver) = gate.driver() else {
            bail!("GATE_INVALID_CONFIGURATION: gate '{}' must declare role, command, gates, tasks, or use", gate.id);
        };
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
            if !self.contours.contains_key(contours) {
                bail!("GATE_INVALID_CONFIGURATION: gate '{}' references undefined contour set '{contours}'", gate.id);
            }
        }
        self.validate_guidance_refs(&gate.guidance, &format!("gate '{}'", gate.id))?;
        if gate
            .when
            .as_deref()
            .is_some_and(|condition| normalize_gate_condition(condition).is_none())
        {
            bail!("GATE_INVALID_CONFIGURATION: gate '{}' has unsupported when condition '{}'; expected code, doc, db_migration, seam, or public_api", gate.id, gate.when.as_deref().unwrap_or_default());
        }
        for symbol in &gate.symbols {
            super::validate_symbol_ref(symbol).with_context(|| {
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
        gate.proof.validate_scheme()?;
        if let Some(expected) = &gate.expected_failure {
            if expected.exit_codes.is_empty() || expected.exit_codes.contains(&0) {
                bail!("ACDD_PROFILE_SCHEMA_INVALID: gate '{}' expected_failure.exit_codes must contain non-zero values", gate.id);
            }
        }
        if driver == GateDriver::Command && gate.command.is_none() {
            bail!(
                "GATE_INVALID_CONFIGURATION: command gate '{}' requires command",
                gate.id
            );
        }
        if let Some(fallback) = &gate.fallback {
            if fallback.command.is_none() && fallback.use_template.is_none() {
                bail!(
                    "GATE_INVALID_CONFIGURATION: gate '{}' fallback requires a command or template",
                    gate.id
                );
            }
            if let Some(template) = &fallback.use_template {
                if !self.gate_templates.contains_key(template) {
                    bail!("GATE_INVALID_CONFIGURATION: gate '{}' fallback references undefined template '{template}'", gate.id);
                }
            }
            if let Some(command) = &fallback.command {
                if !self.commands.contains_key(command) {
                    bail!("GATE_INVALID_CONFIGURATION: gate '{}' fallback references undefined command '{command}'", gate.id);
                }
            }
            if fallback.max_attempts.is_some_and(|attempts| attempts == 0) {
                bail!(
                    "ACDD_PROFILE_SCHEMA_INVALID: gate '{}' fallback.max_attempts must be positive",
                    gate.id
                );
            }
            self.validate_guidance_refs(
                &fallback.guidance,
                &format!("gate '{}' fallback", gate.id),
            )?;
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
                    if let Some(template) = gate
                        .fallback
                        .as_ref()
                        .and_then(|fallback| fallback.use_template.as_ref())
                    {
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

    /// Merge overrides on top of this base profile. Gates replace as a whole sequence.
    pub fn merge_override(&mut self, other: GateProfile) {
        if other.version.is_some() {
            self.version = other.version;
        }
        if !other.subtask_dod.is_empty() {
            self.subtask_dod = other.subtask_dod;
        }
        if !other.review_policy.is_empty() {
            self.review_policy = other.review_policy;
        }
        if !other.contract_zero_exit_policies.is_empty() {
            self.contract_zero_exit_policies = other.contract_zero_exit_policies;
        }
        self.error_codes.extend(other.error_codes);
        self.guidance_slots.extend(other.guidance_slots);
        self.proofs.extend(other.proofs);
        self.guidance.extend(other.guidance);
        self.gate_templates.extend(other.gate_templates);
        if other.limits != ResourceLimits::default() {
            self.limits = other.limits;
        }
        if other.git_commit_note.is_some() {
            self.git_commit_note = other.git_commit_note;
        }
        if other.modules != SubsystemRegistry::default() {
            self.modules = other.modules;
        }
        if other.dag_optimization != DagOptimizationPolicy::default() {
            self.dag_optimization = other.dag_optimization;
        }
        for (key, role) in other.roles {
            if let Some(current) = self.roles.get_mut(&key) {
                current.merge_override(role);
            } else {
                self.roles.insert(key, role);
            }
        }
        self.commands.extend(other.commands);
        for (key, value) in other.policies {
            let current = self.policies.entry(key).or_default();
            if value.description.is_some() {
                current.description = value.description;
            }
            for (setting, value) in value.settings {
                merge_yaml(current.settings.entry(setting).or_default(), value);
            }
        }
        self.contours.extend(other.contours);
        if !other.gates.is_empty() {
            self.gates = other.gates;
        }
        if !other.completed.tools.is_empty() {
            self.completed.tools = other.completed.tools;
        }
        if !other.completed.steps.is_empty() {
            self.completed.steps = other.completed.steps;
        }
    }
}

fn merge_yaml(base: &mut serde_yaml::Value, override_value: serde_yaml::Value) {
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

/// Parse an ACDD profile YAML document. Validation is applied after overrides merge.
pub fn parse_profile(yaml_str: &str) -> Result<GateProfile> {
    serde_yaml::from_str(yaml_str).map_err(|err| anyhow::anyhow!(
        "ACDD_PROFILE_SCHEMA_INVALID: {err}\nSupported fields: version, gates, gate_templates, roles, commands, policies, proofs, error_codes, guidance, guidance_slots, modules, contours, limits, git_commit_note, completed, subtask_dod, review_policy, contract_zero_exit_policies."
    ))
}

/// Merge and validate one profile document against the embedded base profile.
pub fn validate_profile_document(yaml_str: &str) -> Result<GateProfile> {
    let overrides = parse_profile(yaml_str)?;
    let mut profile = (*compiled()).clone();
    profile.merge_override(overrides);
    profile.validate()?;
    Ok(profile)
}

static COMPILED_PROFILE: OnceLock<Arc<GateProfile>> = OnceLock::new();
/// Compile and return the embedded default workflow profile.
pub fn compiled() -> Arc<GateProfile> {
    COMPILED_PROFILE
        .get_or_init(|| {
            let profile = parse_profile(DEFAULT_PROFILE_YAML)
                .expect("embedded acdd.default.yaml must be valid");
            profile
                .validate()
                .expect("embedded acdd.default.yaml must compile");
            profile
                .validate_language_agnostic_defaults()
                .expect("embedded acdd.default.yaml must use language-agnostic command dispatch");
            Arc::new(profile)
        })
        .clone()
}

/// Split a pinned profile reference into a relative path and optional SHA prefix.
pub fn parse_profile_reference(
    reference: &str,
    require_pin: bool,
) -> Result<(PathBuf, Option<String>)> {
    let (path, pin) = match reference.rsplit_once(':') {
        Some((path, pin)) if !path.is_empty() && !pin.is_empty() => {
            (path, Some(pin.to_ascii_lowercase()))
        }
        _ => (reference, None),
    };
    if require_pin && pin.is_none() {
        bail!("ACDD_PROFILE_PIN_REQUIRED: use '<path>:<sha256_prefix>'");
    }
    if let Some(pin) = &pin {
        if pin.len() < 7 || !pin.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("ACDD_PROFILE_PIN_INVALID: SHA-256 prefix must contain at least 7 hexadecimal characters");
        }
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("ACDD_PROFILE_PATH_INVALID: profile path must be relative and confined to the workspace");
    }
    Ok((path.to_path_buf(), pin))
}

fn load_profile_path(root: &Path, reference: &str, require_pin: bool) -> Result<GateProfile> {
    let (relative, pin) = parse_profile_reference(reference, require_pin)?;
    let path = crate::core::tasks::confined_path(root, &relative.to_string_lossy())?;
    let contents = std::fs::read(&path)
        .with_context(|| format!("ACDD_PROFILE_NOT_FOUND: {}", path.display()))?;
    if let Some(prefix) = pin {
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(&contents));
        if !digest.starts_with(&prefix) {
            bail!("TASK_PROFILE_TAMPERED: profile SHA-256 does not match pinned prefix '{prefix}'");
        }
    }
    let text = std::str::from_utf8(&contents)
        .context("ACDD_PROFILE_SCHEMA_INVALID: profile must be UTF-8 YAML")?;
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
pub fn load_for_task(
    root: &Path,
    task_profile: Option<&str>,
    milestone_profile: Option<&str>,
) -> Result<Arc<GateProfile>> {
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
        let text = std::fs::read_to_string(&forge_config)
            .with_context(|| format!("failed to read {}", forge_config.display()))?;
        let value: serde_yaml::Value = serde_yaml::from_str(&text)
            .with_context(|| format!("invalid YAML in {}", forge_config.display()))?;
        if let Some(reference) = value.get("acdd_profile") {
            let reference = reference
                .as_str()
                .context("acdd_profile in forge-mcp.yaml must be a relative file path string")?;
            base.merge_override(load_profile_path(root, reference, false)?);
            base.validate()?;
            return Ok(Arc::new(base));
        }
    }
    let repository_profile = crate::core::tasks::confined_path(root, ".forge/acdd/profile.yaml")?;
    if repository_profile.is_file() {
        let text = std::fs::read_to_string(&repository_profile)
            .with_context(|| format!("failed to read {}", repository_profile.display()))?;
        base.merge_override(
            parse_profile(&text)
                .with_context(|| format!("invalid profile in {}", repository_profile.display()))?,
        );
    }
    base.validate()?;
    Ok(Arc::new(base))
}
