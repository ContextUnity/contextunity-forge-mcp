use super::catalog::SubsystemRegistry;
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, OnceLock},
};

const DEFAULT_PROFILE_YAML: &str = include_str!("../acdd.default.yaml");
mod load;
mod normalize;
#[cfg(test)]
mod tests;
mod validate;

use load::load_profile_path;
pub use load::{load_for_task, load_pinned, load_pinned_over, parse_profile_reference};
pub(crate) use normalize::combine_gate_conditions;
use normalize::*;

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
pub struct AgentDef {
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
    /// Optional external agent runner template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    /// Optional prompt assembled from the active task and role context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_template: Option<String>,
}

/// Compatibility name for existing Rust callers of the agent definition.
pub type RoleDef = AgentDef;

/// Execution configuration projected from an existing role declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerConfig {
    /// Command template configured on the role.
    pub runner: String,
    /// Optional role prompt template, preserved for the runner adapter.
    pub prompt_template: Option<String>,
}

impl RunnerConfig {
    /// Project an existing role into an execution configuration when it has a runner.
    ///
    /// The role declaration remains the wire-format authority; this value is a
    /// runtime projection and does not change profile serialization.
    pub fn from_role(role: &RoleDef) -> Option<Self> {
        role.runner.as_ref().map(|runner| Self {
            runner: runner.clone(),
            prompt_template: role.prompt_template.clone(),
        })
    }
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
        if other.runner.is_some() {
            self.runner = other.runner;
        }
        if other.prompt_template.is_some() {
            self.prompt_template = other.prompt_template;
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

impl ProofKind {
    /// Returns the lowercase string identifier for the proof kind.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::Command => "command",
            Self::Review => "review",
            Self::SeamTest => "seam_test",
            Self::Delivery => "delivery",
            Self::None => "none",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "contract" => Some(Self::Contract),
            "command" => Some(Self::Command),
            "review" => Some(Self::Review),
            "seam_test" => Some(Self::SeamTest),
            "delivery" => Some(Self::Delivery),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// Policy for object properties not explicitly described by a schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AdditionalPropertiesDef {
    /// Accept or reject properties without an explicit schema.
    Boolean(bool),
    /// Validate each property not declared in `properties` against this schema.
    Schema(Box<SchemaDef>),
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
    /// Policy or schema for object properties absent from `properties`.
    #[serde(default, rename = "additionalProperties")]
    pub additional_properties: Option<AdditionalPropertiesDef>,
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
    pub(super) fn validate_definition(&self, path: &str) -> Result<()> {
        match self.schema_type.as_str() {
            "object" => {
                if self.items.is_some() {
                    bail!("ACDD_PROFILE_SCHEMA_INVALID: {path}.items is only valid for arrays");
                }
                if let Some(AdditionalPropertiesDef::Schema(schema)) =
                    self.additional_properties.as_ref()
                {
                    schema.validate_definition(&format!("{path}.additionalProperties"))?;
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
                if !self.properties.is_empty()
                    || !self.required.is_empty()
                    || self.additional_properties.is_some()
                {
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
                if !self.properties.is_empty()
                    || !self.required.is_empty()
                    || self.items.is_some()
                    || self.additional_properties.is_some()
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
                } else {
                    match &self.additional_properties {
                        Some(AdditionalPropertiesDef::Boolean(false)) => {
                            bail!("TASK_EVIDENCE_INVALID: {path}.{name} is not allowed");
                        }
                        Some(AdditionalPropertiesDef::Schema(schema)) => {
                            schema.validate_value(item, &format!("{path}.{name}"))?;
                        }
                        Some(AdditionalPropertiesDef::Boolean(true)) | None => {}
                    }
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
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ProofDef {
    /// One of the built-in proof behaviors.
    Kind(ProofKind),
    /// Recursive evidence schema under `proof: { scheme: ... }`.
    Scheme {
        /// Schema used to validate evidence at this gate.
        scheme: Box<SchemaDef>,
    },
    /// Reference to a schema in the profile-level `proofs` catalog.
    Named(String),
    /// Named catalog proof with the schema resolved during profile normalization.
    ResolvedNamed {
        /// Catalog entry name retained for profile serialization.
        name: String,
        /// Resolved schema consumed by gate proof validation.
        schema: Box<SchemaDef>,
    },
    /// Proof was omitted in YAML and awaits automatic inference.
    #[default]
    Unspecified,
}

impl<'de> Deserialize<'de> for ProofDef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Null,
            Name(String),
            Scheme { scheme: Box<SchemaDef> },
        }

        match Repr::deserialize(deserializer)? {
            Repr::Null => Ok(Self::Unspecified),
            Repr::Name(name) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(serde::de::Error::custom(
                        "proof catalog name cannot be empty",
                    ));
                }
                Ok(ProofKind::from_name(name)
                    .map(Self::Kind)
                    .unwrap_or_else(|| Self::Named(name.to_owned())))
            }
            Repr::Scheme { scheme } => Ok(Self::Scheme { scheme }),
        }
    }
}

impl Serialize for ProofDef {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Kind(kind) => serializer.serialize_str(kind.as_str()),
            Self::Named(name) | Self::ResolvedNamed { name, .. } => serializer.serialize_str(name),
            Self::Scheme { scheme } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("scheme", scheme)?;
                map.end()
            }
            Self::Unspecified => serializer.serialize_none(),
        }
    }
}

