use super::*;
use crate::languages::harness::{query_scalar, ExtractorHarness};
use contextunity_forge_mcp::db::reader;
use std::collections::BTreeMap;

const INITIAL: &str = "<script>class Alpha { work() {} }\nclass Beta { work() {} }\nconst worker = new Alpha();</script>\n<script>worker.work();</script>\n";
const UPDATED: &str = "<script>class Alpha { work() {} }\nclass Beta { work() {} }\nconst worker = new Beta();</script>\n<script>worker.work();</script>\n";
const SLOTS: &str = "<script>class Worker { run() {} }\nconst worker = new Worker();</script>\n";

fn class_method(facts: &contextunity_forge_mcp::core::models::Facts, class: &str) -> String {
    let method = facts
        .nodes
        .iter()
        .find(|node| {
            node.kind == "method"
                && node.name == "work"
                && node.qualname.ends_with(&format!(".{class}.work"))
        })
        .unwrap_or_else(|| panic!("class method {class} missing: {facts:#?}"));
    method.id.clone()
}

fn rows(conn: &rusqlite::Connection) -> Vec<String> {
    let mut statement = conn.prepare(
        "SELECT json_array((SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind,(SELECT path FROM path_dictionary WHERE path_id=edges.path_id),line,(SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id),(SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.confidence_id)) FROM edges WHERE (SELECT path FROM path_dictionary WHERE path_id=edges.path_id)='index.html' ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
    ).unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

#[test]
fn classic_flow_wire_slots_and_constructor_receiver_survive_cold_build_and_delta() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("index.html", INITIAL);
    workspace.write("slots.html", SLOTS);

    let direct = ExtractorHarness::for_lang("html")
        .at_path("index.html")
        .extract(INITIAL);
    let alpha = class_method(&direct, "Alpha");
    let graph =
        ExtractorHarness::link_graph(&BTreeMap::from([("index.html".to_owned(), direct.clone())]));
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == "worker.work"
            && edge.dst == alpha),
        "{graph:#?}"
    );

    let conn = workspace.build_and_open();
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path='slots.html'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let facts = reader::decode_local_facts(&blob).unwrap();
    let projected = serde_json::to_value(&facts).unwrap();
    let scope = projected["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["details"]["classic_global"] == true)
        .unwrap();
    let details = serde_json::to_string(&scope["details"]).unwrap();
    let flow = details.split("\"value_flow\":").nth(1).unwrap();
    assert!(flow.starts_with("{\"bindings\":["), "{details}");
    assert!(flow.contains("],\"fields\":null"), "{details}");

    let expected_alpha: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE (SELECT path FROM path_dictionary WHERE path_id=edges.path_id)='index.html' AND kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id)='worker.work' AND (SELECT id FROM nodes WHERE node_hash=dst_hash)=?1",
        [&alpha], |row| row.get(0),
    ).unwrap();
    assert_eq!(expected_alpha, 1);
    drop(conn);

    workspace.write("index.html", UPDATED);
    workspace.delta(&["index.html"]);
    let updated = ExtractorHarness::for_lang("html")
        .at_path("index.html")
        .extract(UPDATED);
    let beta = class_method(&updated, "Beta");
    let conn = workspace.open();
    let expected_beta: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE (SELECT path FROM path_dictionary WHERE path_id=edges.path_id)='index.html' AND kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id)='worker.work' AND (SELECT id FROM nodes WHERE node_hash=dst_hash)=?1",
        [&beta], |row| row.get(0),
    ).unwrap();
    assert_eq!(expected_beta, 1);
    drop(conn);

    let cold = workspace.root().join("cold.sqlite");
    workspace.build_to(&cold);
    assert_eq!(rows(&workspace.open()), rows(&workspace.open_at(&cold)));
}

