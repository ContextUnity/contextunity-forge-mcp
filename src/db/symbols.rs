use super::{paging, reader};
use crate::core::{
    commitments, config,
    models::stable_hash64,
    response::{
        CoverageOptions, Detail, QueryOptions, ResponsePolicy, SourceOptions, MAX_PAGE_SIZE,
    },
};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
};

#[path = "symbols/search.rs"]
mod search;
pub use search::search_with_options;

#[path = "symbols/test_discovery.rs"]
mod test_discovery;
pub use test_discovery::tests_paged;
pub(crate) use test_discovery::tests_paged_with_path;

/// Filters and paging for a symbol search.
pub struct SearchOptions<'a> {
    /// Optional symbol kind.
    pub kind: Option<&'a str>,
    /// Workspace-relative file or directory scope.
    pub path: Option<&'a str>,
    /// Whether Markdown document symbols are included.
    pub include_docs: bool,
    /// Whether the name or qualified name must match exactly.
    pub exact: bool,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Output controls for symbol inspection.
pub struct InspectOptions<'a> {
    /// Whether linked documentation is included.
    pub show_doc: bool,
    /// Optional bounded source preview.
    pub source: &'a SourceOptions,
    /// Optional resolution coverage.
    pub coverage: CoverageOptions,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Output controls for symbol explanation.
pub struct ExplainOptions<'a> {
    /// Incoming, outgoing, or both directions.
    pub direction: Option<&'a str>,
    /// Whether linked documentation is included.
    pub show_doc: bool,
    /// Optional bounded source preview.
    pub source: &'a SourceOptions,
    /// Optional resolution coverage.
    pub coverage: CoverageOptions,
    /// Bounded page and generation contract.
    pub page: &'a QueryOptions,
}

/// Adds indexed signatures and bounded navigation previews to one search page.
pub(crate) fn search_results_with_navigation(
    conn: &Connection,
    root: &Path,
    mut result: Value,
    include_preview: bool,
) -> Result<Value> {
    const SIGNATURE_CHAR_LIMIT: usize = 512;
    const PREVIEW_HIT_LIMIT: usize = 3;
    const PREVIEW_BYTES_PER_HIT: usize = 4096;

    let has_small_total = result["nodes"]["total"]
        .as_u64()
        .is_some_and(|total| total <= PREVIEW_HIT_LIMIT as u64);
    let Some(items) = result["nodes"]["items"].as_array_mut() else {
        return Ok(result);
    };
    if items.is_empty() {
        return Ok(result);
    }

    let ids = items
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect::<Vec<_>>();
    let serialized_ids = serde_json::to_string(&ids)?;
    let metadata_rows = reader::rows(
        conn,
        "SELECT n.id,json_extract(n.details,'$.signature') AS signature,CASE WHEN n.is_test THEN 1 ELSE 0 END AS is_test,json_extract(n.details,'$.doc') AS docstring FROM nodes n JOIN json_each(?1) requested ON requested.value=n.id",
        &[&serialized_ids],
        ids.len(),
    )?;
    let metadata = metadata_rows
        .into_iter()
        .filter_map(|row| {
            Some((
                row["id"].as_str()?.to_owned(),
                (
                    row["signature"].as_str().map(str::to_owned),
                    row["is_test"].as_i64().map(|value| value != 0),
                    row["docstring"].as_str().map(str::to_owned),
                ),
            ))
        })
        .collect::<HashMap<_, _>>();

    let previews_enabled = include_preview || has_small_total;
    for (index, item) in items.iter_mut().enumerate() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .context("search result has no symbol id")?
            .to_owned();
        let (signature, is_test, docstring) =
            metadata.get(&id).cloned().unwrap_or((None, None, None));
        object.insert(
            "signature".into(),
            signature
                .map(|value| {
                    let mut bounded = value.chars().take(SIGNATURE_CHAR_LIMIT).collect::<String>();
                    if value.chars().count() > SIGNATURE_CHAR_LIMIT {
                        bounded.push('…');
                    }
                    json!(bounded)
                })
                .unwrap_or(Value::Null),
        );
        object.insert(
            "inspect_selector".into(),
            json!({"selector":id,"show_source":true}),
        );

        if let Some(is_test) = is_test {
            object.insert("is_test".into(), json!(is_test));
            let classification =
                if object.get("language").and_then(Value::as_str) == Some("markdown") {
                    "documentation"
                } else if is_test {
                    "test"
                } else {
                    "production"
                };
            object.insert("classification".into(), json!(classification));
        }
        if let Some(docstring) = docstring.as_deref().and_then(first_docstring_sentence) {
            object.insert("docstring_first_sentence".into(), json!(docstring));
        }

        if index < PREVIEW_HIT_LIMIT
            && object.get("language").and_then(Value::as_str) != Some("markdown")
        {
            object.insert("graph_pulse".into(), search_graph_pulse(conn, &id));
        }

        if !previews_enabled
            || index >= PREVIEW_HIT_LIMIT
            || object.get("language").and_then(Value::as_str) == Some("markdown")
        {
            continue;
        }
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .context("search result has no path")?;
        let Ok(text) = verified_source(conn, root, path) else {
            continue;
        };
        let node = json!({
            "id": id,
            "kind": object.get("kind"),
            "name": object.get("name"),
            "qualname": object.get("qualname"),
            "path": path,
            "line": object.get("line"),
            "end_line": object.get("end_line"),
            "language": object.get("language")
        });
        let options = SourceOptions {
            enabled: true,
            leading_lines: 5,
            max_body_lines: 15,
            offset: 0,
        };
        if let Ok((source, preview)) =
            source_preview(conn, &node, &text, &options, PREVIEW_BYTES_PER_HIT)
        {
            object.insert("source".into(), json!(source));
            object.insert("source_preview".into(), preview);
        }
    }
    Ok(result)
}

