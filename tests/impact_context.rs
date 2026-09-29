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
        for id in ["a", "b", "c", "d", "decorator"] {
            conn.execute("INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details,node_hash) VALUES(?1,'function',?1,?1,'fixture.py',1,1,0,'python',0,'{}',?2)", params![id, stable_hash64(id)])?;
        }
        for (from, to, kind) in [
            ("a", "b", "calls"),
            ("a", "c", "calls"),
            ("b", "d", "calls"),
            ("c", "d", "calls"),
            ("d", "a", "calls"),
            ("decorator", "b", "decorates"),
        ] {
            conn.execute("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) VALUES(?1,?2,?3,'fixture.py',1,'fixture','exact',1)", params![from,to,kind])?;
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
    let first = traversal::traverse_paged(&conn, "a", 16, false, &options(2, 0, Detail::Compact)?)?;
    assert_eq!(first["nodes"]["total"], 5);
    assert_eq!(first["nodes"]["items"][0]["id"], "a");
    assert_eq!(first["nodes"]["items"][0]["is_seed"], true);
    assert!(first["nodes"]["items"][0].get("via").is_none());
    assert_eq!(first["nodes"]["items"][1]["id"], "b");
    assert_eq!(first["nodes"]["items"][1]["via"]["id"], "a");
    assert_eq!(first["nodes"]["items"][1]["via"]["kind"], "calls");
    assert!(first["nodes"]["items"][1].get("evidence").is_none());

    let second =
        traversal::traverse_paged(&conn, "a", 16, false, &options(3, 2, Detail::Compact)?)?;
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

    let full = traversal::traverse_paged(&conn, "a", 16, false, &options(5, 0, Detail::Full)?)?;
    assert_eq!(full["nodes"]["items"][4]["via"], items[2]["via"]);
    assert_eq!(full["nodes"]["items"][4]["is_seed"], false);
    let beyond =
        traversal::traverse_paged(&conn, "a", 16, false, &options(2, 100, Detail::Compact)?)?;
    assert_eq!(beyond["nodes"]["total"], 5);
    assert!(beyond["nodes"]["items"]
        .as_array()
        .expect("page items")
        .is_empty());

    let zero = traversal::traverse_paged(&conn, "a", 0, false, &options(5, 0, Detail::Compact)?)?;
    assert_eq!(zero["nodes"]["total"], 1);
    assert_eq!(zero["nodes"]["items"][0]["is_seed"], true);
    let inbound = traversal::traverse_paged(&conn, "a", 1, true, &options(5, 0, Detail::Compact)?)?;
    assert_eq!(inbound["nodes"]["items"][1]["id"], "d");
    assert_eq!(inbound["nodes"]["items"][1]["is_seed"], false);
    assert!(inbound["nodes"]["items"][1].get("via").is_none());
    let shallow_reverse =
        traversal::traverse_paged(&conn, "b", 1, false, &options(5, 0, Detail::Compact)?)?;
    let decorator = shallow_reverse["nodes"]["items"]
        .as_array()
        .expect("page items")
        .iter()
        .find(|item| item["id"] == "decorator")
        .expect("decorator");
    assert_eq!(decorator["distance"], 1);
    assert!(decorator.get("via").is_none());
    conn.execute("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) VALUES('a','a','calls','fixture.py',1,'self','exact',1)",[])?;
    let shallow_self =
        traversal::traverse_paged(&conn, "a", 1, false, &options(5, 0, Detail::Compact)?)?;
    assert_eq!(shallow_self["nodes"]["total"], 3);
    assert_eq!(shallow_self["nodes"]["items"][0]["distance"], 0);
    Ok(())
}

#[test]
fn structural_dependency_uses_the_same_edge_kind_for_reachability_and_explanation() -> Result<()> {
    let workspace = Workspace::new()?;
    let conn = workspace.database()?;
    conn.execute("INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details,node_hash) VALUES('module','module','module','module','module.py',1,1,0,'python',0,'{}',?1)", [stable_hash64("module")])?;
    conn.execute("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) VALUES('a','module','imports','fixture.py',1,'fixture','exact',1)",[])?;
    let page = traversal::traverse_paged(&conn, "a", 16, false, &options(10, 0, Detail::Compact)?)?;
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
    conn.execute("INSERT INTO nodes(id,kind,name,qualname,path,line,end_line,is_test,language,generated,details,node_hash) VALUES('orphan','function','orphan','orphan','orphan.py',1,1,0,'python',0,'{}',?1)", [stable_hash64("orphan")])?;
    let clean = traversal::removal_paged(&conn, "orphan", &options(10, 0, Detail::Compact)?)?;
    assert_eq!(clean["assessment"]["verdict"], "no_indexed_blockers");
    assert_eq!(clean["safe_to_remove"], true);
    let isolated = traversal::traverse_paged(
        &conn,
        "orphan",
        16,
        false,
        &options(10, 0, Detail::Compact)?,
    )?;
    assert_eq!(isolated["nodes"]["total"], 1);
    assert_eq!(isolated["nodes"]["items"][0]["distance"], 0);
    assert_eq!(isolated["nodes"]["items"][0]["is_seed"], true);
    conn.execute("INSERT INTO resolution_coverage(path,line,expression,status,evidence) VALUES('other.py',1,'dynamic()','unresolved','fixture')",[])?;
    conn.execute(
        "INSERT INTO errors(path,line,message) VALUES('other.py',1,'syntax error')",
        [],
    )?;
    let unrelated = traversal::removal_paged(&conn, "orphan", &options(10, 0, Detail::Compact)?)?;
    assert_eq!(unrelated["safe_to_remove"], true);
    assert_eq!(unrelated["target_unresolved_references"], 0);
    assert_eq!(unrelated["workspace_has_unresolved"], true);
    assert_eq!(unrelated["workspace_errors_count"], 1);
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
    assert_eq!(result["target_unresolved_references"], 0);
    assert_eq!(result["unresolved_references"], 1);
    assert_eq!(result["parse_errors"], 1);
    assert_eq!(result["workspace_errors_count"], 1);
    assert_eq!(result["safe_to_remove"], false);
    assert_eq!(result["incoming_dependencies"]["total"], 1);
    assert!(result["proof_scope"]
        .as_str()
        .unwrap_or("")
        .contains("indexed static references only"));
    Ok(())
}