#[cfg(feature = "lang-typescript")]
#[test]
fn inline_handlers_resolve_exact_local_classic_script_providers() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("site/assets/cart.js", "function openCart() {}\n");
    workspace.write(
        "site/index.html",
        "<script src='./assets/cart.js'></script><button onclick='openCart()'>Cart</button>\n",
    );
    for (name, script) in [
        (
            "module",
            "<script type='module' src='./assets/cart.js'></script>",
        ),
        ("async", "<script async src='./assets/cart.js'></script>"),
        ("defer", "<script defer src='./assets/cart.js'></script>"),
        ("dynamic", "<script src='./assets/{name}.js'></script>"),
        ("unimported", ""),
    ] {
        workspace.write(
            format!("site/{name}.html"),
            &format!("{script}<button onclick='openCart()'>Cart</button>\n"),
        );
    }
    workspace.build();
    let conn = workspace.open();
    let linked: i64 = query_scalar(&conn, "SELECT count(*) FROM edges e JOIN path_dictionary owner ON owner.path_id=e.path_id JOIN coverage_evidence evidence ON evidence.evidence_id=e.evidence_id JOIN nodes target ON target.node_hash=e.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE owner.path='site/index.html' AND e.kind='calls' AND evidence.evidence='openCart' AND target_path.path='site/assets/cart.js' AND target.kind='function'", []);
    assert_eq!(
        linked, 1,
        "the handler call links to the exact indexed script provider"
    );
    for name in ["module", "async", "defer", "dynamic", "unimported"] {
        let path = format!("site/{name}.html");
        let status: String = query_scalar(&conn, "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression='openCart'", [path.as_str()]);
        assert_eq!(status, "unresolved", "{name}");
    }
    drop(conn);
    workspace.write("site/assets/duplicate.js", "function openCart() {}\n");
    workspace.write(
        "site/index.html",
        "<script src='./assets/cart.js'></script><script src='./assets/duplicate.js'></script><button onclick='openCart()'>Cart</button>\n",
    );
    workspace.delta(&["site/index.html", "site/assets/duplicate.js"]);
    let conn = workspace.open();
    let status: String = query_scalar(&conn, "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='site/index.html' AND ce.expression='openCart'", []);
    assert_eq!(status, "ambiguous");
    let incremental_aggregates = coverage_aggregate_rows(&conn);
    assert!(
        !incremental_aggregates.0.is_empty(),
        "the inline JavaScript handler exercises owner-language override persistence"
    );
    commitments::verify(&conn).unwrap();
    drop(conn);
    let cold = crate::languages::support::fresh_cold(&workspace);
    let cold_status: String = query_scalar(&cold, "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='site/index.html' AND ce.expression='openCart'", []);
    assert_eq!(cold_status, status);
    assert_eq!(incremental_aggregates, coverage_aggregate_rows(&cold));
    commitments::verify(&cold).unwrap();
}

#[cfg(feature = "lang-typescript")]
#[test]
fn inline_handler_provider_rebinding_fails_closed_after_exact_declaration() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    let script = "function openCart() {}\nopenCart = unknown;\n";
    workspace.write("site/assets/cart.js", script);
    let extracted = ExtractorHarness::for_lang("javascript")
        .at_path("site/assets/cart.js")
        .extract(script);
    assert!(
        extracted.nodes.iter().any(|node| {
            let details = crate::common::assertions::projected_node_details(&extracted, node);
            node.kind == "module"
                && details["rebindings"]
                    .as_array()
                    .is_some_and(|bindings| bindings.iter().any(|binding| binding == "openCart"))
        }),
        "the indexed module records the AST binding write"
    );
    workspace.write(
        "site/index.html",
        "<script src='./assets/cart.js'></script><button onclick='openCart()'>Cart</button>\n",
    );
    workspace.build();
    let conn = workspace.open();
    let status: String = query_scalar(&conn, "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path='site/index.html' AND ce.expression='openCart'", []);
    assert_eq!(
        status, "unresolved",
        "the call target is rebound after the indexed declaration"
    );
    commitments::verify(&conn).unwrap();
}