fn search_graph_pulse(conn: &Connection, id: &str) -> Value {
    let callers = match direct_call_summary(conn, id, true, 3) {
        Ok(summary) => summary,
        Err(_) => {
            json!({"availability":"unavailable","cause":"caller relationships exceeded the indexed query budget"})
        }
    };
    let implementations = (|| -> Result<usize> {
        let budget = reader::QueryBudget::new(conn);
        let count = conn.query_row(
            "SELECT count(*) FROM edges WHERE kind='implements' AND (src_hash=?1 OR dst_hash=?1)",
            [stable_hash64(id)],
            |row| row.get::<_, usize>(0),
        )?;
        budget.check()?;
        Ok(count)
    })()
    .map(|count| json!({"availability":"available","count":count}))
    .unwrap_or_else(|_| json!({"availability":"unavailable","cause":"implementation relationships are unavailable within the indexed query budget"}));

    let covering_tests = QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(3),
        0,
        Some(Detail::Compact),
        None,
    )
    .and_then(|page| test_discovery::tests_paged_with_path(conn, id, None, "inbound", &page))
    .map(|result| {
        let method = result["discovery_method"]
            .as_str()
            .unwrap_or("unavailable")
            .to_owned();
        let total = result["nodes"]["total"].clone();
        json!({
            "availability":"available",
            "discovery_method":method,
            "total":total,
            "items":result["nodes"]["items"],
            "unresolved_references":result["unresolved_references"]
        })
    })
    .unwrap_or_else(|_| json!({"availability":"unavailable","cause":"test relationships are unavailable within the indexed query budget"}));

    json!({
        "availability":"available",
        "callers":callers,
        "implementations":implementations,
        "covering_tests":covering_tests
    })
}

fn first_docstring_sentence(docstring: &str) -> Option<String> {
    const MAX_CHARS: usize = 240;
    let literal = docstring.trim();
    let prefix_len = literal
        .char_indices()
        .take_while(|(_, character)| {
            matches!(character, 'r' | 'R' | 'u' | 'U' | 'b' | 'B' | 'f' | 'F')
        })
        .take(3)
        .last()
        .map_or(0, |(index, character)| index + character.len_utf8());
    let content = (prefix_len > 0)
        .then(|| strip_string_literal_delimiters(&literal[prefix_len..]))
        .flatten()
        .or_else(|| strip_string_literal_delimiters(literal))
        .unwrap_or(literal);
    let sentence = content.trim().lines().next()?.trim();
    if sentence.is_empty() {
        return None;
    }
    let end = sentence
        .char_indices()
        .find_map(|(index, character)| {
            (matches!(character, '.' | '!' | '?')
                && sentence[index + character.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace))
            .then_some(index + character.len_utf8())
        })
        .unwrap_or(sentence.len());
    let sentence = sentence.get(..end)?;
    let mut bounded = sentence.chars().take(MAX_CHARS).collect::<String>();
    if sentence.chars().count() > MAX_CHARS {
        bounded.push('…');
    }
    Some(bounded)
}

fn strip_string_literal_delimiters(literal: &str) -> Option<&str> {
    ["\"\"\"", "'''", "\"", "'"].iter().find_map(|delimiter| {
        literal
            .strip_prefix(delimiter)
            .and_then(|body| body.strip_suffix(delimiter))
    })
}

