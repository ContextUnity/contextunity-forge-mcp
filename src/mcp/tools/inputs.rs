use crate::core::format::OutputFormat;
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
fn impact_depth() -> u32 {
    1
}
fn inbound() -> String {
    "inbound".into()
}

fn aliased_value<'a>(
    canonical: Option<&'a str>,
    aliases: &[Option<&'a str>],
    name: &str,
) -> anyhow::Result<Option<&'a str>> {
    let mut values = canonical
        .into_iter()
        .chain(aliases.iter().copied().flatten());
    let value = values.next();
    anyhow::ensure!(values.next().is_none(), "duplicate field `{name}`");
    Ok(value)
}

fn aliased_string<'a>(
    canonical: &'a str,
    aliases: &[Option<&'a str>],
    name: &str,
) -> anyhow::Result<&'a str> {
    Ok(
        aliased_value((!canonical.is_empty()).then_some(canonical), aliases, name)?
            .unwrap_or(canonical),
    )
}

// An empty schema object accepts any JSON value and stays valid for MCP clients.
fn checkpoint_content_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::Map::new().into()
}

fn output_format_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), Value::String("string".into()));
    schema.insert("enum".into(), serde_json::json!(["markdown", "json"]));
    schema.into()
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
    /// Worktree selector: omit to keep the canonical server root; `.` selects it explicitly. Absolute paths or values containing `/` or `\` are resolved explicitly (relative paths start at the server root) and must name a directory. Otherwise, canonical-path-deduplicated registered Git worktrees and immediate `.worktrees/` children are matched in order: exact name, literal prefix (or zero-normalized numeric milestone prefix), suffix, then substring. The first non-empty tier must be unique (`WORKTREE_AMBIGUOUS`); no match returns `WORKTREE_NOT_FOUND`.
    pub worktree: Option<String>,
    /// Output format. Omit for readable Markdown; choose JSON for machine clients.
    #[serde(default)]
    #[schemars(schema_with = "output_format_schema")]
    pub format: Option<OutputFormat>,
}
impl PageInput {
    pub(super) fn output_format(&self) -> OutputFormat {
        self.format.unwrap_or_default()
    }

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
    /// Optional database aspects: 'counts', 'components', 'languages', 'coverage_by_workspace', 'cycles', 'entrypoints', 'entrypoints_with_tests', 'topology', 'compiled_profiles', or 'metadata'. The discovery adapter also accepts 'discovery', 'manifest_components', 'documentation_hubs', and 'milestones'. 'entrypoints' excludes tests; 'entrypoints_with_tests' includes them. An omitted or empty list selects every database aspect except 'cycles', 'topology', and both entrypoint aspects and includes discovery; unsupported values report all accepted names.
    pub aspects: Option<Vec<String>>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents selector data.
pub struct Selector {
    /// Exact symbol ID or name; path forms include path:name, path:known_kind:name, path:line, and path#Lline.
    pub selector: String,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents inspect data.
pub struct Inspect {
    /// Exact symbol ID or name; path forms include path:name, path:known_kind:name, path:line, and path#Lline.
    pub selector: String,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
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
    /// Optional source body line limit; values clamp to 1..=500, omitted uses the configured default (200 by default).
    pub max_body_lines: Option<usize>,
    /// Body line offset returned by a previous source page. Nonzero offsets require that page's generation explicitly.
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
    /// Exact symbol ID or name; path forms include path:name, path:known_kind:name, path:line, and path#Lline.
    pub selector: String,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
    /// Optional show source value.
    pub show_source: Option<bool>,
    /// Optional leading lines value.
    pub leading_lines: Option<usize>,
    /// Optional smaller source body line limit; omitted requests the complete symbol within the response byte budget.
    pub max_body_lines: Option<usize>,
    /// Body line offset returned by a previous source page. Nonzero offsets require that page's generation explicitly.
    #[serde(default)]
    pub source_offset: usize,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl Snippet {
    pub(super) fn source(&self, policy: &ResponsePolicy) -> anyhow::Result<SourceOptions> {
        anyhow::ensure!(
            self.source_offset == 0 || self.page.generation.is_some(),
            "source continuation requires generation from the previous preview; pass the previous page's generation or restart at source_offset=0"
        );
        SourceOptions::resolve_full_symbol(
            policy,
            self.leading_lines,
            self.max_body_lines,
            self.source_offset,
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
        "source continuation requires generation from the previous preview; pass the previous page's generation or restart at source_offset=0"
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
    /// Exact symbol ID or name; path forms include path:name, path:known_kind:name, path:line, and path#Lline.
    pub selector: String,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
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
    /// Optional source body line limit; values clamp to 1..=500, omitted uses the configured default (200 by default).
    pub max_body_lines: Option<usize>,
    /// Body line offset returned by a previous source page. Nonzero offsets require that page's generation explicitly.
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
            "source continuation requires generation from the previous preview; pass the previous page's generation or restart at source_offset=0"
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
    /// Symbol pattern; query is accepted as an alias. Use '*' with a path or kind scope to list all symbols in that scope.
    #[serde(default)]
    pub pattern: String,
    /// Alternative to pattern; provide exactly one of pattern or query.
    pub query: Option<String>,
    /// Match only the complete symbol name or qualified name, without full-text search.
    #[serde(default)]
    pub exact: bool,
    /// Optional kind value.
    pub kind: Option<String>,
    /// Restrict results to a workspace-relative file or directory; an empty string leaves results unfiltered.
    pub path: Option<String>,
    /// Group results by file path.
    #[serde(default)]
    pub group_by_file: bool,
    /// Include Markdown documentation nodes in the code symbol search.
    #[serde(default)]
    pub include_docs: bool,
    /// Include source previews for up to three hits when the total exceeds three; totals at most three include previews automatically.
    #[serde(default)]
    pub include_preview: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl SearchSymbols {
    pub(super) fn pattern(&self) -> anyhow::Result<&str> {
        aliased_string(&self.pattern, &[self.query.as_deref()], "pattern")
    }
}
#[derive(Deserialize, JsonSchema)]
/// Represents tests data.
pub struct Tests {
    /// Exact symbol ID or name; path forms include path:name, path:known_kind:name, path:line, and path#Lline.
    pub selector: String,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
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
    /// Optional exact symbol ID or name; required when changes is omitted. Provide exactly one of selector or changes.
    pub selector: Option<String>,
    /// Git change view: staged, unstaged, or head. Exactly one of selector or changes is required.
    pub changes: Option<String>,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default = "impact_depth")]
    /// Maximum graph depth; omitted depth defaults to one edge.
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
/// Bounded graph or read-only SQL query. SQL uses the compact canonical tables;
/// join nodes to path_dictionary on path_id and edges to nodes on node_hash.
pub struct Query {
    /// Query operation: 'sql' (default when 'sql' is provided), 'untested', 'implementations', 'slice', or 'unwired'.
    #[serde(default)]
    pub operation: String,
    /// Exact symbol ID or name (path:name, path:known_kind:name, path:line, path#Lline), or the read-only SELECT/WITH SQL statement when operation='sql'.
    pub selector: Option<String>,
    /// Read-only SELECT/WITH SQL statement; implies operation='sql' when operation is omitted.
    /// Schema: nodes(node_id, id, kind, name, qualname, path_id, line, end_line, is_test, language, generated, details, node_hash, owner_path_id); path_dictionary(path_id, path); edges(src_hash, kind, dst_hash, path_id, line, evidence_id, confidence_id, occurrence_count); files(path, status, digest, size, language, is_test, generated, path_hash); doc_sections(doc_id, path, section_title, doc_type, content, invariants, referenced_symbols, is_invariant).
    /// Paths: FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id.
    /// Endpoints: FROM edges e JOIN nodes src ON e.src_hash=src.node_hash JOIN nodes dst ON e.dst_hash=dst.node_hash.
    pub sql: Option<String>,
    /// Alias for selector (or SQL query when operation='sql').
    pub query: Option<String>,
    /// File or directory scope for the untested operation; cannot conflict with selector or path.
    pub target: Option<String>,
    /// Restrict selector resolution to a workspace-relative file or directory.
    pub path: Option<String>,
    /// Impact direction: 'inbound' (default) or 'outbound'.
    pub direction: Option<String>,
    /// Include paged resolution coverage for inspect and explain operations.
    #[serde(default)]
    pub include_coverage: bool,
    #[serde(default = "depth")]
    /// Maximum graph depth for generic query operations; impact defaults to two.
    pub depth: u32,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
#[derive(Deserialize, JsonSchema)]
/// Represents analyze data.
pub struct Analyze {
    /// File or directory target; accepts unified 'path' and 'selector' aliases. Omit target for workspace totals.
    #[serde(default)]
    pub target: String,
    /// Alternative to target for a file, directory, or read-only SQL statement.
    pub path: Option<String>,
    /// Alternative to target or path; omit all three for workspace totals.
    pub selector: Option<String>,
    /// Whether include cycles applies.
    pub include_cycles: Option<bool>,
    /// Read stored syntax diagnostics only; does not run a linter or reparse source.
    #[serde(default)]
    pub lint: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl Analyze {
    pub(super) fn target(&self) -> anyhow::Result<&str> {
        Ok(aliased_value(
            (!self.target.is_empty()).then_some(self.target.as_str()),
            &[self.path.as_deref(), self.selector.as_deref()],
            "target",
        )?
        .unwrap_or(""))
    }
}
#[derive(Deserialize, JsonSchema)]
/// Represents ast data.
pub struct Ast {
    /// AST pattern; accepts 'query' as a unified alias.
    #[serde(default)]
    pub pattern: String,
    /// Alternative to pattern; provide exactly one of pattern or query.
    pub query: Option<String>,
    /// REQUIRED unless 'path' is a single file with a recognized extension (e.g. 'src/lib.rs'). Always pass 'language' (e.g. 'rust', 'python', 'typescript') when 'path' is omitted or is a directory. The language value (e.g. 'python', 'rust', 'typescript'). Optional if 'path' has a recognized file extension.
    pub language: Option<String>,
    /// Optional path value.
    pub path: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl Ast {
    pub(super) fn pattern(&self) -> anyhow::Result<&str> {
        aliased_string(&self.pattern, &[self.query.as_deref()], "pattern")
    }
}
#[derive(Deserialize, JsonSchema)]
/// Represents search docs data.
pub struct SearchDocs {
    /// Documentation query; accepts 'pattern' as a unified alias.
    #[serde(default)]
    pub query: String,
    /// Alternative to query for documentation search terms.
    pub pattern: Option<String>,
    /// Optional doc type value.
    pub doc_type: Option<String>,
    /// Optional documentation component or directory prefix filter; accepts 'path' as a unified alias.
    pub component: Option<String>,
    /// Alternative to component for a documentation component or directory prefix.
    pub path: Option<String>,
    /// Include a short match-centered content excerpt in each result (recommended: true for zero-round-trip triage).
    #[serde(default)]
    pub include_excerpt: bool,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl SearchDocs {
    pub(super) fn query(&self) -> anyhow::Result<&str> {
        aliased_string(&self.query, &[self.pattern.as_deref()], "query")
    }

    pub(super) fn component(&self) -> anyhow::Result<Option<&str>> {
        aliased_value(
            self.component.as_deref(),
            &[self.path.as_deref()],
            "component",
        )
    }
}
#[derive(Deserialize, JsonSchema)]
/// Represents get doc data.
pub struct GetDoc {
    /// Document path or ID; accepts unified 'path' and 'selector' aliases.
    #[serde(default)]
    pub path_or_id: String,
    /// Alternative to path_or_id for an indexed document path or ID.
    pub path: Option<String>,
    /// Alternative to path_or_id or path; provide exactly one document selector.
    pub selector: Option<String>,
    /// Optional exact section title. Omit with detail='compact' to inspect the document outline before reading a section.
    pub section: Option<String>,
    #[serde(flatten)]
    /// The page value.
    pub page: PageInput,
}
impl GetDoc {
    pub(super) fn path_or_id(&self) -> anyhow::Result<&str> {
        aliased_string(
            &self.path_or_id,
            &[self.path.as_deref(), self.selector.as_deref()],
            "path_or_id",
        )
    }
}
#[derive(Deserialize, JsonSchema)]
/// Represents guide data.
pub struct Guide {
    #[serde(default)]
    /// Optional topic: init, adapter, docs, acdd, query, ast, or validate.
    pub topic: Option<String>,
    #[serde(default)]
    /// Whether force applies.
    pub force: bool,
    /// Worktree selector: omit to keep the canonical server root; `.` selects it explicitly. Absolute paths or values containing `/` or `\` are resolved explicitly (relative paths start at the server root) and must name a directory. Otherwise, canonical-path-deduplicated registered Git worktrees and immediate `.worktrees/` children are matched in order: exact name, literal prefix (or zero-normalized numeric milestone prefix), suffix, then substring. The first non-empty tier must be unique (`WORKTREE_AMBIGUOUS`); no match returns `WORKTREE_NOT_FOUND`.
    pub worktree: Option<String>,
    /// Output format. Omit for readable Markdown; choose JSON for machine clients.
    #[serde(default)]
    #[schemars(schema_with = "output_format_schema")]
    pub format: Option<OutputFormat>,
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
    /// Worktree selector: omit to keep the canonical server root; `.` selects it explicitly. Absolute paths or values containing `/` or `\` are resolved explicitly (relative paths start at the server root) and must name a directory. Otherwise, canonical-path-deduplicated registered Git worktrees and immediate `.worktrees/` children are matched in order: exact name, literal prefix (or zero-normalized numeric milestone prefix), suffix, then substring. The first non-empty tier must be unique (`WORKTREE_AMBIGUOUS`); no match returns `WORKTREE_NOT_FOUND`.
    pub worktree: Option<String>,
    /// Output format. Omit for readable Markdown; choose JSON for machine clients.
    #[serde(default)]
    #[schemars(schema_with = "output_format_schema")]
    pub format: Option<OutputFormat>,
}

fn deserialize_task_input<'de, D, T>(
    deserializer: D,
    extract_worktree: bool,
) -> Result<(T, Option<OutputFormat>, Option<String>), D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    use serde::de::Error as _;

    let mut properties = serde_json::Map::<String, Value>::deserialize(deserializer)?;
    let format = properties
        .remove("format")
        .map(serde_json::from_value)
        .transpose()
        .map_err(D::Error::custom)?;
    let worktree = if extract_worktree {
        serde_json::from_value(properties.remove("worktree").unwrap_or(Value::Null))
            .map_err(D::Error::custom)?
    } else {
        None
    };
    let request = serde_json::from_value(Value::Object(properties)).map_err(D::Error::custom)?;
    Ok((request, format, worktree))
}

macro_rules! task_input {
    ($name:ident, $inner:ty) => {
        #[derive(JsonSchema)]
        /// Existing task request fields plus an optional output format.
        #[schemars(deny_unknown_fields)]
        pub struct $name {
            #[serde(flatten)]
            #[schemars(flatten)]
            /// The operation's original typed request fields.
            pub request: $inner,
            /// Output format. Omit for readable Markdown; choose JSON for machine clients.
            #[serde(default)]
            #[schemars(schema_with = "output_format_schema")]
            pub format: Option<OutputFormat>,
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let (request, format, _) =
                    deserialize_task_input::<D, $inner>(deserializer, false)?;
                Ok(Self { request, format })
            }
        }
    };
    ($name:ident, $inner:ty, routing_worktree) => {
        #[derive(JsonSchema)]
        /// Existing task request fields plus an optional worktree selector and output format.
        #[schemars(deny_unknown_fields)]
        pub struct $name {
            #[serde(flatten)]
            #[schemars(flatten)]
            /// The operation's original typed request fields.
            pub request: $inner,
            /// Worktree selector: omit to keep the canonical server root; `.` selects it explicitly. Absolute paths or values containing `/` or `\` are resolved explicitly (relative paths start at the server root) and must name a directory. Otherwise, canonical-path-deduplicated registered Git worktrees and immediate `.worktrees/` children are matched in order: exact name, literal prefix (or zero-normalized numeric milestone prefix), suffix, then substring. The first non-empty tier must be unique (`WORKTREE_AMBIGUOUS`); no match returns `WORKTREE_NOT_FOUND`.
            #[serde(default)]
            pub worktree: Option<String>,
            /// Output format. Omit for readable Markdown; choose JSON for machine clients.
            #[serde(default)]
            #[schemars(schema_with = "output_format_schema")]
            pub format: Option<OutputFormat>,
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let (request, format, worktree) =
                    deserialize_task_input::<D, $inner>(deserializer, true)?;
                Ok(Self {
                    request,
                    worktree,
                    format,
                })
            }
        }
    };
}

task_input!(TaskListInput, crate::engine::tasks::List, routing_worktree);
task_input!(TaskClaimInput, crate::engine::tasks::Claim);
task_input!(
    TaskSubmitInput,
    crate::engine::tasks::Submit,
    routing_worktree
);
task_input!(TaskManageInput, crate::engine::tasks::Manage);
task_input!(
    TaskBlackboardInput,
    crate::engine::tasks::BlackboardRequest,
    routing_worktree
);
