use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum TraversalKind {
    Impact,
    Explain,
    Tests,
    Analyze,
}

pub(super) fn kind_for(title: &str, value: &Value) -> Option<TraversalKind> {
    match title {
        "code_map_impact" => Some(TraversalKind::Impact),
        "code_map_explain" => Some(TraversalKind::Explain),
        "code_map_tests" => Some(TraversalKind::Tests),
        "code_map_analyze" => Some(TraversalKind::Analyze),
        // The graph-query operation shares producers with the dedicated tools.
        // Shape checks keep unrelated query results on the generic path.
        "code_map_query" if value.get("incoming").is_some() || value.get("outgoing").is_some() => {
            Some(TraversalKind::Explain)
        }
        "code_map_query"
            if value.get("direction").is_some()
                && value.get("depth").is_some()
                && value.get("nodes").is_some() =>
        {
            Some(TraversalKind::Impact)
        }
        _ => None,
    }
}

pub(super) fn render(value: &Value, title: &str, kind: TraversalKind) -> String {
    let mut output = format!("# {}\n\n", escape_text(title));
    match kind {
        TraversalKind::Impact => render_impact(&mut output, value),
        TraversalKind::Explain => render_explain(&mut output, value),
        TraversalKind::Tests => render_tests(&mut output, value),
        TraversalKind::Analyze => render_analyze(&mut output, value),
    }
    output
}

fn render_impact(output: &mut String, value: &Value) {
    output.push_str("## Traversal\n\n");
    for field in ["direction", "depth", "mode", "complete"] {
        if let Some(value) = value.get(field) {
            output.push_str(&format!("- **{}:** {}\n", label(field), scalar(value)));
        }
    }
    if value.get("complete") == Some(&Value::Bool(false)) {
        output.push_str("\nTraversal is incomplete within the reported bounds; review unmapped changes and continuation details below.\n");
    }
    if let Some(selector) = value.get("selector") {
        render_symbol(output, "Selected target", selector);
    }
    if let Some(invariants) = value.get("governing_invariants") {
        render_invariants(output, invariants);
    }

    let mut consumed = vec![
        "direction",
        "depth",
        "mode",
        "complete",
        "selector",
        "governing_invariants",
    ];
    for (field, heading) in [
        ("nodes", "Reached nodes"),
        ("changed_symbols", "Changed symbols"),
        ("tests", "Affected tests"),
        ("unmapped_changes", "Unmapped changes"),
        ("affected_docs", "Affected documents"),
    ] {
        if let Some(page) = value.get(field) {
            render_page(output, heading, page, |output, item| {
                if field == "nodes" {
                    render_impact_node(output, item);
                } else if field == "tests" {
                    render_test_item(output, item, value);
                } else {
                    render_record(output, item, "item");
                }
            });
            consumed.push(field);
        }
    }
    if let Some(changes) = value.get("changes") {
        render_changes(output, changes);
        consumed.push("changes");
    }
    render_remaining(output, value, &consumed, 0);
}

fn render_explain(output: &mut String, value: &Value) {
    if let Some(node) = value.get("node") {
        render_symbol(output, "Selected symbol", node);
    }
    if let Some(invariants) = value.get("governing_invariants") {
        render_invariants(output, invariants);
    }
    let mut consumed = vec!["node", "governing_invariants"];
    for field in ["incoming", "outgoing"] {
        if let Some(page) = value.get(field) {
            let heading = if field == "incoming" {
                "Inbound relationships"
            } else {
                "Outbound relationships"
            };
            render_page(output, heading, page, render_edge);
            consumed.push(field);
        }
    }
    for (field, heading) in [
        ("documents", "Verified documentation"),
        ("affected_docs", "Affected documents"),
        ("analysis_gaps", "Analysis gaps"),
    ] {
        if let Some(page) = value.get(field) {
            render_page(output, heading, page, |output, item| {
                render_record(output, item, "document");
            });
            consumed.push(field);
        }
    }
    render_remaining(output, value, &consumed, 0);
}

