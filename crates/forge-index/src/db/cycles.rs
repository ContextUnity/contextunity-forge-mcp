use super::reader::{path_bounds, QueryBudget};
use anyhow::{bail, Result};
use petgraph::graphmap::DiGraphMap;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashSet;

fn components(conn: &Connection, filter_path: Option<&str>) -> Result<Vec<Vec<i64>>> {
    let budget = QueryBudget::new(conn);
    let selected = if let Some(path) = filter_path.filter(|p| !p.is_empty()) {
        let path = path.trim_end_matches('/');
        let (prefix, end) = path_bounds(path);
        let mut statement = conn.prepare(
            "SELECT n.node_hash FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE p.path=?1 OR (p.path>=?2 AND p.path<?3) LIMIT 1000001",
        )?;
        let selected = statement
            .query_map([path, &prefix, &end], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        if selected.len() > 1_000_000 {
            bail!("cycle scope exceeds node budget");
        }
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        Some(selected)
    } else {
        None
    };

    // Internal graph data uses integer keys. The response byte budget applies only to output.
    let mut statement = conn.prepare(
        "SELECT src_hash,dst_hash FROM edges WHERE kind IN('calls','imports') LIMIT 500001",
    )?;
    let edges = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut graph = DiGraphMap::<i64, ()>::new();
    for (count, edge) in edges.enumerate() {
        if count == 500_000 {
            bail!("cycle analysis exceeds 500000 edge budget");
        }
        let (src, dst) = edge?;
        graph.add_edge(src, dst, ());
        budget.check()?;
    }
    let components = petgraph::algo::kosaraju_scc(&graph);
    budget.check()?;
    let mut output = Vec::new();
    for mut component in components {
        budget.check()?;
        if component.len() == 1 && !graph.contains_edge(component[0], component[0]) {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|scope| !component.iter().any(|n| scope.contains(n)))
        {
            continue;
        }
        component.sort_unstable();
        output.push(component);
    }
    Ok(output)
}

/// Performs cycles.
pub fn cycles(conn: &Connection, filter_path: Option<&str>) -> Result<Value> {
    let components = components(conn, filter_path)?;
    let budget = QueryBudget::new(conn);
    let mut lookup = conn.prepare_cached("SELECT id FROM nodes WHERE node_hash=?1")?;
    let mut output = Vec::new();
    let mut bytes = 2usize;
    for component in components {
        let mut ids = Vec::with_capacity(component.len());
        for node in component {
            budget.check()?;
            let id: String = lookup.query_row([node], |r| r.get(0))?;
            bytes = bytes.saturating_add(id.len().saturating_mul(6) + 4);
            if bytes > 8 * 1024 * 1024 {
                bail!("cycle result exceeds the 8 MiB output budget; analyze an indexed file or directory path to narrow the scope, or use code_map_analyze with include_cycles=false for diagnostic totals");
            }
            ids.push(id);
        }
        ids.sort();
        output.push(ids);
    }
    output.sort();
    Ok(json!(output))
}

pub(super) fn summary(conn: &Connection, filter_path: Option<&str>) -> Result<Value> {
    let mut components = components(conn, filter_path)?;
    components.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
    let total = components.len();
    let total_nodes: usize = components.iter().map(Vec::len).sum();
    let mut lookup = conn.prepare_cached("SELECT n.id,p.path,n.line FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.node_hash=?1")?;
    let mut largest = Vec::new();
    for component in components.iter().take(5) {
        let (id, path, line): (String, String, i64) =
            lookup.query_row([component[0]], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        largest.push(
            json!({"node_count":component.len(),"representative":id,"path":path,"line":line}),
        );
    }
    Ok(
        json!({"total":total,"total_nodes":total_nodes,"largest":largest,"omitted_cycles":total.saturating_sub(5),"continuation_hint":if total > 0 {Some("Use code_map_query with operation slice and a representative selector to inspect bounded dependencies.")} else {None}}),
    )
}
