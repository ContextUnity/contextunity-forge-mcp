use serde_json::Value;
use std::path::Path;

/// Semantic discovery operations with a domain-specific Markdown layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryKind {
    /// Workspace inventory and indexed health.
    Overview,
    /// One selected symbol and its verified context.
    Inspect,
    /// Ranked symbol matches and bounded graph context.
    Search,
    /// AST matches or a profile-provided syntax diagnostic.
    Ast,
}

/// Builds a file URI from a path already resolved by the owning adapter.
///
/// This function only encodes a coordinate. Callers must first resolve the
/// workspace-relative path with the canonical workspace path resolver.
pub fn file_uri_for_path(path: &Path, line: Option<usize>) -> Option<String> {
    let path = path.to_str()?.replace('\\', "/");
    if !path.starts_with('/') {
        return None;
    }
    let mut uri = String::from("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(char::from(byte));
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    if let Some(line) = line {
        uri.push_str(&format!("#L{line}"));
    }
    Some(uri)
}

pub(super) fn render(value: &Value, title: &str, kind: DiscoveryKind) -> String {
    if kind == DiscoveryKind::Ast && value.get("diagnostic").is_some() {
        return render_ast_diagnostic(&value["diagnostic"], title);
    }

    let mut output = format!("# {}\n\n", escape_text(title));
    match kind {
        DiscoveryKind::Overview => render_overview(&mut output, value),
        DiscoveryKind::Inspect => render_inspect(&mut output, value),
        DiscoveryKind::Search => render_search(&mut output, value),
        DiscoveryKind::Ast => render_ast_matches(&mut output, value),
    }
    let scope = operation_scope(value, kind);
    render_paging(&mut output, value, scope.as_deref());
    output
}

pub(super) fn ast_legacy_error(diagnostic: &Value) -> String {
    let code = text(diagnostic, "code").unwrap_or("unsupported_ast_pattern");
    let language = text(diagnostic, "language").unwrap_or("unknown");
    let span = &diagnostic["span"];
    let start_line = number(span, "start_line").unwrap_or(1);
    let start_column = number(span, "start_column").unwrap_or(1);
    let end_line = number(span, "end_line").unwrap_or(start_line);
    let end_column = number(span, "end_column").unwrap_or(start_column);
    let hint = text(diagnostic, "hint").unwrap_or("No profile hint was supplied.");
    let examples = strings(&diagnostic["examples"])
        .into_iter()
        .map(|example| format!("  - {}", example.replace('\n', "\n    ")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{code}: Unsupported AST pattern for language '{language}' at {start_line}:{start_column} to {end_line}:{end_column}.\nHint: {hint}\nExamples of valid patterns for {language}:\n{examples}"
    )
}

fn render_ast_diagnostic(diagnostic: &Value, title: &str) -> String {
    let mut output = format!("# {}\n\n> [!CAUTION]\n", escape_text(title));
    let code = text(diagnostic, "code").unwrap_or("unsupported_ast_pattern");
    let language = text(diagnostic, "language").unwrap_or("unknown");
    output.push_str(&format!(
        "> **{}** for `{}`.\n",
        escape_text(code),
        escape_text(language)
    ));
    let span = &diagnostic["span"];
    let mut coordinates = Vec::new();
    for field in [
        "start_byte",
        "end_byte",
        "start_line",
        "start_column",
        "end_line",
        "end_column",
    ] {
        if let Some(value) = number(span, field) {
            coordinates.push(format!("{field}={value}"));
        }
    }
    if !coordinates.is_empty() {
        output.push_str(&format!(
            "> **Pattern span:** {}.\n",
            coordinates.join(", ")
        ));
    }
    if let Some(hint) = text(diagnostic, "hint") {
        output.push_str(&format!("> **Hint:** {}\n", quote_lines(hint)));
    }
    let examples = strings(&diagnostic["examples"]);
    if !examples.is_empty() {
        output.push_str("\n**Valid patterns for this profile**\n\n");
        for example in examples {
            output.push_str(&format!("- `{}`\n", code_span(&example)));
        }
    }
    output
}

fn render_overview(output: &mut String, value: &Value) {
    output.push_str("## Workspace\n\n");
    let workspace_root = text(value, "workspace_root")
        .or_else(|| text(&value["workspace"], "root"))
        .or_else(|| text(&value["metadata"], "workspace_root"));
    if let Some(root) = workspace_root {
        output.push_str(&format!("- Root: `{}`\n", escape_text(root)));
    }
    let scope = operation_scope(value, DiscoveryKind::Overview)
        .or_else(|| workspace_root.map(|root| format!("workspace_root={root}")));
    if let Some(worktree) = text(&value["workspace"], "worktree") {
        output.push_str(&format!("- Worktree: `{}`\n", escape_text(worktree)));
    }
    if let Some(branch) = text(&value["workspace"], "branch") {
        output.push_str(&format!("- Branch: `{}`\n", escape_text(branch)));
    }
    if let Some(freshness) = value.get("freshness") {
        let status = text(freshness, "status").unwrap_or("unavailable");
        if !matches!(status, "matched" | "source_inventory_matched") {
            let cause = text(freshness, "cause")
                .or_else(|| text(freshness, "refresh"))
                .unwrap_or("freshness evidence unavailable");
            output.push_str(&format!(
                "- Freshness warning: {} ({})\n",
                escape_text(status),
                escape_text(cause)
            ));
        }
    }

    if let Some(counts) = value.get("counts").and_then(Value::as_object) {
        output.push_str("\n## Indexed counts\n\n");
        for key in ["files", "nodes", "edges", "doc_sections", "parse_errors"] {
            if let Some(count) = counts.get(key) {
                output.push_str(&format!("- {}: {}\n", label(key), scalar(count)));
            }
        }
        if let Some(unresolved) = counts.get("unresolved") {
            output.push_str(&format!(
                "- Unresolved or ambiguous references: {}\n",
                scalar(unresolved)
            ));
        }
        if let Some(external) = counts.get("external_imports") {
            output.push_str(&format!(
                "- External references: {} (excluded from health)\n",
                scalar(external)
            ));
        }
    }

    if let Some(languages) = value.get("languages").and_then(page_items) {
        output.push_str("\n## Languages\n\n");
        for language in languages {
            let name = text(language, "language").unwrap_or("unknown");
            let files = language
                .get("files")
                .map(scalar)
                .unwrap_or_else(|| "unavailable".into());
            output.push_str(&format!("- `{}`: {} files", escape_text(name), files));
            for (field, description) in [
                ("resolved", "resolved"),
                ("unresolved", "unresolved"),
                ("ambiguous", "ambiguous"),
                ("external_imports", "external references"),
                ("parse_errors", "parse errors"),
            ] {
                if let Some(count) = language.get(field) {
                    output.push_str(&format!("; {description}: {}", scalar(count)));
                }
            }
            let profiles = strings(&value["compiled_profiles"]);
            if profiles.iter().any(|profile| profile == name) {
                output.push_str("; compiled profile available");
            }
            output.push('\n');
        }
    }

    if let Some(components) = value.get("components") {
        render_directory_components(output, components, scope.as_deref());
    }
    if let Some(profiles) = value.get("compiled_profiles") {
        output.push_str("\n## Compiled language profiles\n\n");
        let profiles = strings(profiles);
        if profiles.is_empty() {
            output.push_str("No compiled language profiles are available in this build.\n");
        } else {
            for profile in profiles {
                output.push_str(&format!("- `{}`\n", escape_text(&profile)));
            }
        }
    }

    if let Some(scopes) = value.get("coverage_by_workspace").and_then(page_items) {
        output.push_str("\n## Resolution health\n\n");
        for scope in scopes {
            let name = text(scope, "name").unwrap_or("workspace");
            output.push_str(&format!("### {}\n\n", escape_text(name)));
            render_resolution_health(output, scope);
        }
    }

    if let Some(discovery) = value.get("discovery") {
        render_manifest_components(output, &discovery["manifest_components"], scope.as_deref());
        render_documentation_hubs(output, &discovery["documentation_hubs"], scope.as_deref());
        render_milestones(output, &discovery["active_milestones"]);
    }
    if let Some(manifests) = value.get("manifest_components") {
        render_manifest_components(output, manifests, scope.as_deref());
    }
    if let Some(hubs) = value.get("documentation_hubs") {
        render_documentation_hubs(output, hubs, scope.as_deref());
    }
    if let Some(milestones) = value.get("active_milestones") {
        render_milestones(output, milestones);
    }
    if let Some(cycles) = value.get("cycles") {
        render_json_section(output, "Dependency cycles", cycles);
    }
    if let Some(metadata) = value.get("metadata") {
        render_metadata(output, metadata);
    }
}

fn render_directory_components(output: &mut String, page: &Value, scope: Option<&str>) {
    output.push_str("\n## Directory components\n\n");
    let items = page_items(page).unwrap_or(&[]);
    if items.is_empty() && page.get("total").and_then(Value::as_u64) == Some(0) {
        output.push_str("No directory components were indexed.\n");
    }
    for item in items {
        let id = text(item, "id");
        let name = text(item, "name").unwrap_or("component");
        let path = text(item, "path").unwrap_or("path unavailable");
        let id = id
            .map(|id| format!(" (`{}`)", escape_text(id)))
            .unwrap_or_default();
        output.push_str(&format!(
            "- **{}**{id} — `{}`\n",
            escape_text(name),
            escape_text(path)
        ));
    }
    render_page_continuation(output, page, "directory components", scope);
}

fn render_metadata(output: &mut String, metadata: &Value) {
    let Some(entries) = metadata.as_array() else {
        render_json_section(output, "Workspace metadata", metadata);
        return;
    };
    if entries.is_empty() {
        return;
    }
    output.push_str("\n## Workspace metadata\n\n");
    for entry in entries {
        let key = text(entry, "key").unwrap_or("metadata key unavailable");
        let value = entry
            .get("value")
            .map(scalar)
            .unwrap_or_else(|| "unavailable".into());
        output.push_str(&format!("- `{}`: `{}`\n", escape_text(key), value));
    }
}

fn render_json_section(output: &mut String, title: &str, value: &Value) {
    output.push_str(&format!("\n## {}\n\n```json\n", escape_text(title)));
    output.push_str(&serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned()));
    output.push_str("\n```\n");
}

fn render_resolution_health(output: &mut String, scope: &Value) {
    let health = &scope["resolution_health"];
    if text(health, "availability") == Some("available") {
        let resolved = number(health, "resolved").unwrap_or(0);
        let denominator = number(health, "denominator").unwrap_or(0);
        let percent = health
            .get("percent")
            .and_then(Value::as_f64)
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "unavailable".to_owned());
        output.push_str(&format!(
            "Resolution Health: {percent} ({resolved} / {denominator} non-external references)\n"
        ));
    } else {
        let cause = text(health, "cause").unwrap_or("insufficient indexed evidence");
        output.push_str(&format!(
            "Resolution Health: unavailable ({})\n",
            escape_text(cause)
        ));
    }
    if let (Some(resolved), Some(unresolved), Some(ambiguous)) = (
        number(scope, "resolved"),
        number(scope, "unresolved"),
        number(scope, "ambiguous"),
    ) {
        output.push_str(&format!(
            "Resolved: {resolved}; unresolved: {unresolved}; ambiguous: {ambiguous}\n"
        ));
    }

    let external = scope
        .get("external_imports")
        .map(scalar)
        .unwrap_or_else(|| "unavailable".into());
    output.push_str(&format!(
        "External references: {external} (excluded from health); external crate count: unavailable\n"
    ));

    let reasons = &scope["diagnostic_reasons"];
    if text(reasons, "availability") != Some("available") {
        let cause = text(reasons, "cause").unwrap_or("diagnostic aggregate unavailable");
        output.push_str(&format!(
            "Diagnostic reasons: unavailable ({})\n",
            escape_text(cause)
        ));
        return;
    }
    let Some(counts) = reasons.get("counts").and_then(Value::as_object) else {
        output.push_str("Diagnostic reasons: unavailable (reason matrix not supplied)\n");
        return;
    };
    output.push_str("\nNon-external diagnostic references:\n");
    let mut external_reason_rows = Vec::new();
    for (reason, statuses) in counts {
        let Some(statuses) = statuses.as_object() else {
            continue;
        };
        let mut non_external_total = 0_u64;
        for (status, count) in statuses {
            if status != "external" {
                if let Some(count) = count.as_u64() {
                    non_external_total = non_external_total.saturating_add(count);
                }
            }
        }
        if non_external_total > 0 {
            output.push_str(&format!(
                "- {} ({reason}): {non_external_total}\n",
                reason_label(reason)
            ));
        }
        for (status, count) in statuses {
            if status != "external" {
                if let Some(count) = count.as_u64() {
                    output.push_str(&format!(
                        "  - Reason ({reason}, status={status}): {count}\n"
                    ));
                }
            } else if count.as_u64().is_some_and(|count| count > 0) {
                external_reason_rows.push((reason.as_str(), count));
            }
        }
    }
    if !external_reason_rows.is_empty() {
        output.push_str("External-status diagnostic references:\n");
        for (reason, count) in external_reason_rows {
            if let Some(count) = count.as_u64() {
                output.push_str(&format!(
                    "- {} (reason={reason}, status=external): {count}\n",
                    reason_label(reason)
                ));
            }
        }
    }
    output.push_str("Unindexed package/file inventory: unavailable\n");
}

