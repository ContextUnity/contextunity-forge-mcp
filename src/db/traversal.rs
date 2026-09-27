use super::{paging, reader};
use crate::core::response::QueryOptions;
use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
// Stored decorates edges point from decorator to target; dependency walks reverse them.
const FORWARD_DEPENDENCIES: &str =
    "'calls','inherits','implements','mutates','handles','references'";
const REVERSE_DEPENDENCIES: &str = "'decorates'";
const MAX_DEEP_TRAVERSAL_FRONTIER: usize = 1000;

pub(super) fn immediate_links(
    conn: &Connection,
    id: &str,
    inbound: bool,
    structural: bool,
) -> Result<usize> {
    let (forward, reverse) = if inbound {
        ("dst_public_id", "src_public_id")
    } else {
        ("src_public_id", "dst_public_id")
    };
    let extra = if structural {
        ",'imports','contains','documents'"
    } else {
        ""
    };
    let forward = paging::count(
        conn,
        &format!("SELECT count(*) FROM edges WHERE {forward}=?1 AND kind IN({FORWARD_DEPENDENCIES}{extra})"),
        &[&id],
    )?;
    let reverse = paging::count(
        conn,
        &format!(
            "SELECT count(*) FROM edges WHERE {reverse}=?1 AND kind IN({REVERSE_DEPENDENCIES})"
        ),
        &[&id],
    )?;
    Ok(forward.saturating_add(reverse))
}

fn admit_traversal(conn: &Connection, id: &str, depth: u32, inbound: bool) -> Result<()> {
    if depth <= 1 {
        return Ok(());
    }
    let immediate = immediate_links(conn, id, inbound, true)?;
    if immediate > MAX_DEEP_TRAVERSAL_FRONTIER {
        bail!("selector {id} has {immediate} immediate graph links; depth {depth} would expand a broad graph before pagination. Retry with depth=1 and page the result, or select a narrower module or symbol. Reducing limit alone does not reduce traversal work");
    }
    Ok(())
}

