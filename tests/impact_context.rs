use anyhow::Result;
use contextunity_forge_mcp::{
    core::{
        models::stable_hash64,
        response::{Detail, QueryOptions, ResponsePolicy},
        schema::SCHEMA_DDL,
    },
    db::traversal,
};
use rusqlite::{params, Connection};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_impact_context_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }

    fn database(&self) -> Result<Connection> {
        let conn = Connection::open(self.0.join("graph.sqlite"))?;
        conn.execute_batch(SCHEMA_DDL)?;
        conn.execute(
            "INSERT INTO metadata(key,value) VALUES('output_root','test-generation')",
            [],
        )?;
        conn.execute_batch("INSERT INTO path_dictionary(path_id,path) VALUES(1,'fixture.py'),(2,'module.py'); INSERT INTO coverage_evidence(evidence_id,evidence) VALUES(1,'fixture'),(2,'exact'),(3,'self');")?;
        for id in ["a", "b", "c", "d", "decorator"] {
            conn.execute("INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) VALUES(?1,'function',?1,?1,1,1,1,0,'python',0,'{}',?2,1)", params![id, stable_hash64(id)])?;
        }
        for (from, to, kind) in [
            ("a", "b", "calls"),
            ("a", "c", "calls"),
            ("b", "d", "calls"),
            ("c", "d", "calls"),
            ("d", "a", "calls"),
            ("decorator", "b", "decorates"),
        ] {
            conn.execute("INSERT INTO edges(src_hash,dst_hash,kind,path_id,line,evidence_id,confidence_id,occurrence_count) VALUES(?1,?2,?3,1,1,1,2,1)", params![stable_hash64(from),stable_hash64(to),kind])?;
        }
        Ok(conn)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options(limit: usize, offset: usize, detail: Detail) -> Result<QueryOptions> {
    QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(limit),
        offset,
        Some(detail),
        (offset > 0).then(|| "test-generation".into()),
    )
}

#[test]
fn impact_pages_explain_shortest_links_without_repeating_graph_evidence() -> Result<()> {
    let workspace = Workspace::new()?;
    let conn = workspace.database()?;
    let first = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 16,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(2, 0, Detail::Compact)?,
        },
    )?;
    assert_eq!(first["nodes"]["total"], 5);
    assert_eq!(first["nodes"]["items"][0]["id"], "a");
    assert_eq!(first["nodes"]["items"][0]["is_seed"], true);
    assert!(first["nodes"]["items"][0].get("via").is_none());
    assert_eq!(first["nodes"]["items"][1]["id"], "b");
    assert_eq!(first["nodes"]["items"][1]["via"]["id"], "a");
    assert_eq!(first["nodes"]["items"][1]["via"]["kind"], "calls");
    assert!(first["nodes"]["items"][1].get("evidence").is_none());

    let second = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 16,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(3, 2, Detail::Compact)?,
        },
    )?;
    assert_eq!(second["nodes"]["total"], 5);
    let items = second["nodes"]["items"].as_array().expect("page items");
    assert_eq!(
        items
            .iter()
            .map(|item| item["id"].as_str().unwrap_or(""))
            .collect::<Vec<_>>(),
        vec!["c", "d", "decorator"]
    );
    assert_eq!(items[1]["distance"], 2);
    assert_eq!(items[1]["via"]["id"], "b");
    assert_eq!(items[2]["via"]["id"], "b");
    assert_eq!(items[2]["via"]["kind"], "decorates");
    assert_eq!(second["nodes"]["has_more"], false);

    let full = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 16,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(5, 0, Detail::Full)?,
        },
    )?;
    assert_eq!(full["nodes"]["items"][4]["via"], items[2]["via"]);
    assert_eq!(full["nodes"]["items"][4]["is_seed"], false);
    let beyond = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 16,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(2, 100, Detail::Compact)?,
        },
    )?;
    assert_eq!(beyond["nodes"]["total"], 5);
    assert!(beyond["nodes"]["items"]
        .as_array()
        .expect("page items")
        .is_empty());

    let zero = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 0,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(5, 0, Detail::Compact)?,
        },
    )?;
    assert_eq!(zero["nodes"]["total"], 1);
    assert_eq!(zero["nodes"]["items"][0]["is_seed"], true);
    let inbound = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 1,
            inbound: true,
            mode: None,
            edge_types: None,
            page: &options(5, 0, Detail::Compact)?,
        },
    )?;
    assert_eq!(inbound["nodes"]["items"][1]["id"], "d");
    assert_eq!(inbound["nodes"]["items"][1]["is_seed"], false);
    assert!(inbound["nodes"]["items"][1].get("via").is_none());
    let shallow_reverse = traversal::traverse_with_options(
        &conn,
        "b",
        &traversal::TraversalOptions {
            depth: 1,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(5, 0, Detail::Compact)?,
        },
    )?;
    let decorator = shallow_reverse["nodes"]["items"]
        .as_array()
        .expect("page items")
        .iter()
        .find(|item| item["id"] == "decorator")
        .expect("decorator");
    assert_eq!(decorator["distance"], 1);
    assert!(decorator.get("via").is_none());
    conn.execute("INSERT INTO edges(src_hash,dst_hash,kind,path_id,line,evidence_id,confidence_id,occurrence_count) VALUES(?1,?1,'calls',1,1,3,2,1)",[stable_hash64("a")])?;
    let shallow_self = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 1,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(5, 0, Detail::Compact)?,
        },
    )?;
    assert_eq!(shallow_self["nodes"]["total"], 3);
    assert_eq!(shallow_self["nodes"]["items"][0]["distance"], 0);
    Ok(())
}