impl ProofDef {
    /// Return the built-in proof kind, or `None` for a schema proof.
    pub fn kind(&self) -> Option<ProofKind> {
        match self {
            Self::Kind(kind) => Some(*kind),
            Self::Scheme { .. } => None,
            Self::Named(_) => None,
            Self::ResolvedNamed { .. } => None,
            Self::Unspecified => Some(ProofKind::None),
        }
    }
    /// Validate the schema definition for a scheme proof.
    pub fn validate_scheme(&self) -> Result<()> {
        if let Self::Scheme { scheme } = self {
            scheme.validate_definition("proof.scheme")?;
        }
        Ok(())
    }

    /// Resolve a named proof against the profile catalog, retaining inline schemas.
    pub fn resolve_schema<'a>(
        &'a self,
        proofs: &'a BTreeMap<String, ProofCatalogDef>,
    ) -> Option<&'a SchemaDef> {
        normalize::resolve_proof_schema(self, proofs)
    }

    /// Return the catalog name when this proof references a reusable schema.
    pub fn catalog_name(&self) -> Option<&str> {
        match self {
            Self::Named(name) | Self::ResolvedNamed { name, .. } => Some(name),
            _ => None,
        }
    }

    fn is_unspecified(&self) -> bool {
        matches!(self, Self::Unspecified)
    }
}

/// A guidance entry: either a plain text string or a structured review lens with criteria.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum GuidanceEntry {
    /// Direct text guidance.
    Text(String),
    /// Structured inspection lens with operational criteria.
    Structured {
        /// Short purpose statement shown to reviewers or agents.
        description: String,
        /// Concrete checks required for this guidance or contour.
        #[serde(default)]
        criteria: Vec<String>,
    },
}

impl GuidanceEntry {
    /// Return the textual guidance or structured description.
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) => text.as_str(),
            Self::Structured { description, .. } => description.as_str(),
        }
    }

    /// Render guidance with each operational criterion as a bullet point.
    pub fn rendered_text(&self) -> String {
        let mut text = self.text().to_owned();
        for criterion in self.criteria() {
            text.push_str("\n- ");
            text.push_str(criterion);
        }
        text
    }

    /// Return operational criteria if this is a structured guidance entry.
    pub fn criteria(&self) -> &[String] {
        match self {
            Self::Text(_) => &[],
            Self::Structured { criteria, .. } => criteria.as_slice(),
        }
    }
}