pub(super) fn dependency_steps(inbound: bool, bounded: bool, structural: bool) -> String {
    let (from, to) = if inbound {
        ("dst_public_id", "src_public_id")
    } else {
        ("src_public_id", "dst_public_id")
    };
    let depth = if bounded { ",w.depth+1" } else { "" };
    let guard = if bounded { "w.depth<?2 AND " } else { "" };
    let extra = if structural {
        ",'imports','contains','documents'"
    } else {
        ""
    };
    format!("SELECT e.{to}{depth} FROM walk w JOIN edges e ON e.{from}=w.id WHERE {guard}e.kind IN({FORWARD_DEPENDENCIES}{extra}) UNION SELECT e.{from}{depth} FROM walk w JOIN edges e ON e.{to}=w.id WHERE {guard}e.kind IN({REVERSE_DEPENDENCIES})")
}
pub fn traverse(
    conn: &Connection,
    selector: &str,
    depth: u32,
    inbound: bool,
    limit: usize,
) -> Result<Value> {
    if depth > 16 {
        bail!("depth must be <=16");
    }
    reader::validate_limit(limit)?;
    let node = reader::select(conn, selector)?;
    let id = node["id"].as_str().unwrap_or("");
    admit_traversal(conn, id, depth, inbound)?;
    let size = limit + 1;
    let steps = dependency_steps(inbound, true, true);
    let sql = format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {steps}) SELECT n.*,min(w.depth) distance FROM walk w JOIN nodes n ON n.id=w.id GROUP BY n.id ORDER BY distance,n.id LIMIT ?3");
    let mut nodes = reader::rows(conn, &sql, &[&id, &depth, &size], size)?;
    let truncated = nodes.len() > limit;
    nodes.truncate(limit);
    Ok(
        json!({"selector":node,"direction":if inbound{"inbound"}else{"outbound"},"depth":depth,"nodes":nodes,"truncated":truncated,"limit":limit}),
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
    let sql = format!("SELECT e.* FROM edges e WHERE e.dst_public_id IN(SELECT value FROM json_each(?1)) AND e.src_public_id NOT IN(SELECT value FROM json_each(?1)) AND e.kind NOT IN('contains','references_doc',{REVERSE_DEPENDENCIES}) UNION SELECT e.* FROM edges e WHERE e.src_public_id IN(SELECT value FROM json_each(?1)) AND e.dst_public_id NOT IN(SELECT value FROM json_each(?1)) AND e.kind IN({REVERSE_DEPENDENCIES}) ORDER BY path,line");
    let callers = reader::rows(conn, &sql, &[&encoded], 1001)?;
    let unresolved: i64 = conn.query_row(
        "SELECT count(*)FROM resolution_coverage WHERE status IN('unresolved','ambiguous')",
        [],
        |r| r.get(0),
    )?;
    let errors: i64 = conn.query_row("SELECT count(*)FROM errors", [], |r| r.get(0))?;
    Ok(
        json!({"selector":selector,"selected_ids":ids,"incoming_dependencies":callers,"unresolved_references":unresolved,"parse_errors":errors,"safe_to_remove":callers.is_empty()&&unresolved==0&&errors==0,"proof_scope":"indexed static references only; dynamic entrypoints and external callers require separate authority"}),
    )
}
pub use super::cycles::cycles;
pub fn query(
    conn: &Connection,
    operation: &str,
    selector: Option<&str>,
    depth: u32,
    limit: usize,
) -> Result<Value> {
    match operation {
        "overview" => reader::overview(conn),
        "inspect" => reader::inspect(conn, selector.unwrap_or(""), true),
        "explain" => reader::explain(conn, selector.unwrap_or("")),
        "impact" => traverse(conn, selector.unwrap_or(""), depth, true, limit),
        "slice" => traverse(conn, selector.unwrap_or(""), depth, false, limit),
        "unwired" => Ok(json!({
            "nodes": reader::rows(
                conn,
                "SELECT n.* FROM nodes n WHERE n.kind IN('function','method') AND NOT EXISTS(SELECT 1 FROM edges e WHERE e.dst_public_id=n.id AND e.kind='calls') ORDER BY n.path,n.line",
                &[],
                limit
            )?,
            "meaning": "no indexed static caller; not a dead-code proof"
        })),
        "raw_cypher" => cypher(conn, selector.unwrap_or(""), limit),
        _ => bail!("unknown operation; supported: overview,inspect,explain,impact,slice,unwired,raw_cypher"),
    }
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
    bail!("supported Cypher subset: MATCH (n) RETURN n; MATCH (n:kind) RETURN n; MATCH (a)-[e]->(b) RETURN a,e,b. Pass limit as a separate tool argument, not a Cypher LIMIT clause")
}

pub fn traverse_paged(
    conn: &Connection,
    selector: &str,
    depth: u32,
    inbound: bool,
    options: &QueryOptions,
) -> Result<Value> {
    if depth > 16 {
        bail!("depth must be <=16");
    }
    paging::generation(conn, options)?;
    let node = reader::select_detail(conn, selector, options.detail)?;
    let id = node["id"].as_str().unwrap_or("");
    admit_traversal(conn, id, depth, inbound)?;
    let steps = dependency_steps(inbound, true, true);
    let sql = format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {steps}), reached(id,distance) AS (SELECT id,min(depth) FROM walk GROUP BY id) SELECT {},r.distance FROM reached r JOIN nodes n ON n.id=r.id ORDER BY r.distance,n.id", paging::nodes("n", options.detail));
    Ok(
        json!({"selector":node,"direction":if inbound {"inbound"} else {"outbound"},"depth":depth,"nodes":paging::query(conn, &sql, &[&id,&depth], options)?}),
    )
}

