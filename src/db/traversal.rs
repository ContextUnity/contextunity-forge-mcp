use super::{paging, reader, symbols};
use crate::core::{
    models::stable_hash64,
    response::{CoverageOptions, QueryOptions},
};
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
// Stored decorates edges point from decorator to target; dependency walks reverse them.
pub(super) const FORWARD_DEPENDENCIES: &str =
    "'calls','inherits','implements','overrides','extends','includes','mutates','handles','references','calls_endpoint'";
pub(super) const REVERSE_DEPENDENCIES: &str = "'decorates'";
const STRUCTURAL_DEPENDENCIES: &str = ",'imports','contains','documents'";
const MAX_DEEP_TRAVERSAL_FRONTIER: usize = 1000;

/// Bounds and filters a graph traversal.
pub struct TraversalOptions<'a> {
    /// Maximum graph depth.
    pub depth: u32,
    /// Whether to follow incoming relationships.
    pub inbound: bool,
    /// Optional relationship mode.
    pub mode: Option<&'a str>,
    /// Optional exact relationship kinds.
    pub edge_types: Option<&'a [String]>,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Bounds a generic graph query.
pub struct GraphQueryOptions<'a> {
    /// Maximum graph depth for traversal operations.
    pub depth: u32,
    /// Incoming or outgoing impact direction.
    pub direction: Option<&'a str>,
    /// Optional resolution coverage.
    pub coverage: CoverageOptions,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

fn removal_assessment(
    dependencies: usize,
    exact_dependencies: bool,
    target_unresolved: usize,
    target_errors: usize,
) -> Value {
    let mut reasons = Vec::new();
    if dependencies > 0 {
        reasons.push(if exact_dependencies {
            json!({"kind":"incoming_dependencies","scope":"selection","count":dependencies})
        } else {
            json!({"kind":"incoming_dependencies","scope":"selection","at_least":dependencies})
        });
    }
    if target_unresolved > 0 {
        reasons.push(
            json!({"kind":"unresolved_references","scope":"target","count":target_unresolved}),
        );
    }
    if target_errors > 0 {
        reasons.push(json!({"kind":"parse_errors","scope":"target","count":target_errors}));
    }
    json!({"verdict":if reasons.is_empty() {"no_indexed_blockers"} else {"blocked"},"blocking_reasons":reasons})
}

fn removal_diagnostics(conn: &Connection, selected_ids: &[String]) -> Result<(usize, usize)> {
    let ids = serde_json::to_string(selected_ids)?;
    let target_unresolved = paging::count(
        conn,
        "SELECT count(*) FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN path_dictionary p ON p.path_id=c.path_id WHERE c.status IN('unresolved','ambiguous') AND (x.expression IN (SELECT n.name FROM nodes n WHERE n.id IN (SELECT value FROM json_each(?1))) OR x.expression IN (SELECT n.qualname FROM nodes n WHERE n.id IN (SELECT value FROM json_each(?1))) OR p.path IN (SELECT np.path FROM nodes n JOIN path_dictionary np ON np.path_id=n.path_id WHERE n.kind='module' AND n.id IN (SELECT value FROM json_each(?1))) OR (EXISTS(SELECT 1 FROM nodes n WHERE n.kind='module' AND n.id IN (SELECT value FROM json_each(?1))) AND p.path IN (SELECT ep.path FROM edges e JOIN path_dictionary ep ON ep.path_id=e.path_id JOIN nodes m ON m.node_hash=e.dst_hash JOIN path_dictionary mp ON mp.path_id=m.path_id WHERE e.kind='imports' AND mp.path IN (SELECT np.path FROM nodes n JOIN path_dictionary np ON np.path_id=n.path_id WHERE n.kind='module' AND n.id IN (SELECT value FROM json_each(?1))))))",
        &[&ids],
    )?;
    let target_errors = paging::count(
        conn,
        "SELECT count(*) FROM errors WHERE path IN (SELECT p.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.id IN (SELECT value FROM json_each(?1)))",
        &[&ids],
    )?;
    Ok((target_unresolved, target_errors))
}

