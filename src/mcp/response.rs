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
    result_with_symbol(value, policy, None)
}

pub(crate) fn result_with_symbol(
    value: anyhow::Result<Value>,
    policy: &ResponsePolicy,
    explain: Option<bool>,
) -> CallToolResult {
    let budget = policy.max_output_bytes.clamp(1024, MAX_OUTPUT_BYTES) - ENVELOPE_RESERVE;
    match value {
        Ok(mut value) => {
            super::metadata::finalize_mcp_metadata_for_symbol(&mut value, explain);
            let output_too_large = || {
                bounded_error(
                    "Output exceeds the response byte limit. Use detail='compact', a smaller limit, or a narrower selector/SQL projection. A single oversized item must be read from its local file; no items were skipped and no continuation was consumed.",
                    budget,
                )
            };
            let size = success_json_bytes(&value);
            if size > budget {
                let mut excess = size.saturating_sub(budget).saturating_add(256);
                if !trim_page_tails(&mut value, &mut excess) || success_json_bytes(&value) > budget
                {
                    return output_too_large();
                }
            }
            CallToolResult::success(vec![ContentBlock::text(value.to_string())])
        }
        Err(error) => {
            if error.is::<crate::db::tasks_store::TaskAlreadyClaimed>() {
                return CallToolResult::error(vec![ContentBlock::text(serde_json::json!({"error":{"code":"TASK_ALREADY_CLAIMED","message":"Task already has an active claim"}}).to_string())]);
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

/// Performs compact mcp metadata.
pub fn compact_mcp_metadata(value: &mut Value) {
    super::metadata::finalize_mcp_metadata(value);
}

/// Performs checkpoint result.
pub fn checkpoint_result(value: anyhow::Result<Value>, policy: &ResponsePolicy) -> CallToolResult {
    match value {
        Ok(value) => {
            let budget = policy.max_output_bytes.clamp(1024, MAX_OUTPUT_BYTES) - ENVELOPE_RESERVE;
            if success_json_bytes(&value) <= budget {
                CallToolResult::success(vec![ContentBlock::text(value.to_string())])
            } else {
                bounded_error("Checkpoint output exceeds the response byte limit. Read the complete saved content from the local .forge/checkpoints.json file; checkpoint contents are unchanged and are not paginated.", budget)
            }
        }
        Err(error) => result(Err(error), policy),
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

fn success_json_bytes(value: &Value) -> usize {
    let empty = CallToolResult::success(vec![ContentBlock::text("")]);
    serialized_bytes(&empty).saturating_add(escaped_json_bytes(value))
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
        let actual = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
        assert_eq!(success_json_bytes(&value), serialized_bytes(&actual));
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
