//! Shared, pure response formatting and presentation compaction.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

mod coordination;
mod discovery;
pub(crate) mod lifecycle;
mod traversal;
pub use discovery::{DiscoveryKind, file_uri_for_path};

/// Selects the wire presentation without changing the semantic response value.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[schemars(rename = "OutputFormat")]
pub enum OutputFormat {
    /// Readable Markdown is the default for human-facing output.
    #[default]
    Markdown,
    /// Machine-readable JSON compatible with previous output.
    Json,
}

impl OutputFormat {
    /// Parses the exact protocol spellings accepted by CLI and MCP.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "markdown" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "invalid format {value:?}; expected 'markdown' or 'json'"
            )),
        }
    }
}

/// Describes the semantic operation whose result is being presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    /// Indexed navigation, analysis, documentation, and source results.
    Navigation,
    /// Read-only SQL result values, which are opaque user data.
    Sql,
    /// Durable task lifecycle data and guidance.
    Task,
    /// Built-in guidance text.
    Guide,
    /// User-owned checkpoint content.
    Checkpoint,
    /// Other structured values with no compaction policy.
    General,
}

/// Formats one resolved semantic value for the requested surface.
pub fn render(value: &Value, format: OutputFormat, title: &str) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
        OutputFormat::Markdown => match discovery_kind_for_title(title) {
            Some(kind) => discovery::render(value, title, kind),
            None if lifecycle::handles_title(title) => lifecycle::render(value, title),
            None => match title {
                "task_blackboard" => coordination::render_blackboard(value, title),
                "search_docs" | "get_doc" => coordination::render_docs(value, title),
                _ => render_markdown(value, title),
            },
        },
    }
}

/// Formats a checkpoint result using the request's explicit action.
pub fn render_checkpoint(value: &Value, format: OutputFormat, title: &str, action: &str) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
        OutputFormat::Markdown => coordination::render_checkpoint(value, title, action),
    }
}

fn discovery_kind_for_title(title: &str) -> Option<DiscoveryKind> {
    match title {
        "code_map_overview" => Some(DiscoveryKind::Overview),
        "code_map_inspect" => Some(DiscoveryKind::Inspect),
        "code_map_search" => Some(DiscoveryKind::Search),
        "ast_grep_search" => Some(DiscoveryKind::Ast),
        _ => None,
    }
}

/// Formats a discovery result using its semantic operation identity.
///
/// The generic renderer remains the compatibility path for every other operation
/// and for JSON. Discovery layouts consume only the evidence supplied by the
/// operation adapter; this leaf module does not open files or query the index.
pub fn render_presented(
    value: &Value,
    format: OutputFormat,
    title: &str,
    discovery: Option<DiscoveryKind>,
) -> String {
    match (format, discovery) {
        (OutputFormat::Markdown, Some(kind)) => discovery::render(value, title, kind),
        _ => render(value, format, title),
    }
}

/// Renders one operation with an admitted domain layout when its semantic
/// operation is known. JSON and opaque SQL results retain the shared generic
/// projection.
pub fn render_for_operation(
    value: &Value,
    format: OutputFormat,
    title: &str,
    discovery: Option<DiscoveryKind>,
    operation: OperationKind,
) -> String {
    if format == OutputFormat::Markdown
        && operation == OperationKind::Navigation
        && discovery.is_none()
    {
        if let Some(kind) = traversal::kind_for(title, value) {
            return traversal::render(value, title, kind);
        }
    }
    render_presented(value, format, title, discovery)
}

pub(crate) fn ast_legacy_error(diagnostic: &Value) -> String {
    discovery::ast_legacy_error(diagnostic)
}