pub(super) fn immediate_links(
    conn: &Connection,
    id: &str,
    inbound: bool,
    structural: bool,
) -> Result<usize> {
    let (forward, reverse) = if inbound {
        ("dst_hash", "src_hash")
    } else {
        ("src_hash", "dst_hash")
    };
    let extra = if structural {
        STRUCTURAL_DEPENDENCIES
    } else {
        ""
    };
    let forward = paging::count(
        conn,
        &format!("SELECT count(*) FROM edges WHERE {forward}=?1 AND kind IN({FORWARD_DEPENDENCIES}{extra})"),
        &[&stable_hash64(id)],
    )?;
    let reverse = paging::count(
        conn,
        &format!(
            "SELECT count(*) FROM edges WHERE {reverse}=?1 AND kind IN({REVERSE_DEPENDENCIES})"
        ),
        &[&stable_hash64(id)],
    )?;
    Ok(forward.saturating_add(reverse))
}

fn admit_traversal(conn: &Connection, id: &str, depth: u32, inbound: bool) -> Result<usize> {
    if depth <= 1 {
        return Ok(0);
    }
    let immediate = immediate_links(conn, id, inbound, true)?;
    if immediate > MAX_DEEP_TRAVERSAL_FRONTIER {
        bail!("selector {id} has {immediate} immediate graph links; depth {depth} would expand a broad graph before pagination. Retry with depth=1 and page the result, or select a narrower module or symbol. Reducing limit alone does not reduce traversal work");
    }
    Ok(immediate)
}

pub(super) fn resolve_traversal_kinds(
    mode: Option<&str>,
    edge_types: Option<&[String]>,
    structural: bool,
) -> (String, String) {
    if let Some(types) = edge_types {
        if !types.is_empty() {
            let mut forward = Vec::new();
            let mut reverse = Vec::new();
            for t in types {
                let lower = t.to_ascii_lowercase();
                if lower == "decorates" {
                    reverse.push(format!("'{lower}'"));
                } else {
                    forward.push(format!("'{lower}'"));
                }
            }
            let f_str = if forward.is_empty() {
                "'__none__'".into()
            } else {
                forward.join(",")
            };
            let r_str = reverse.join(",");
            return (f_str, r_str);
        }
    }

    match mode {
        Some("data-flow") | Some("dataflow") => {
            ("'mutates','references','handles'".into(), String::new())
        }
        Some("all") => (
            format!("{FORWARD_DEPENDENCIES}{STRUCTURAL_DEPENDENCIES}"),
            REVERSE_DEPENDENCIES.into(),
        ),
        _ => {
            let extra = if structural {
                STRUCTURAL_DEPENDENCIES
            } else {
                ""
            };
            (
                format!("{FORWARD_DEPENDENCIES}{extra}"),
                REVERSE_DEPENDENCIES.into(),
            )
        }
    }
}

pub(super) fn custom_dependency_steps(
    inbound: bool,
    bounded: bool,
    forward: &str,
    reverse: &str,
) -> String {
    let (from, to) = if inbound {
        ("dst_hash", "src_hash")
    } else {
        ("src_hash", "dst_hash")
    };
    let depth = if bounded { ",w.depth+1" } else { "" };
    let guard = if bounded { "w.depth<?2 AND " } else { "" };
    if reverse.is_empty() {
        format!("SELECT e.{to}{depth} FROM walk w JOIN edges e ON e.{from}=w.id WHERE {guard}e.kind IN({forward})")
    } else {
        format!("SELECT e.{to}{depth} FROM walk w JOIN edges e ON e.{from}=w.id WHERE {guard}e.kind IN({forward}) UNION SELECT e.{from}{depth} FROM walk w JOIN edges e ON e.{to}=w.id WHERE {guard}e.kind IN({reverse})")
    }
}

pub(super) fn dependency_steps(inbound: bool, bounded: bool, structural: bool) -> String {
    let (forward, reverse) = resolve_traversal_kinds(None, None, structural);
    custom_dependency_steps(inbound, bounded, &forward, &reverse)
}
pub use super::cycles::cycles;

/// Performs impact inbound.
pub fn impact_inbound(direction: Option<&str>) -> Result<bool> {
    match direction.unwrap_or("inbound") {
        "inbound" => Ok(true),
        "outbound" => Ok(false),
        _ => bail!("impact direction must be inbound or outbound"),
    }
}