pub(crate) fn verified_source(conn: &Connection, root: &Path, path: &str) -> Result<String> {
    let digest: String = conn.query_row("SELECT digest FROM files WHERE path=?1", [path], |r| {
        r.get(0)
    })?;
    let linked_json: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='adapter_linked_workspaces'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let linked: Value = linked_json
        .map(|json| serde_json::from_str(&json))
        .transpose()?
        .unwrap_or_else(|| json!([]));
    let linked_root = linked.as_array().and_then(|workspaces| {
        workspaces.iter().find_map(|workspace| {
            let name = workspace.get("name")?.as_str()?;
            let prefix = format!("[{name}]/");
            let relative = path.strip_prefix(&prefix)?;
            let root = workspace.get("path")?.as_str()?;
            Some((std::path::PathBuf::from(root), relative))
        })
    });
    let (source_root, source_path) = match linked_root {
        Some((linked_root, relative)) => (linked_root, relative.to_owned()),
        None => (root.to_path_buf(), path.to_owned()),
    };
    let adapter = config::load_adapter(&source_root, None)?;
    let resolved = config::resolve_file_path(&source_root, &adapter, &source_path)?;
    let file = File::open(resolved).context("source file unavailable; rebuild index")?;
    if !file.metadata()?.is_file() {
        bail!("source must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take(config::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > config::MAX_FILE_BYTES {
        bail!("source exceeds file byte limit");
    }
    if commitments::hash(&bytes) != digest {
        bail!("source changed since indexing; update or rebuild index");
    }
    String::from_utf8(bytes).context("source is not UTF-8")
}

/// Inspect one indexed symbol with bounded documentation, coverage, and source.
pub fn inspect_with_options(
    conn: &Connection,
    root: &Path,
    selector: &str,
    options: &InspectOptions<'_>,
) -> Result<Value> {
    inspect_with_path_options(conn, root, selector, None, options)
}

pub(crate) fn inspect_with_path_options(
    conn: &Connection,
    root: &Path,
    selector: &str,
    path: Option<&str>,
    options: &InspectOptions<'_>,
) -> Result<Value> {
    let result = inspect_paged_response(
        conn,
        selector,
        path,
        options.show_doc,
        options.page,
        options.coverage,
    )?;
    with_source(conn, root, result, options.source, options.page)
}

/// Performs snippet paged.
pub fn snippet_paged(
    conn: &Connection,
    root: &Path,
    selector: &str,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    snippet_paged_with_path(conn, root, selector, None, source, options)
}

pub(crate) fn snippet_paged_with_path(
    conn: &Connection,
    root: &Path,
    selector: &str,
    path: Option<&str>,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let node = reader::select_detail_with_path(conn, selector, path, options.detail)?;
    with_source(
        conn,
        root,
        json!({"node":node,"generation":generation}),
        source,
        options,
    )
}

/// Explain direct relationships of one indexed symbol.
pub fn explain_with_options(
    conn: &Connection,
    root: &Path,
    selector: &str,
    options: &ExplainOptions<'_>,
) -> Result<Value> {
    explain_with_path_options(conn, root, selector, None, options)
}

pub(crate) fn explain_with_path_options(
    conn: &Connection,
    root: &Path,
    selector: &str,
    path: Option<&str>,
    options: &ExplainOptions<'_>,
) -> Result<Value> {
    let result = explain_paged_response(
        conn,
        selector,
        path,
        options.direction,
        options.show_doc,
        options.page,
        options.coverage,
    )?;
    with_source(conn, root, result, options.source, options.page)
}

pub(crate) fn inspect_paged_response(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
    coverage: CoverageOptions,
) -> Result<Value> {
    let mut result = if coverage.include_coverage {
        reader::inspect_paged_with_path(conn, selector, path, show_doc, options)?
    } else {
        inspect_without_coverage(conn, selector, path, show_doc, options)?
    };
    let id = result["node"]["id"]
        .as_str()
        .context("invalid selected node id")?
        .to_owned();
    result["documented_in"] = paging::query(
        conn,
        "SELECT d.doc_id,d.path,d.section_title,d.doc_type,n.line doc_line,e.line FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN doc_sections d ON d.doc_id=n.id WHERE e.kind='references_doc' AND e.src_hash=?1 ORDER BY d.path,d.doc_id,e.line",
        &[&stable_hash64(&id)],
        options,
    )?;
    result["governing_invariants"] = reader::governing_invariants(conn, &id)?;
    let result = with_analysis_gaps(conn, result, options)?;
    summarize_symbol(conn, result, coverage.include_coverage, options.detail)
}

pub(crate) fn attach_inspect_document_contents(
    conn: &Connection,
    result: &mut Value,
) -> Result<()> {
    let Some(items) = result
        .get("documented_in")
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    if items.is_empty() {
        return Ok(());
    }
    ensure!(
        items.len() <= MAX_PAGE_SIZE,
        "documented_in page exceeds the response page bound"
    );

    let doc_ids = items
        .iter()
        .map(|item| {
            item.get("doc_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .context("documented_in page item is missing doc_id")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut contents = BTreeMap::new();
    if let Some(documents) = result
        .get("documents")
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
    {
        for document in documents {
            let (Some(doc_id), Some(content)) = (
                document.get("doc_id").and_then(Value::as_str),
                document.get("content").and_then(Value::as_str),
            ) else {
                continue;
            };
            contents
                .entry(doc_id.to_owned())
                .or_insert_with(|| content.to_owned());
        }
    }
    let available = contents.keys().cloned().collect::<BTreeSet<_>>();
    let missing = doc_ids
        .difference(&available)
        .cloned()
        .collect::<BTreeSet<_>>();
    contents.extend(reader::document_contents_by_ids(conn, &missing)?);
    ensure!(
        doc_ids.iter().all(|doc_id| contents.contains_key(doc_id)),
        "documented_in page has no matching document content"
    );

    let mut attached = HashSet::new();
    let items = result
        .get_mut("documented_in")
        .and_then(|page| page.get_mut("items"))
        .and_then(Value::as_array_mut)
        .context("documented_in page items changed during content projection")?;
    for item in items {
        let doc_id = item
            .get("doc_id")
            .and_then(Value::as_str)
            .context("documented_in page item is missing doc_id")?
            .to_owned();
        let object = item
            .as_object_mut()
            .context("documented_in page item is not an object")?;
        if attached.insert(doc_id.clone()) {
            let content = contents
                .get(&doc_id)
                .context("documented_in page has no matching document content")?;
            object.insert("content".into(), json!(content));
        } else {
            object.remove("content");
        }
    }
    Ok(())
}

pub(crate) fn explain_paged_response(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    direction: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
    coverage: CoverageOptions,
) -> Result<Value> {
    let result = if coverage.include_coverage {
        reader::explain_with_path_options(
            conn,
            selector,
            path,
            &reader::ExplainOptions {
                direction,
                show_doc,
                page: options,
            },
        )?
    } else {
        explain_without_coverage(conn, selector, path, direction, show_doc, options)?
    };
    let result = with_analysis_gaps(conn, result, options)?;
    summarize_symbol(conn, result, coverage.include_coverage, options.detail)
}

fn with_analysis_gaps(
    conn: &Connection,
    mut result: Value,
    options: &QueryOptions,
) -> Result<Value> {
    let node = &result["node"];
    let scope_symbol = node["id"].as_str().context("invalid selected node id")?;
    let path = node["path"]
        .as_str()
        .context("invalid selected node path")?;
    let start = node["line"]
        .as_i64()
        .context("invalid selected start line")?;
    let end = node["end_line"]
        .as_i64()
        .context("invalid selected end line")?;
    result["analysis_gaps"] = paging::query(
        conn,
        "SELECT p.path,c.line,x.expression AS reason,v.evidence,?4 AS scope_symbol \
         FROM resolution_coverage c \
         JOIN path_dictionary p ON p.path_id=c.path_id \
         JOIN coverage_expressions x ON x.expression_id=c.expression_id \
         JOIN coverage_evidence v ON v.evidence_id=c.evidence_id \
         WHERE c.path_id=(SELECT path_id FROM path_dictionary WHERE path=?1) \
           AND c.line BETWEEN ?2 AND ?3 AND v.evidence LIKE 'analysis gap:%' \
         ORDER BY c.line,x.expression,v.evidence",
        &[&path, &start, &end, &scope_symbol],
        options,
    )?;
    Ok(result)
}

fn inspect_without_coverage(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    let generation = paging::generation(conn, options)?;
    let node = reader::select_detail_with_path(conn, selector, path, options.detail)?;
    let id = node["id"].as_str().context("invalid node id")?;
    let mut documents = if show_doc {
        paging::query(
            conn,
            &format!(
                "SELECT {} FROM doc_sections d JOIN nodes dn ON dn.id=d.doc_id JOIN edges e ON e.dst_hash=dn.node_hash WHERE e.src_hash=?1 AND e.kind='references_doc' ORDER BY d.is_invariant DESC,d.path,d.doc_id",
                paging::docs("d", options.detail)
            ),
            &[&stable_hash64(id)],
            options,
        )?
    } else {
        paging::value(Vec::new(), 0, options, &generation)
    };
    if documents["total"] == 0 {
        documents = json!({"total": 0});
    }
    Ok(json!({"node":node,"documents":documents,"generation":generation}))
}

fn explain_without_coverage(
    conn: &Connection,
    selector: &str,
    path: Option<&str>,
    direction: Option<&str>,
    show_doc: bool,
    options: &QueryOptions,
) -> Result<Value> {
    let mut result = inspect_without_coverage(conn, selector, path, show_doc, options)?;
    let id = result["node"]["id"]
        .as_str()
        .context("invalid node id")?
        .to_owned();
    let node_hash = stable_hash64(&id);
    let direction = match direction.unwrap_or("both") {
        "inbound" => "incoming",
        "outbound" => "outgoing",
        direction => direction,
    };
    match direction {
        "both" | "incoming" => {
            let mut incoming = paging::query(
                conn,
                &format!(
                    "SELECT {} FROM edges e WHERE e.dst_hash=?1 ORDER BY e.kind,e.src_hash,e.path_id,e.line",
                    paging::edges("e", options.detail)
                ),
                &[&node_hash],
                options,
            )?;
            annotate_edge_direction(&mut incoming, "inbound");
            result["incoming"] = incoming;
        }
        "outgoing" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE dst_hash=?1",
                &[&node_hash],
            )?;
            result["incoming"] = json!({"total":total,"omitted":true,"hint":"Pass direction='incoming' to page incoming edges."});
        }
        _ => bail!("direction must be both, incoming (inbound), or outgoing (outbound)"),
    }
    match direction {
        "both" | "outgoing" => {
            let mut outgoing = paging::query(
                conn,
                &format!(
                    "SELECT {} FROM edges e WHERE e.src_hash=?1 ORDER BY e.kind,e.dst_hash,e.path_id,e.line",
                    paging::edges("e", options.detail)
                ),
                &[&node_hash],
                options,
            )?;
            annotate_edge_direction(&mut outgoing, "outbound");
            result["outgoing"] = outgoing;
        }
        "incoming" => {
            let total = paging::count(
                conn,
                "SELECT count(*) FROM edges WHERE src_hash=?1",
                &[&node_hash],
            )?;
            result["outgoing"] = json!({"total":total,"omitted":true,"hint":"Pass direction='outgoing' to page outgoing edges."});
        }
        _ => {}
    }
    let implementors = reader::rows(
        conn,
        "SELECT n.id AS src_public_id,e.kind,p.path,e.line,v.evidence,n.name,n.kind AS node_kind FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=e.path_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.dst_hash=?1 AND e.kind IN('implements','overrides','extends') ORDER BY p.path,e.line",
        &[&node_hash],
        50,
    )?;
    if !implementors.is_empty() {
        result["implementors"] = json!(implementors);
    }
    let implements = reader::rows(
        conn,
        "SELECT n.id AS dst_public_id,e.kind,p.path,e.line,v.evidence,n.name,n.kind AS node_kind FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.path_id JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.src_hash=?1 AND e.kind IN('implements','overrides','extends') ORDER BY p.path,e.line",
        &[&node_hash],
        50,
    )?;
    if !implements.is_empty() {
        result["implements"] = json!(implements);
    }
    result["direction"] = json!(direction);
    Ok(result)
}

fn annotate_edge_direction(page: &mut Value, direction: &str) {
    if let Some(items) = page["items"].as_array_mut() {
        for item in items {
            if let Some(edge) = item.as_object_mut() {
                edge.insert("direction".into(), json!(direction));
            }
        }
    }
}

fn summarize_symbol(
    conn: &Connection,
    mut result: Value,
    include_coverage: bool,
    detail: crate::core::response::Detail,
) -> Result<Value> {
    if !include_coverage {
        result
            .as_object_mut()
            .context("invalid symbol result")?
            .remove("coverage");
    }
    if detail == Detail::Compact {
        let summary = compact_summary(conn, &result["node"])?;
        result["summary"] = summary;
    }
    Ok(result)
}

fn compact_summary(conn: &Connection, node: &Value) -> Result<Value> {
    const PREVIEW_LIMIT: i64 = 5;
    let id = node["id"].as_str().context("selected node has no id")?;
    let (
        signature,
        docstring,
        receiver_type,
        receiver,
        receiver_name,
        is_method,
        is_static,
    ) = conn.query_row(
        "SELECT json_extract(details,'$.signature'),json_extract(details,'$.doc'),json_extract(details,'$.receiver_type'),json_extract(details,'$.receiver'),json_extract(details,'$.receiver_name'),json_extract(details,'$.is_method'),json_extract(details,'$.is_static') FROM nodes WHERE id=?1",
        [id],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<bool>>(5)?,
                row.get::<_, Option<bool>>(6)?,
            ))
        },
    )?;
    let mut container = reader::rows(
        conn,
        "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.dst_hash=?1 AND e.kind='contains' ORDER BY CASE WHEN n.kind IN('module','file') THEN 1 ELSE 0 END,n.line DESC,n.end_line,n.id LIMIT 1",
        &[&stable_hash64(id)],
        1,
    )?
    .into_iter()
    .next()
    .unwrap_or(Value::Null);
    let receiver = receiver_type.or(receiver);
    let is_method = node["kind"] == "method" || is_method.unwrap_or(false);
    let is_type_container = matches!(
        container["kind"].as_str(),
        Some("class" | "enum" | "interface" | "record" | "struct" | "trait" | "type")
    );
    if is_method && !is_type_container {
        if let Some(receiver) = receiver.as_deref() {
            let path = node["path"].as_str().unwrap_or("");
            let candidates = reader::rows(
                conn,
                "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2 COLLATE NOCASE AND n.name=?2 COLLATE BINARY AND n.kind IN('class','enum','interface','record','struct','trait','type') ORDER BY n.line,n.id LIMIT 101",
                &[&path, &receiver],
                101,
            )?;
            let receiver_container = if candidates.len() == 1 {
                candidates.into_iter().next()
            } else if (2..=100).contains(&candidates.len()) {
                let method_qualname = node["qualname"].as_str().unwrap_or("");
                let module_qualname = container["qualname"].as_str().unwrap_or("");
                let mut scoped = candidates
                    .into_iter()
                    .filter(|candidate| {
                        let qualname = candidate["qualname"].as_str().unwrap_or("");
                        let impl_owner = !module_qualname.is_empty() && qualname == module_qualname;
                        let method_owner = [".", "::"].iter().any(|separator| {
                            method_qualname.starts_with(&format!("{qualname}{separator}"))
                        });
                        let module_owner = [".", "::"].iter().any(|separator| {
                            qualname == format!("{module_qualname}{separator}{receiver}")
                        });
                        impl_owner || method_owner || module_owner
                    })
                    .collect::<Vec<_>>();
                if scoped.len() > 1 {
                    let method_line = node["line"].as_u64().unwrap_or_default();
                    let nearest_line = scoped
                        .iter()
                        .filter_map(|candidate| candidate["line"].as_u64())
                        .map(|line| line.abs_diff(method_line))
                        .min();
                    if let Some(nearest_line) = nearest_line {
                        scoped.retain(|candidate| {
                            candidate["line"]
                                .as_u64()
                                .is_some_and(|line| line.abs_diff(method_line) == nearest_line)
                        });
                    }
                }
                if scoped.len() == 1 {
                    scoped.pop()
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(receiver_container) = receiver_container {
                container = receiver_container;
            } else {
                container = json!({
                    "kind": "receiver",
                    "name": receiver,
                    "path": node["path"],
                    "line": node["line"]
                });
            }
        }
    }
    let inbound = direct_call_summary(conn, id, true, PREVIEW_LIMIT)?;
    let outbound = direct_call_summary(conn, id, false, PREVIEW_LIMIT)?;
    let mut summary = json!({
        "signature": signature,
        "docstring": docstring,
        "container": container,
        "calls": {
            "inbound": inbound,
            "outbound": outbound
        }
    });
    if is_method {
        summary["method"] = json!({
            "receiver": receiver,
            "receiver_name": receiver_name,
            "is_static": is_static
        });
    }
    Ok(summary)
}

fn direct_call_summary(
    conn: &Connection,
    id: &str,
    inbound: bool,
    preview_limit: i64,
) -> Result<Value> {
    let (count_sql, preview_sql) = if inbound {
        (
            "SELECT count(*),coalesce(sum(occurrence_count),0) FROM edges WHERE dst_hash=?1 AND kind='calls'",
            "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.dst_hash=?1 AND e.kind='calls' ORDER BY p.path,n.line,n.id LIMIT ?2",
        )
    } else {
        (
            "SELECT count(*),coalesce(sum(occurrence_count),0) FROM edges WHERE src_hash=?1 AND kind='calls'",
            "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=n.path_id WHERE e.src_hash=?1 AND e.kind='calls' ORDER BY p.path,n.line,n.id LIMIT ?2",
        )
    };
    let (count, occurrences) = {
        let budget = reader::QueryBudget::new(conn);
        let counts = conn.query_row(count_sql, [stable_hash64(id)], |row| {
            Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?))
        })?;
        budget.check()?;
        counts
    };
    let preview = reader::rows(
        conn,
        preview_sql,
        &[&stable_hash64(id), &preview_limit],
        preview_limit as usize,
    )?;
    Ok(json!({"count":count,"occurrences":occurrences,"preview":preview}))
}

