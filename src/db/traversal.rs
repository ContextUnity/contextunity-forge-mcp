use super::reader;
use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
pub fn traverse(conn: &Connection, selector: &str, depth: u32, inbound: bool) -> Result<Value> {
    if depth > 16 {
        bail!("depth must be <=16");
    }
    let node = reader::select(conn, selector)?;
    let id = node["id"].as_str().unwrap_or("");
    let (from, to) = if inbound {
        ("dst_public_id", "src_public_id")
    } else {
        ("src_public_id", "dst_public_id")
    };
    let sql=format!("WITH RECURSIVE walk(id,depth)AS(SELECT ?1,0 UNION SELECT e.{to},w.depth+1 FROM walk w JOIN edges e ON e.{from}=w.id WHERE w.depth<?2 AND e.kind IN('calls','imports','contains','documents'))SELECT n.*,min(w.depth)distance FROM walk w JOIN nodes n ON n.id=w.id GROUP BY n.id ORDER BY distance,n.id LIMIT 1001");
    let mut nodes = reader::rows(conn, &sql, &[&id, &depth], 1001)?;
    let truncated = nodes.len() > 1000;
    nodes.truncate(1000);
    Ok(
        json!({"selector":node,"direction":if inbound{"inbound"}else{"outbound"},"depth":depth,"nodes":nodes,"truncated":truncated}),
    )
}
pub fn removal(conn: &Connection, selector: &str) -> Result<Value> {
    let nodes = reader::rows(
        conn,
        "SELECT id FROM nodes WHERE path=?1",
        &[&selector],
        10001,
    )?;
    let selected = if nodes.is_empty() {
        vec![reader::select(conn, selector)?]
    } else {
        nodes
    };
    if selected.len() > 10000 {
        bail!("removal scope exceeds10000 nodes");
    }
    let ids: Vec<_> = selected.iter().filter_map(|n| n["id"].as_str()).collect();
    let encoded = serde_json::to_string(&ids)?;
    let callers=reader::rows(conn,"SELECT e.* FROM edges e WHERE e.dst_public_id IN(SELECT value FROM json_each(?1))AND e.src_public_id NOT IN(SELECT value FROM json_each(?1))AND e.kind NOT IN('contains','references_doc')ORDER BY e.path,e.line",&[&encoded],1001)?;
    let unresolved: i64 = conn.query_row(
        "SELECT count(*)FROM resolution_coverage WHERE status!='resolved'",
        [],
        |r| r.get(0),
    )?;
    let errors: i64 = conn.query_row("SELECT count(*)FROM errors", [], |r| r.get(0))?;
    Ok(
        json!({"selector":selector,"selected_ids":ids,"incoming_dependencies":callers,"unresolved_references":unresolved,"parse_errors":errors,"safe_to_remove":callers.is_empty()&&unresolved==0&&errors==0,"proof_scope":"indexed static references only; dynamic entrypoints and external callers require separate authority"}),
    )
}
pub fn cycles(conn: &Connection) -> Result<Value> {
    let edges = reader::rows(
        conn,
        "SELECT src_public_id,dst_public_id FROM edges WHERE kind IN('calls','imports')",
        &[],
        500000,
    )?;
    let mut graph = petgraph::graphmap::DiGraphMap::<&str, ()>::new();
    for e in &edges {
        if let (Some(a), Some(b)) = (e["src_public_id"].as_str(), e["dst_public_id"].as_str()) {
            graph.add_edge(a, b, ());
        }
    }
    let cycles: Vec<_> = petgraph::algo::kosaraju_scc(&graph)
        .into_iter()
        .filter(|c| c.len() > 1 || c.first().is_some_and(|n| graph.contains_edge(n, n)))
        .collect();
    Ok(json!(cycles))
}
pub fn query(
    conn: &Connection,
    operation: &str,
    selector: Option<&str>,
    depth: u32,
    limit: usize,
) -> Result<Value> {
    match operation{
 "overview"=>reader::overview(conn),"inspect"=>reader::inspect(conn,selector.unwrap_or(""),true),"explain"=>reader::explain(conn,selector.unwrap_or("")),"impact"=>traverse(conn,selector.unwrap_or(""),depth,true),"slice"=>traverse(conn,selector.unwrap_or(""),depth,false),"unwired"=>Ok(json!({"nodes":reader::rows(conn,"SELECT n.* FROM nodes n WHERE n.kind IN('function','method')AND NOT EXISTS(SELECT 1 FROM edges e WHERE e.dst_public_id=n.id AND e.kind='calls')ORDER BY n.path,n.line",&[],limit)?,"meaning":"no indexed static caller; not a dead-code proof"})),
 "raw_cypher"=>cypher(conn,selector.unwrap_or(""),limit),_=>bail!("unknown operation; supported: overview,inspect,explain,impact,slice,unwired,raw_cypher")}
}
fn cypher(conn: &Connection, query: &str, limit: usize) -> Result<Value> {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized == "MATCH (n) RETURN n" {
        return Ok(
            json!({"rows":reader::rows(conn,"SELECT * FROM nodes ORDER BY node_id",&[],limit)?}),
        );
    }
    if normalized == "MATCH (a)-[e]->(b) RETURN a,e,b" {
        return Ok(
            json!({"rows":reader::rows(conn,"SELECT a.id source,e.kind,b.id target FROM edges e JOIN nodes a ON a.id=e.src_public_id JOIN nodes b ON b.id=e.dst_public_id ORDER BY e.edge_id",&[],limit)?}),
        );
    }
    if let Some(kind) = normalized
        .strip_prefix("MATCH (n:")
        .and_then(|s| s.strip_suffix(") RETURN n"))
    {
        if !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("invalid node label");
        }
        return Ok(
            json!({"rows":reader::rows(conn,"SELECT * FROM nodes WHERE kind=?1 ORDER BY node_id",&[&kind],limit)?}),
        );
    }
    bail!("supported Cypher subset: MATCH (n) RETURN n; MATCH (n:kind) RETURN n; MATCH (a)-[e]->(b) RETURN a,e,b")
}