/// Applies only presentation compaction permitted for a known operation.
///
/// SQL rows, task records, guidance, checkpoint content, and unknown values are
/// opaque. Navigation compaction is restricted to known result envelopes and
/// exact server-owned freshness metadata; it never walks arbitrary user fields.
pub fn compact(value: &mut Value, operation: OperationKind, explain: Option<bool>) {
    if operation != OperationKind::Navigation {
        return;
    }
    shorten_metadata_hashes(value);
    let Some(root) = value.as_object_mut() else {
        return;
    };

    if let Some(explain) = explain {
        compact_symbol(root, explain);
    }

    let source_continuation = root
        .get("source_preview")
        .and_then(Value::as_object)
        .is_some_and(|preview| {
            preview.get("has_more") == Some(&Value::Bool(true))
                || preview
                    .get("next_source_offset")
                    .and_then(Value::as_u64)
                    .is_some_and(|offset| offset > 0)
        });

    let mut active_nested_continuation = false;
    let mut saw_page = false;
    let mut all_pages_empty = true;
    for key in known_page_fields() {
        if let Some(page) = root.get_mut(*key).and_then(Value::as_object_mut) {
            if is_paging_envelope(page) {
                saw_page = true;
                all_pages_empty &= empty_page(page);
                active_nested_continuation |= page.get("has_more") == Some(&Value::Bool(true))
                    || page
                        .get("next_offset")
                        .is_some_and(|offset| !offset.is_null());
            }
            compact_navigation_page(page);
        }
    }

    if !source_continuation && !active_nested_continuation {
        root.remove("generation");
    }

    compact_freshness(root);
    if saw_page
        && all_pages_empty
        && !source_continuation
        && root
            .get("freshness")
            .and_then(Value::as_object)
            .is_some_and(|freshness| {
                freshness.get("status") == Some(&Value::String("matched".into()))
                    && freshness.get("refresh") == Some(&Value::String("none".into()))
            })
    {
        root.remove("freshness");
    }
    move_owned_metadata_to_end(root);
}

/// Formats and orders verified server-owned metadata at the end of a result.
/// Kept as the compatibility entry point used by existing MCP callers.
pub fn finalize_metadata(value: &mut Value) {
    compact(value, OperationKind::Navigation, None);
}

/// Shortens a verified display hash, preserving UTF-8 boundaries.
pub fn shorten_hash(hash: &str) -> &str {
    const SHORT_HASH_LEN: usize = 8;
    if hash.len() > SHORT_HASH_LEN {
        hash.get(..SHORT_HASH_LEN).unwrap_or(hash)
    } else {
        hash
    }
}

/// Applies the legacy hash shortening helper only to explicit presentation metadata.
/// Active continuation generations and nested/user values remain untouched.
pub fn shorten_metadata_hashes(value: &mut Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    if let Some(Value::Object(metadata)) = root.get_mut("metadata") {
        if is_tool_metadata(metadata) {
            shorten_fields(metadata, &["output_root", "corpus_hash"]);
        }
    }
    if let Some(Value::Object(freshness)) = root.get_mut("freshness") {
        if is_freshness_envelope(freshness) {
            shorten_fields(freshness, &["output_root", "corpus_hash"]);
        }
    }
}

fn render_markdown(value: &Value, title: &str) -> String {
    let mut output = format!("# {}\n", title);
    match value {
        Value::Object(object) if !object.is_empty() => {
            for (key, child) in ordered_fields(object) {
                render_field(&mut output, key, child, 0);
            }
        }
        other => render_value(&mut output, other, 0),
    }
    output
}

fn render_field(output: &mut String, key: &str, value: &Value, depth: usize) {
    match value {
        Value::Object(object) if !object.is_empty() => {
            indent(output, depth);
            output.push_str("- **");
            output.push_str(&escape_label(key));
            output.push_str("**\n");
            for (child_key, child) in ordered_fields(object) {
                render_field(output, child_key, child, depth + 1);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            indent(output, depth);
            output.push_str("- **");
            output.push_str(&escape_label(key));
            output.push_str("**\n");
            for (index, item) in items.iter().enumerate() {
                render_field(output, &format!("[{index}]"), item, depth + 1);
            }
        }
        scalar => {
            indent(output, depth);
            output.push_str("- **");
            output.push_str(&escape_label(key));
            output.push_str("**: ");
            output.push_str(&inline_code(&json_scalar(scalar)));
            output.push('\n');
        }
    }
}

fn render_value(output: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(object) if !object.is_empty() => {
            for (key, child) in object {
                render_field(output, key, child, depth);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, item) in items.iter().enumerate() {
                render_field(output, &format!("[{index}]"), item, depth);
            }
        }
        scalar => {
            indent(output, depth);
            output.push_str(&inline_code(&json_scalar(scalar)));
            output.push('\n');
        }
    }
}

fn ordered_fields(object: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut fields = object
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
    fields.sort_unstable_by_key(|(left, _)| *left);
    fields
}