fn with_source(
    conn: &Connection,
    root: &Path,
    mut result: Value,
    source: &SourceOptions,
    options: &QueryOptions,
) -> Result<Value> {
    if !source.enabled {
        return Ok(result);
    }
    anyhow::ensure!(
        source.leading_lines <= 20 && (1..=500).contains(&source.max_body_lines),
        "invalid source preview bounds"
    );
    anyhow::ensure!(
        source.offset == 0 || options.generation.is_some(),
        "source continuation requires generation from the previous page"
    );
    let node = &result["node"];
    let path = node["path"].as_str().context("invalid node path")?;
    let text = verified_source(conn, root, path)?;
    let (snippet, preview) = source_preview(conn, node, &text, source, 8192)?;
    let gen = result.as_object_mut().and_then(|m| m.remove("generation"));
    result["source"] = json!(snippet);
    result["source_preview"] = preview;
    if let Some(gen) = gen {
        if let Some(m) = result.as_object_mut() {
            m.insert("generation".into(), gen);
        }
    }
    Ok(result)
}

fn source_preview(
    conn: &Connection,
    node: &Value,
    text: &str,
    options: &SourceOptions,
    source_bytes: usize,
) -> Result<(String, Value)> {
    let path = node["path"].as_str().context("invalid node path")?;
    let start = node["line"].as_u64().context("invalid start line")? as usize;
    let end = node["end_line"].as_u64().context("invalid end line")? as usize;
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    anyhow::ensure!(
        start > 0 && end >= start && end <= lines.len(),
        "node has no valid source range"
    );
    let line_start: usize = lines[..start - 1].iter().map(|line| line.len()).sum();
    let line_end: usize = line_start
        + lines[start - 1..end]
            .iter()
            .map(|line| line.len())
            .sum::<usize>();
    let mut range = symbol_range(node, text)?.unwrap_or(line_start..line_end);
    anyhow::ensure!(
        range.start >= line_start && range.end <= line_end,
        "source boundary differs from indexed symbol"
    );
    let starts_on_clean_line = text[line_start..range.start].trim().is_empty();
    if text[line_start..range.start].trim().is_empty() {
        range.start = line_start;
    }
    if text[range.end..line_end].trim().is_empty() {
        range.end = line_end;
    }
    let body: Vec<_> = text[range].split_inclusive('\n').collect();
    anyhow::ensure!(
        options.offset <= body.len(),
        "source_offset exceeds symbol body"
    );
    let previous: usize = conn.query_row("SELECT coalesce(max(n.end_line),0) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.end_line<?2 AND n.kind NOT IN('file','module','component')", rusqlite::params![path,start], |r| r.get(0))?;
    let leading_start = (start - 1)
        .saturating_sub(options.leading_lines)
        .max(previous);
    let available_leading = if options.offset == 0 && starts_on_clean_line {
        start - 1 - leading_start
    } else {
        0
    };
    let mut leading = String::new();
    let mut leading_count = 0;
    if options.offset == 0 && starts_on_clean_line {
        for line in lines[leading_start..start - 1].iter().rev() {
            if escaped_cost(line) + escaped_cost(&leading) > source_bytes / 4 {
                break;
            }
            leading.insert_str(0, line);
            leading_count += 1;
        }
    }
    let mut output = leading;
    let mut cost = escaped_cost(&output);
    let mut emitted = 0;
    let mut truncated_line = false;
    for line in body
        .iter()
        .skip(options.offset)
        .take(options.max_body_lines)
    {
        let line_cost = escaped_cost(line);
        if cost + line_cost > source_bytes.saturating_sub(2) {
            if emitted == 0 {
                let available = source_bytes.saturating_sub(2).saturating_sub(cost);
                let mut used = 0;
                for ch in line.chars() {
                    let next = escaped_char_cost(ch);
                    if used + next > available {
                        break;
                    }
                    output.push(ch);
                    used += next;
                }
                truncated_line = true;
            }
            break;
        }
        output.push_str(line);
        cost += line_cost;
        emitted += 1;
    }
    let next = options.offset + emitted;
    let has_more = next < body.len();
    let hint = if truncated_line {
        Some(format!("Line {} exceeds the source preview byte budget; read that line from local file {path}. No line continuation can return the omitted characters.",start+next))
    } else if has_more {
        Some("Repeat the same tool and selector with show_source=true, next_source_offset as source_offset, and this generation.".to_owned())
    } else {
        None
    };
    let start_line = (!output.is_empty()).then_some(start + options.offset - leading_count);
    let end_line = (!output.is_empty()).then_some(if emitted == 0 {
        start + options.offset
    } else {
        start + next - 1
    });
    Ok((
        output,
        json!({"start_line":start_line,"end_line":end_line,"total_body_lines":body.len(),"body_offset":options.offset,"body_lines":emitted,"leading_lines":leading_count,"next_source_offset":if has_more && !truncated_line {Some(next)} else {None},"has_more":has_more,"omitted_leading_lines":available_leading.saturating_sub(leading_count),"omitted_body_lines":body.len().saturating_sub(next),"truncated_line":truncated_line,"continuation_hint":hint}),
    ))
}

