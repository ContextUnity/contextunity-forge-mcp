use crate::core::format::{self, DiscoveryKind, OperationKind, OutputFormat};
use crate::core::response::{ResponsePolicy, MAX_OUTPUT_BYTES};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::AsyncWrite;

// The tool result is JSON inside a text block, then escaped by JSON-RPC again.
const ENVELOPE_RESERVE: usize = 256;

/// Performs result.
pub fn result(value: anyhow::Result<Value>, policy: &ResponsePolicy) -> CallToolResult {
    result_with_format(value, policy, OutputFormat::default())
}

/// Formats a public response with an explicit presentation selection.
pub fn result_with_format(
    value: anyhow::Result<Value>,
    policy: &ResponsePolicy,
    output_format: OutputFormat,
) -> CallToolResult {
    result_for(
        value,
        policy,
        output_format,
        OperationKind::Navigation,
        "Result",
        None,
    )
}

pub(crate) fn result_for(
    value: anyhow::Result<Value>,
    policy: &ResponsePolicy,
    output_format: OutputFormat,
    operation: OperationKind,
    title: &str,
    explain: Option<bool>,
) -> CallToolResult {
    result_for_presented(
        value,
        policy,
        output_format,
        operation,
        title,
        explain,
        None,
    )
}

pub(crate) fn result_for_presented(
    value: anyhow::Result<Value>,
    policy: &ResponsePolicy,
    output_format: OutputFormat,
    operation: OperationKind,
    title: &str,
    explain: Option<bool>,
    discovery: Option<DiscoveryKind>,
) -> CallToolResult {
    let budget = policy.max_output_bytes.clamp(1024, MAX_OUTPUT_BYTES) - ENVELOPE_RESERVE;
    match value {
        Ok(value) => {
            if discovery == Some(DiscoveryKind::Ast) && value.get("diagnostic").is_some() {
                let diagnostic = &value["diagnostic"];
                let rendered = match output_format {
                    OutputFormat::Markdown => format::render_for_operation(
                        &value,
                        output_format,
                        title,
                        discovery,
                        operation,
                    ),
                    OutputFormat::Json => format::ast_legacy_error(diagnostic),
                };
                return if success_text_bytes(&rendered) <= budget {
                    CallToolResult::error(vec![ContentBlock::text(rendered)])
                } else {
                    bounded_error(
                        "AST diagnostic exceeds the response byte limit; reduce the pattern or use JSON output.",
                        budget,
                    )
                };
            }
            let mut compacted = value.clone();
            format::compact(&mut compacted, operation, explain);
            let rendered = format::render_for_operation(
                &compacted,
                output_format,
                title,
                discovery,
                operation,
            );
            if success_text_bytes(&rendered) <= budget {
                return CallToolResult::success(vec![ContentBlock::text(rendered)]);
            }

            let output_too_large = || {
                bounded_error(
                    "Output exceeds the response byte limit. Use detail='compact', a smaller limit, or a narrower selector/SQL projection. A single oversized item must be read from its local file; no items were skipped and no continuation was consumed.",
                    budget,
                )
            };
            // Preserve candidate tokens until tail trimming has decided which pages
            // remain active; compact only after the retained set is known.
            let mut trimmed_value = value;
            let original = format::render_for_operation(
                &trimmed_value,
                output_format,
                title,
                discovery,
                operation,
            );
            let mut excess = success_text_bytes(&original)
                .saturating_sub(budget)
                .saturating_add(256);
            let trimmed = if title == "get_code_snippet" {
                trim_snippet_source_to_budget(
                    &mut trimmed_value,
                    output_format,
                    operation,
                    title,
                    explain,
                    discovery,
                    budget,
                )
            } else {
                trim_page_tails(&mut trimmed_value, &mut excess)
            };
            if !trimmed {
                if title == "get_code_snippet" && trimmed_value.get("source_preview").is_some() {
                    return bounded_error(
                        "Snippet line exceeds the serialized response byte budget; use a larger response budget or read the local source file. No partial line was returned.",
                        budget,
                    );
                }
                return output_too_large();
            }
            format::compact(&mut trimmed_value, operation, explain);
            let rendered = format::render_for_operation(
                &trimmed_value,
                output_format,
                title,
                discovery,
                operation,
            );
            if success_text_bytes(&rendered) > budget {
                return output_too_large();
            }
            CallToolResult::success(vec![ContentBlock::text(rendered)])
        }
        Err(error) => {
            if let Some(crate::db::reader::SelectorError::Ambiguous {
                selector,
                candidates,
            }) = error.downcast_ref::<crate::db::reader::SelectorError>()
            {
                let bounded =
                    |text: &str, maximum: usize| text.chars().take(maximum).collect::<String>();
                let candidate_values = candidates
                    .iter()
                    .take(8)
                    .map(|candidate| {
                        json!({
                            "id": bounded(&candidate.id, 256),
                            "path": bounded(&candidate.path, 256),
                            "line": candidate.line,
                            "qualname": bounded(&candidate.qualname, 256),
                            "signature": bounded(&candidate.signature, 512),
                        })
                    })
                    .collect::<Vec<_>>();
                let envelope = json!({
                    "outcome":"ambiguous",
                    "selector":bounded(selector, 512),
                    "message":format!("ambiguous selector {}; use an exact node id or workspace-relative path", bounded(selector, 512)),
                    "total":candidates.len(),
                    "candidates":candidate_values,
                });
                return CallToolResult::error(vec![ContentBlock::text(envelope.to_string())]);
            }
            if let Some(claim) = error.downcast_ref::<crate::db::tasks_store::TaskAlreadyClaimed>()
            {
                return CallToolResult::error(vec![ContentBlock::text(
                    serde_json::json!({
                        "error": {
                            "code": "TASK_ALREADY_CLAIMED",
                            "message": claim.to_string(),
                        }
                    })
                    .to_string(),
                )]);
            }
            let interrupted = error.chain().any(|cause| {
                matches!(
                    cause.downcast_ref::<rusqlite::Error>(),
                    Some(rusqlite::Error::SqliteFailure(code, _))
                        if code.code == rusqlite::ErrorCode::OperationInterrupted
                )
            });
            if interrupted {
                bounded_error(
                    "Graph query exceeded the 2-second SQLite budget. Narrow the selector to an indexed symbol or path, reduce traversal depth, and retry; reducing the page limit alone may not reduce query work. For diagnostic totals, use code_map_analyze with target='' (cycles are omitted by default); for cycles, analyze a smaller path with include_cycles=true.",
                    budget,
                )
            } else {
                bounded_error(&format!("{error:#}"), budget)
            }
        }
    }
}