fn sql_selector(selector: Option<&str>) -> Result<&str> {
    selector
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("sql operation requires a SELECT or WITH selector"))
}

/// Traverse direct indexed relationships with bounded paging.
pub fn traverse_with_options(
    conn: &Connection,
    selector: &str,
    request: &TraversalOptions<'_>,
) -> Result<Value> {
    traverse_with_path_options(conn, selector, None, request)
}

pub(crate) fn traverse_with_path_options(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    request: &TraversalOptions<'_>,
) -> Result<Value> {
    let depth = request.depth;
    let inbound = request.inbound;
    let mode = request.mode;
    let edge_types = request.edge_types;
    let options = request.page;
    if depth > 16 {
        bail!("depth must be <=16");
    }
    let generation = paging::generation(conn, options)?;
    let node = reader::select_detail_with_path(conn, selector, path, options.detail)?;
    let id = node["id"].as_str().context("selected node has no id")?;
    let id_hash = stable_hash64(id);
    let immediate = admit_traversal(conn, id, depth, inbound)?;
    let (forward, reverse) = resolve_traversal_kinds(mode, edge_types, true);
    let mode_str = mode.unwrap_or("calls");

    if depth == 0 || (depth > 1 && immediate == 0) {
        let items = if options.offset == 0 {
            let mut seed = node.clone();
            let seed_fields = seed.as_object_mut().context("invalid selected node")?;
            seed_fields.insert("distance".into(), json!(0));
            seed_fields.insert("is_seed".into(), json!(true));
            vec![seed]
        } else {
            Vec::new()
        };
        let mut result = json!({
            "selector": node,
            "direction": if inbound { "inbound" } else { "outbound" },
            "depth": depth,
            "mode": mode_str,
            "nodes": paging::value(items, 1, options, &generation)
        });
        if let Some(types) = edge_types {
            result["edge_types"] = json!(types);
        }
        return Ok(result);
    }
    let steps = custom_dependency_steps(inbound, true, &forward, &reverse);
    if depth == 1 {
        let columns = paging::nodes("n", options.detail);
        let base = format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {steps}), reached(id,distance) AS (SELECT id,min(depth) FROM walk GROUP BY id) SELECT {columns},r.distance FROM reached r JOIN nodes n ON n.node_hash=r.id ORDER BY r.distance,n.id");
        let count = reader::rows(
            conn,
            &format!("SELECT count(*) total FROM ({base})"),
            &[&id_hash, &depth],
            1,
        )?;
        let total = count[0]["total"]
            .as_u64()
            .context("traversal total is missing")?;
        let total = usize::try_from(total)?;
        let limit = i64::try_from(options.limit)?;
        let offset = i64::try_from(options.offset)?;
        let mut items = reader::rows(
            conn,
            &format!("SELECT * FROM ({base}) LIMIT ?3 OFFSET ?4"),
            &[&id_hash, &depth, &limit, &offset],
            options.limit,
        )?;
        for item in &mut items {
            let fields = item.as_object_mut().context("invalid traversal node")?;
            fields.insert(
                "is_seed".into(),
                json!(fields.get("id").and_then(Value::as_str) == Some(id)),
            );
        }
        let mut result = json!({
            "selector": node,
            "direction": if inbound { "inbound" } else { "outbound" },
            "depth": depth,
            "mode": mode_str,
            "nodes": paging::value(items, total, options, &generation)
        });
        if let Some(types) = edge_types {
            result["edge_types"] = json!(types);
        }
        return Ok(result);
    }
    // Reuse the bounded reachability result for the exact count, page, and path evidence.
    let columns = paging::nodes("n", options.detail);
    let (forward_from, forward_to, reverse_from, reverse_to) = if inbound {
        ("src_hash", "dst_hash", "dst_hash", "src_hash")
    } else {
        ("dst_hash", "src_hash", "src_hash", "dst_hash")
    };
    let reverse_union = if reverse.is_empty() {
        String::new()
    } else {
        format!("UNION ALL SELECT predecessor.id AS predecessor,e.kind AS edge_kind FROM edges e JOIN reached prior ON prior.id=e.{reverse_to} AND prior.distance=p.distance-1 JOIN nodes predecessor ON predecessor.node_hash=e.{reverse_to} WHERE e.{reverse_from}=p.__hash AND e.kind IN({reverse})")
    };
    let sql = if immediate <= 3 && options.offset == 0 {
        // On sparse walks, count the reached rows in the page scan itself.
        format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {steps}), \
        reached(id,distance) AS (SELECT id,min(depth) FROM walk GROUP BY id), \
        page AS (SELECT {columns},n.node_hash __hash,r.distance,count(*) OVER() __total FROM reached r JOIN nodes n ON n.node_hash=r.id ORDER BY r.distance,n.id LIMIT ?3 OFFSET ?4) \
        SELECT p.*,CASE WHEN p.distance=0 THEN NULL ELSE (SELECT json_array(predecessor,edge_kind) FROM ( \
            SELECT predecessor.id AS predecessor,e.kind AS edge_kind FROM edges e JOIN reached prior ON prior.id=e.{forward_to} AND prior.distance=p.distance-1 JOIN nodes predecessor ON predecessor.node_hash=e.{forward_to} WHERE e.{forward_from}=p.__hash AND e.kind IN({forward}) \
            {reverse_union} \
        ) ORDER BY predecessor,edge_kind LIMIT 1) END __reason FROM page p")
    } else {
        format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {steps}), \
        reached(id,distance) AS MATERIALIZED (SELECT id,min(depth) FROM walk GROUP BY id), \
        page AS (SELECT {columns},n.node_hash __hash,r.distance FROM reached r JOIN nodes n ON n.node_hash=r.id ORDER BY r.distance,n.id LIMIT ?3 OFFSET ?4) \
        SELECT totals.__total,p.*,CASE WHEN p.distance=0 THEN NULL ELSE (SELECT json_array(predecessor,edge_kind) FROM ( \
            SELECT predecessor.id AS predecessor,e.kind AS edge_kind FROM edges e JOIN reached prior ON prior.id=e.{forward_to} AND prior.distance=p.distance-1 JOIN nodes predecessor ON predecessor.node_hash=e.{forward_to} WHERE e.{forward_from}=p.__hash AND e.kind IN({forward}) \
            {reverse_union} \
        ) ORDER BY predecessor,edge_kind LIMIT 1) END __reason \
        FROM (SELECT count(*) __total FROM reached r JOIN nodes n ON n.node_hash=r.id) totals \
        LEFT JOIN page p ON 1 ORDER BY p.distance,p.id")
    };
    let limit = i64::try_from(options.limit)?;
    let offset = i64::try_from(options.offset)?;
    let mut items = reader::rows(
        conn,
        &sql,
        &[&id_hash, &depth, &limit, &offset],
        options.limit,
    )?;
    let total = items
        .first()
        .and_then(|row| row["__total"].as_u64())
        .context("traversal total is missing")?;
    let total = usize::try_from(total)?;
    items.retain(|row| !row["id"].is_null());
    for item in &mut items {
        let item = item.as_object_mut().context("invalid traversal node")?;
        item.remove("__total");
        item.remove("__hash");
        let reason = item
            .remove("__reason")
            .and_then(|value| value.as_str().map(str::to_owned));
        let reason = reason
            .map(|encoded| serde_json::from_str::<(String, String)>(&encoded))
            .transpose()?;
        let predecessor = item
            .remove("__predecessor")
            .and_then(|value| value.as_str().map(str::to_owned))
            .or_else(|| reason.as_ref().map(|pair| pair.0.clone()));
        let edge_kind = item
            .remove("__edge_kind")
            .and_then(|value| value.as_str().map(str::to_owned))
            .or_else(|| reason.map(|pair| pair.1));
        if item.get("id").and_then(Value::as_str) == Some(id) {
            item.insert("is_seed".into(), json!(true));
        } else {
            item.insert("is_seed".into(), json!(false));
            item.insert("via".into(), json!({"id":predecessor.context("reached node has no predecessor")?,"kind":edge_kind.context("reached node has no edge kind")?}));
        }
    }
    let mut result = json!({
        "selector": node,
        "direction": if inbound { "inbound" } else { "outbound" },
        "depth": depth,
        "mode": mode_str,
        "nodes": paging::value(items, total, options, &generation),
    });
    if let Some(types) = edge_types {
        result["edge_types"] = json!(types);
    }
    Ok(result)
}