#[test]
fn structural_dependency_uses_the_same_edge_kind_for_reachability_and_explanation() -> Result<()> {
    let workspace = Workspace::new()?;
    let conn = workspace.database()?;
    conn.execute("INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) VALUES('module','module','module','module',2,1,1,0,'python',0,'{}',?1,2)", [stable_hash64("module")])?;
    conn.execute("INSERT INTO edges(src_hash,dst_hash,kind,path_id,line,evidence_id,confidence_id,occurrence_count) VALUES(?1,?2,'imports',1,1,1,2,1)",params![stable_hash64("a"),stable_hash64("module")])?;
    let page = traversal::traverse_with_options(
        &conn,
        "a",
        &traversal::TraversalOptions {
            depth: 16,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(10, 0, Detail::Compact)?,
        },
    )?;
    let imported = page["nodes"]["items"]
        .as_array()
        .expect("page items")
        .iter()
        .find(|item| item["id"] == "module")
        .expect("imported module");
    assert_eq!(imported["distance"], 1);
    assert_eq!(imported["via"]["id"], "a");
    assert_eq!(imported["via"]["kind"], "imports");
    Ok(())
}

#[test]
fn removal_assessment_names_each_indexed_blocker_without_changing_verdict() -> Result<()> {
    let workspace = Workspace::new()?;
    let conn = workspace.database()?;
    conn.execute_batch("INSERT INTO path_dictionary(path_id,path) VALUES(3,'orphan.py'),(4,'other.py'),(5,'caller.py'); INSERT INTO coverage_expressions(expression_id,expression) VALUES(1,'dynamic()'),(2,'orphan');")?;
    conn.execute("INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) VALUES('orphan','function','orphan','orphan',3,1,1,0,'python',0,'{}',?1,3)", [stable_hash64("orphan")])?;
    let clean = traversal::removal_paged(&conn, "orphan", &options(10, 0, Detail::Compact)?)?;
    assert_eq!(clean["assessment"]["verdict"], "no_indexed_blockers");
    assert_eq!(clean["safe_to_remove"], true);
    let isolated = traversal::traverse_with_options(
        &conn,
        "orphan",
        &traversal::TraversalOptions {
            depth: 16,
            inbound: false,
            mode: None,
            edge_types: None,
            page: &options(10, 0, Detail::Compact)?,
        },
    )?;
    assert_eq!(isolated["nodes"]["total"], 1);
    assert_eq!(isolated["nodes"]["items"][0]["distance"], 0);
    assert_eq!(isolated["nodes"]["items"][0]["is_seed"], true);
    conn.execute("INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES(4,1,1,'unresolved',1)",[])?;
    conn.execute(
        "INSERT INTO errors(path,line,message) VALUES('other.py',1,'syntax error')",
        [],
    )?;
    let unrelated = traversal::removal_paged(&conn, "orphan", &options(10, 0, Detail::Compact)?)?;
    assert_eq!(unrelated["safe_to_remove"], true);
    assert_eq!(unrelated["unresolved_references"], 0);
    assert_eq!(unrelated["parse_errors"], 0);
    for duplicate in [
        "target_safe_to_remove",
        "target_unresolved_references",
        "target_parse_errors",
    ] {
        assert!(unrelated.get(duplicate).is_none());
    }
    let result = traversal::removal_paged(&conn, "b", &options(10, 0, Detail::Compact)?)?;
    assert_eq!(result["assessment"]["verdict"], "blocked");
    assert_eq!(
        result["assessment"]["blocking_reasons"]
            .as_array()
            .expect("reasons")
            .len(),
        1
    );
    assert_eq!(
        result["assessment"]["blocking_reasons"][0]["kind"],
        "incoming_dependencies"
    );
    assert_eq!(
        result["assessment"]["blocking_reasons"][0]["scope"],
        "selection"
    );
    assert_eq!(result["assessment"]["blocking_reasons"][0]["count"], 1);
    assert_eq!(result["unresolved_references"], 0);
    assert_eq!(result["parse_errors"], 0);
    assert_eq!(result["safe_to_remove"], false);
    assert_eq!(result["incoming_dependencies"]["total"], 1);
    assert!(result["proof_scope"]
        .as_str()
        .unwrap_or("")
        .contains("indexed static references only"));

    // Target-scoped unresolved reference blocks removal
    conn.execute(
        "INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id) VALUES(5,1,2,'unresolved',1)",
        [],
    )?;
    let blocked_target =
        traversal::removal_paged(&conn, "orphan", &options(10, 0, Detail::Compact)?)?;
    assert_eq!(blocked_target["safe_to_remove"], false);
    assert_eq!(blocked_target["unresolved_references"], 1);
    assert_eq!(blocked_target["assessment"]["verdict"], "blocked");
    assert_eq!(
        blocked_target["assessment"]["blocking_reasons"][0]["kind"],
        "unresolved_references"
    );
    Ok(())
}
