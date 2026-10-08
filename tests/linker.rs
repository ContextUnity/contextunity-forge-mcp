#[path = "common/mod.rs"]
pub mod common;
#[path = "linker/method_semantics.rs"]
mod method_semantics;
#[path = "linker/optimizations.rs"]
mod optimizations;
#[path = "linker/overload_disambiguation.rs"]
mod overload_disambiguation;
#[path = "linker/receivers/inherited.rs"]
mod inherited_receivers;
#[path = "linker/receivers/typed.rs"]
mod typed_receivers;