fn escaped_char_cost(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0c}' => 2,
        ch if ch <= '\u{1f}' => 6,
        ch => ch.len_utf8(),
    }
}
fn escaped_cost(text: &str) -> usize {
    text.chars().map(escaped_char_cost).sum()
}

fn symbol_range(node: &Value, text: &str) -> Result<Option<std::ops::Range<usize>>> {
    let kind = node["kind"].as_str().unwrap_or("");
    if matches!(kind, "file" | "module" | "component") {
        return Ok(None);
    }
    let path = node["path"].as_str().context("invalid node path")?;
    let language = node["language"].as_str().context("invalid language")?;
    let profile = crate::engine::languages::require(language)?;
    let start = node["line"].as_u64().context("invalid start line")? as usize;
    let name = node["name"].as_str().context("invalid node name")?;
    let mut parser = profile.create_parser(path)?;
    parser.set_timeout_micros(2_000_000);
    let tree = parser
        .parse(text, None)
        .context("source preview parse exceeds time budget")?;
    let mut stack = vec![tree.root_node()];
    let mut matches = Vec::new();
    while let Some(syntax) = stack.pop() {
        if syntax.start_position().row + 1 > start || syntax.end_position().row + 1 < start {
            continue;
        }
        if syntax.start_position().row + 1 == start
            && profile.symbol_with_source(syntax, text) == Some(kind)
        {
            let candidate = profile
                .symbol_name(syntax, text)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!("anonymous@{}:{}", start, syntax.start_position().column + 1)
                });
            if candidate == name {
                matches.push(syntax.byte_range());
            }
        }
        let mut cursor = syntax.walk();
        stack.extend(syntax.named_children(&mut cursor));
    }
    matches.sort_by_key(|range| range.start);
    let qualname = node["qualname"].as_str().unwrap_or(name);
    let base = format!("{}:{path}:{start}:{name}", profile.node_prefix(kind));
    let field_base = format!("field:{path}:{start}:{qualname}");
    let id = node["id"].as_str().context("invalid node id")?;
    let selected = if id == base || id == field_base {
        matches.into_iter().next()
    } else {
        matches.into_iter().find(|range| {
            let col = text[..range.start].rsplit('\n').next().unwrap_or("").len();
            id == format!("{base}:{col}") || id == format!("{field_base}:{col}")
        })
    };
    if selected.is_none() && (id.starts_with("field:") || id.starts_with("template:")) {
        return Ok(None);
    }
    selected
        .map(Some)
        .context("indexed symbol boundary is unavailable; read the local file instead")
}

