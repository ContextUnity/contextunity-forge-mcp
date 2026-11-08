use super::harness::{ExtractorHarness, LinkedFacts};
use contextunity_forge_mcp::core::models::{Coverage, Graph};

mod cargo_ecosystem;
mod closures;
mod dynamic_receivers;
mod factory_receivers;
mod macros;
mod payloads;
mod receivers;
mod syntax;
mod workspace_factories;

fn linked(path: &str, source: &str) -> LinkedFacts {
    ExtractorHarness::for_lang("rust")
        .at_path(path)
        .linked(source)
}

fn coverage(graph: &Graph, expression: impl AsRef<str>) -> &Coverage {
    let expression = expression.as_ref();
    graph
        .coverage
        .iter()
        .find(|row| row.expression == expression)
        .unwrap_or_else(|| panic!("missing {expression}: {:#?}", graph.coverage))
}