#[cfg(feature = "lang-typescript")]
#[test]
fn included_inline_handlers_follow_only_unique_prior_classic_scripts() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write("site/assets/cart.js", "function openCart() {}\n");
    workspace.write("site/assets/other.js", "function openCart() {}\n");
    for (name, script) in [
        ("unique", "<script src='./assets/cart.js'></script>"),
        (
            "ambiguous",
            "<script src='./assets/cart.js'></script><script src='./assets/other.js'></script>",
        ),
        ("async", "<script async src='./assets/cart.js'></script>"),
    ] {
        workspace.write(
            format!("site/{name}.html"),
            &format!("{script}\n{{% include 'parts/{name}.html' %}}\n"),
        );
        workspace.write(
            format!("site/parts/{name}.html"),
            "<button onclick='openCart()'>Cart</button>\n",
        );
    }
    workspace.write(
        "site/parts/shared.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/first.html",
        "<script src='./assets/cart.js'></script>\n{% include 'parts/shared.html' %}\n",
    );
    workspace.write(
        "site/second.html",
        "<script src='./assets/other.js'></script>\n{% include 'parts/shared.html' %}\n",
    );
    workspace.write(
        "site/parts/same_line.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/same_line.html",
        "<script src='./assets/cart.js'></script>{% include 'parts/same_line.html' %}\n",
    );
    workspace.write(
        "site/parts/late.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/late.html",
        "{% include 'parts/late.html' %}\n<script src='./assets/cart.js'></script>\n",
    );
    workspace.build();
    let status = |conn: &rusqlite::Connection, name: &str| -> String {
        query_scalar(conn, "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression='openCart'", [format!("site/parts/{name}.html")])
    };
    let conn = workspace.open();
    for (name, expected) in [
        ("unique", "resolved"),
        ("ambiguous", "ambiguous"),
        ("async", "unresolved"),
        ("shared", "ambiguous"),
        ("same_line", "unresolved"),
        ("late", "unresolved"),
    ] {
        assert_eq!(status(&conn, name), expected, "{name}");
    }
    commitments::verify(&conn).unwrap();
    drop(conn);
    workspace.write(
        "site/unique.html",
        "<script async src='./assets/cart.js'></script>\n{% include 'parts/unique.html' %}\n",
    );
    workspace.delta(&["site/unique.html"]);
    let delta = workspace.open();
    let cold = crate::languages::support::fresh_cold(&workspace);
    assert_eq!(status(&delta, "unique"), "unresolved");
    for name in [
        "unique",
        "ambiguous",
        "async",
        "shared",
        "same_line",
        "late",
    ] {
        assert_eq!(status(&delta, name), status(&cold, name), "{name}");
    }
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
    drop(delta);
    drop(cold);
    workspace.write(
        "site/second.html",
        "<script async src='./assets/other.js'></script>\n{% include 'parts/shared.html' %}\n",
    );
    workspace.delta(&["site/second.html"]);
    let delta = workspace.open();
    let cold = crate::languages::support::fresh_cold(&workspace);
    assert_eq!(
        status(&delta, "shared"),
        "unresolved",
        "one include parent lacks a proven provider"
    );
    assert_eq!(status(&delta, "shared"), status(&cold, "shared"));
    commitments::verify(&delta).unwrap();
    commitments::verify(&cold).unwrap();
}

#[cfg(feature = "lang-typescript")]
#[test]
fn included_inline_handlers_respect_manifest_scope_and_bounded_ancestry() {
    let workspace = crate::languages::PersistedWorkspaceHarness::new();
    workspace.write(
        "site/package.json",
        "{\"name\":\"site\",\"version\":\"1.0.0\"}\n",
    );
    workspace.write(
        "site/sub/package.json",
        "{\"name\":\"sub\",\"version\":\"1.0.0\"}\n",
    );
    workspace.write("site/assets/cart.js", "function openCart() {}\n");
    workspace.write(
        "site/sub/child.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.write(
        "site/scoped.html",
        "<script src='./assets/cart.js'></script>\n{% include 'sub/child.html' %}\n",
    );
    workspace.write(
        "site/chain0.html",
        "<script src='./assets/cart.js'></script>\n{% include 'chain1.html' %}\n",
    );
    for index in 1..17 {
        let handler = if index == 15 {
            "<button onclick='openCart()'>Cart</button>\n"
        } else {
            ""
        };
        workspace.write(
            format!("site/chain{index}.html"),
            &format!("{{% include 'chain{}.html' %}}\n{handler}", index + 1),
        );
    }
    workspace.write(
        "site/chain17.html",
        "<button onclick='openCart()'>Cart</button>\n",
    );
    workspace.build();
    let conn = workspace.open();
    for (path, expected) in [
        ("site/sub/child.html", "unresolved"),
        ("site/chain15.html", "resolved"),
        ("site/chain17.html", "unresolved"),
    ] {
        let status: String = query_scalar(&conn, "SELECT rc.status FROM resolution_coverage rc JOIN path_dictionary pd ON pd.path_id=rc.path_id JOIN coverage_expressions ce ON ce.expression_id=rc.expression_id WHERE pd.path=?1 AND ce.expression='openCart'", [path]);
        assert_eq!(status, expected, "{path}");
    }
    commitments::verify(&conn).unwrap();
}