pub fn removal_paged(conn: &Connection, selector: &str, options: &QueryOptions) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let file_count = paging::count(
        conn,
        "SELECT count(*) FROM nodes WHERE path=?1",
        &[&selector],
    )?;
    let (selection, selected_id) = if file_count > 0 {
        ("SELECT id FROM nodes WHERE path=?1", selector.to_owned())
    } else {
        let node = reader::select_detail(conn, selector, crate::core::response::Detail::Compact)?;
        (
            "SELECT id FROM nodes WHERE id=?1",
            node["id"].as_str().unwrap_or("").to_owned(),
        )
    };
    if file_count > 10000 {
        bail!("removal scope exceeds10000 nodes");
    }
    let dependencies = format!("WITH selected(id) AS ({selection}) SELECT e.edge_id FROM edges e WHERE e.dst_public_id IN(SELECT id FROM selected) AND e.src_public_id NOT IN(SELECT id FROM selected) AND e.kind NOT IN('contains','references_doc',{REVERSE_DEPENDENCIES}) UNION SELECT e.edge_id FROM edges e WHERE e.src_public_id IN(SELECT id FROM selected) AND e.dst_public_id NOT IN(SELECT id FROM selected) AND e.kind IN({REVERSE_DEPENDENCIES})");
    let dependency_count = paging::count(
        conn,
        &format!("SELECT count(*) FROM ({dependencies})"),
        &[&selected_id],
    )?;
    let unresolved = paging::count(
        conn,
        "SELECT count(*) FROM resolution_coverage WHERE status IN('unresolved','ambiguous')",
        &[],
    )?;
    let errors = paging::count(conn, "SELECT count(*) FROM errors", &[])?;
    let callers = paging::query(conn, &format!("SELECT {} FROM ({dependencies}) d JOIN edges e ON e.edge_id=d.edge_id ORDER BY e.path,e.line,e.src_public_id,e.dst_public_id,e.kind", paging::edges("e", options.detail)), &[&selected_id], options)?;
    Ok(json!({"selector":selector,"generation":generation,
        "selected_ids":paging::query(conn, &format!("{selection} ORDER BY id"), &[&selected_id], options)?,
        "incoming_dependencies":callers,"unresolved_references":unresolved,"parse_errors":errors,
        "safe_to_remove":dependency_count == 0 && unresolved == 0 && errors == 0,
        "proof_scope":"indexed static references only; dynamic entrypoints and external callers require separate authority"}))
}

pub fn query_paged(
    conn: &Connection,
    operation: &str,
    selector: Option<&str>,
    depth: u32,
    options: &QueryOptions,
) -> Result<Value> {
    match operation {
        "overview" => reader::overview_paged(conn, options),
        "inspect" => reader::inspect_paged(conn, selector.unwrap_or(""), true, options),
        "explain" => reader::explain_paged(conn, selector.unwrap_or(""), None, options),
        "impact" => traverse_paged(conn, selector.unwrap_or(""), depth, true, options),
        "slice" => traverse_paged(conn, selector.unwrap_or(""), depth, false, options),
        "unwired" => Ok(json!({"nodes":paging::query(conn, &format!("SELECT {} FROM nodes n WHERE n.kind IN('function','method') AND NOT EXISTS(SELECT 1 FROM edges e WHERE e.dst_public_id=n.id AND e.kind='calls') ORDER BY n.path,n.line,n.id", paging::nodes("n", options.detail)), &[], options)?, "meaning":"no indexed static caller; not a dead-code proof"})),
        "raw_cypher" => cypher_paged(conn, selector.unwrap_or(""), options),
        _ => bail!("unknown operation; supported: overview,inspect,explain,impact,slice,unwired,raw_cypher"),
    }
}

fn cypher_paged(conn: &Connection, query: &str, options: &QueryOptions) -> Result<Value> {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let columns = paging::nodes("n", options.detail);
    let rows = if normalized == "MATCH (n) RETURN n" {
        paging::query(
            conn,
            &format!("SELECT {columns} FROM nodes n ORDER BY n.id"),
            &[],
            options,
        )?
    } else if normalized == "MATCH (a)-[e]->(b) RETURN a,e,b" {
        paging::query(conn, "SELECT e.src_public_id source,e.kind,e.dst_public_id target FROM edges e ORDER BY e.src_public_id,e.kind,e.dst_public_id", &[], options)?
    } else if let Some(kind) = normalized
        .strip_prefix("MATCH (n:")
        .and_then(|s| s.strip_suffix(") RETURN n"))
    {
        if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("invalid node label");
        }
        paging::query(
            conn,
            &format!("SELECT {columns} FROM nodes n WHERE n.kind=?1 ORDER BY n.id"),
            &[&kind],
            options,
        )?
    } else {
        bail!("supported Cypher subset: MATCH (n) RETURN n; MATCH (n:kind) RETURN n; MATCH (a)-[e]->(b) RETURN a,e,b. Pass limit as a separate tool argument, not a Cypher LIMIT clause");
    };
    Ok(json!({"rows":rows}))
}