fn reason_label(reason: &str) -> &'static str {
    match reason {
        "dynamic_callback" => "Dynamic callbacks",
        "unresolved_receiver_type" => "Unresolved receiver type",
        "framework_magic" => "Framework magic",
        "external_unindexed" => "Unindexed external references",
        "ambiguous_candidates" => "Ambiguous candidates",
        "shadowed_binding" => "Shadowed binding",
        "unknown" => "Unknown reason",
        _ => "Recorded reason",
    }
}

fn render_manifest_components(output: &mut String, page: &Value, scope: Option<&str>) {
    if page.is_null() {
        return;
    }
    output.push_str("\n## Manifest components\n\n");
    if text(page, "availability") == Some("unavailable")
        || page.get("available") == Some(&Value::Bool(false))
    {
        output.push_str(&format!(
            "Manifest inventory unavailable: {}\n",
            text(page, "cause").unwrap_or("inventory bounds prevented a complete scan")
        ));
        return;
    }
    let components = page_items(page).unwrap_or(&[]);
    if components.is_empty() {
        output.push_str(
            "No declared package components were found in the bounded manifest inventory.\n",
        );
    }
    for component in components {
        let name = text(component, "name").unwrap_or_else(|| {
            if text(component, "unit") == Some("workspace") {
                "workspace"
            } else {
                "unnamed component"
            }
        });
        let ecosystem = text(component, "ecosystem").unwrap_or("unknown ecosystem");
        let unit = text(component, "unit").unwrap_or("package");
        output.push_str(&format!(
            "- **{}** (`{ecosystem}`, {unit})",
            escape_text(name)
        ));
        if let Some(workspace) =
            text(component, "workspace").filter(|workspace| *workspace != "primary")
        {
            output.push_str(&format!(" · workspace `{}`", escape_text(workspace)));
        }
        if let Some(description) = text(component, "description") {
            output.push_str(&format!(": {}", escape_text(description)));
        }
        let root = text(component, "root").unwrap_or("");
        if !root.is_empty() {
            if let Some(uri) = text(component, "root_uri") {
                output.push_str(&format!(" — [root `{}`]({})", escape_text(root), uri));
            } else {
                output.push_str(&format!(" — root `{}`", escape_text(root)));
            }
        }
        if let (Some(path), Some(uri)) = (
            text(component, "manifest_path"),
            text(component, "manifest_uri"),
        ) {
            output.push_str(&format!(" · [manifest `{}`]({})", escape_text(path), uri));
        }
        if let Some(members) = component.get("workspace_members").and_then(Value::as_array) {
            let members = members
                .iter()
                .filter_map(Value::as_str)
                .map(escape_text)
                .collect::<Vec<_>>();
            if !members.is_empty() {
                output.push_str(&format!(
                    " · declared workspace members: {}",
                    members.join(", ")
                ));
            }
        }
        output.push('\n');
    }
    for diagnostic in strings(&page["diagnostics"]) {
        output.push_str(&format!(
            "- Manifest diagnostic: {}\n",
            escape_text(&diagnostic)
        ));
    }
    if let Some(diagnostics) = page.get("diagnostics").and_then(Value::as_array) {
        for diagnostic in diagnostics.iter().filter_map(Value::as_object) {
            let path = diagnostic
                .get("manifest_path")
                .or_else(|| diagnostic.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("manifest");
            let code = diagnostic
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("manifest_diagnostic");
            let reason = diagnostic
                .get("reason")
                .or_else(|| diagnostic.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("could not establish package identity");
            output.push_str(&format!(
                "- Manifest diagnostic at `{}` (`{}`): {}\n",
                escape_text(path),
                escape_text(code),
                escape_text(reason)
            ));
        }
    }
    if page.get("diagnostics_truncated") == Some(&Value::Bool(true)) {
        let total = page
            .get("diagnostic_total")
            .map(scalar)
            .unwrap_or_else(|| "unknown".into());
        output.push_str(&format!(
            "- Manifest diagnostics truncated; {total} found.\n"
        ));
    } else if page.get("diagnostics_available") == Some(&Value::Bool(false)) {
        output.push_str(&format!(
            "- Manifest diagnostics unavailable: {}\n",
            text(page, "diagnostics_cause").unwrap_or("inventory bounds prevented a complete scan")
        ));
    }
    render_page_continuation(output, page, "manifest components", scope);
}

fn render_documentation_hubs(output: &mut String, page: &Value, scope: Option<&str>) {
    if page.is_null() {
        return;
    }
    output.push_str("\n## Discovery documentation hub\n\n");
    if page.get("available") == Some(&Value::Bool(false)) {
        output.push_str(&format!(
            "Documentation hub unavailable: {}\n",
            text(page, "cause").unwrap_or("indexed document inventory unavailable")
        ));
        return;
    }
    for item in page_items(page).unwrap_or(&[]) {
        let path = text(item, "path").unwrap_or("unknown document");
        let section = text(item, "section_title").unwrap_or("document");
        let line = number(item, "line");
        let anchor = line.map(|line| format!("#L{line}")).unwrap_or_default();
        if let Some(uri) = text(item, "uri") {
            output.push_str(&format!(
                "- [{} — {}{}]({uri})\n",
                escape_text(path),
                escape_text(section),
                anchor
            ));
        } else {
            output.push_str(&format!(
                "- `{}` — {}{}\n",
                escape_text(path),
                escape_text(section),
                anchor
            ));
        }
    }
    render_page_continuation(output, page, "documentation hubs", scope);
}

fn render_milestones(output: &mut String, milestones: &Value) {
    if milestones.is_null() {
        return;
    }
    output.push_str("\n## Discovery active milestones\n\n");
    if text(milestones, "availability") == Some("unavailable") {
        output.push_str(&format!(
            "Active milestone inventory unavailable: {}\n",
            text(milestones, "cause").unwrap_or("canonical milestone inventory unavailable")
        ));
        return;
    }
    for item in page_items(milestones).unwrap_or(&[]) {
        let id = text(item, "id").unwrap_or("milestone");
        let title = text(item, "title").unwrap_or("");
        let status = text(item, "status").unwrap_or("active");
        let completion = text(item, "completion").unwrap_or("progress unavailable");
        let path = text(item, "path");
        match (path, text(item, "uri")) {
            (Some(_), Some(uri)) => output.push_str(&format!(
                "- [{}]({uri}) — {} (`{}`; {completion} complete)\n",
                escape_text(id),
                escape_text(title),
                escape_text(status),
                completion = escape_text(completion)
            )),
            (Some(path), None) => output.push_str(&format!(
                "- **{}** — {} (`{}`; {completion} complete; document `{}` has no resolved URI)\n",
                escape_text(id),
                escape_text(title),
                escape_text(status),
                escape_text(path),
                completion = escape_text(completion)
            )),
            (None, _) => output.push_str(&format!(
                "- **{}** — {} (`{}`; {completion} complete; document coordinate unavailable)\n",
                escape_text(id),
                escape_text(title),
                escape_text(status),
                completion = escape_text(completion)
            )),
        }
    }
    if milestones
        .get("omitted")
        .and_then(Value::as_u64)
        .is_some_and(|count| count > 0)
    {
        output.push_str(&format!(
            "- {} additional active milestones omitted by the response bound.\n",
            scalar(&milestones["omitted"])
        ));
    }
}

fn render_inspect(output: &mut String, value: &Value) {
    let node = &value["node"];
    let summary = &value["summary"];
    output.push_str("## Symbol\n\n");
    if let Some(name) = text(node, "qualname").or_else(|| text(node, "name")) {
        output.push_str(&format!("- **{}**", escape_text(name)));
        if let Some(kind) = text(node, "kind") {
            output.push_str(&format!(" (`{}`)", escape_text(kind)));
        }
        let details = &node["details"];
        if let Some(signature) = text(node, "signature")
            .or_else(|| text(summary, "signature"))
            .or_else(|| text(details, "signature"))
        {
            output.push_str(&format!(" — `{}`", code_span(signature)));
        }
        output.push('\n');
    }
    if let (Some(path), Some(line)) = (text(node, "path"), number(node, "line")) {
        if let Some(uri) = text(node, "uri") {
            output.push_str(&format!(
                "- Source: [{}#L{}]({})\n",
                escape_text(path),
                line,
                uri
            ));
        } else {
            output.push_str(&format!("- Source: `{}`#L{}\n", escape_text(path), line));
        }
    }
    if let Some(docstring) = text(node, "docstring")
        .or_else(|| text(summary, "docstring"))
        .or_else(|| text(&node["details"], "doc"))
    {
        output.push_str(&format!("\n{}\n", escape_text(docstring)));
    }
    if let Some(container) = text(node, "container") {
        output.push_str(&format!("- Container: `{}`\n", escape_text(container)));
    } else if let Some(container) = summary.get("container").filter(|value| !value.is_null()) {
        let name = text(container, "qualname")
            .or_else(|| text(container, "name"))
            .unwrap_or("container");
        let path = text(container, "path").unwrap_or("");
        let line = number(container, "line");
        let coordinate = line
            .map(|line| format!("{path}#L{line}"))
            .unwrap_or_else(|| path.to_owned());
        if let Some(uri) = text(container, "uri") {
            output.push_str(&format!("- Container: [{}]({uri})\n", escape_text(name)));
        } else {
            output.push_str(&format!(
                "- Container: `{}` at `{}`\n",
                escape_text(name),
                escape_text(&coordinate)
            ));
        }
    }
    if let Some(calls) = summary.get("calls").and_then(Value::as_object) {
        let mut rendered = false;
        for (direction, title) in [("inbound", "Callers"), ("outbound", "Calls")] {
            let Some(relation) = calls.get(direction) else {
                continue;
            };
            let Some(count) = relation.get("count") else {
                continue;
            };
            if !rendered {
                output.push_str("\n### Available relation categories\n\n");
                rendered = true;
            }
            output.push_str(&format!("- {title}: {}\n", scalar(count)));
            render_graph_items(
                output,
                relation.get("preview"),
                if direction == "inbound" {
                    "caller"
                } else {
                    "callee"
                },
            );
        }
    }

    let invariants = &value["governing_invariants"];
    let direct = invariants["direct"].as_array().cloned().unwrap_or_default();
    if !direct.is_empty() {
        output.push_str("\n> [!IMPORTANT] Governing Invariants\n> Direct governing invariants\n");
        for rule in direct {
            let id = text(&rule, "id").unwrap_or("invariant");
            let name = text(&rule, "rule").unwrap_or("");
            let source = &rule["source"];
            let source_path = text(source, "path");
            let source_line = number(source, "line");
            let source_uri = text(source, "uri");
            let source_link = match (source_path, source_line, source_uri) {
                (Some(path), Some(line), Some(uri)) => format!(" — [{path}#L{line}]({uri})"),
                (Some(path), Some(line), None) => format!(" — `{path}`#L{line}"),
                _ => String::new(),
            };
            output.push_str(&format!(
                "\n> - `{}`: {}{}",
                escape_text(id),
                escape_text(name),
                source_link
            ));
        }
        output.push('\n');
    }
    if let Some(omitted) = invariants.get("omitted_direct") {
        if omitted.as_u64().is_some_and(|count| count > 0) {
            output.push_str(&format!(
                "\n- Omitted direct invariants: {}\n",
                scalar(omitted)
            ));
        }
    }
    if let Some(transitive) = invariants.get("transitive_ids").and_then(Value::as_array) {
        if !transitive.is_empty() {
            output.push_str("\n### One-hop or transitive invariant references\n\n");
            for id in transitive.iter().filter_map(Value::as_str) {
                output.push_str(&format!("- `{}`\n", escape_text(id)));
            }
        }
    }
    if let Some(omitted) = invariants.get("omitted_transitive") {
        if omitted.as_u64().is_some_and(|count| count > 0) {
            output.push_str(&format!(
                "\n- Omitted one-hop invariant references: {}\n",
                scalar(omitted)
            ));
        }
    }
    if let Some(sources) = invariants
        .get("omitted_sources")
        .and_then(Value::as_array)
        .filter(|sources| !sources.is_empty())
    {
        output.push_str("\n### One-hop invariant source references\n\n");
        for reference in sources {
            let id = text(reference, "id").unwrap_or("invariant");
            let source = &reference["source"];
            let path = text(source, "path");
            let line = number(source, "line");
            let uri = text(source, "uri");
            let coordinate = match (path, line, uri) {
                (Some(path), Some(line), Some(uri)) => {
                    format!("[{path}#L{line}]({uri})")
                }
                (Some(path), Some(line), None) => {
                    format!("`{}`#L{line} (link unavailable)", escape_text(path))
                }
                (Some(path), None, Some(uri)) => format!("[{path}]({uri})"),
                (Some(path), None, None) => {
                    format!("`{}` (line and link unavailable)", escape_text(path))
                }
                (None, _, Some(uri)) => format!("[source]({uri})"),
                (None, _, None) => "source coordinate unavailable".to_owned(),
            };
            output.push_str(&format!("- `{}`: {coordinate}\n", escape_text(id)));
        }
    }
    for key in ["documented_in", "analysis_gaps"] {
        if let Some(items) = value.get(key).and_then(page_items) {
            if !items.is_empty() {
                output.push_str(&format!("\n### {}\n\n", label(key)));
                for item in items {
                    render_evidence_row(output, item);
                }
            }
        }
    }
    for key in ["documents", "coverage"] {
        if let Some(page) = value.get(key) {
            if page_items(page).is_some_and(|items| !items.is_empty())
                || page.get("statuses").is_some()
            {
                render_json_section(output, &label(key), page);
            }
            let scope = operation_scope(value, DiscoveryKind::Inspect);
            render_page_continuation(output, page, key, scope.as_deref());
        }
    }
    if let Some(source) = text(value, "source") {
        output.push_str("\n### Source preview\n\n");
        if let Some(preview) = value.get("source_preview") {
            let start = preview
                .get("start_line")
                .map(scalar)
                .unwrap_or_else(|| "unavailable".into());
            let end = preview
                .get("end_line")
                .map(scalar)
                .unwrap_or_else(|| "unavailable".into());
            output.push_str(&format!("Source range: lines {start}-{end}.\n\n"));
        }
        output.push_str("```text\n");
        output.push_str(source);
        if !source.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("```\n");
        render_source_preview_state(output, value);
    }
}

fn render_source_preview_state(output: &mut String, value: &Value) {
    let Some(preview) = value.get("source_preview") else {
        return;
    };
    let fields = [
        ("body_offset", "body_offset"),
        ("body_lines", "body_lines"),
        ("total_body_lines", "total_body_lines"),
        ("has_more", "has_more"),
        ("truncated_line", "truncated_line"),
        ("omitted_leading_lines", "omitted_leading_lines"),
        ("omitted_body_lines", "omitted_body_lines"),
    ];
    let state = fields
        .iter()
        .filter_map(|(field, label)| {
            preview
                .get(*field)
                .map(|value| format!("{label}={}", scalar(value)))
        })
        .collect::<Vec<_>>();
    if !state.is_empty() {
        output.push_str(&format!("\nSource preview state: {}.\n", state.join(", ")));
    }
    if let Some(hint) = text(preview, "continuation_hint") {
        output.push_str(&format!("Source preview note: {}\n", escape_text(hint)));
    }
    if preview.get("has_more") == Some(&Value::Bool(true))
        && preview.get("truncated_line") != Some(&Value::Bool(true))
    {
        let offset = preview
            .get("next_source_offset")
            .map(scalar)
            .unwrap_or_else(|| "unavailable".into());
        let generation = text(value, "generation").unwrap_or("unavailable");
        let selector = text(&value["node"], "id").unwrap_or("the same selected symbol");
        let path = text(&value["node"], "path").unwrap_or("the selected path");
        output.push_str(&format!(
            "Continue source for {} in {} with source_offset {offset} and generation {}.\n",
            inline_code(selector),
            inline_code(path),
            inline_code(generation)
        ));
    }
}

fn render_search(output: &mut String, value: &Value) {
    let query = text(&value["request_scope"], "pattern")
        .or_else(|| text(value, "query"))
        .or_else(|| text(value, "pattern"));
    let page = &value["nodes"];
    let items = page_items(page).unwrap_or(&[]);
    let groups = page.get("grouped_by_file").and_then(Value::as_object);
    let grouped_count = groups
        .into_iter()
        .flat_map(|groups| groups.values())
        .filter_map(Value::as_array)
        .map(Vec::len)
        .sum::<usize>();
    if items.is_empty() && grouped_count == 0 {
        if page.get("total").and_then(Value::as_u64) == Some(0) {
            output.push_str("## Search\n\n");
            output.push_str(&format!("No indexed matches for `{}`. This result is limited to the request and does not establish global absence. Try a narrower symbol name or widen the kind/path scope.\n", escape_text(query.unwrap_or("the requested pattern"))));
        } else {
            output.push_str("No matches are present on this page; the total or remaining search scope is unavailable.\n");
        }
        let scope = operation_scope(value, DiscoveryKind::Search);
        render_page_continuation(output, page, "search results", scope.as_deref().or(query));
        return;
    }
    let total = page
        .get("total")
        .map(scalar)
        .unwrap_or_else(|| "unavailable".into());
    output.push_str(&format!(
        "## Search for `{}` — {total} indexed matches\n\n",
        escape_text(query.unwrap_or("the requested pattern"))
    ));
    let mut index = 1;
    for item in items {
        render_search_item(output, item, text(item, "path").unwrap_or(""), index);
        index += 1;
    }
    if let Some(groups) = groups {
        for (path, entries) in groups {
            if let Some(entries) = entries.as_array() {
                for item in entries {
                    render_search_item(output, item, path, index);
                    index += 1;
                }
            }
        }
    }
    let scope = operation_scope(value, DiscoveryKind::Search);
    render_page_continuation(output, page, "search results", scope.as_deref().or(query));
}

fn render_search_item(output: &mut String, item: &Value, path: &str, index: usize) {
    let name = text(item, "qualname")
        .or_else(|| text(item, "name"))
        .unwrap_or("indexed symbol");
    let line = number(item, "line");
    let coordinate = if let Some(line) = line {
        format!("{path}#L{line}")
    } else {
        path.to_owned()
    };
    if let Some(uri) = text(item, "uri") {
        output.push_str(&format!(
            "### {index}. [{}]({uri})\n\n- Source: [{}]({uri})\n",
            escape_text(name),
            escape_text(&coordinate)
        ));
    } else if !coordinate.is_empty() {
        output.push_str(&format!(
            "### {index}. {}\n\n- Source: `{}`\n",
            escape_text(name),
            escape_text(&coordinate)
        ));
    } else {
        output.push_str(&format!("### {index}. {}\n\n", escape_text(name)));
    }
    if let Some(kind) = text(item, "kind") {
        output.push_str(&format!("- Kind: `{}`\n", escape_text(kind)));
    }
    for field in ["visibility", "container"] {
        if let Some(value) = text(item, field) {
            output.push_str(&format!("- {}: `{}`\n", label(field), escape_text(value)));
        }
    }
    if let Some(reason) = text(item, "match_reason") {
        output.push_str(&format!(
            "- Match reason: `{}`\n",
            escape_text(match_reason_label(reason))
        ));
    }
    if let Some(classification) = text(item, "classification") {
        output.push_str(&format!(
            "- Classification: `{}`\n",
            escape_text(classification)
        ));
    }
    if let Some(signature) = text(item, "signature") {
        output.push_str(&format!("- Signature: `{}`\n", code_span(signature)));
    }
    if let Some(docstring) = text(item, "docstring_first_sentence") {
        output.push_str(&format!("- Documentation: {}\n", escape_text(docstring)));
    }
    let pulse = &item["graph_pulse"];
    if !pulse.is_null() {
        output.push_str("- Graph pulse:\n");
        if let Some(callers) = pulse.get("callers") {
            output.push_str(&format!(
                "  - callers: {}\n",
                callers
                    .get("count")
                    .map(scalar)
                    .unwrap_or_else(|| "unavailable".into())
            ));
            render_graph_items(output, callers.get("preview"), "caller");
        }
        if let Some(implementations) = pulse.get("implementations") {
            output.push_str(&format!(
                "  - implementation relations: {}\n",
                implementations
                    .get("count")
                    .map(scalar)
                    .unwrap_or_else(|| "unavailable".into())
            ));
        }
        if let Some(tests) = pulse.get("covering_tests") {
            match text(tests, "discovery_method") {
                Some("graph") => {
                    output.push_str(&format!(
                        "  - verified covering tests: {}\n",
                        tests.get("total").map(scalar).unwrap_or_else(|| "0".into())
                    ));
                    render_graph_items(output, tests.get("items"), "test");
                }
                Some(method) => output.push_str(&format!(
                    "  - test discovery: {} (not a verified call path)\n",
                    escape_text(method)
                )),
                None => output.push_str("  - verified covering tests: unavailable\n"),
            }
        }
    }
    if let Some(source) = text(item, "source") {
        output.push_str("\n```text\n");
        output.push_str(source);
        if !source.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("```\n");
    }
    output.push('\n');
}

fn match_reason_label(reason: &str) -> &str {
    match reason {
        "exact_name" => "exact symbol name",
        "name_prefix" => "name prefix",
        "exact_qualified_name" => "exact qualified name",
        "name_fragment" => "name fragment",
        "indexed_text" => "indexed text",
        "name_pattern" => "name pattern",
        "qualified_pattern" => "qualified-name pattern",
        _ => reason,
    }
}

fn render_graph_items(output: &mut String, items: Option<&Value>, role: &str) {
    let Some(items) = items.and_then(Value::as_array) else {
        return;
    };
    for item in items {
        let name = text(item, "qualname")
            .or_else(|| text(item, "name"))
            .unwrap_or("indexed symbol");
        let path = text(item, "path").unwrap_or("");
        let line = number(item, "line");
        let target = text(item, "uri");
        let coordinate = line
            .map(|line| format!("{path}#L{line}"))
            .unwrap_or_else(|| path.to_owned());
        if let Some(target) = target {
            output.push_str(&format!(
                "    - {role}: [{} — `{}`]({target})\n",
                escape_text(name),
                escape_text(&coordinate)
            ));
        } else if coordinate.is_empty() {
            output.push_str(&format!("    - {role}: {}\n", escape_text(name)));
        } else {
            output.push_str(&format!(
                "    - {role}: {} at `{}`\n",
                escape_text(name),
                escape_text(&coordinate)
            ));
        }
    }
}

fn render_ast_matches(output: &mut String, value: &Value) {
    let page = &value["matches"];
    let matches = page_items(page).unwrap_or(&[]);
    let pattern = text(value, "pattern").unwrap_or("the requested pattern");
    let language = text(value, "language").unwrap_or("language unavailable");
    output.push_str(&format!(
        "## AST search for `{}` in `{}`\n\n",
        escape_text(pattern),
        escape_text(language)
    ));
    if let Some(total) = page.get("total").and_then(Value::as_u64) {
        output.push_str(&format!("{} total matches\n\n", total));
    } else if page.get("total_status").and_then(Value::as_str) == Some("unavailable")
        || page.get("computation_truncated") == Some(&Value::Bool(true))
        || value.get("truncated") == Some(&Value::Bool(true))
    {
        output.push_str(&format!(
            "{} matches on this page; total unavailable\n\n",
            matches.len()
        ));
    } else {
        output.push_str(&format!("{} matches\n\n", matches.len()));
    }
    if matches.is_empty() {
        output.push_str("No AST matches were returned for the bounded search.\n");
    }
    for matched in matches {
        output.push_str("- Match");
        if let Some(path) = text(matched, "path") {
            let line = number(matched, "line");
            let end_line = number(matched, "end_line");
            let label = line
                .map(|line| match end_line {
                    Some(end) if end != line => format!("{path}#L{line}-L{end}"),
                    _ => format!("{path}#L{line}"),
                })
                .unwrap_or_else(|| path.to_owned());
            if let Some(uri) = text(matched, "uri") {
                output.push_str(&format!(" in [{}]({uri})", escape_text(&label)));
            } else {
                output.push_str(&format!(" in `{}`", escape_text(&label)));
            }
        }
        output.push('\n');
        if let Some(snippet) = text(matched, "snippet").or_else(|| text(matched, "text")) {
            output.push_str("  - Matched syntax:\n\n```text\n");
            output.push_str(snippet);
            if !snippet.ends_with('\n') {
                output.push('\n');
            }
            output.push_str("```\n");
        }
        for key in ["captures", "enclosing"] {
            if let Some(field) = matched.get(key) {
                output.push_str(&format!("  - {}: {}\n", label(key), scalar_or_json(field)));
            }
        }
    }
    if page.get("total_status").and_then(Value::as_str) == Some("unavailable")
        || page.get("computation_truncated") == Some(&Value::Bool(true))
        || value.get("truncated") == Some(&Value::Bool(true))
    {
        output.push_str("\n- Total: unavailable for this bounded search.\n");
    } else if let Some(total) = page.get("total") {
        output.push_str(&format!("\n- Total: {}\n", scalar(total)));
    }
    if page.is_object() {
        let scope = operation_scope(value, DiscoveryKind::Ast)
            .unwrap_or_else(|| format!("pattern={pattern}, language={language}"));
        render_page_continuation(output, page, "AST matches", Some(&scope));
    }
    if let Some(truncated) = value.get("truncated") {
        output.push_str(&format!("\n- Search truncated: {}\n", scalar(truncated)));
    }
}

fn render_evidence_row(output: &mut String, item: &Value) {
    let path = text(item, "path").unwrap_or("indexed evidence");
    let line = number(item, "doc_line").or_else(|| number(item, "line"));
    let section = text(item, "section_title")
        .or_else(|| text(item, "reason"))
        .unwrap_or("");
    let label = line
        .map(|line| format!("{path}#L{line}"))
        .unwrap_or_else(|| path.to_owned());
    if let Some(uri) = text(item, "uri") {
        output.push_str(&format!("- [{}]({uri})", escape_text(&label)));
    } else {
        output.push_str(&format!("- `{}`", escape_text(&label)));
    }
    if !section.is_empty() {
        output.push_str(&format!(" — {}", escape_text(section)));
    }
    output.push('\n');
}

fn render_page_continuation(output: &mut String, page: &Value, name: &str, scope: Option<&str>) {
    if page.get("has_more") == Some(&Value::Bool(true)) {
        let next = page
            .get("next_offset")
            .map(scalar)
            .unwrap_or_else(|| "unknown".into());
        let generation = text(page, "generation");
        match generation {
            Some(generation) => {
                let scope = scope
                    .map(|scope| format!(" for {}", inline_code(scope)))
                    .unwrap_or_else(|| " for the same original request scope".to_owned());
                output.push_str(&format!(
                    "\nContinue {name}{scope} at offset {next} with generation {}.\n",
                    inline_code(generation)
                ));
            }
            None => output.push_str(&format!(
                "\nMore {name} remain at offset {next}, but the accepted generation is unavailable; continuation cannot be safely replayed.\n"
            )),
        }
    }
    if page.get("computation_truncated") == Some(&Value::Bool(true)) {
        output.push_str("\nThe bounded computation was truncated; totals may be unavailable.\n");
    }
}

fn render_paging(output: &mut String, value: &Value, scope: Option<&str>) {
    for key in [
        "languages",
        "coverage_by_workspace",
        "documented_in",
        "analysis_gaps",
    ] {
        if let Some(page) = value
            .get(key)
            .filter(|page| page.get("has_more") == Some(&Value::Bool(true)))
        {
            render_page_continuation(output, page, &label(key), scope);
        }
    }
}

fn operation_scope(value: &Value, kind: DiscoveryKind) -> Option<String> {
    if let Some(scope) = value.get("request_scope").and_then(Value::as_object) {
        let entries = scope
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                format!("{key}={value}")
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            return Some(entries.join(", "));
        }
    }
    match kind {
        DiscoveryKind::Overview => {
            text(value, "workspace_root").map(|root| format!("workspace_root={root}"))
        }
        DiscoveryKind::Inspect => {
            let node = &value["node"];
            match (text(node, "id"), text(node, "path")) {
                (Some(id), Some(path)) => Some(format!("selector={id}, path={path}")),
                (Some(id), None) => Some(format!("selector={id}")),
                _ => None,
            }
        }
        DiscoveryKind::Search => text(value, "pattern").map(|pattern| format!("pattern={pattern}")),
        DiscoveryKind::Ast => {
            let pattern = text(value, "pattern")?;
            let language = text(value, "language").unwrap_or("language unavailable");
            Some(format!("pattern={pattern}, language={language}"))
        }
    }
}

fn page_items(value: &Value) -> Option<&[Value]> {
    value.as_array().map(Vec::as_slice).or_else(|| {
        value
            .get("items")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => escape_text(text),
        _ => value.to_string(),
    }
}

fn scalar_or_json(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        format!("`{}`", code_span(text))
    } else {
        format!("`{}`", value)
    }
}

fn code_span(value: &str) -> String {
    value.replace('`', "\\`").replace('\n', " ")
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

fn label(value: &str) -> String {
    value.replace('_', " ")
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn quote_lines(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("> {}\n", escape_text(line)))
        .collect()
}
