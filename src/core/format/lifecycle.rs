//! Pure Markdown presentation for task lifecycle responses.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Whether a public task operation uses the lifecycle presentation.
pub(super) fn handles_title(title: &str) -> bool {
    matches!(
        title,
        "task_list" | "task_claim" | "task_submit" | "task_manage"
    )
}

/// Renders one task response without changing its JSON domain value.
pub(super) fn render(value: &Value, title: &str) -> String {
    match title {
        "task_list" => render_task_list(value),
        "task_claim" => render_context(value, "Task claim"),
        "task_submit" => render_record(value, "task_submit", "Task submission"),
        "task_manage" => render_manage(value),
        _ => render_record(value, title, "Task result"),
    }
}

/// Renders one blocker using the embedded task-context presentation contract.
///
/// Coordination uses this same pure helper for standalone blackboard messages.
pub(crate) fn render_blocker_callout(message: &Value) -> String {
    let id = message
        .get("id")
        .map(scalar_text)
        .unwrap_or_else(|| "unknown".into());
    let author = message
        .get("author")
        .map(scalar_text)
        .unwrap_or_else(|| "unknown".into());
    let mut output = format!("> [!CAUTION] BLOCKER (#{id}) by {author}\n");
    quote_field(&mut output, "Message id", message.get("id"));
    let scope = message
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if message.get("subtask_ref").and_then(Value::as_str).is_some() {
                "subtask"
            } else if message.get("task_id").and_then(Value::as_str).is_some() {
                "task"
            } else if message
                .get("milestone_ref")
                .and_then(Value::as_str)
                .is_some()
            {
                "milestone"
            } else {
                "unknown"
            }
        });
    output.push_str("> Scope: ");
    output.push_str(&inline_code(scope));
    output.push('\n');
    quote_field(&mut output, "Milestone ref", message.get("milestone_ref"));
    quote_field(&mut output, "Task id", message.get("task_id"));
    quote_field(&mut output, "Subtask ref", message.get("subtask_ref"));
    quote_field(&mut output, "Gate", message.get("gate"));
    quote_field(&mut output, "Timestamp", message.get("created_at"));

    if let Some(payload) = message.get("payload").and_then(Value::as_str) {
        append_quoted_body(&mut output, payload);
        if summary_is_truncated(payload) {
            append_recovery_hint(&mut output, message);
        }
    } else {
        output.push_str("> Body not included in this message summary.\n");
        append_recovery_hint(&mut output, message);
    }
    output.push('\n');
    output
}

fn render_manage(value: &Value) -> String {
    if value.get("spec").is_some() {
        return render_record(value, "task_manage", "Task management result");
    }
    if value.get("context_bundle").is_some_and(Value::is_object)
        || (value.get("workflow_guidance").is_some_and(Value::is_object)
            && value.get("task_id").is_some()
            && value.get("allowed_write_scope").is_some())
    {
        return render_context(value, "Task context");
    }
    if value.get("tasks").and_then(Value::as_array).is_some() {
        return render_task_list(value);
    }
    render_record(value, "task_manage", "Task management result")
}

fn render_task_list(value: &Value) -> String {
    let mut output = String::from("# Task list\n");
    let mut state = RenderState::default();
    let Some(object) = value.as_object() else {
        render_field(&mut output, "result", value, 0, &mut state);
        return output;
    };
    if let Some(resolution) = object.get("resolution") {
        output.push_str(&super::coordination::render_resolution_notice(resolution));
    }

    if let Some(tasks) = object.get("tasks").and_then(Value::as_array) {
        output.push_str("\n## Tasks\n");
        if tasks.is_empty() {
            output.push_str("\nNo tasks were returned for this selection.\n");
        } else {
            for task in tasks {
                output.push('\n');
                render_task_row(&mut output, task, &mut state);
            }
        }
    }
    if let Some(workspaces) = object.get("workspaces").and_then(Value::as_object) {
        output.push_str("\n## Workspaces\n");
        for (name, workspace) in ordered_fields(workspaces) {
            output.push_str("\n### ");
            output.push_str(&inline_code(name));
            output.push('\n');
            render_field(&mut output, "workspace", workspace, 0, &mut state);
        }
    }

    let remaining = fields_without(object, &["tasks", "workspaces", "resolution"]);
    if !remaining.is_empty() {
        output.push_str("\n## Listing details\n");
        render_fields(&mut output, &remaining, 0, &mut state);
    }
    output
}

