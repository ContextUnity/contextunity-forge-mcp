//! Shared models for layered reads.

use serde::{Deserialize, Serialize};

/// Requested reader operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    /// Select a mode from the request.
    #[default]
    Auto,
    /// Extract an indexed symbol.
    Snippet,
    /// Show a structural or document outline.
    Outline,
    /// Read a line window.
    Lines,
    /// Read unprocessed text.
    Raw,
    /// Search text and surrounding context.
    Search,
    /// Show changes against a Git baseline.
    Delta,
}

/// Optional bounds on a text read; lines are one-based.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadBounds {
    /// First line to include.
    pub start_line: Option<usize>,
    /// Maximum number of lines to include.
    pub line_count: Option<usize>,
}

/// Text and metadata returned by a reader operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadResult {
    /// Workspace-relative path selected by the request.
    pub path: String,
    /// Operation that produced the content.
    pub mode: ReadMode,
    /// Rendered text or diff.
    pub content: String,
    /// True when the response does not contain all requested content.
    pub truncated: bool,
}
