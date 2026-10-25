#![cfg(feature = "lang-rust")]

use contextunity_forge_mcp::engine::{ast, linker};
use std::collections::BTreeMap;

mod builtin_receivers;
mod deref_pointers;
mod field_chains;
mod iterators;
mod module_paths;
mod trait_dispatch;
mod value_flow;
