/// Implements ast support.
pub use forge_index::ast;
/// Implements docs support.
pub use forge_index::docs;
/// Implements languages support.
pub use forge_index::languages;
/// Implements linker support.
pub use forge_index::linker;
/// Implements milestone lifecycle support.
pub use forge_tasks::milestones;
/// Indexing pipeline orchestration.
pub mod pipeline;
/// Shared layered file and symbol reader for CLI and MCP.
pub mod reader;
/// Implements scanner support.
pub use forge_index::scanner;
/// Implements tasks support.
pub mod tasks;