#[cfg(all(test, feature = "lang-python"))]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_boundary_recovery_and_preview_resilience() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::core::schema::SCHEMA_DDL).unwrap();
        let options = SourceOptions {
            enabled: true,
            leading_lines: 0,
            max_body_lines: 35,
            offset: 0,
        };
        for (id, kind, name, text, expected) in [
            (
                "type:case.py:1:Strategy",
                "type",
                "Strategy",
                "Strategy = Literal['direct']; neighbor = 1\n",
                "Strategy = Literal['direct']",
            ),
            (
                "py:case.py:1:pool_size",
                "variable",
                "pool_size",
                "pool_size = 3; neighbor = 1\n",
                "pool_size = 3",
            ),
            (
                "type:case.py:1:Strategy",
                "type",
                "Strategy",
                "type Strategy = str; neighbor = 1\n",
                "type Strategy = str",
            ),
            (
                "field:case.py:1:Pool.size",
                "field",
                "size",
                "    self.size = (\n        3\n    )\n",
                "    self.size = (\n        3\n    )\n",
            ),
        ] {
            let node = json!({"id":id,"kind":kind,"name":name,"qualname":format!("Pool.{name}"),"path":"case.py","line":1,"end_line":text.lines().count(),"language":"python"});
            let (source, _) = source_preview(&conn, &node, text, &options, 8192).unwrap();
            assert_eq!(source, expected, "{id}");
        }
        if cfg!(feature = "lang-html") {
            let node = json!({"id":"template:case.html:1:strategy","kind":"variable","name":"strategy","path":"case.html","line":1,"end_line":1,"language":"html"});
            let text = "{% set strategy = 'direct' %}\n";
            assert_eq!(
                source_preview(&conn, &node, text, &options, 8192)
                    .unwrap()
                    .0,
                text
            );
        }
        let unresolved = json!({"id":"py:case.py:1:missing","kind":"function","name":"missing","path":"case.py","line":1,"end_line":1,"language":"python"});
        assert!(symbol_range(&unresolved, "# no declaration\n").is_err());
        let results = json!({"nodes":{"total":1,"items":[unresolved]}});
        let result = search_results_with_navigation(&conn, Path::new("."), results, true).unwrap();
        let hit = &result["nodes"]["items"][0];
        assert_eq!(hit["path"], "case.py");
        assert_eq!(hit["line"], 1);
        assert_eq!(hit["inspect_selector"]["selector"], hit["id"]);
        assert!(hit.get("source_preview").is_none());
    }
}