fn trim_snippet_source_to_budget(
    value: &mut Value,
    output_format: OutputFormat,
    operation: OperationKind,
    title: &str,
    explain: Option<bool>,
    discovery: Option<DiscoveryKind>,
    budget: usize,
) -> bool {
    let Some(source) = value.get("source").and_then(Value::as_str) else {
        return false;
    };
    let Some(preview) = value.get("source_preview") else {
        return false;
    };
    let Some(node_line) = value["node"]["line"]
        .as_u64()
        .and_then(|line| usize::try_from(line).ok())
    else {
        return false;
    };
    let Some(body_offset) = preview["body_offset"]
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
    else {
        return false;
    };
    let Some(total_body_lines) = preview["total_body_lines"]
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
    else {
        return false;
    };
    let Some(body_lines) = preview["body_lines"]
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
    else {
        return false;
    };
    let Some(leading_lines) = preview["leading_lines"]
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
    else {
        return false;
    };
    let omitted_leading = preview["omitted_leading_lines"]
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .unwrap_or_default();
    let Some(body_end) = body_offset.checked_add(body_lines) else {
        return false;
    };
    if body_end > total_body_lines {
        return false;
    }

    let source_lines = source.split_inclusive('\n').collect::<Vec<_>>();
    if leading_lines > source_lines.len() {
        return false;
    }
    let leading_parts = &source_lines[..leading_lines];
    let body_parts = &source_lines[leading_lines..];
    if body_parts.len() < body_lines {
        return false;
    }
    let source_page = SnippetSourcePage {
        node_line,
        body_offset,
        total_body_lines,
        original_leading: leading_lines,
        omitted_leading,
    };
    let fits = |candidate: &Value| {
        let mut compacted = candidate.clone();
        format::compact(&mut compacted, operation, explain);
        let rendered =
            format::render_for_operation(&compacted, output_format, title, discovery, operation);
        success_text_bytes(&rendered) <= budget
    };

    if body_lines > 0 {
        for kept_leading in (0..=leading_lines).rev() {
            let leading_text = leading_parts[leading_lines - kept_leading..].concat();
            let mut low = 1;
            let mut high = body_lines;
            let mut best = None;
            while low <= high {
                let kept_body = low + (high - low) / 2;
                let body_text = body_parts[..kept_body].concat();
                let Some(candidate) = snippet_source_candidate(
                    value,
                    &source_page,
                    kept_leading,
                    kept_body,
                    &leading_text,
                    &body_text,
                ) else {
                    return false;
                };
                if fits(&candidate) {
                    best = Some((kept_body, candidate));
                    low = kept_body.saturating_add(1);
                } else {
                    high = kept_body - 1;
                }
            }
            if let Some((_, candidate)) = best {
                *value = candidate;
                return true;
            }
        }
    } else if body_offset == total_body_lines {
        for kept_leading in (0..=leading_lines).rev() {
            let leading_text = leading_parts[leading_lines - kept_leading..].concat();
            let Some(candidate) = snippet_source_candidate(
                value,
                &source_page,
                kept_leading,
                0,
                &leading_text,
                "",
            ) else {
                return false;
            };
            if fits(&candidate) {
                *value = candidate;
                return true;
            }
        }
    }

    false
}