fn render_task_row(output: &mut String, task: &Value, state: &mut RenderState) {
    let Some(object) = task.as_object() else {
        render_field(output, "task", task, 0, state);
        return;
    };
    let id = object
        .get("task_id")
        .map(scalar_text)
        .unwrap_or_else(|| "Task".into());
    output.push_str("### ");
    output.push_str(&inline_code(&id));
    output.push('\n');

    const ROW_FIELDS: &[&str] = &[
        "task_id",
        "target",
        "status",
        "stage",
        "owner",
        "agent_type",
        "rev",
        "depends_on",
        "subtasks",
    ];
    for key in ROW_FIELDS {
        if let Some(value) = object.get(*key) {
            match *key {
                "status" => render_status_checkbox(output, "Task status", value, 0),
                "subtasks" => render_subtask_rows(output, value, state),
                _ => render_field(output, key, value, 0, state),
            }
        }
    }
    let remaining = fields_without(object, ROW_FIELDS);
    if !remaining.is_empty() {
        output.push_str("- **Additional task fields**\n");
        render_fields(output, &remaining, 1, state);
    }
}

fn render_status_checkbox(output: &mut String, label: &str, value: &Value, depth: usize) {
    let status = scalar_text(value);
    bullet_prefix(output, depth);
    output.push('[');
    output.push_str(if status == "completed" { "x" } else { " " });
    output.push_str("] **");
    output.push_str(label);
    output.push_str("**: ");
    output.push_str(&inline_code(&status));
    output.push('\n');
}

fn render_subtask_rows(output: &mut String, value: &Value, state: &mut RenderState) {
    let Some(subtasks) = value.as_array() else {
        render_field(output, "subtasks", value, 0, state);
        return;
    };
    bullet_prefix(output, 0);
    output.push_str("**Subtasks**\n");
    for subtask in subtasks {
        let Some(fields) = subtask.as_object() else {
            render_field(output, "subtask", subtask, 1, state);
            continue;
        };
        let reference = fields
            .get("subtask_ref")
            .map(scalar_text)
            .unwrap_or_else(|| "subtask".into());
        let status = fields.get("status").cloned().unwrap_or(Value::Null);
        bullet_prefix(output, 1);
        output.push('[');
        output.push_str(if status.as_str() == Some("completed") {
            "x"
        } else {
            " "
        });
        output.push_str("] **Subtask**: ");
        output.push_str(&inline_code(&reference));
        if !status.is_null() {
            output.push_str("; **Status**: ");
            output.push_str(&inline_code(&scalar_text(&status)));
        }
        output.push('\n');
        let details = fields_without(fields, &["subtask_ref", "status"]);
        render_fields(output, &details, 2, state);
    }
}

