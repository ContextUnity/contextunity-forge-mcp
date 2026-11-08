//! ContextUnity Forge AST extraction, linking, and index persistence.

#![allow(dead_code, unused_imports)]

pub mod ast;
pub mod db;
pub mod docs;
pub mod languages;
pub mod linker;
pub mod scanner;

pub use ast::AstToken;
pub use db::{initialize_code_map_schema, CODE_MAP_SCHEMA_VERSION, SCHEMA_DDL};
pub use languages::LanguageRegistry;
pub use linker::RouteIndex;

// Compatibility module aliases for internal references across extracted modules
pub(crate) mod core {
    pub use forge_core::*;
    pub mod commitments {
        pub use crate::db::commitments::*;
        pub use forge_core::commitments::*;
    }
    pub mod schema {
        pub use crate::db::schema::*;
    }
}

pub(crate) mod engine {
    pub use crate::ast;
    pub use crate::docs;
    pub use crate::languages;
    pub use crate::linker;
    pub use crate::scanner;
}

/// Returns the forge-index crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