fn slice_paged(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    depth: u32,
    options: &QueryOptions,
) -> Result<Value> {
    match traverse_with_path_options(
        conn,
        selector,
        path,
        &TraversalOptions {
            depth,
            inbound: false,
            mode: None,
            edge_types: None,
            page: options,
        },
    ) {
        Ok(result) => return Ok(result),
        Err(error)
            if !matches!(
                error.downcast_ref::<reader::SelectorError>(),
                Some(reader::SelectorError::NotFound { .. })
            ) =>
        {
            return Err(error);
        }
        Err(_) => {}
    }
    let Some(path) = reader::directory_path(conn, selector)? else {
        let suggestions = reader::indexed_path_suggestions(conn, selector)?;
        return Err(reader::SelectorError::NotFound {
            selector: selector.to_owned(),
            suggestions,
        }
        .into());
    };
    let (lower, upper) = reader::path_bounds(&path);
    let nodes = paging::query(
        conn,
        &format!(
            "SELECT {} FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path>=?1 AND p.path<?2 \
             ORDER BY CASE WHEN n.kind IN ('module','file') THEN 0 ELSE 1 END,p.path,n.line,n.id",
            paging::nodes_with_path("n", options.detail, "p.path")
        ),
        &[&lower, &upper],
        options,
    )?;
    Ok(json!({
        "selector": {"kind":"directory","path":path},
        "scope":"directory_members",
        "direction":"outbound",
        "requested_depth":depth,
        "graph_depth_applied":0,
        "nodes":nodes,
        "hint":"Directory mode lists indexed nodes with graph depth 0. Select a returned exact node id or file path to traverse graph relationships at the requested depth."
    }))
}