fn render_context(value: &Value, heading: &str) -> String {
    let mut output = format!("# {heading}\n");
    let Some(object) = value.as_object() else {
        let mut state = RenderState::default();
        render_field(&mut output, "result", value, 0, &mut state);
        return output;
    };
    let mut state = RenderState::default();

    render_lifecycle_state(&mut output, object);
    let bundle = object.get("context_bundle").and_then(Value::as_object);
    let contract = bundle
        .and_then(|bundle| bundle.get("contract"))
        .and_then(Value::as_object);
    if let Some(contract) = contract {
        output.push_str("\n## Contract\n");
        let contract_identity_fields = [
            "task_id",
            "stage",
            "status",
            "claim_revision",
            "contract_revision",
            "worker_id",
            "worktree",
        ]
        .into_iter()
        .filter(|field| object.get(*field) == contract.get(*field))
        .collect::<Vec<_>>();
        let contract_fields = fields_without(contract, &contract_identity_fields);
        render_fields(&mut output, &contract_fields, 0, &mut state);
    } else {
        let contract_fields = root_contract_fields(object);
        if !contract_fields.is_empty() {
            output.push_str("\n## Contract\n");
            render_fields(&mut output, &contract_fields, 0, &mut state);
        }
    }

    let guidance_sources = [
        object.get("workflow_guidance"),
        bundle.and_then(|bundle| bundle.get("guidance")),
    ];
    let mut rendered_guidance = Vec::<&Value>::new();
    for source in guidance_sources.into_iter().flatten() {
        let Some(guidance) = source.as_object() else {
            continue;
        };
        if rendered_guidance.contains(&source) {
            continue;
        }
        let heading = if rendered_guidance.is_empty() {
            "Stage guidance"
        } else {
            "Additional stage guidance"
        };
        rendered_guidance.push(source);
        output.push_str(&format!("\n## {heading}\n"));
        let guidance_fields = fields_without(guidance, &["blackboard_messages"]);
        render_fields(&mut output, &guidance_fields, 0, &mut state);
    }

    if let Some(bundle) = bundle {
        let extra = fields_without(
            bundle,
            &["contract", "guidance", "blackboard", "blackboard_info"],
        );
        if !extra.is_empty() {
            output.push_str("\n## Scope and proof context\n");
            render_fields(&mut output, &extra, 0, &mut state);
        }
    }

    let blackboard_sources = [
        object.get("blackboard"),
        bundle.and_then(|bundle| bundle.get("blackboard")),
        object
            .get("workflow_guidance")
            .and_then(|guidance| guidance.get("blackboard_messages")),
        bundle
            .and_then(|bundle| bundle.get("guidance"))
            .and_then(|guidance| guidance.get("blackboard_messages")),
    ];
    let mut rendered_blackboards = Vec::<&Value>::new();
    for source in blackboard_sources.into_iter().flatten() {
        let Some(messages) = source.as_array() else {
            continue;
        };
        if rendered_blackboards.contains(&source) {
            continue;
        }
        rendered_blackboards.push(source);
        render_blackboard(&mut output, messages, &mut state);
    }

    let blackboard_infos = [
        object.get("blackboard_info"),
        bundle.and_then(|bundle| bundle.get("blackboard_info")),
    ];
    let mut rendered_infos = Vec::<&Value>::new();
    for info in blackboard_infos.into_iter().flatten() {
        if rendered_infos.contains(&info) {
            continue;
        }
        rendered_infos.push(info);
        output.push_str("\n## Blackboard summary\n");
        render_field(&mut output, "blackboard_info", info, 0, &mut state);
    }

    let mut skip = vec![
        "task_id",
        "stage",
        "status",
        "claim_revision",
        "contract_revision",
        "worker_id",
        "worktree",
        "digest",
        "milestone_ref",
        "context_bundle",
        "workflow_guidance",
        "blackboard",
        "blackboard_info",
    ];
    if let Some(contract) = contract {
        for (root_key, contract_key) in [
            ("goal", "target"),
            ("proof_policy", "proof_policy"),
            ("allowed_write_scope", "allowed_scope"),
            ("subtasks", "subtasks"),
            ("depends_on", "depends_on"),
            ("applicable_invariants", "invariants"),
        ] {
            if object.get(root_key) == contract.get(contract_key) {
                skip.push(root_key);
            }
        }
    } else {
        skip.extend([
            "goal",
            "proof_policy",
            "allowed_write_scope",
            "subtasks",
            "depends_on",
            "applicable_invariants",
        ]);
    }
    let remaining = fields_without(object, &skip);
    if !remaining.is_empty() {
        output.push_str("\n## Additional task data\n");
        render_fields(&mut output, &remaining, 0, &mut state);
    }
    output
}