fn render_tests(output: &mut String, value: &Value) {
    output.push_str("## Test mapping\n\n");
    for field in ["direction", "discovery_method", "unresolved_references"] {
        if let Some(value) = value.get(field) {
            output.push_str(&format!("- **{}:** {}\n", label(field), scalar(value)));
        }
    }
    if let Some(scope) = value.get("scope") {
        output.push_str(&format!("- **Scope:** {}\n", scalar(scope)));
    }
    if value["discovery_method"] == "lexical_fallback" {
        output.push_str(
            "\n> [!NOTE]\n> Lexical fallback is unverified and supplies no graph call path.\n",
        );
    } else if value["discovery_method"] == "graph" {
        output.push_str("\nDiscovery uses verified exact-call graph paths.\n");
    }
    if let Some(count) = value.get("unresolved_references").and_then(Value::as_u64) {
        if count > 0 {
            output.push_str(&format!("\nThe selected symbol scope contains {count} unresolved reference(s), which may conceal dynamic test callers.\n"));
        }
    }
    if let Some(selector) = value.get("selector") {
        render_symbol(output, "Selected symbol", selector);
    }
    let mut consumed = vec![
        "direction",
        "discovery_method",
        "unresolved_references",
        "scope",
        "selector",
    ];
    if let Some(page) = value.get("nodes") {
        render_page(
            output,
            "Related tests and dependencies",
            page,
            |output, item| {
                render_test_item(output, item, value);
            },
        );
        consumed.push("nodes");
    }
    render_remaining(output, value, &consumed, 0);
}

fn render_analyze(output: &mut String, value: &Value) {
    output.push_str("## Analysis\n\n");
    for field in [
        "target",
        "mode",
        "status",
        "scope",
        "total_errors",
        "total_unresolved",
        "total_external_imports",
    ] {
        if let Some(value) = value.get(field) {
            output.push_str(&format!("- **{}:** {}\n", label(field), scalar(value)));
        }
    }

    let mut consumed = vec![
        "target",
        "mode",
        "status",
        "scope",
        "total_errors",
        "total_unresolved",
        "total_external_imports",
    ];
    if let Some(diagnostics) = value.get("diagnostics") {
        render_page(output, "Syntax diagnostics", diagnostics, |output, item| {
            render_record(output, item, "diagnostic");
        });
        consumed.push("diagnostics");
    }
    for (field, heading) in [
        ("untested_public_surface", "Untested public surface"),
        ("errors", "Parse errors"),
        ("resolution", "Unresolved and ambiguous references"),
        ("external_imports", "External references"),
        ("doc_diagnostics", "Documentation diagnostics"),
        ("top_files", "Files with the most recorded issues"),
        ("resolution_statuses", "Resolution statuses"),
        ("resolution_causes", "Resolution causes"),
        ("parse_error_languages", "Parse errors by language"),
    ] {
        if let Some(page) = value.get(field) {
            render_page(output, heading, page, |output, item| {
                render_record(output, item, "record");
            });
            consumed.push(field);
        }
    }
    render_remaining(output, value, &consumed, 0);
}

fn render_symbol(output: &mut String, heading: &str, value: &Value) {
    output.push_str(&format!("## {}\n\n", escape_text(heading)));
    render_record(output, value, "symbol");
}

fn render_record(output: &mut String, value: &Value, fallback: &str) {
    render_record_excluding(output, value, fallback, &[]);
}

fn render_record_excluding(
    output: &mut String,
    value: &Value,
    fallback: &str,
    extra_excluded: &[&'static str],
) {
    if !value.is_object() {
        render_remaining(output, value, &[], 1);
        return;
    }
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| value.get("qualname").and_then(Value::as_str))
        .unwrap_or(fallback);
    let id = value.get("id").and_then(Value::as_str);
    let path = value.get("path").and_then(Value::as_str);
    let line = value.get("line").and_then(Value::as_i64).or_else(|| {
        value
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|line| i64::try_from(line).ok())
    });
    let uri = value.get("uri").and_then(Value::as_str);
    output.push_str("- ");
    if let Some(uri) = uri {
        output.push_str(&format!("[{}]({uri})", escape_link_label(name)));
    } else {
        output.push_str(&format!("**{}**", escape_text(name)));
    }
    if let Some(role) = value.get("test_role").and_then(Value::as_str) {
        output.push_str(&format!(" ({})", inline(role)));
    }
    if let Some(id) = id {
        output.push_str(&format!(" — id {}", inline(id)));
    }
    if let (Some(path), Some(line)) = (path, line) {
        output.push_str(&format!(" — {}", coordinate(path, line, uri)));
    } else if let Some(path) = path {
        output.push_str(&format!(" — {}", inline(path)));
    }
    output.push('\n');

    let mut excluded = record_exclusions(value);
    excluded.extend_from_slice(extra_excluded);
    render_remaining(output, value, &excluded, 1);
}