struct SnippetSourcePage {
    node_line: usize,
    body_offset: usize,
    total_body_lines: usize,
    original_leading: usize,
    omitted_leading: usize,
}

fn snippet_source_candidate(
    original: &Value,
    page: &SnippetSourcePage,
    leading_lines: usize,
    body_lines: usize,
    leading: &str,
    body: &str,
) -> Option<Value> {
    let next = page.body_offset.checked_add(body_lines)?;
    let more = next < page.total_body_lines;
    let start_line = if leading_lines > 0 {
        page.node_line.checked_sub(leading_lines)?
    } else {
        page.node_line.checked_add(page.body_offset)?
    };
    let end_line = if body_lines > 0 {
        page.node_line.checked_add(next.checked_sub(1)?)
    } else if leading_lines > 0 {
        page.node_line.checked_sub(1)
    } else {
        None
    };
    let hint = if more {
        Some("Repeat the same tool and selector with show_source=true, next_source_offset as source_offset, and this generation.".to_owned())
    } else {
        None
    };
    let dropped_leading = page.original_leading.checked_sub(leading_lines)?;
    let omitted_leading = page.omitted_leading.checked_add(dropped_leading)?;
    let start_line_value =
        (!leading.is_empty() || !body.is_empty()).then_some(start_line);
    let mut candidate = original.clone();
    candidate["source"] = json!(format!("{leading}{body}"));
    {
        let preview = candidate.get_mut("source_preview")?.as_object_mut()?;
        preview.insert("start_line".into(), json!(start_line_value));
        preview.insert("end_line".into(), json!(end_line));
        preview.insert("body_lines".into(), json!(body_lines));
        preview.insert("leading_lines".into(), json!(leading_lines));
        preview.insert("omitted_leading_lines".into(), json!(omitted_leading));
        preview.insert(
            "omitted_body_lines".into(),
            json!(page.total_body_lines.saturating_sub(next)),
        );
        preview.insert("has_more".into(), json!(more));
        preview.insert("truncated_line".into(), json!(false));
        preview.insert(
            "next_source_offset".into(),
            json!(more.then_some(next)),
        );
        preview.insert("continuation_hint".into(), json!(hint));
    }
    if let (Some(path), Some(start), Some(end)) = (
        candidate["node"]["path"].as_str(),
        start_line_value.and_then(|line| u64::try_from(line).ok()),
        end_line.and_then(|line| u64::try_from(line).ok()),
    ) {
        candidate["header"] = json!(format!(
            "{} {} in {path}:{start}-{end}",
            candidate["node"]["kind"].as_str().unwrap_or("symbol"),
            candidate["node"]["qualname"]
                .as_str()
                .or_else(|| candidate["node"]["name"].as_str())
                .unwrap_or("symbol")
        ));
    } else if let Some(object) = candidate.as_object_mut() {
        object.remove("header");
    }
    Some(candidate)
}