fn render_lifecycle_state(output: &mut String, object: &Map<String, Value>) {
    const STATE_FIELDS: &[&str] = &[
        "task_id",
        "milestone_ref",
        "stage",
        "status",
        "claim_revision",
        "contract_revision",
        "worker_id",
        "worktree",
    ];
    let fields = object
        .iter()
        .filter(|(key, _)| STATE_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    if fields.is_empty() {
        return;
    }
    output.push_str("\n## Lifecycle state\n");
    let mut state = RenderState::default();
    render_fields(output, &fields, 0, &mut state);
}

fn root_contract_fields(object: &Map<String, Value>) -> Map<String, Value> {
    let mut fields = Map::new();
    for (target, source) in [
        ("target", "goal"),
        ("proof_policy", "proof_policy"),
        ("allowed_scope", "allowed_write_scope"),
        ("subtasks", "subtasks"),
        ("depends_on", "depends_on"),
        ("invariants", "applicable_invariants"),
    ] {
        if let Some(value) = object.get(source) {
            fields.insert(target.into(), value.clone());
        }
    }
    fields
}

fn render_record(value: &Value, title: &str, heading: &str) -> String {
    let mut output = format!("# {heading}\n");
    let Some(object) = value.as_object() else {
        let mut state = RenderState::default();
        render_field(&mut output, "result", value, 0, &mut state);
        return output;
    };
    let mut state = RenderState::default();
    render_lifecycle_state(&mut output, object);
    if title == "task_manage" && object.get("spec").is_some() {
        output.push_str("\n## Task record\n");
    } else {
        output.push_str("\n## Result details\n");
    }
    let skip = [
        "task_id",
        "milestone_ref",
        "stage",
        "status",
        "claim_revision",
        "contract_revision",
        "worker_id",
        "worktree",
        "digest",
        "_presentation_scope_uris",
    ];
    let task_inspection = title == "task_manage" && object.get("spec").is_some();
    let fields = if task_inspection {
        task_inspection_fields(object, &skip)
    } else {
        fields_without(object, &skip)
    };
    let scope_uris = task_inspection
        .then(|| {
            object
                .get("_presentation_scope_uris")
                .and_then(Value::as_object)
        })
        .flatten();
    if let Some(scope_uris) = scope_uris {
        render_task_fields(&mut output, &fields, scope_uris, 0, &mut state);
    } else {
        render_fields(&mut output, &fields, 0, &mut state);
    }
    output
}

fn task_inspection_fields(object: &Map<String, Value>, skip: &[&str]) -> Map<String, Value> {
    let mut fields = fields_without(object, skip);
    let Some(mut spec) = object.get("spec").and_then(Value::as_object).cloned() else {
        return fields;
    };
    let mut authority = object
        .get("authority_snapshot")
        .and_then(Value::as_object)
        .cloned();

    for (spec_key, authority_key, response_key) in [
        ("target", "task_target", "goal"),
        ("proof_policy", "task_proof_policy", "proof_policy"),
    ] {
        if let Some(value) = spec.get(spec_key) {
            if authority
                .as_ref()
                .and_then(|snapshot| snapshot.get(authority_key))
                == Some(value)
            {
                if let Some(snapshot) = authority.as_mut() {
                    snapshot.remove(authority_key);
                }
            }
            if fields.get(response_key) == Some(value) {
                fields.remove(response_key);
            }
        }
    }

    if let (Some(current), Some(initial), Some(snapshot), Some(allowed)) = (
        spec.get("scope").and_then(Value::as_array),
        fields.get("initial_scope").and_then(Value::as_array),
        authority.as_ref().and_then(|value| value.get("task_scope")),
        fields.get("allowed_write_scope").and_then(Value::as_array),
    ) {
        let current = unique_scope_values(current);
        let initial = unique_scope_values(initial);
        if snapshot
            .as_array()
            .is_some_and(|scope| scope_values_equal(&initial, &unique_scope_values(scope)))
            && scope_values_equal(&unique_scope_values(allowed), &current)
        {
            let presentation_scope = if scope_values_equal(&initial, &current) {
                Some(Value::Array(current.clone()))
            } else if let Some(added) = extended_scope_values(&initial, &current) {
                let mut scope = Map::new();
                scope.insert("initial_scope".into(), Value::Array(initial));
                scope.insert("extended_scope".into(), Value::Array(added));
                Some(Value::Object(scope))
            } else {
                None
            };
            if let Some(scope) = presentation_scope {
                spec.insert("scope".into(), scope);
                fields.remove("initial_scope");
                fields.remove("allowed_write_scope");
                if let Some(snapshot) = authority.as_mut() {
                    snapshot.remove("task_scope");
                }
            }
        }
    }

    if let Some(task_invariants) = spec.get("invariants").and_then(Value::as_array) {
        if authority
            .as_ref()
            .and_then(|snapshot| snapshot.get("task_invariants"))
            == spec.get("invariants")
        {
            let milestone_invariants = authority
                .as_ref()
                .and_then(|snapshot| snapshot.get("milestone_invariants"))
                .and_then(Value::as_array)
                .cloned();
            if let Some(snapshot) = authority.as_mut() {
                snapshot.remove("task_invariants");
            }
            if let Some(applicable) = fields
                .get("applicable_invariants")
                .and_then(Value::as_array)
                .cloned()
            {
                let remaining = applicable
                    .iter()
                    .filter(|invariant| {
                        !task_invariants.contains(invariant)
                            && !milestone_invariants
                                .as_ref()
                                .is_some_and(|items| items.contains(invariant))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if remaining.len() != applicable.len() {
                    if remaining.is_empty() {
                        fields.remove("applicable_invariants");
                    } else {
                        fields.insert("applicable_invariants".into(), Value::Array(remaining));
                    }
                }
            }
        }
    }

    fields.insert("spec".into(), Value::Object(spec));
    if let Some(authority) = authority {
        fields.insert("authority_snapshot".into(), Value::Object(authority));
    }
    fields
}

fn unique_scope_values(values: &[Value]) -> Vec<Value> {
    let mut unique = Vec::with_capacity(values.len());
    for value in values {
        if !unique.contains(value) {
            unique.push(value.clone());
        }
    }
    unique
}

fn scope_values_equal(left: &[Value], right: &[Value]) -> bool {
    left.len() == right.len() && left.iter().all(|value| right.contains(value))
}

fn extended_scope_values(initial: &[Value], current: &[Value]) -> Option<Vec<Value>> {
    if initial.len() >= current.len() || !initial.iter().all(|value| current.contains(value)) {
        return None;
    }
    Some(
        current
            .iter()
            .filter(|value| !initial.contains(value))
            .cloned()
            .collect(),
    )
}

fn render_blackboard(output: &mut String, messages: &[Value], state: &mut RenderState) {
    output.push_str("\n## Blackboard messages\n");
    if messages.is_empty() {
        output.push_str("\nNo messages were included in this context.\n");
        return;
    }
    for message in messages {
        let key = message.get("id").map(scalar_text);
        if let Some(key) = key.as_ref() {
            if state.seen_messages.get(key) == Some(message) {
                continue;
            }
            state.seen_messages.insert(key.clone(), message.clone());
        }
        if message.get("topic").and_then(Value::as_str) == Some("blockers") {
            output.push('\n');
            output.push_str(&render_blocker_callout(message));
        } else {
            render_nonblocker_message(output, message, state);
        }
    }
}

fn render_nonblocker_message(output: &mut String, message: &Value, state: &mut RenderState) {
    let topic = message
        .get("topic")
        .map(scalar_text)
        .unwrap_or_else(|| "message".into());
    let id = message
        .get("id")
        .map(scalar_text)
        .unwrap_or_else(|| "unknown".into());
    output.push_str("\n### Blackboard ");
    output.push_str(&topic);
    output.push_str(" message (#");
    output.push_str(&id);
    output.push_str(")\n");
    let fields = message
        .as_object()
        .map(|object| fields_without(object, &["payload"]));
    if let Some(fields) = fields {
        render_fields(output, &fields, 0, state);
    }
    let payload = message
        .get("payload")
        .and_then(Value::as_str)
        .unwrap_or_default();
    output.push_str("- **Body**\n");
    append_quoted_body(output, payload);
    if summary_is_truncated(payload) {
        append_recovery_hint(output, message);
    }
}

fn append_recovery_hint(output: &mut String, message: &Value) {
    let id = message
        .get("id")
        .map(scalar_text)
        .unwrap_or_else(|| "unknown".into());
    output.push_str("> Full body recovery: ");
    output.push_str(&inline_code(&format!(
        "task_blackboard(action: \"inspect\", message_id: {id})"
    )));
    output.push_str(".\n");
}

fn append_quoted_body(output: &mut String, payload: &str) {
    if payload.is_empty() {
        output.push_str("> Body: \"\"\n");
        return;
    }
    output.push_str("> Body:\n");
    for line in payload.split_inclusive('\n') {
        output.push_str("> ");
        output.push_str(line);
        if !line.ends_with('\n') {
            output.push('\n');
        }
    }
}

fn quote_field(output: &mut String, label: &str, value: Option<&Value>) {
    let value = value.map(scalar_text).unwrap_or_else(|| "null".into());
    output.push_str("> ");
    output.push_str(label);
    output.push_str(": ");
    output.push_str(&inline_code(&value));
    output.push('\n');
}

fn summary_is_truncated(payload: &str) -> bool {
    payload.ends_with("... [truncated]")
}

fn render_task_fields(
    output: &mut String,
    fields: &Map<String, Value>,
    scope_uris: &Map<String, Value>,
    depth: usize,
    state: &mut RenderState,
) {
    for (key, value) in ordered_fields(fields) {
        if key == "spec" {
            if let Some(spec) = value.as_object() {
                bullet_prefix(output, depth);
                output.push_str("**");
                output.push_str(&escape_label(display_label(key)));
                output.push_str("**\n");
                for (spec_key, spec_value) in ordered_fields(spec) {
                    if spec_key == "scope" {
                        render_task_scope(output, spec_value, scope_uris, depth + 1, state);
                    } else {
                        render_field(output, spec_key, spec_value, depth + 1, state);
                    }
                }
                continue;
            }
        }
        render_field(output, key, value, depth, state);
    }
}

fn render_task_scope(
    output: &mut String,
    scope: &Value,
    scope_uris: &Map<String, Value>,
    depth: usize,
    state: &mut RenderState,
) {
    match scope {
        Value::Array(paths) => render_scope_group(output, "scope", paths, scope_uris, depth, state),
        Value::Object(groups) => {
            bullet_prefix(output, depth);
            output.push_str("**Scope**\n");
            for (key, value) in ordered_fields(groups) {
                if let Some(paths) = value.as_array() {
                    render_scope_group(output, key, paths, scope_uris, depth + 1, state);
                } else {
                    render_field(output, key, value, depth + 1, state);
                }
            }
        }
        _ => render_field(output, "scope", scope, depth, state),
    }
}

fn render_scope_group(
    output: &mut String,
    key: &str,
    paths: &[Value],
    scope_uris: &Map<String, Value>,
    depth: usize,
    state: &mut RenderState,
) {
    if paths.is_empty() {
        render_field(output, key, &Value::Array(Vec::new()), depth, state);
        return;
    }
    bullet_prefix(output, depth);
    output.push_str("**");
    output.push_str(&escape_label(display_label(key)));
    output.push_str("**\n");
    for (index, value) in paths.iter().enumerate() {
        let item_key = format!("[{index}]");
        let Some(path) = value.as_str() else {
            render_field(output, &item_key, value, depth + 1, state);
            continue;
        };
        if let Some(uri) = scope_uris.get(path).and_then(Value::as_str) {
            bullet_prefix(output, depth + 1);
            output.push_str(&format!("[{}]({uri})\n", escape_label(path)));
        } else {
            render_field(output, &item_key, value, depth + 1, state);
        }
    }
}

#[derive(Default)]
struct RenderState {
    seen_messages: BTreeMap<String, Value>,
}

fn render_fields(
    output: &mut String,
    fields: &Map<String, Value>,
    depth: usize,
    state: &mut RenderState,
) {
    for (key, value) in ordered_fields(fields) {
        render_field(output, key, value, depth, state);
    }
}

fn render_field(
    output: &mut String,
    key: &str,
    value: &Value,
    depth: usize,
    state: &mut RenderState,
) {
    match value {
        Value::Object(object) if object.is_empty() => {
            bullet_prefix(output, depth);
            output.push_str("**");
            output.push_str(&escape_label(display_label(key)));
            output.push_str("**: ");
            output.push_str(&inline_code("{}"));
            output.push('\n');
        }
        Value::Object(object) => {
            bullet_prefix(output, depth);
            output.push_str("**");
            output.push_str(&escape_label(display_label(key)));
            output.push_str("**\n");
            render_fields(output, object, depth + 1, state);
        }
        Value::Array(items) if items.is_empty() => {
            bullet_prefix(output, depth);
            output.push_str("**");
            output.push_str(&escape_label(display_label(key)));
            output.push_str("**: ");
            output.push_str(&inline_code("[]"));
            output.push('\n');
        }
        Value::Array(items) => {
            bullet_prefix(output, depth);
            output.push_str("**");
            output.push_str(&escape_label(display_label(key)));
            output.push_str("**\n");
            for (index, item) in items.iter().enumerate() {
                render_field(output, &format!("[{index}]"), item, depth + 1, state);
            }
        }
        Value::String(text) if text.contains('\n') => {
            bullet_prefix(output, depth);
            output.push_str("**");
            output.push_str(&escape_label(display_label(key)));
            output.push_str("**\n");
            append_fenced_text(output, text, depth + 1);
        }
        scalar => {
            bullet_prefix(output, depth);
            output.push_str("**");
            output.push_str(&escape_label(display_label(key)));
            output.push_str("**: ");
            let text = match scalar {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            output.push_str(&inline_code(&text));
            output.push('\n');
        }
    }
}

fn append_fenced_text(output: &mut String, text: &str, depth: usize) {
    let longest = longest_backtick_run(text);
    let backtick = char::from(96).to_string();
    let fence = backtick.repeat(longest.max(2) + 1);
    indent(output, depth);
    output.push_str(&fence);
    output.push_str("text\n");
    indent(output, depth);
    output.push_str(text);
    if !text.ends_with('\n') {
        output.push('\n');
    }
    indent(output, depth);
    output.push_str(&fence);
    output.push('\n');
}

fn fields_without(object: &Map<String, Value>, excluded: &[&str]) -> Map<String, Value> {
    object
        .iter()
        .filter(|(key, _)| !excluded.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn ordered_fields(object: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut fields = object
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
    fields.sort_unstable_by_key(|(key, _)| *key);
    fields
}

fn display_label(key: &str) -> &str {
    match key {
        "task_id" => "Task ID",
        "milestone_ref" => "Milestone",
        "stage" => "Stage",
        "status" => "Status",
        "claim_revision" => "Claim revision",
        "contract_revision" | "rev" => "Contract revision",
        "worker_id" | "owner" => "Worker",
        "worktree" => "Worktree",
        "goal" | "target" => "Target",
        "allowed_scope" | "allowed_write_scope" => "Write scope",
        "initial_scope" => "Initial scope",
        "extended_scope" => "Extended scope",
        "proof_policy" => "Proof policy",
        "depends_on" => "Dependencies",
        "applicable_invariants" | "invariants" => "Invariants",
        "subtask_dod" => "Subtask definition of done",
        "actionable_steps" => "Actionable steps",
        "expected_evidence_template" => "Expected evidence template",
        "scoped_commands" => "Scoped commands",
        "receipt" => "Durable receipt",
        "findings" => "Findings",
        "candidate_snapshot" => "Candidate snapshot",
        "latest_snapshot" => "Latest snapshot",
        _ => key,
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn inline_code(value: &str) -> String {
    let delimiter = char::from(96)
        .to_string()
        .repeat(longest_backtick_run(value) + 1);
    let padded = value.starts_with(char::from(96))
        || value.ends_with(char::from(96))
        || value.starts_with(' ')
        || value.ends_with(' ');
    if padded {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

fn longest_backtick_run(value: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in value.chars() {
        if character == char::from(96) {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('*', "\\*")
}

fn bullet_prefix(output: &mut String, depth: usize) {
    indent(output, depth);
    output.push_str("- ");
}

fn indent(output: &mut String, depth: usize) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}