fn json_scalar(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn inline_code(value: &str) -> String {
    let mut longest = 0;
    let mut current = 0;
    for character in value.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    let delimiter = "`".repeat(longest + 1);
    let padding = value.starts_with('`')
        || value.ends_with('`')
        || value.starts_with(' ')
        || value.ends_with(' ');
    if padding {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

fn indent(output: &mut String, depth: usize) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('*', "\\*")
}

fn known_page_fields() -> &'static [&'static str] {
    &[
        "components",
        "languages",
        "coverage_by_workspace",
        "nodes",
        "items",
        "matches",
        "documented_in",
        "governing_invariants",
        "analysis_gaps",
        "incoming",
        "outgoing",
        "calls",
        "callers",
        "implements",
        "implementors",
        "overrides",
        "dependencies",
        "unwired",
        "untested_public_surface",
        "doc_diagnostics",
        "referenced_symbol_candidates",
    ]
}

fn compact_navigation_page(page: &mut Map<String, Value>) {
    compact_navigation_items(page);
    if !is_paging_envelope(page) {
        return;
    }

    let active = page.get("has_more") == Some(&Value::Bool(true))
        || page
            .get("next_offset")
            .is_some_and(|offset| !offset.is_null());
    if active {
        return;
    }

    if page_limited(page) {
        if let Some(Value::String(generation)) = page.get_mut("generation") {
            *generation = shorten_hash(generation).to_owned();
        }
        return;
    }

    if empty_page(page) {
        for field in [
            "offset",
            "limit",
            "has_more",
            "generation",
            "next_offset",
            "continuation_hint",
        ] {
            page.remove(field);
        }
        return;
    }

    page.remove("generation");
    page.remove("continuation_hint");
    if page.get("has_more") == Some(&Value::Bool(false)) {
        page.remove("next_offset");
    }
}

fn empty_page(page: &Map<String, Value>) -> bool {
    !page_limited(page)
        && page.get("has_more") != Some(&Value::Bool(true))
        && (page.get("total").is_some_and(|total| total == 0)
            || page
                .get("items")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty))
}

fn compact_navigation_items(page: &mut Map<String, Value>) {
    if let Some(items) = page.get_mut("items").and_then(Value::as_array_mut) {
        for item in items {
            compact_navigation_item(item);
        }
    }
    if let Some(groups) = page
        .get_mut("grouped_by_file")
        .and_then(Value::as_object_mut)
    {
        for group in groups.values_mut() {
            if let Some(items) = group.as_array_mut() {
                for item in items {
                    compact_navigation_item(item);
                }
            }
        }
    }
}

fn compact_navigation_item(value: &mut Value) {
    let Some(item) = value.as_object_mut() else {
        return;
    };
    if !["id", "kind", "name", "path"]
        .iter()
        .all(|key| item.get(*key).is_some_and(Value::is_string))
    {
        return;
    }
    for flag in ["is_test", "generated"] {
        if let Some(value) = item.get_mut(flag) {
            if value == false || value == 0 {
                item.remove(flag);
            } else if value == true || value.as_i64().is_some_and(|number| number != 0) {
                *value = Value::Bool(true);
            }
        }
    }
    if item.get("qualname") == item.get("name") {
        item.remove("qualname");
    }
}

fn is_paging_envelope(object: &Map<String, Value>) -> bool {
    object.contains_key("offset")
        && object.contains_key("limit")
        && object.get("generation").is_some_and(Value::is_string)
}

fn page_limited(page: &Map<String, Value>) -> bool {
    ["computation_truncated", "byte_limited"]
        .iter()
        .any(|field| page.get(*field) == Some(&Value::Bool(true)))
}

fn compact_symbol(object: &mut Map<String, Value>, explain: bool) {
    for key in ["documents", "coverage"] {
        let empty = object.get(key).is_some_and(is_empty_collection);
        if empty {
            object.remove(key);
        }
    }
    if explain {
        let relations = [
            "calls",
            "callers",
            "implements",
            "implementors",
            "overrides",
            "dependencies",
            "unwired",
            "incoming",
            "outgoing",
        ]
        .into_iter()
        .filter(|key| object.contains_key(*key))
        .collect::<Vec<_>>();
        for key in relations {
            if object.get(key).is_some_and(is_empty_collection) {
                object.remove(key);
            }
        }
        let present = [
            "calls",
            "callers",
            "implements",
            "implementors",
            "overrides",
            "dependencies",
            "unwired",
            "incoming",
            "outgoing",
        ]
        .into_iter()
        .filter(|key| object.contains_key(*key))
        .collect::<Vec<_>>();
        object.insert("relations".into(), serde_json::json!(present));
    }
}

fn is_empty_collection(value: &Value) -> bool {
    value.as_array().is_some_and(Vec::is_empty)
        || value.get("total").is_some_and(|total| total == 0)
        || (value.get("total").is_none()
            && value
                .get("items")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty))
        || value.as_object().is_some_and(Map::is_empty)
}

