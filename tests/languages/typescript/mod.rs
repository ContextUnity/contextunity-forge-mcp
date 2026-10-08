#![cfg(feature = "lang-typescript")]

use contextunity_forge_mcp::{
    core::models::Facts,
    engine::{ast, linker},
};
use std::collections::BTreeMap;

fn calls(files: &[(&str, &str)], expression: &str) -> Vec<String> {
    let facts: BTreeMap<String, Facts> = files
        .iter()
        .map(|(path, source)| {
            (
                (*path).to_owned(),
                ast::extract(
                    path,
                    if path.ends_with(".js") {
                        "javascript"
                    } else {
                        "typescript"
                    },
                    source,
                )
                .unwrap(),
            )
        })
        .collect();
    let graph = linker::link(&facts);
    graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "calls" && edge.evidence == expression)
        .map(|edge| {
            facts
                .values()
                .flat_map(|facts| &facts.nodes)
                .find(|node| node.id == edge.dst)
                .unwrap()
                .path
                .clone()
        })
        .collect()
}

fn resolves(source: &str, expression: &str) -> bool {
    let facts = ast::extract("service.ts", "typescript", source).unwrap();
    linker::link(&BTreeMap::from([("service.ts".to_owned(), facts)]))
        .edges
        .iter()
        .any(|edge| edge.kind == "calls" && edge.evidence == expression)
}

mod dom;
mod frameworks;
mod grammar;
mod modules;
