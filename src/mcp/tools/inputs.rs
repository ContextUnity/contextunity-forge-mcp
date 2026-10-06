use crate::core::response::{Detail, QueryOptions, ResponsePolicy, SourceOptions};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

fn yes() -> bool {
    true
}
fn depth() -> u32 {
    2
}
fn inbound() -> String {
    "inbound".into()
}

// An empty schema object accepts any JSON value and stays valid for MCP clients.
fn checkpoint_content_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::Map::new().into()
}

#[derive(Default, Deserialize, JsonSchema)]
/// Represents page input data.
pub struct PageInput {
    /// Items per collection, 1..=100; defaults to adapter response.page_size (30).
    pub limit: Option<usize>,
    #[serde(default)]
    /// The offset value.
    pub offset: usize,
    /// Compact omits heavy node details; full remains subject to the byte limit.
    pub detail: Option<Detail>,
    /// Use the generation returned by the previous page when offset is nonzero.
    pub generation: Option<String>,
}
impl PageInput {
    pub(super) fn resolve(&self, policy: &ResponsePolicy) -> anyhow::Result<QueryOptions> {
        QueryOptions::resolve(
            policy,
            self.limit,
            self.offset,
            self.detail,
            self.generation.clone(),
        )
    }
}
#[derive(Default, Deserialize, JsonSchema)]
/// Represents overview input data.
pub struct OverviewInput {
    /// Optional aspects to include: 'counts', 'components', 'languages', 'cycles', 'compiled_profiles', 'metadata'. If omitted, all standard aspects are included.
    pub aspects: Option<Vec<String>>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents selector data.
pub struct Selector {
    /// The selector value.
    pub selector: String,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents inspect data.
pub struct Inspect {
    /// The selector value.
    pub selector: String,
    #[serde(default = "yes")]
    /// Whether show doc applies.
    pub show_doc: bool,
    /// Include paged resolution coverage in addition to the compact summary.
    #[serde(default)]
    pub include_coverage: bool,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional max body lines value.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl Inspect {
    pub(super) fn source(
        &self,
        policy: &ResponsePolicy,
        required: bool,
    ) -> anyhow::Result<SourceOptions> {
        source_options(
            policy,
            self.show_source,
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
            self.page.generation.as_deref(),
            required,
        )
    }
}

#[derive(Deserialize, JsonSchema)]
/// Represents snippet data.
pub struct Snippet {
    /// The selector value.
    pub selector: String,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional max body lines value.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl Snippet {
    pub(super) fn source(&self, policy: &ResponsePolicy) -> anyhow::Result<SourceOptions> {
        source_options(
            policy,
            self.show_source,
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
            self.page.generation.as_deref(),
            true,
        )
    }
}

fn source_options(
    policy: &ResponsePolicy,
    show_source: Option<bool>,
    leading_lines: Option<usize>,
    max_body_lines: Option<usize>,
    source_offset: usize,
    generation: Option<&str>,
    required: bool,
) -> anyhow::Result<SourceOptions> {
    anyhow::ensure!(
        source_offset == 0 || generation.is_some(),
        "source continuation requires generation from the previous preview"
    );
    SourceOptions::resolve(
        policy,
        if required { Some(true) } else { show_source },
        leading_lines,
        max_body_lines,
        source_offset,
    )
}
#[derive(Deserialize, JsonSchema)]
/// Represents explain data.
pub struct Explain {
    /// The selector value.
    pub selector: String,
    /// Optional direction value.
    pub direction: Option<String>,
    #[serde(default = "yes")]
    /// Whether show doc applies.
    pub show_doc: bool,
    /// Include paged resolution coverage in addition to the compact summary.
    #[serde(default)]
    pub include_coverage: bool,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional max body lines value.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned as next_source_offset by a previous preview.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl Explain {
    pub(super) fn source(
        &self,
        policy: &ResponsePolicy,
        required: bool,
    ) -> anyhow::Result<SourceOptions> {
        anyhow::ensure!(
            self.source_offset == 0 || self.page.generation.is_some(),
            "source continuation requires generation from the previous preview"
        );
        SourceOptions::resolve(
            policy,
            if required {
                Some(true)
            } else {
                self.show_source
            },
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
        )
    }
}
#[derive(Deserialize, JsonSchema)]
/// Represents search symbols data.
pub struct SearchSymbols {
    /// The pattern value.
    pub pattern: String,
    /// Match only the complete symbol name or qualified name, without full-text search.
    #[serde(default)]
    pub exact: bool,
    /// Optional kind value.
    pub kind: Option<String>,
    /// Restrict results to a workspace-relative file or directory.
    pub path: Option<String>,
    /// Group results by file path.
    #[serde(default)]
    pub group_by_file: bool,
    /// Include Markdown documentation nodes in the code symbol search.
    #[serde(default)]
    pub include_docs: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents tests data.
pub struct Tests {
    /// The selector value.
    pub selector: String,
    #[serde(default = "inbound")]
    /// The direction value.
    pub direction: String,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents impact data.
pub struct Impact {
    /// The selector value.
    pub selector: String,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default = "depth")]
    /// The depth value.
    pub depth: u32,
    /// Traversal mode: 'calls' (default, invocation/call graph), 'data-flow' (parameter/assignment flow), 'all' (all dependency edges).
    pub mode: Option<String>,
    /// Optional specific edge kinds to follow (e.g. ['calls', 'mutates', 'inherits', 'implements', 'imports']).
    pub edge_types: Option<Vec<String>>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents query data.
pub struct Query {
    /// The operation value.
    pub operation: String,
    /// Optional selector value.
    pub selector: Option<String>,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    pub direction: Option<String>,
    /// Include paged resolution coverage for inspect and explain operations.
    #[serde(default)]
    pub include_coverage: bool,
    #[serde(default = "depth")]
    /// The depth value.
    pub depth: u32,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents analyze data.
pub struct Analyze {
    /// The target value.
    pub target: String,
    /// Whether include cycles applies.
    pub include_cycles: Option<bool>,
    /// Read stored syntax diagnostics only; does not run a linter or reparse source.
    #[serde(default)]
    pub lint: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents ast data.
pub struct Ast {
    /// The pattern value.
    pub pattern: String,
    /// The language value.
    pub language: String,
    /// Optional path value.
    pub path: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents search docs data.
pub struct SearchDocs {
    /// The query value.
    pub query: String,
    /// Optional doc type value.
    pub doc_type: Option<String>,
    /// Optional component value.
    pub component: Option<String>,
    /// Include a short match-centered content excerpt in each result.
    #[serde(default)]
    pub include_excerpt: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents get doc data.
pub struct GetDoc {
    /// The path or id value.
    pub path_or_id: String,
    /// Optional section value.
    pub section: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents guide data.
pub struct Guide {
    #[serde(default)]
    /// Optional topic value.
    pub topic: Option<String>,
    #[serde(default)]
    /// Whether force applies.
    pub force: bool,
}
#[derive(Deserialize, JsonSchema)]
/// Represents checkpoint data.
pub struct Checkpoint {
    /// The action value.
    pub action: String,
    /// Optional name value.
    pub name: Option<String>,
    #[schemars(schema_with = "checkpoint_content_schema")]
    /// Optional content value.
    pub content: Option<Value>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum BlackboardAction {
    Post,
    Read,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Blackboard {
    pub(super) action: BlackboardAction,
    pub(super) task_id: String,
    pub(super) author: Option<String>,
    pub(super) topic: Option<String>,
    pub(super) payload: Option<String>,
    pub(super) limit: Option<usize>,
}