fn compact_freshness(root: &mut Map<String, Value>) {
    let Some(freshness) = root.get_mut("freshness").and_then(Value::as_object_mut) else {
        return;
    };
    if !is_freshness_envelope(freshness) {
        return;
    }
    freshness.remove("checked_at_unix_ms");
    freshness.remove("inventory_scan_ms");
    if matches!(
        freshness.get("status").and_then(Value::as_str),
        Some("matched" | "source_inventory_matched")
    ) && freshness.get("refresh") == Some(&Value::String("none".into()))
    {
        for field in ["output_root", "corpus_hash", "generation"] {
            freshness.remove(field);
        }
        freshness.insert("status".into(), Value::String("matched".into()));
    }
}

fn is_freshness_envelope(object: &Map<String, Value>) -> bool {
    const ALLOWED: [&str; 7] = [
        "status",
        "output_root",
        "corpus_hash",
        "checked_at_unix_ms",
        "files_checked",
        "inventory_scan_ms",
        "refresh",
    ];
    object.get("status").is_some_and(Value::is_string)
        && object.get("output_root").is_some_and(Value::is_string)
        && object.get("corpus_hash").is_some_and(Value::is_string)
        && object.get("refresh").is_some_and(Value::is_string)
        && object.keys().all(|key| ALLOWED.contains(&key.as_str()))
}

fn is_tool_metadata(object: &Map<String, Value>) -> bool {
    object.get("generation").is_some_and(Value::is_string)
        && object.iter().all(|(key, value)| match key.as_str() {
            "generation" | "status" | "refresh" | "output_root" | "corpus_hash" => {
                value.is_string()
            }
            "files_checked" => value.as_u64().is_some(),
            _ => false,
        })
}

fn shorten_fields(object: &mut Map<String, Value>, fields: &[&str]) {
    for field in fields {
        if let Some(Value::String(hash)) = object.get_mut(*field) {
            *hash = shorten_hash(hash).to_owned();
        }
    }
}

fn move_owned_metadata_to_end(root: &mut Map<String, Value>) {
    let metadata = root
        .get("metadata")
        .and_then(Value::as_object)
        .is_some_and(is_tool_metadata)
        .then(|| root.remove("metadata"))
        .flatten();
    let generation = root.remove("generation");
    let freshness = root.remove("freshness");
    if let Some(metadata) = metadata {
        root.insert("metadata".into(), metadata);
    }
    if let Some(generation) = generation {
        root.insert("generation".into(), generation);
    }
    if let Some(freshness) = freshness {
        root.insert("freshness".into(), freshness);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_keeps_all_values_and_marks_the_operation() {
        let rendered = render(
            &json!({"count":0,"complete":false,"items":["row",null]}),
            OutputFormat::Markdown,
            "code_map_query result",
        );
        assert!(rendered.starts_with("# code_map_query result"));
        assert!(rendered.contains("**count**: `0`"));
        assert!(rendered.contains("**complete**: `false`"));
        assert!(rendered.contains("\"row\""));
        assert!(rendered.contains("`null`"));
    }

    #[test]
    fn opaque_operations_are_never_compacted_by_field_name() {
        let value = json!({"generation":"opaque","metadata":{"generation":"user"},"items":[],"complete":false});
        for operation in [
            OperationKind::Sql,
            OperationKind::Task,
            OperationKind::Guide,
            OperationKind::Checkpoint,
            OperationKind::General,
        ] {
            let mut actual = value.clone();
            compact(&mut actual, operation, None);
            assert_eq!(actual, value);
        }
    }

    #[test]
    fn navigation_compaction_keeps_active_tokens_and_false_or_zero() {
        let mut value = json!({
            "count":0,"complete":false,
            "nodes":{"total":1,"offset":0,"limit":1,"has_more":false,"next_offset":null,
                "generation":"inactive-token","items":[{"name":"x"}]},
            "outgoing":{"total":2,"offset":0,"limit":1,"has_more":true,"next_offset":1,
                "generation":"active-token","items":[{"name":"y"}]},
            "freshness":{"status":"source_inventory_matched","output_root":"display-root",
                "corpus_hash":"display-hash","refresh":"none"}
        });
        compact(&mut value, OperationKind::Navigation, None);
        assert!(value.get("generation").is_none());
        assert!(value["nodes"].get("generation").is_none());
        assert_eq!(value["outgoing"]["generation"], "active-token");
        assert_eq!(value["count"], 0);
        assert_eq!(value["complete"], false);
        assert!(value["freshness"].get("output_root").is_none());
    }
}