pub(crate) fn rejected_arguments(fields: &[String]) -> CallToolResult {
    let fields = fields
        .iter()
        .take(16)
        .map(|field| field.chars().take(128).collect::<String>())
        .collect::<Vec<_>>();
    CallToolResult::error(vec![ContentBlock::text(
        json!({"error":{"code":"rejected_arguments","fields":fields}}).to_string(),
    )])
}

pub(crate) fn invalid_format(value: &Value, policy: &ResponsePolicy) -> CallToolResult {
    let budget = policy.max_output_bytes.clamp(1024, MAX_OUTPUT_BYTES) - ENVELOPE_RESERVE;
    let mut preview_chars = 128;
    loop {
        let display_value = match value {
            Value::String(value) => {
                let preview = value.chars().take(preview_chars).collect::<String>();
                let truncated = preview.chars().count() < value.chars().count();
                let encoded =
                    serde_json::to_string(&preview).unwrap_or_else(|_| "\"<string>\"".into());
                if truncated {
                    format!("{encoded} (truncated)")
                } else {
                    encoded
                }
            }
            Value::Array(_) => "<array>".to_owned(),
            Value::Object(_) => "<object>".to_owned(),
            _ => value.to_string(),
        };
        let message = format!("invalid format {display_value}; expected 'markdown' or 'json'");
        let result = CallToolResult::error(vec![ContentBlock::text(
            json!({
                "error": {
                    "code": "INVALID_FORMAT",
                    "message": message
                }
            })
            .to_string(),
        )]);
        if serialized_bytes(&result) <= budget {
            return result;
        }
        if !matches!(value, Value::String(_)) || preview_chars == 0 {
            return CallToolResult::error(vec![ContentBlock::text(
                json!({
                    "error": {
                        "code": "INVALID_FORMAT",
                        "message": "invalid format value; expected 'markdown' or 'json'"
                    }
                })
                .to_string(),
            )]);
        }
        preview_chars /= 2;
    }
}

/// Performs compact mcp metadata.
pub fn compact_mcp_metadata(value: &mut Value) {
    super::metadata::finalize_mcp_metadata(value);
}

/// Performs checkpoint result.
pub fn checkpoint_result(value: anyhow::Result<Value>, policy: &ResponsePolicy) -> CallToolResult {
    checkpoint_result_for(
        value,
        policy,
        OutputFormat::default(),
        "session_checkpoint",
        "",
    )
}

