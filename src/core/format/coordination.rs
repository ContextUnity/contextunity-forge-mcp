use serde_json::{Map, Value};

/// Renders standalone blackboard pages without changing their stored order or evidence.
pub(super) fn render_blackboard(value: &Value, title: &str) -> String {
    let mut output = format!("# {}\n\n", escape_heading(title));
    if let Some(message) = value.get("message") {
        render_message(&mut output, message);
        return output;
    }

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if messages.is_empty() && value.get("pagination").is_none() {
        render_fields(&mut output, value, 0, &[]);
        return output;
    }
    let mut groups: Vec<(String, Vec<&Value>)> = Vec::new();
    for message in messages {
        let topic = message
            .get("topic")
            .and_then(Value::as_str)
            .unwrap_or("(topic unavailable)")
            .to_owned();
        if let Some(index) = groups.iter().position(|(known, _)| known == &topic) {
            groups[index].1.push(message);
        } else {
            groups.push((topic, vec![message]));
        }
    }

    for (topic, entries) in groups {
        output.push_str("## ");
        output.push_str(&escape_heading(&topic));
        output.push_str("\n\n");
        for message in entries {
            if topic == "blockers" {
                output.push_str(&crate::core::format::lifecycle::render_blocker_callout(
                    message,
                ));
                if !output.ends_with("\n\n") {
                    output.push('\n');
                }
            } else {
                render_message(&mut output, message);
            }
        }
    }

    if let Some(pagination) = value.get("pagination") {
        output.push_str("## Pagination\n\n");
        render_fields(&mut output, pagination, 0, &[]);
        output.push('\n');
    }
    output
}

fn render_message(output: &mut String, message: &Value) {
    if message.get("topic").and_then(Value::as_str) == Some("blockers") {
        output.push_str(&crate::core::format::lifecycle::render_blocker_callout(
            message,
        ));
        if !output.ends_with("\n\n") {
            output.push('\n');
        }
        return;
    }

    let id = message
        .get("id")
        .map(scalar)
        .unwrap_or_else(|| "null".into());
    output.push_str("### Message #");
    output.push_str(&id);
    output.push_str("\n\n");
    render_fields(output, message, 0, &["payload", "id", "topic"]);
    if let Some(topic) = message.get("topic") {
        append_field(output, "topic", topic, 0);
    }
    if let Some(payload) = message.get("payload").and_then(Value::as_str) {
        output.push_str("\n**Body**\n\n");
        append_verbatim_block(output, payload);
    } else {
        output.push_str("\nPayload is not included in this summary. Inspect the full message with `task_blackboard(action: \"inspect\", message_id: ");
        output.push_str(&id);
        output.push_str(")`.\n\n");
    }
}

/// Renders search results and document pages from the sections and relation pages supplied by
/// the reader. URI links are attached by the selected workspace adapter before this is called.
pub(super) fn render_docs(value: &Value, title: &str) -> String {
    let mut output = format!("# {}\n\n", escape_heading(title));
    if title == "search_docs" {
        output.push_str("## Search results\n\n");
        render_section_page(&mut output, "Sections", &value["sections"], true);
        render_recognized_freshness(&mut output, value);
        return output;
    }

    render_section_page(&mut output, "Document sections", &value["sections"], false);
    for (field, heading, line_is_coordinate) in [
        ("cites_doc", "Verified document citations", false),
        (
            "cited_by_doc",
            "Verified incoming document citations",
            false,
        ),
        ("referenced_symbols", "Verified referenced symbols", true),
    ] {
        if let Some(page) = value.get(field) {
            render_entity_page(&mut output, heading, page, line_is_coordinate);
        }
    }
    if let Some(candidates) = value.get("referenced_symbol_candidates") {
        output.push_str("## Ambiguous symbol candidates (navigation only; unverified)\n\n");
        render_candidate_page(&mut output, candidates);
    }
    render_recognized_freshness(&mut output, value);
    output
}