fn record_exclusions(value: &Value) -> Vec<&'static str> {
    let mut excluded = Vec::new();
    let name = value.get("name").and_then(Value::as_str);
    let qualname = value.get("qualname").and_then(Value::as_str);
    if name.is_some() {
        excluded.push("name");
        if qualname == name {
            excluded.push("qualname");
        }
    } else if qualname.is_some() {
        excluded.push("qualname");
    }
    if value.get("id").and_then(Value::as_str).is_some() {
        excluded.push("id");
    }
    let path = value.get("path").and_then(Value::as_str);
    if path.is_some() {
        excluded.push("path");
    }
    let has_line = value
        .get("line")
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|line| i64::try_from(line).ok())
        })
        .is_some();
    if path.is_some() && has_line {
        excluded.push("line");
    }
    if value.get("uri").and_then(Value::as_str).is_some() {
        excluded.push("uri");
    }
    if value.get("test_role").and_then(Value::as_str).is_some() {
        excluded.push("test_role");
    }
    excluded
}

fn render_impact_node(output: &mut String, value: &Value) {
    let mut consumed = Vec::new();
    if value.get("edge").is_some() {
        consumed.push("edge");
    }
    if value.get("via").is_some() {
        consumed.push("via");
    }
    render_record_excluding(output, value, "reached node", &consumed);
    if let Some(edge) = value.get("edge") {
        render_edge(output, edge);
    }
    if let Some(via) = value.get("via") {
        if let Some(object) = via.as_object() {
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("unknown predecessor");
            let kind = object
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown relation");
            output.push_str(&format!(
                "  - Via {} using {}; this depth-grouped path has no call-site coordinate.\n",
                inline(id),
                inline(kind)
            ));
            render_remaining(output, via, &["id", "kind"], 2);
        } else {
            output.push_str("  - Via details: ");
            render_remaining(output, via, &[], 2);
        }
    }
}

fn render_edge(output: &mut String, edge: &Value) {
    let kind = edge
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("relationship");
    let confidence = edge.get("confidence").and_then(Value::as_str);
    let badge = match (kind, confidence) {
        ("calls", Some("exact")) => Some("[proven call]"),
        ("possible_calls", _) => Some("[heuristic / candidate]"),
        _ => None,
    };
    output.push_str("  - ");
    if let Some(badge) = badge {
        output.push_str(badge);
        output.push(' ');
    }
    output.push_str(&inline(kind));
    if let Some(source) = edge.get("src_public_id").and_then(Value::as_str) {
        output.push_str(&format!(" from {}", inline(source)));
    }
    if let Some(target) = edge.get("dst_public_id").and_then(Value::as_str) {
        output.push_str(&format!(" to {}", inline(target)));
    }
    if let Some(direction) = edge.get("direction").and_then(Value::as_str) {
        output.push_str(&format!(" ({})", escape_text(direction)));
    }
    if let (Some(path), Some(line)) = (
        edge.get("path").and_then(Value::as_str),
        edge.get("line").and_then(Value::as_i64).or_else(|| {
            edge.get("line")
                .and_then(Value::as_u64)
                .and_then(|line| i64::try_from(line).ok())
        }),
    ) {
        output.push_str(&format!(
            " at {}",
            coordinate(path, line, edge.get("uri").and_then(Value::as_str))
        ));
    } else if let Some(path) = edge.get("path").and_then(Value::as_str) {
        output.push_str(&format!(" at {}", inline(path)));
    }
    if let Some(confidence) = confidence {
        output.push_str(&format!("; confidence {}", inline(confidence)));
    }
    if let Some(count) = edge.get("occurrence_count") {
        output.push_str(&format!("; occurrences {}", scalar(count)));
    }
    output.push('\n');
    render_remaining(
        output,
        edge,
        &[
            "kind",
            "confidence",
            "src_public_id",
            "dst_public_id",
            "direction",
            "path",
            "line",
            "uri",
            "occurrence_count",
        ],
        2,
    );
}