pub(crate) fn checkpoint_result_for(
    value: anyhow::Result<Value>,
    policy: &ResponsePolicy,
    output_format: OutputFormat,
    title: &str,
    action: &str,
) -> CallToolResult {
    match value {
        Ok(value) => {
            let budget = policy.max_output_bytes.clamp(1024, MAX_OUTPUT_BYTES) - ENVELOPE_RESERVE;
            let rendered = format::render_checkpoint(&value, output_format, title, action);
            if success_text_bytes(&rendered) <= budget {
                CallToolResult::success(vec![ContentBlock::text(rendered)])
            } else {
                bounded_error(
                    "Checkpoint output exceeds the response byte limit. Read the complete saved content from the local .forge/checkpoints.json file; checkpoint contents are unchanged and are not paginated.",
                    budget,
                )
            }
        }
        Err(error) => result_for(
            Err(error),
            policy,
            output_format,
            OperationKind::Checkpoint,
            title,
            None,
        ),
    }
}

/// Performs serialized bytes.
pub fn serialized_bytes(result: &CallToolResult) -> usize {
    let mut counter = JsonByteCounter::default();
    serde_json::to_writer(&mut counter, result).map_or(usize::MAX, |()| counter.bytes)
}

#[derive(Default)]
struct JsonByteCounter {
    bytes: usize,
    escaped_bytes: usize,
}

impl io::Write for JsonByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        for byte in bytes {
            self.escaped_bytes = self.escaped_bytes.saturating_add(match byte {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 1,
                0..=31 => 5,
                _ => 0,
            });
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn escaped_json_bytes(value: &Value) -> usize {
    let mut counter = JsonByteCounter::default();
    if serde_json::to_writer(&mut counter, value).is_err() {
        return usize::MAX;
    }
    counter.bytes.saturating_add(counter.escaped_bytes)
}

fn success_text_bytes(text: &str) -> usize {
    let result = CallToolResult::success(vec![ContentBlock::text(text)]);
    serialized_bytes(&result)
}

/// Performs enforce.
pub fn enforce(result: CallToolResult, policy: &ResponsePolicy) -> CallToolResult {
    let budget = policy.max_output_bytes.clamp(1024, MAX_OUTPUT_BYTES) - ENVELOPE_RESERVE;
    if serialized_bytes(&result) <= budget {
        return result;
    }
    let text = result
        .content
        .first()
        .and_then(ContentBlock::as_text)
        .map(|text| text.text.as_str());
    if result.is_error == Some(true) {
        bounded_error(
            text.unwrap_or("Tool error exceeds the response byte limit; narrow the request"),
            budget,
        )
    } else {
        bounded_error(
            "Tool output exceeds the response byte limit; narrow the request",
            budget,
        )
    }
}

/// Performs bounded error.
pub fn bounded_error(message: &str, budget: usize) -> CallToolResult {
    let mut end = message.len().min(budget);
    loop {
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        let text = if end < message.len() {
            format!("{} [error truncated; narrow the request]", &message[..end])
        } else {
            message.to_owned()
        };
        let result = CallToolResult::error(vec![ContentBlock::text(text)]);
        if serialized_bytes(&result) <= budget || end == 0 {
            return result;
        }
        end /= 2;
    }
}

fn trim_page_tails(value: &mut Value, excess: &mut usize) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let mut trimmed = false;
    if object.contains_key("total")
        && (object.contains_key("next_offset")
            || object.contains_key("offset")
            || object.contains_key("has_more"))
    {
        if let Some(items) = object.get_mut("items").and_then(Value::as_array_mut) {
            while items.len() > 1 && *excess > 0 {
                let removed = items.pop().expect("page tail exists");
                *excess = excess.saturating_sub(escaped_json_bytes(&removed).saturating_add(1));
                trimmed = true;
            }
            if trimmed {
                let returned = items.len();
                let offset = object.get("offset").and_then(Value::as_u64).unwrap_or(0);
                let next = offset.saturating_add(returned as u64);
                object.insert("has_more".into(), json!(true));
                object.insert("next_offset".into(), json!(next));
                object.insert("byte_limited".into(), json!(true));
                object.insert("continuation_hint".into(), json!(format!("Repeat the same tool and filters with offset={next} and this page's generation; items after this offset were omitted to respect max_output_bytes.")));
            }
        }
    }
    if *excess == 0 {
        return trimmed;
    }
    // Preserve scalar node metadata, source continuation and freshness evidence.
    // Collection boundaries are produced by the database; arbitrary arrays are not pages.
    for child in object.values_mut() {
        trimmed |= trim_page_tails(child, excess);
        if *excess == 0 {
            break;
        }
    }
    trimmed
}