fn render_recognized_freshness(output: &mut String, value: &Value) {
    let Some(freshness) = value.get("freshness") else {
        return;
    };
    let Some(object) = freshness.as_object() else {
        return;
    };
    const ALLOWED: [&str; 7] = [
        "status",
        "output_root",
        "corpus_hash",
        "checked_at_unix_ms",
        "files_checked",
        "inventory_scan_ms",
        "refresh",
    ];
    let source_identity = object.get("output_root").is_some_and(Value::is_string)
        && object.get("corpus_hash").is_some_and(Value::is_string);
    let compacted_match = object.get("status").and_then(Value::as_str) == Some("matched")
        && object.get("refresh").and_then(Value::as_str) == Some("none")
        && object
            .get("files_checked")
            .and_then(Value::as_u64)
            .is_some();
    let is_server_envelope = object.get("status").is_some_and(Value::is_string)
        && object.get("refresh").is_some_and(Value::is_string)
        && object.keys().all(|key| ALLOWED.contains(&key.as_str()))
        && (source_identity || compacted_match);
    if is_server_envelope {
        render_field(output, "freshness", freshness, 0);
    }
}

/// Renders a checkpoint according to the explicit action selected by the caller.
pub(super) fn render_checkpoint(value: &Value, title: &str, action: &str) -> String {
    match action {
        "list" => {
            let mut output = format!("# {}\n\n## Checkpoints\n\n", escape_heading(title));
            if let Some(entries) = value.as_object() {
                for (name, entry) in entries {
                    output.push_str("- **");
                    output.push_str(&escape_label(name));
                    output.push_str("**: ");
                    output.push_str(&scalar(entry.get("bytes").unwrap_or(&Value::Null)));
                    output.push_str(" bytes\n");
                }
            } else {
                append_verbatim_block(&mut output, &scalar(value));
            }
            output
        }
        "save" | "delete" => {
            let mut output = format!("# {}\n\n", escape_heading(title));
            render_fields(&mut output, value, 0, &[]);
            output
        }
        // A get result is arbitrary user-owned JSON. Keep it on the lossless generic path.
        "get" => render_generic(value, title),
        _ => render_generic(value, title),
    }
}

fn render_section_page(output: &mut String, heading: &str, page: &Value, excerpts: bool) {
    render_page_metadata(output, heading, page);
    let items = page.get("items").and_then(Value::as_array);
    if let Some(items) = items {
        for section in items {
            let title = section
                .get("section_title")
                .and_then(Value::as_str)
                .or_else(|| section.get("path").and_then(Value::as_str))
                .unwrap_or("Document section");
            output.push_str("### ");
            output.push_str(&escape_heading(title));
            output.push_str("\n\n");
            render_location(output, section);
            render_fields(
                output,
                section,
                0,
                &["content", "excerpt", "uri", "path", "line", "end_line"],
            );
            if let Some(content) = section.get("content").and_then(Value::as_str) {
                output.push_str("\n#### Source Markdown\n\n");
                output.push_str(content);
                if !content.ends_with('\n') {
                    output.push('\n');
                }
                output.push('\n');
            }
            if excerpts {
                if let Some(excerpt) = section.get("excerpt").and_then(Value::as_str) {
                    output.push_str("**Match excerpt**\n\n");
                    append_verbatim_block(output, excerpt);
                }
            }
        }
    }
}

fn render_entity_page(output: &mut String, heading: &str, page: &Value, line_is_coordinate: bool) {
    render_page_metadata(output, heading, page);
    let Some(items) = page.get("items").and_then(Value::as_array) else {
        render_fields(output, page, 0, &[]);
        return;
    };
    for (index, item) in items.iter().enumerate() {
        output.push_str("### Entry ");
        output.push_str(&(index + 1).to_string());
        output.push_str("\n\n");
        if line_is_coordinate {
            render_location(output, item);
            render_fields(output, item, 0, &["uri", "path", "line", "end_line"]);
        } else {
            render_path_link(output, item);
            render_fields(output, item, 0, &["uri", "path"]);
        }
    }
}

