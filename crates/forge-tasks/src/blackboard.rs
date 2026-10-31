//! Agent coordination blackboard and shared context records.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::engine::blackboard::{
    blackboard, BlackboardAction, BlackboardRequest, BlackboardScope,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A persistent record on the coordination blackboard.
pub struct BlackboardEntry {
    /// Auto-incrementing entry ID.
    pub id: i64,
    /// Scope boundary.
    pub scope: BlackboardScope,
    /// Associated reference (task ID or milestone ID).
    pub reference: String,
    /// Authoring agent or worker identity.
    pub author: String,
    /// Topic or message classification.
    pub topic: String,
    /// Markdown or JSON payload text.
    pub message: String,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}