impl From<String> for GuidanceEntry {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for GuidanceEntry {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
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
        if limit == 0
            || (limit > forge_core::config::MAX_METADATA_BYTES as u64 && name != "max_log_bytes")
            || (name == "max_log_bytes" && limit > forge_core::response::MAX_OUTPUT_BYTES as u64)
        {
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
    /// Optional output parsing rules shared by the selected commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<CommandOutputSpec>,
}

/// Supported persistence hooks surrounding one gate transition.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct PersistencePolicy {
    /// Snapshot or commit captured before a gate starts.
    pub pre: Option<PersistenceAction>,
    /// Snapshot or commit created after a gate passes.
    pub post: Option<PersistenceAction>,
    /// Whether the working tree must remain identical to the reviewed snapshot.
    pub immutable: bool,
    /// Named telemetry or command policy attached as a Git note.
    pub git_commit_note: Option<String>,
}

impl<'de> Deserialize<'de> for PersistencePolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct StructuredPolicy {
            #[serde(default)]
            pre: Option<PersistenceAction>,
            #[serde(default)]
            post: Option<PersistenceAction>,
            #[serde(default)]
            immutable: bool,
            #[serde(default)]
            git_commit_note: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Action(PersistenceAction),
            Structured(StructuredPolicy),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Action(action) => Ok(PersistencePolicy {
                post: Some(action),
                ..Default::default()
            }),
            Helper::Structured(s) => Ok(PersistencePolicy {
                pre: s.pre,
                post: s.post,
                immutable: s.immutable,
                git_commit_note: s.git_commit_note,
            }),
        }
    }
}

/// A persistence action supported by the gate profile schema.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceAction {
    /// Capture an immutable tree snapshot.
    Snapshot,
    /// Create a Git commit.
    Commit,
    /// Write run traces to structured execution logs.
    #[serde(alias = "log")]
    Logfile,
    /// Ephemeral in-memory execution.
    None,
}

/// Resource bounds for profile-controlled metadata and trace excerpts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceLimits {
    /// Maximum cleaned diagnostic size before retaining a raw log.
    pub diagnostic_inline_bytes: usize,
    /// Maximum accepted runtime process timeout override in seconds.
    pub max_command_timeout_seconds: u64,
    /// Maximum size of custom task and gate metadata.
    pub max_metadata_bytes: usize,
    /// Maximum size of persisted trace summaries.
    pub max_trace_summary_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            diagnostic_inline_bytes: 4096,
            max_command_timeout_seconds: 1800,
            max_metadata_bytes: forge_core::config::MAX_METADATA_BYTES,
            max_trace_summary_bytes: forge_core::config::MAX_METADATA_BYTES,
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
        Self {
            max_bytes: forge_core::config::MAX_METADATA_BYTES,
        }
    }
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
    /// Conditional selector for this entire task track (e.g. `when: code`, `when: doc`).
    pub when: Option<String>,
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

/// Regular expressions used to extract test result counts from command output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandCountsSpec {
    /// Pattern identifying the number of passed tests.
    pub passed: String,
    /// Pattern identifying the number of failed tests.
    pub failed: String,
}

/// Regular expressions used to extract individual test outcomes and identities from command output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandTestsSpec {
    /// Regular expression matching an individual passed test and capturing its test reference or name in capture group 1.
    pub passed: String,
    /// Optional regular expression matching an individual failed test and capturing its test reference or name in capture group 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// Optional regular expression matching an individual ignored or skipped test and capturing its test reference or name in capture group 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignored: Option<String>,
    /// Optional regular expression identifying the active test suite, target, or file in output or command.
    /// Capture group 1 extracts the suite/target/file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Declarative parsing and diagnostic rules for a registered command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandOutputSpec {
    /// Optional patterns for extracting test counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<CommandCountsSpec>,
    /// Whether the command must report test counts.
    #[serde(default = "default_require_counts")]
    pub require_counts: bool,
    /// Patterns for lines omitted from diagnostic output.
    #[serde(default)]
    pub noise: Vec<String>,
    /// Patterns identifying diagnostic error lines.
    #[serde(default)]
    pub errors: Vec<String>,
    /// Optional pattern identifying the seam in the command string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seam_ref_pattern: Option<String>,
    /// Optional rules for extracting individual test outcomes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<CommandTestsSpec>,
}

fn default_require_counts() -> bool {
    true
}