/// Performs removal paged.
pub fn removal_paged(conn: &Connection, selector: &str, options: &QueryOptions) -> Result<Value> {
    removal_paged_with_path(conn, selector, None, options)
}

pub(crate) fn removal_paged_with_path(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    options: &QueryOptions,
) -> Result<Value> {
    let trimmed = selector.trim();
    let unpeeled = trimmed
        .strip_prefix("file://")
        .or_else(|| trimmed.strip_prefix("file:"))
        .unwrap_or(trimmed);
    let normalized_path = unpeeled.strip_prefix("./").unwrap_or(unpeeled);
    if normalized_path.ends_with(".md") {
        return Err(reader::SelectorError::DocLink {
            target: normalized_path.to_owned(),
        }
        .into());
    }
    let generation = paging::generation(conn, options)?;
    let selected_node = reader::select_detail_with_path(
        conn,
        selector,
        path,
        crate::core::response::Detail::Compact,
    )?;
    let file_count = paging::count(
        conn,
        "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1",
        &[&normalized_path],
    )?;
    let (selection, selected_id) = if file_count > 0 {
        (
            "SELECT n.node_hash FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1",
            normalized_path.to_owned(),
        )
    } else {
        (
            "SELECT node_hash FROM nodes WHERE id=?1",
            selected_node["id"].as_str().unwrap_or("").to_owned(),
        )
    };
    if file_count > 10000 {
        bail!("removal scope exceeds 10000 nodes");
    }
    let selected = format!("selected(hash) AS ({selection})");
    let dependency_rows = format!("SELECT e.src_hash,e.dst_hash,e.kind FROM edges e WHERE e.dst_hash IN(SELECT hash FROM selected) AND e.src_hash NOT IN(SELECT hash FROM selected) AND e.kind NOT IN('contains','references_doc',{REVERSE_DEPENDENCIES}) UNION SELECT e.src_hash,e.dst_hash,e.kind FROM edges e WHERE e.src_hash IN(SELECT hash FROM selected) AND e.dst_hash NOT IN(SELECT hash FROM selected) AND e.kind IN({REVERSE_DEPENDENCIES})");
    let dependency_cte =
        format!("WITH {selected}, dependencies AS MATERIALIZED ({dependency_rows})");
    let selection_sql = if file_count > 0 {
        "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 ORDER BY n.id"
    } else {
        "SELECT id FROM nodes WHERE id=?1 ORDER BY id"
    };
    let diagnostic_selected = reader::rows(conn, selection_sql, &[&selected_id], 10001)?;
    let selected_ids: Vec<String> = diagnostic_selected
        .iter()
        .filter_map(|node| node["id"].as_str().map(str::to_owned))
        .collect();
    let (target_unresolved, target_errors) = removal_diagnostics(conn, &selected_ids)?;
    let selected_total = selected_ids.len();
    let selected_items = selected_ids
        .iter()
        .skip(options.offset)
        .take(options.limit)
        .map(|id| json!({"id":id}))
        .collect();
    let selected_page = paging::value(selected_items, selected_total, options, &generation);
    let callers = paging::query(
        conn,
        &format!("{dependency_cte} SELECT {} FROM dependencies d JOIN edges e ON e.src_hash=d.src_hash AND e.dst_hash=d.dst_hash AND e.kind=d.kind ORDER BY e.path_id,e.line,e.src_hash,e.dst_hash,e.kind", paging::edges("e", options.detail)),
        &[&selected_id],
        options,
    )?;
    let dependency_count = callers["total"]
        .as_u64()
        .map(usize::try_from)
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("removal dependency total is missing"))?;
    let safe_to_remove = dependency_count == 0 && target_unresolved == 0 && target_errors == 0;
    Ok(
        json!({"assessment":removal_assessment(dependency_count,true,target_unresolved,target_errors),"selector":selector,
        "selected_ids":selected_page,
        "incoming_dependencies":callers,"safe_to_remove":safe_to_remove,"unresolved_references":target_unresolved,"parse_errors":target_errors,
        "proof_scope":"indexed static references only; dynamic entrypoints and external callers require separate authority",
        "generation":generation}),
    )
}