fn render_test_item(output: &mut String, item: &Value, response: &Value) {
    render_record_heading(output, item, "test result");
    if let Some(role) = item.get("test_role").and_then(Value::as_str) {
        output.push_str(&format!("  - Supplied role: {}\n", inline(role)));
    }
    let Some(connection) = item.get("connection") else {
        let excluded = record_exclusions(item);
        render_remaining(output, item, &excluded, 1);
        return;
    };
    let relation = connection.get("relation").and_then(Value::as_str);
    if let Some(relation) = relation {
        output.push_str(&format!("  - Relationship: {}\n", inline(relation)));
    }
    let inbound = response.get("direction").and_then(Value::as_str) == Some("inbound");
    let graph = response.get("discovery_method").and_then(Value::as_str) == Some("graph");
    let mut rendered_path = false;
    if inbound && graph {
        if let (Some(ids), Some(nodes)) = (
            connection.get("call_path").and_then(Value::as_array),
            connection
                .get("verified_call_path_nodes")
                .and_then(Value::as_array),
        ) {
            if ids.len() == nodes.len()
                && ids
                    .iter()
                    .zip(nodes)
                    .all(|(id, node)| id.as_str().is_some() && node.get("id") == Some(id))
                && !nodes.is_empty()
            {
                output.push_str("  - Verified call path: ");
                for (index, node) in nodes.iter().enumerate() {
                    if index > 0 {
                        output.push_str(" -> ");
                    }
                    render_path_node(
                        output,
                        node,
                        index + 1 == nodes.len(),
                        &response["selector"]["id"],
                    );
                }
                output.push('\n');
                rendered_path = true;
            }
        }
    }
    let mut connection_excluded = Vec::new();
    if relation.is_some() {
        connection_excluded.push("relation");
    }
    if rendered_path {
        connection_excluded.extend(["call_path", "verified_call_path_nodes"]);
    }
    if connection.as_object().is_some_and(|fields| {
        fields
            .keys()
            .any(|key| !connection_excluded.contains(&key.as_str()))
    }) {
        output.push_str("  - Connection details:\n");
        render_remaining(output, connection, &connection_excluded, 2);
    } else if !connection.is_object() {
        render_remaining(output, connection, &[], 2);
    } else if connection
        .as_object()
        .is_some_and(|fields| fields.is_empty())
    {
        output.push_str("  - Connection is an empty object.\n");
    }
    let mut excluded = record_exclusions(item);
    excluded.push("connection");
    render_remaining(output, item, &excluded, 1);
}

fn render_record_heading(output: &mut String, item: &Value, fallback: &str) {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| item.get("qualname").and_then(Value::as_str))
        .unwrap_or(fallback);
    output.push_str("- ");
    if let Some(uri) = item.get("uri").and_then(Value::as_str) {
        output.push_str(&format!("[{}]({uri})", escape_link_label(name)));
    } else {
        output.push_str(&format!("**{}**", escape_text(name)));
    }
    if let Some(id) = item.get("id").and_then(Value::as_str) {
        output.push_str(&format!(" — id {}", inline(id)));
    }
    if let (Some(path), Some(line)) = (
        item.get("path").and_then(Value::as_str),
        item.get("line").and_then(Value::as_i64).or_else(|| {
            item.get("line")
                .and_then(Value::as_u64)
                .and_then(|line| i64::try_from(line).ok())
        }),
    ) {
        output.push_str(&format!(
            " — {}",
            coordinate(path, line, item.get("uri").and_then(Value::as_str))
        ));
    } else if let Some(path) = item.get("path").and_then(Value::as_str) {
        output.push_str(&format!(" — {}", inline(path)));
    }
    output.push('\n');
}

fn render_path_node(output: &mut String, node: &Value, last: bool, target_id: &Value) {
    let name = node
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| node.get("qualname").and_then(Value::as_str))
        .or_else(|| node.get("id").and_then(Value::as_str))
        .unwrap_or("unavailable node");
    if let Some(uri) = node.get("uri").and_then(Value::as_str) {
        output.push_str(&format!("[{}]({uri})", escape_link_label(name)));
    } else {
        output.push_str(&escape_text(name));
    }
    if let Some(id) = node.get("id").and_then(Value::as_str) {
        output.push_str(&format!(" {}", inline(id)));
    }
    if let (Some(path), Some(line)) = (
        node.get("path").and_then(Value::as_str),
        node.get("line").and_then(Value::as_i64).or_else(|| {
            node.get("line")
                .and_then(Value::as_u64)
                .and_then(|line| i64::try_from(line).ok())
        }),
    ) {
        output.push_str(&format!(
            " at {}",
            coordinate(path, line, node.get("uri").and_then(Value::as_str))
        ));
    } else if let Some(path) = node.get("path").and_then(Value::as_str) {
        output.push_str(&format!(" at {}", inline(path)));
    }
    if let Some(role) = node.get("test_role").and_then(Value::as_str) {
        output.push_str(&format!(" ({})", inline(role)));
    }
    if last
        && node
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| target_id.as_str() == Some(id))
    {
        output.push_str(" (target)");
    }
    render_remaining(output, node, &record_exclusions(node), 2);
}