impl Default for CommandOutputSpec {
    fn default() -> Self {
        Self {
            counts: None,
            require_counts: true,
            noise: Vec::new(),
            errors: Vec::new(),
            seam_ref_pattern: None,
            tests: None,
        }
    }
}

impl CommandOutputSpec {
    fn validate(&self) -> Result<()> {
        let mut patterns: Vec<(&str, &str)> = self
            .noise
            .iter()
            .map(|p| ("noise", p.as_str()))
            .chain(self.errors.iter().map(|p| ("errors", p.as_str())))
            .collect();
        if let Some(counts) = &self.counts {
            patterns.push(("counts.passed", &counts.passed));
            patterns.push(("counts.failed", &counts.failed));
        }
        if let Some(pattern) = &self.seam_ref_pattern {
            patterns.push(("seam_ref_pattern", pattern));
        }
        if let Some(tests) = &self.tests {
            patterns.push(("tests.passed", &tests.passed));
            if let Some(failed) = &tests.failed {
                patterns.push(("tests.failed", failed));
            }
            if let Some(ignored) = &tests.ignored {
                patterns.push(("tests.ignored", ignored));
            }
            if let Some(target) = &tests.target {
                patterns.push(("tests.target", target));
            }
        }
        for (field, pattern) in patterns {
            let regex = regex::Regex::new(pattern)
                .with_context(|| format!("ACDD_PROFILE_INVALID: invalid output.{field} regex"))?;
            if (field.starts_with("counts.")
                || field == "seam_ref_pattern"
                || field == "tests.passed"
                || field == "tests.failed"
                || field == "tests.target")
                && regex.captures_len() < 2
            {
                bail!("ACDD_PROFILE_INVALID: output.{field} regex requires capture group 1");
            }
        }
        Ok(())
    }
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
        /// Project-path patterns mapped to scoped command arguments.
        #[serde(default)]
        scope_args_by_pattern: BTreeMap<String, String>,
        /// Optional declarative output parsing rules.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<CommandOutputSpec>,
    },
    /// Commands selected declaratively from source path patterns.
    Pattern(PatternCommands),
}
pub(crate) fn glob_pattern_matches(pattern: &str, path: &str) -> bool {
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct GateDef {
    /// Stable stage identifier persisted in each task.
    #[serde(default)]
    pub id: String,
    /// Role assigned to this gate, if any.
    #[serde(default, rename = "agent", alias = "role")]
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
    /// Original gate predicate before a task track predicate is composed into it.
    #[doc(hidden)]
    #[serde(skip)]
    pub declared_when: Option<String>,
    /// Closed proof behavior accepted at this gate.
    #[serde(default, skip_serializing_if = "ProofDef::is_unspecified")]
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
    #[serde(default, alias = "loop")]
    pub reject_routing: Option<RejectRouting>,
    /// Snapshot, commit, immutability, and rejection behavior.
    #[serde(default, alias = "save")]
    pub persistence: PersistencePolicy,
    /// Typed failure signature expected from a red seam or conditional command.
    #[serde(default)]
    pub expected_failure: Option<ExpectedFailure>,
    /// Maximum retry count for conditional execution.
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// Whether this gate creates a delivery receipt.
    #[serde(default)]
    pub receipt: bool,
    /// Whether handoff may absorb remaining working-tree polish.
    #[serde(default, alias = "absorb")]
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
    /// Target gate to route to when this command or gate execution fails.
    #[serde(default, alias = "fail")]
    pub on_fail: Option<String>,
    /// Target gate to route to when this gate succeeds (defaults to next gate if omitted).
    #[serde(default, alias = "pass")]
    pub on_success: Option<String>,
    /// Prior review gates whose evidence is included in delivery.
    #[serde(default, alias = "need")]
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
    /// YAML fields explicitly supplied for this gate, used while composing profile layers.
    #[doc(hidden)]
    #[serde(skip)]
    pub explicit_fields: BTreeSet<String>,
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
    /// Resolve a gate's driver with command taking precedence over role when bound.
    pub fn driver(&self) -> Option<GateDriver> {
        if self.role.is_some() {
            Some(GateDriver::Agent)
        } else if self.command.is_some() {
            Some(GateDriver::Command)
        } else if !self.gates.is_empty()
            || self.tasks.is_some()
            || self.use_template.is_some()
            || gate_is_delivery(self)
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

pub(crate) fn gate_is_delivery(gate: &GateDef) -> bool {
    gate.proof.kind() == Some(ProofKind::Delivery) || gate.receipt
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
    /// Contract proof policies that permit a zero red exit code.
    #[serde(default)]
    pub contract_zero_exit_policies: Vec<String>,
    /// Named recursive evidence schemas.
    #[serde(default)]
    pub proofs: BTreeMap<String, ProofCatalogDef>,
    /// Named guidance entries, including dotted slot keys such as `hitl.pro` and `contour.<name>`.
    #[serde(default)]
    pub guidance: BTreeMap<String, GuidanceEntry>,
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
    /// Available agent roles and their model guidance.
    #[serde(default, rename = "agents", alias = "roles")]
    pub roles: BTreeMap<String, AgentDef>,
    /// Registered shell commands usable by command proof gates.
    #[serde(default)]
    pub commands: BTreeMap<String, CommandDef>,
    /// Optional project-path patterns mapped to arguments for scoped command guidance.
    #[serde(default)]
    pub scope_args_by_pattern: BTreeMap<String, String>,
    /// Hierarchical workflow policy values.
    #[serde(default)]
    pub policies: BTreeMap<String, PolicyDef>,
    /// Ordered workflow gates.
    #[serde(default)]
    pub gates: Vec<GateDef>,
    /// Guidance displayed after completion.
    #[serde(default)]
    pub completed: CompletedGate,
    /// Default persistence/save mode for the profile (commit, snapshot, logfile, none).
    #[serde(default, alias = "save")]
    pub persistence: Option<PersistenceAction>,
    /// Optional admitted paths for milestone adjustments and polish.
    #[serde(default)]
    pub polish_scope: Vec<String>,
}

impl GateProfile {
    /// Authoritative maximum log bytes for command evidence in this profile.
    pub fn max_log_bytes(&self) -> usize {
        self.policies
            .get("proof")
            .and_then(|p| p.settings.get("command"))
            .and_then(|cmd| cmd.get("max_log_bytes"))
            .and_then(|val| val.as_u64())
            .map(|val| val as usize)
            .unwrap_or(forge_core::response::MAX_OUTPUT_BYTES)
    }

    /// Authoritative maximum metadata bytes for custom task/gate metadata and notes in this profile.
    pub fn max_metadata_bytes(&self) -> usize {
        self.limits.max_metadata_bytes
    }
}

struct ParsedProfile {
    profile: GateProfile,
    inherit_defaults: bool,
    commands_declared: bool,
}

/// Parse an ACDD profile YAML document. Validation is applied after overrides merge.
pub fn parse_profile(yaml_str: &str) -> Result<GateProfile> {
    Ok(parse_profile_document(yaml_str)?.profile)
}

/// Merge and validate one profile document against the embedded base profile.
pub fn validate_profile_document(yaml_str: &str) -> Result<GateProfile> {
    let document = parse_profile_document(yaml_str)?;
    let mut profile = if document.inherit_defaults {
        (*compiled()).clone()
    } else {
        GateProfile::default()
    };
    merge_profile_document(&mut profile, document);
    profile.validate()?;
    Ok(profile)
}

static COMPILED_PROFILE: OnceLock<Arc<GateProfile>> = OnceLock::new();
/// Compile and return the embedded default workflow profile.
pub fn compiled() -> Arc<GateProfile> {
    COMPILED_PROFILE
        .get_or_init(|| {
            let mut profile = parse_profile(DEFAULT_PROFILE_YAML)
                .expect("embedded acdd.default.yaml must be valid");
            normalize_profile(&mut profile);
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

/// Parse a workspace configuration file and return its YAML value and optional `limits.max_metadata_bytes`.
pub fn parse_config_metadata_limit(
    config_path: &Path,
) -> Result<(serde_yaml::Value, Option<usize>)> {
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let value: serde_yaml::Value = serde_yaml::from_str(&text)
        .with_context(|| format!("invalid YAML in {}", config_path.display()))?;
    let limit = if let Some(limits) = value.get("limits") {
        let limits_map = limits.as_mapping().with_context(|| {
            format!(
                "ACDD_PROFILE_SCHEMA_INVALID: limits in {} must be a mapping",
                config_path.display()
            )
        })?;
        limits_map
            .get(serde_yaml::Value::String("max_metadata_bytes".into()))
            .map(|raw_meta| {
                let meta = raw_meta.as_u64().with_context(|| {
                    format!(
                        "ACDD_PROFILE_SCHEMA_INVALID: limits.max_metadata_bytes in {} must be a positive integer",
                        config_path.display()
                    )
                })?;
                usize::try_from(meta).with_context(|| {
                    format!(
                        "ACDD_PROFILE_SCHEMA_INVALID: limits.max_metadata_bytes in {} exceeds usize",
                        config_path.display()
                    )
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok((value, limit))
}

/// Apply workspace configuration `limits.max_metadata_bytes` onto a profile and validate it.
pub fn apply_config_metadata_limit(
    config_path: Option<&Path>,
    mut profile: GateProfile,
) -> Result<Arc<GateProfile>> {
    if let Some(path) = config_path.filter(|path| path.is_file()) {
        let (_, limit) = parse_config_metadata_limit(path)?;
        if let Some(meta) = limit {
            profile.limits.max_metadata_bytes = meta;
        }
    }
    profile.validate()?;
    Ok(Arc::new(profile))
}

/// Split a pinned profile reference into a relative path and optional SHA prefix.
/// Resolve workspace profile links and repository defaults, then validate the merged profile.
pub fn load_for_workspace(root: &Path) -> Result<Arc<GateProfile>> {
    if let Some(settings) = crate::tasks::configuration::current(root) {
        if let Some(config) = settings.config.as_deref() {
            if !config.is_file() {
                bail!(
                    "ACDD_PROFILE_SCHEMA_INVALID: selected workspace config {} does not exist",
                    config.display()
                );
            }
            return apply_config_metadata_limit(Some(config), (*settings.profile).clone());
        }
        return Ok(settings.profile.clone());
    }
    let mut base = (*compiled()).clone();
    let mut forge_max_metadata_bytes = None;
    let forge_config = crate::tasks::configuration::config_path(root);
    if forge_config.is_file() {
        let (value, limit) = parse_config_metadata_limit(&forge_config)?;
        forge_max_metadata_bytes = limit;
        if let Some(reference) = value.get("acdd_profile") {
            let reference = reference
                .as_str()
                .context("acdd_profile in forge-mcp.yaml must be a relative file path string")?;
            merge_profile_document(&mut base, load_profile_path(root, reference, false)?);
            if let Some(meta) = forge_max_metadata_bytes {
                base.limits.max_metadata_bytes = meta;
            }
            base.validate()?;
            return Ok(Arc::new(base));
        }
    }
    let repository_profile = crate::tasks::confined_path(root, ".forge/acdd/profile.yaml")?;
    if repository_profile.is_file() {
        let text = std::fs::read_to_string(&repository_profile)
            .with_context(|| format!("failed to read {}", repository_profile.display()))?;
        let document = parse_profile_document(&text)
            .with_context(|| format!("invalid profile in {}", repository_profile.display()))?;
        merge_profile_document(&mut base, document);
    }
    if let Some(meta) = forge_max_metadata_bytes {
        base.limits.max_metadata_bytes = meta;
    }
    base.validate()?;
    if !base.gates.iter().any(|g| {
        gate_is_delivery(g)
            || g.gates.iter().any(gate_is_delivery)
            || g.tasks
                .as_ref()
                .is_some_and(|t| t.gates.iter().any(gate_is_delivery))
    }) {
        bail!("ACDD_PROFILE_INVALID: a merged workspace profile must contain a terminal delivery gate");
    }
    Ok(Arc::new(base))
}
