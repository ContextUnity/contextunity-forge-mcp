//! SQLite index database storage, schema management, and query helpers.

pub mod cache;
pub mod commitments;
pub mod cycles;
pub mod lint;
pub mod paging;
pub mod reader;
pub mod schema;
pub mod symbols;
pub mod traversal;
pub mod writer;

pub use schema::{initialize_code_map_schema, CODE_MAP_SCHEMA_VERSION, SCHEMA_DDL};
pub use writer::delta_persist as delta;
