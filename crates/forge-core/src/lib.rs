//! Pure Rust ContextUnity Forge domain models, interner, commitments, and configuration.

pub mod commitments;
pub mod config;
pub mod format;
pub mod fs;
pub mod git;
pub mod interner;
pub mod models;
pub mod response;
pub mod semantic;
pub mod subsystems;
pub mod sys;
pub mod typed_facts;

pub use commitments::{DomainCommitment, MerkleCommitment};
pub use config::{Detail, ResponsePolicy, ScannerLimits, SourceContextPolicy};
pub use format::stable_hash64;
pub use interner::{LinkerInterner, PathId, SymbolId};
pub use models::{Coverage, Edge, Facts, FileEntry, Graph, Node, Reference, is_test};
pub use models::NodeSemanticMeta;
pub use typed_facts::{NodeSemanticEntry, SemanticStore};

/// Returns the forge-core crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