/// Performs stdio.
pub fn stdio() -> (tokio::io::Stdin, BoundedWriter<tokio::io::Stdout>) {
    (tokio::io::stdin(), BoundedWriter::new(tokio::io::stdout()))
}

/// Buffer a complete JSON-RPC line before publishing any of its bytes.
/// Oversized protocol errors are replaced before any bytes reach the client.
pub struct BoundedWriter<W> {
    writer: W,
    line: Vec<u8>,
    pending: Vec<u8>,
    written: usize,
    overflow: bool,
}

impl<W> BoundedWriter<W> {
    /// Creates a new instance.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            line: Vec::new(),
            pending: Vec::new(),
            written: 0,
            overflow: false,
        }
    }
}

impl<W: AsyncWrite + Unpin> BoundedWriter<W> {
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.written < self.pending.len() {
            let count = std::task::ready!(
                Pin::new(&mut self.writer).poll_write(cx, &self.pending[self.written..])
            )?;
            if count == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.written += count;
        }
        self.pending.clear();
        self.written = 0;
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for BoundedWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        std::task::ready!(this.drain(cx))?;
        let count = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |at| at + 1);
        let accepted = count.min(MAX_OUTPUT_BYTES.saturating_sub(this.line.len()));
        this.overflow |= accepted < count;
        this.line.extend_from_slice(&bytes[..accepted]);
        if count > 0 && bytes[count - 1] == b'\n' {
            if this.overflow {
                this.pending = protocol_limit_error(&this.line);
                this.line.clear();
                this.overflow = false;
            } else {
                std::mem::swap(&mut this.line, &mut this.pending);
            }
        }
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.drain(cx))?;
        Pin::new(&mut this.writer).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.drain(cx))?;
        if !this.line.is_empty() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete JSON-RPC response",
            )));
        }
        Pin::new(&mut this.writer).poll_shutdown(cx)
    }
}

fn protocol_limit_error(prefix: &[u8]) -> Vec<u8> {
    // rmcp serializes the top-level id before result/error. Decode only that value;
    // the retained prefix may end inside a large message or an oversized id.
    let id = prefix
        .windows(5)
        .position(|part| part == b"\"id\":")
        .and_then(|at| {
            let mut decoder = serde_json::Deserializer::from_slice(&prefix[at + 5..]);
            <Value as serde::Deserialize>::deserialize(&mut decoder).ok()
        })
        .filter(|id| {
            matches!(id, Value::String(_) | Value::Number(_)) && id.to_string().len() <= 128
        })
        .unwrap_or(Value::Null);
    let mut bytes = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"Response exceeds 65536 bytes. Narrow the request arguments or output. Request ids longer than 128 bytes are omitted from this error."}}).to_string().into_bytes();
    bytes.push(b'\n');
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_size_matches_the_serialized_mcp_envelope() {
        let value = json!({"message":"quotes: \"; slash: \\; newline: \n; unicode: λ", "items":[1, true, null]});
        let text = value.to_string();
        let actual = CallToolResult::success(vec![ContentBlock::text(text.clone())]);
        assert_eq!(success_text_bytes(&text), serialized_bytes(&actual));
    }

    #[test]
    fn interrupted_sqlite_query_reports_a_recovery_path() {
        let sqlite_error = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERRUPT),
            None,
        );
        let result = result(Err(sqlite_error.into()), &ResponsePolicy::default());
        assert_eq!(result.is_error, Some(true));
        let message = result.content[0].as_text().unwrap().text.as_str();
        assert!(message.contains("2-second SQLite budget"), "{message}");
        assert!(message.contains("reduce traversal depth"), "{message}");
    }
}