/// Execute a bounded graph operation through one options contract.
pub fn query_with_options(
    conn: &Connection,
    operation: &str,
    selector: Option<&str>,
    request: &GraphQueryOptions<'_>,
) -> Result<Value> {
    query_with_path_options(conn, operation, selector, None, request)
}

pub(crate) fn query_with_path_options(
    conn: &Connection,
    operation: &str,
    selector: Option<&str>,
    path: Option<&str>,
    request: &GraphQueryOptions<'_>,
) -> Result<Value> {
    let depth = request.depth;
    let direction = request.direction;
    let coverage = request.coverage;
    let options = request.page;
    let op = operation.trim();
    match op {
        "overview" => reader::overview_with_options(
            conn,
            &reader::OverviewOptions {
                aspects: None,
                page: options,
            },
        ),
        "inspect" => symbols::inspect_paged_response(
            conn,
            selector.unwrap_or(""),
            path,
            true,
            options,
            coverage,
        ),
        "explain" => symbols::explain_paged_response(
            conn,
            selector.unwrap_or(""),
            path,
            None,
            true,
            options,
            coverage,
        ),
        "impact" => traverse_with_path_options(
            conn,
            selector.unwrap_or(""),
            path,
            &TraversalOptions {
                depth,
                inbound: impact_inbound(direction)?,
                mode: None,
                edge_types: None,
                page: options,
            },
        ),
        "slice" => slice_paged(conn, selector.unwrap_or(""), path, depth, options),
        "unwired" => Ok(
            json!({"nodes":paging::query(conn, &format!("SELECT {} FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.kind IN('function','method') AND NOT EXISTS(SELECT 1 FROM edges e WHERE e.dst_hash=n.node_hash AND e.kind='calls') ORDER BY p.path,n.line,n.id", paging::nodes_with_path("n", options.detail, "p.path")), &[], options)?, "meaning":"no indexed static caller; not a dead-code proof"}),
        ),
        "sql" => reader::analyze_paged(conn, sql_selector(selector)?, None, options),
        "doctor" => reader::overview_with_options(
            conn,
            &reader::OverviewOptions {
                aspects: None,
                page: options,
            },
        ),
        "search" | "discover" | "find" => symbols::search_with_options(
            conn,
            selector.unwrap_or(""),
            &symbols::SearchOptions {
                kind: None,
                path,
                include_docs: false,
                exact: false,
                page: options,
            },
        ),
        _ => {
            bail!("unknown operation; supported: overview,inspect,explain,impact,slice,unwired,sql")
        }
    }
}