fn render_candidate_page(output: &mut String, page: &Value) {
    render_page_metadata(output, "Candidate page", page);
    let Some(items) = page.get("items").and_then(Value::as_array) else {
        render_fields(output, page, 0, &[]);
        return;
    };
    for (index, candidate) in items.iter().enumerate() {
        output.push_str("### Candidate ");
        output.push_str(&(index + 1).to_string());
        output.push_str("\n\n");
        for (field, label) in [("source", "Claim source"), ("target", "Possible target")] {
            if let Some(entity) = candidate.get(field) {
                output.push_str("**");
                output.push_str(label);
                output.push_str("**\n\n");
                render_location(output, entity);
                render_fields(output, entity, 0, &["uri", "path", "line", "end_line"]);
            }
        }
        render_fields(output, candidate, 0, &["source", "target"]);
    }
}

fn render_page_metadata(output: &mut String, heading: &str, page: &Value) {
    output.push_str("## ");
    output.push_str(&escape_heading(heading));
    output.push_str("\n\n");
    render_fields(output, page, 0, &["items"]);
    if page.get("items").is_some() {
        output.push('\n');
    }
}

fn render_location(output: &mut String, entity: &Value) {
    let path = entity.get("path").and_then(Value::as_str);
    let uri = entity.get("uri").and_then(Value::as_str);
    let line = entity.get("line").and_then(Value::as_u64);
    if let Some(path) = path {
        output.push_str("**Location**: ");
        if let Some(uri) = uri {
            output.push('[');
            output.push_str(&escape_label(path));
            output.push(']');
            output.push('(');
            output.push_str(uri);
            output.push(')');
        } else {
            output.push_str(&inline_code(path));
        }
        if let Some(line) = line {
            output.push_str(" line ");
            output.push_str(&line.to_string());
        }
        if let Some(end_line) = entity.get("end_line").and_then(Value::as_u64) {
            if line.is_some() {
                output.push('–');
                output.push_str(&end_line.to_string());
            } else {
                output.push_str(" through line ");
                output.push_str(&end_line.to_string());
            }
        }
        output.push_str("\n\n");
    }
}

fn render_path_link(output: &mut String, entity: &Value) {
    let Some(path) = entity.get("path").and_then(Value::as_str) else {
        return;
    };
    output.push_str("**Document**: ");
    if let Some(uri) = entity.get("uri").and_then(Value::as_str) {
        output.push('[');
        output.push_str(&escape_label(path));
        output.push_str("](");
        output.push_str(uri);
        output.push(')');
    } else {
        output.push_str(&inline_code(path));
    }
    output.push_str("\n\n");
}

fn render_fields(output: &mut String, value: &Value, depth: usize, excluded: &[&str]) {
    match value {
        Value::Object(object) => {
            for (key, child) in ordered_fields(object) {
                if excluded.contains(&key) {
                    continue;
                }
                render_field(output, key, child, depth);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                render_field(output, &format!("[{index}]"), item, depth);
            }
        }
        scalar_value => {
            output.push_str(&inline_code(&scalar(scalar_value)));
            output.push('\n');
        }
    }
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
        scalar_value => append_field(output, key, scalar_value, depth),
    }
}

fn append_field(output: &mut String, key: &str, value: &Value, depth: usize) {
    indent(output, depth);
    output.push_str("- **");
    output.push_str(&escape_label(key));
    output.push_str("**: ");
    output.push_str(&inline_code(&scalar(value)));
    output.push('\n');
}

fn append_verbatim_block(output: &mut String, text: &str) {
    let fence = "`".repeat(longest_backticks(text).saturating_add(1).max(3));
    output.push_str(&fence);
    output.push_str("text\n");
    output.push_str(text);
    if !text.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&fence);
    output.push_str("\n\n");
}

fn render_generic(value: &Value, title: &str) -> String {
    super::render_markdown(value, title)
}

fn ordered_fields(object: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut fields = object
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
    fields.sort_unstable_by_key(|(key, _)| *key);
    fields
}

fn scalar(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn inline_code(value: &str) -> String {
    let delimiter = "`".repeat(longest_backticks(value) + 1);
    if value.starts_with('`') || value.ends_with('`') {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

fn longest_backticks(value: &str) -> usize {
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
    longest
}

fn indent(output: &mut String, depth: usize) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_heading(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}