fn render_changes(output: &mut String, value: &Value) {
    output.push_str("## Git change view\n\n");
    if let Some(files) = value.get("files").and_then(Value::as_array) {
        output.push_str("### Changed files\n\n");
        if files.is_empty() {
            output.push_str("No changed files in this Git view.\n");
        }
        for file in files {
            render_record(output, file, "changed file");
        }
        render_remaining(output, value, &["files"], 0);
    } else {
        render_remaining(output, value, &[], 0);
    }
}

fn render_invariants(output: &mut String, value: &Value) {
    let mut excluded = Vec::new();
    if let Some(rules) = value
        .get("direct")
        .and_then(Value::as_array)
        .filter(|rules| !rules.is_empty())
    {
        excluded.push("direct");
        output.push_str("\n> [!IMPORTANT] Governing Invariants\n");
        for rule in rules {
            let id = rule
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("identifier unavailable");
            let text = rule
                .get("rule")
                .and_then(Value::as_str)
                .unwrap_or("rule text unavailable");
            let source = rule.get("source").unwrap_or(&Value::Null);
            let location = match (
                source.get("path").and_then(Value::as_str),
                source.get("line").and_then(Value::as_i64).or_else(|| {
                    source
                        .get("line")
                        .and_then(Value::as_u64)
                        .and_then(|line| i64::try_from(line).ok())
                }),
            ) {
                (Some(path), Some(line)) => format!(
                    " — {}",
                    coordinate(path, line, source.get("uri").and_then(Value::as_str))
                ),
                (Some(path), None) => format!(" — {}", inline(path)),
                _ => String::new(),
            };
            for (index, line) in text.lines().enumerate() {
                if index == 0 {
                    output.push_str(&format!("> - {}: {}", inline(id), escape_text(line)));
                } else {
                    output.push_str(&format!("\n>   {}", escape_text(line)));
                }
            }
            output.push_str(&location);
            output.push('\n');
            render_remaining(output, rule, &["id", "rule", "source"], 2);
            if let Some(source) = rule.get("source") {
                render_remaining(output, source, &["path", "line", "uri"], 2);
            }
        }
        output.push('\n');
    }
    render_remaining(output, value, &excluded, 0);
}

fn render_page(
    output: &mut String,
    heading: &str,
    page: &Value,
    mut render_item: impl FnMut(&mut String, &Value),
) {
    let items = page.get("items").and_then(Value::as_array);
    if items.is_none_or(Vec::is_empty)
        && page.get("total").and_then(Value::as_u64) == Some(0)
        && page.get("has_more") != Some(&Value::Bool(true))
        && page.get("next_offset").is_none_or(Value::is_null)
        && page.get("omitted") != Some(&Value::Bool(true))
    {
        return;
    }
    output.push_str(&format!("\n## {}\n\n", escape_text(heading)));
    let Some(items) = items else {
        render_remaining(output, page, &[], 0);
        return;
    };
    render_remaining(output, page, &["items"], 0);
    for item in items {
        render_item(output, item);
    }
}

fn render_remaining(output: &mut String, value: &Value, excluded: &[&str], depth: usize) {
    if let Some(object) = value.as_object() {
        for (key, child) in super::ordered_fields(object) {
            if !excluded.contains(&key) {
                super::render_field(output, key, child, depth);
            }
        }
    } else {
        super::render_value(output, value, depth);
    }
}

fn coordinate(path: &str, line: i64, uri: Option<&str>) -> String {
    let location = inline(&format!("{path}:{line}"));
    match uri {
        Some(uri) => format!("[{location}]({uri})"),
        None => location,
    }
}

fn label(value: &str) -> String {
    value
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => escape_text(text),
        _ => inline(&serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())),
    }
}

fn inline(value: &str) -> String {
    super::inline_code(value)
}

fn escape_link_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_text(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}
