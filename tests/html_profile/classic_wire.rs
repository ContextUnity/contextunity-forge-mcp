use super::Workspace;
use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::{ast, linker},
};
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

fn rows(db: &std::path::Path, root: &std::path::Path) -> Vec<String> {
    let conn = reader::open(db, root).unwrap();
    let mut statement = conn.prepare(
        "SELECT json_array(src_public_id,dst_public_id,kind,path,line,evidence,confidence) FROM edges WHERE path='index.html' ORDER BY src_public_id,dst_public_id,kind",
    ).unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

#[test]
fn classic_flow_wire_slots_and_constructor_receiver_survive_cold_build_and_delta() {
    let workspace = Workspace::new();
    workspace.write("index.html", INITIAL);
    workspace.write("slots.html", SLOTS);

    let direct = ast::extract("index.html", "html", INITIAL).unwrap();
    let alpha = class_method(&direct, "Alpha");
    let graph = linker::link(&BTreeMap::from([("index.html".to_owned(), direct.clone())]));
    assert!(
        graph.edges.iter().any(|edge| edge.kind == "calls"
            && edge.evidence == "worker.work"
            && edge.dst == alpha),
        "{graph:#?}"
    );

    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path='slots.html'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let facts: contextunity_forge_mcp::core::models::Facts =
        serde_json::from_slice(&zstd::stream::decode_all(blob.as_slice()).unwrap()).unwrap();
    let scope = facts
        .nodes
        .iter()
        .find(|node| node.details["classic_global"] == true)
        .unwrap();
    let details = serde_json::to_string(&scope.details).unwrap();
    let flow = details.split("\"value_flow\":").nth(1).unwrap();
    assert!(flow.starts_with("{\"bindings\":["), "{details}");
    assert!(flow.contains("],\"fields\":null}"), "{details}");

    let expected_alpha: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE path='index.html' AND kind='calls' AND evidence='worker.work' AND dst_public_id=?1",
        [&alpha], |row| row.get(0),
    ).unwrap();
    assert_eq!(expected_alpha, 1);
    drop(conn);

    workspace.write("index.html", UPDATED);
    writer::delta(&workspace.0, &workspace.db(), &["index.html".into()]).unwrap();
    let updated = ast::extract("index.html", "html", UPDATED).unwrap();
    let beta = class_method(&updated, "Beta");
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let expected_beta: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE path='index.html' AND kind='calls' AND evidence='worker.work' AND dst_public_id=?1",
        [&beta], |row| row.get(0),
    ).unwrap();
    assert_eq!(expected_beta, 1);
    drop(conn);

    let cold = workspace.0.join("cold.sqlite");
    writer::build(&workspace.0, &cold, None).unwrap();
    assert_eq!(
        rows(&workspace.db(), &workspace.0),
        rows(&cold, &workspace.0)
    );
}
