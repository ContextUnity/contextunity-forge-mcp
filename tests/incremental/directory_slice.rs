#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::response::{CoverageOptions, QueryOptions, ResponsePolicy},
    db::{reader, traversal},
};
use std::time::Instant;

use crate::common::Workspace;

fn options(limit: usize) -> QueryOptions {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None).unwrap()
}

#[test]
fn directory_suffix_lists_real_nodes_and_exact_file_keeps_graph_traversal() {
    let ws = Workspace::new();
    let file = "extensions/commerce/src/contextunity/commerce/ingestion/worker.py";
    ws.write(file, "from contextunity.commerce.connectors.horoshop.client import fetch\ndef publish():\n    return fetch()\n");
    ws.write(
        "extensions/commerce/src/contextunity/commerce/consumer.py",
        "from contextunity.commerce.ingestion.worker import publish\ndef consume():\n    return publish()\n",
    );
    ws.write(
        "extensions/commerce/src/contextunity/commerce/ingestion2/unrelated.py",
        "def unrelated(): pass\n",
    );
    ws.write(
        "alpha/a.py",
        "from beta.b import target\nfrom data.db import run_query\nfrom foundation.reader import read\ndef run():\n    target()\n    read()\n    return run_query()\n",
    );
    ws.write("beta/b.py", "def target():\n    return 1\n");
    ws.write(
        "alpha/schema.sql",
        "CREATE TABLE alpha_table (id INTEGER);\n",
    );
    ws.write(
        "foundation/schema.sql",
        "CREATE TABLE foundation_table (id INTEGER);\n",
    );
    ws.write("foundation/reader.py", "def read(): pass\n");
    ws.write(
        "domain/model.py",
        "def inner(): pass\ndef outer(): inner()\n",
    );
    ws.write(
        "data/db.py",
        "from beta.b import target\ndef run_query():\n    return target()\n",
    );
    ws.write("data/schema.sql", "CREATE TABLE accounts (id INTEGER);\n");
    for index in 1..=3 {
        ws.write(format!("dependency{index}/dep.py"), "def work(): pass\n");
        ws.write(
            format!("client{index}/caller.py"),
            "from coordinator.core import run\ndef call(): run()\n",
        );
    }
    ws.write("coordinator/core.py", "from dependency1.dep import work as first\nfrom dependency2.dep import work as second\nfrom dependency3.dep import work as third\ndef run():\n    first()\n    second()\n    third()\n");
    ws.write(
        "extensions/commerce/src/contextunity/commerce/connectors/horoshop/client.py",
        "def fetch(): pass\n",
    );
    ws.write(
        "extensions/commerce/src/contextunity/commerce/pim/views/products/editor/views.py",
        "def edit(): pass\n",
    );
    ws.build();
    let conn = ws.open();
    let projection_exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='edge_endpoint_paths'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(projection_exists, 1, "production index must contain endpoint adjacency");
    let edge_count: i64 = conn.query_row("SELECT count(*) FROM edges", [], |row| row.get(0)).unwrap();
    let endpoint_count: i64 = conn.query_row("SELECT count(*) FROM edge_endpoint_paths", [], |row| row.get(0)).unwrap();
    assert_eq!(endpoint_count, 2 * edge_count);
    let result = traversal::query_with_options(
        &conn,
        "slice",
        Some("contextunity/commerce/ingestion"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap();
    assert_eq!(result["scope"], "directory_members");
    assert_eq!(
        result["selector"]["path"],
        "extensions/commerce/src/contextunity/commerce/ingestion"
    );
    assert_eq!(result["requested_depth"], 1);
    assert_eq!(result["graph_depth_applied"], 0);
    assert!(result["edges"]["items"].is_array(), "{result}");
    for field in ["exported_symbols", "inbound_modules", "outbound_modules"] {
        assert!(
            result["boundary_contract"][field]["items"].is_array(),
            "{field}: {result}"
        );
    }
    let boundary = &result["boundary_contract"];
    assert!(
        result["edges"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "contains"
                && edge["dst_public_id"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("publish")),
        "{result}"
    );
    assert!(
        boundary["exported_symbols"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "publish" && node["external_caller_count"] == 2),
        "{boundary}"
    );
    let incoming = boundary["inbound_modules"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| module["module"] == "extensions/commerce/src/contextunity/commerce")
        .unwrap();
    assert_eq!(incoming["edge_count"], 3);
    assert!(incoming["symbol_pairs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| pair["kind"] == "calls"
            && pair["source_id"].as_str().unwrap().contains("consume")
            && pair["target_id"].as_str().unwrap().contains("publish")));
    let outgoing = &boundary["outbound_modules"]["items"][0];
    assert_eq!(outgoing["edge_count"], 3);
    assert!(outgoing["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| pair["kind"] == "calls" && pair["name"] == "fetch"));
    for field in ["exported_symbols", "inbound_modules", "outbound_modules"] {
        let expected = boundary[field]["items"].as_array().unwrap();
        let mut actual = Vec::new();
        let mut generation = None;
        for offset in 0..expected.len() {
            let page = QueryOptions::resolve(
                &ResponsePolicy::default(),
                Some(1),
                offset,
                None,
                generation.clone(),
            )
            .unwrap();
            let part = traversal::query_with_options(
                &conn,
                "slice",
                Some("contextunity/commerce/ingestion"),
                &traversal::GraphQueryOptions {
                    depth: 1,
                    direction: None,
                    coverage: CoverageOptions::default(),
                    page: &page,
                },
            )
            .unwrap();
            if generation.is_none() {
                generation = part["boundary_contract"][field]["generation"]
                    .as_str()
                    .map(str::to_owned);
            }
            actual.extend(
                part["boundary_contract"][field]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
        }
        assert_eq!(&actual, expected, "{field}");
    }
    let expected_edges = result["edges"]["items"].as_array().unwrap();
    let mut actual_edges = Vec::new();
    let mut generation = None;
    for offset in 0..expected_edges.len() {
        let page = QueryOptions::resolve(
            &ResponsePolicy::default(),
            Some(1),
            offset,
            None,
            generation.clone(),
        )
        .unwrap();
        let part = traversal::query_with_options(
            &conn,
            "slice",
            Some("contextunity/commerce/ingestion"),
            &traversal::GraphQueryOptions {
                depth: 1,
                direction: None,
                coverage: CoverageOptions::default(),
                page: &page,
            },
        )
        .unwrap();
        if generation.is_none() {
            generation = part["edges"]["generation"].as_str().map(str::to_owned);
        }
        actual_edges.extend(part["edges"]["items"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(&actual_edges, expected_edges);
    assert!(
        boundary["inbound_modules"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|module| module["module"] == "extensions/commerce/src/contextunity/commerce"),
        "{boundary}"
    );
    assert!(
        boundary["outbound_modules"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|module| module["module"]
                == "extensions/commerce/src/contextunity/commerce/connectors/horoshop"),
        "{boundary}"
    );
    assert!(!boundary["exported_symbols"]
        .to_string()
        .contains("unrelated"));
    let aspects = vec!["topology".to_owned()];
    let topology = reader::overview_with_options(
        &conn,
        &reader::OverviewOptions {
            aspects: Some(&aspects),
            page: &options(100),
        },
    )
    .unwrap();
    let modules = topology["topology"]["modules"]["items"].as_array().unwrap();
    assert!(
        modules
            .iter()
            .any(|module| module["module"] == "extensions/commerce"
                && module["symbol_count"].as_u64().unwrap() > 0),
        "{topology}"
    );
    assert!(
        modules.iter().any(|module| module["module"] == "domain"
            && module["role"] == "domain"
            && module["internal_edges"].as_u64().unwrap() > 0),
        "{topology}"
    );
    assert!(
        modules
            .iter()
            .any(|module| module["module"] == "coordinator" && module["role"] == "orchestrator"),
        "{topology}"
    );
    assert!(
        modules
            .iter()
            .any(|module| module["module"] == "data" && module["role"] == "data_access"),
        "{topology}"
    );
    let edge = topology["topology"]["module_edges"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["source_module"] == "alpha" && edge["target_module"] == "beta")
        .unwrap();
    let expected_weight: i64 = conn.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes t ON t.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary tp ON tp.path_id=t.path_id WHERE sp.path='alpha/a.py' AND tp.path='beta/b.py'", [], |row| row.get(0)).unwrap();
    assert_eq!(edge["edge_count"], expected_weight);
    let dominant: String = conn.query_row("SELECT e.kind FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes t ON t.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary tp ON tp.path_id=t.path_id WHERE sp.path='alpha/a.py' AND tp.path='beta/b.py' GROUP BY e.kind ORDER BY count(*) DESC,e.kind ASC LIMIT 1", [], |row| row.get(0)).unwrap();
    assert_eq!(edge["dominant_kind"], dominant);
    let alpha = modules
        .iter()
        .find(|module| module["module"] == "alpha")
        .unwrap();
    let internal = alpha["internal_edges"].as_f64().unwrap();
    let external = alpha["inbound_external_edges"].as_f64().unwrap()
        + alpha["outbound_external_edges"].as_f64().unwrap();
    assert!(
        (alpha["cohesion_ratio"].as_f64().unwrap() - internal / (internal + external)).abs()
            < 0.000001
    );
    assert!(
        modules.iter().any(|module| module["module"] == "alpha"
            && module["table_count"].as_u64().unwrap() > 0
            && module["role"] == "adapter_or_entrypoint"),
        "{topology}"
    );
    assert!(
        modules
            .iter()
            .any(|module| module["module"] == "beta" && module["role"] == "foundation"),
        "{topology}"
    );
    assert!(
        modules.iter().any(|module| module["module"] == "foundation"
            && module["table_count"].as_u64().unwrap() > 0
            && module["role"] == "foundation"),
        "{topology}"
    );
    assert_eq!(
        topology,
        reader::overview_with_options(
            &conn,
            &reader::OverviewOptions {
                aspects: Some(&aspects),
                page: &options(100)
            }
        )
        .unwrap()
    );
    for field in ["modules", "module_edges"] {
        let expected = topology["topology"][field]["items"].as_array().unwrap();
        let mut actual = Vec::new();
        let mut generation = None;
        for offset in 0..expected.len() {
            let page = QueryOptions::resolve(
                &ResponsePolicy::default(),
                Some(1),
                offset,
                None,
                generation.clone(),
            )
            .unwrap();
            let part = reader::overview_with_options(
                &conn,
                &reader::OverviewOptions {
                    aspects: Some(&aspects),
                    page: &page,
                },
            )
            .unwrap();
            if generation.is_none() {
                generation = part["topology"][field]["generation"]
                    .as_str()
                    .map(str::to_owned);
            }
            actual.extend(
                part["topology"][field]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
        }
        assert_eq!(&actual, expected, "{field}");
    }
    let stale = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(1),
        1,
        None,
        Some("stale".into()),
    )
    .unwrap();
    assert!(reader::overview_with_options(
        &conn,
        &reader::OverviewOptions {
            aspects: Some(&aspects),
            page: &stale
        }
    )
    .is_err());
    assert!(traversal::query_with_options(
        &conn,
        "slice",
        Some("contextunity/commerce/ingestion"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &stale
        }
    )
    .is_err());
    assert!(result["nodes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["path"] == file));
    for (selector, expected) in [
        (
            "contextunity/commerce/connectors/horoshop",
            "extensions/commerce/src/contextunity/commerce/connectors/horoshop",
        ),
        (
            "contextunity/commerce/pim/views/products/editor",
            "extensions/commerce/src/contextunity/commerce/pim/views/products/editor",
        ),
    ] {
        let scoped = traversal::query_with_options(
            &conn,
            "slice",
            Some(selector),
            &traversal::GraphQueryOptions {
                depth: 1,
                direction: None,
                coverage: CoverageOptions::default(),
                page: &options(20),
            },
        )
        .unwrap();
        assert_eq!(scoped["selector"]["path"], expected);
        assert_eq!(scoped["nodes"]["total"], 2);
    }
    let exact = traversal::query_with_options(
        &conn,
        "slice",
        Some(file),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap();
    assert_eq!(exact["depth"], 1);
    assert!(exact.get("scope").is_none());
}

#[test]
fn duplicate_directory_suffix_requires_full_path_and_unknown_path_suggests_indexed_file() {
    let ws = Workspace::new();
    ws.write(
        "extensions/commerce/src/contextunity/commerce/ingestion/a.py",
        "def a(): pass\n",
    );
    ws.write(
        "services/api/src/contextunity/commerce/ingestion/b.py",
        "def b(): pass\n",
    );
    ws.build();
    let conn = ws.open();
    let error = traversal::query_with_options(
        &conn,
        "slice",
        Some("contextunity/commerce/ingestion"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("ambiguous selector"), "{error}");
    assert!(
        error.contains("extensions/commerce/src/contextunity/commerce/ingestion"),
        "{error}"
    );
    assert!(
        error.contains("services/api/src/contextunity/commerce/ingestion"),
        "{error}"
    );
    let exact = traversal::query_with_options(
        &conn,
        "slice",
        Some("extensions/commerce/src/contextunity/commerce/ingestion"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap();
    assert_eq!(exact["nodes"]["total"], 2);
    let unknown = traversal::query_with_options(
        &conn,
        "slice",
        Some("contextunity/commerce/ingest"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(20),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(unknown.contains("indexed path suggestions"), "{unknown}");
    assert!(unknown.contains("ingestion/a.py"), "{unknown}");
}

#[test]
fn large_directory_is_paged_without_graph_expansion() {
    let ws = Workspace::new();
    let mut code = String::new();
    for index in 0..1100 {
        code.push_str(&format!("def item_{index:04}(): pass\n"));
    }
    ws.write("pkg/large/items.py", &code);
    ws.build();
    let conn = ws.open();
    let started = Instant::now();
    let first = traversal::query_with_options(
        &conn,
        "slice",
        Some("pkg/large"),
        &traversal::GraphQueryOptions {
            depth: 16,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(1),
        },
    )
    .unwrap();
    assert_eq!(first["nodes"]["total"], 1101);
    assert_eq!(first["nodes"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["nodes"]["has_more"], true);
    assert_eq!(first["graph_depth_applied"], 0);
    println!(
        "directory_slice_large_query_ms={:.3}",
        started.elapsed().as_secs_f64() * 1000.0
    );
    drop(conn);
    let writer = rusqlite::Connection::open(ws.db()).unwrap();
    writer.execute_batch("DELETE FROM edges").unwrap();
    let empty = traversal::query_with_options(
        &writer,
        "slice",
        Some("pkg/large"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(1),
        },
    )
    .unwrap();
    assert_eq!(empty["edges"]["total"], 0);
    assert_eq!(empty["boundary_contract"]["exported_symbols"]["total"], 0);
    writer.execute_batch("INSERT OR IGNORE INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) SELECT a.node_hash,'calls',b.node_hash,a.path_id,1,(SELECT evidence_id FROM coverage_evidence LIMIT 1),(SELECT evidence_id FROM coverage_evidence LIMIT 1),1 FROM nodes a CROSS JOIN nodes b WHERE a.kind='function' AND b.kind='function' LIMIT 10001").unwrap();
    let conn = writer;
    let dense = traversal::query_with_options(
        &conn,
        "slice",
        Some("pkg/large"),
        &traversal::GraphQueryOptions {
            depth: 1,
            direction: None,
            coverage: CoverageOptions::default(),
            page: &options(1),
        },
    )
    .unwrap();
    assert_eq!(dense["edges"]["total"], 10001);
    assert_eq!(dense["edges"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(dense["edges"]["has_more"], true);
}
