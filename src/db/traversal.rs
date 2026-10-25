use super::{paging, reader, symbols};
use crate::core::{
    commitments, config, git,
    models::stable_hash64,
    response::{CoverageOptions, QueryOptions, MAX_PAGE_SIZE},
};
use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};
// Stored decorates edges point from decorator to target; dependency walks reverse them.
pub(super) const FORWARD_DEPENDENCIES: &str =
    "'calls','possible_calls','inherits','implements','overrides','extends','includes','mutates','handles','references','calls_endpoint'";
pub(super) const REVERSE_DEPENDENCIES: &str = "'decorates'";
const STRUCTURAL_DEPENDENCIES: &str = ",'imports','contains','documents'";
const MAX_DEEP_TRAVERSAL_FRONTIER: usize = 1000;
const MAX_CHANGE_FILES: usize = 100;
const MAX_CHANGE_SEEDS: usize = 64;
const MAX_CHANGE_RESULT_NODES: usize = 10_000;
const MAX_GIT_DIFF_BYTES: usize = 8 * 1024 * 1024;

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

pub use super::cycles::cycles;

/// Performs impact inbound.
pub fn impact_inbound(direction: Option<&str>) -> Result<bool> {
    match direction.unwrap_or("inbound") {
        "inbound" => Ok(true),
        "outbound" => Ok(false),
        _ => bail!("impact direction must be inbound or outbound"),
    }
}

#[derive(Clone, Debug)]
struct ChangedRange {
    path: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct ParsedHunk {
    added_range: Option<(usize, usize)>,
    deleted_only: bool,
}

struct HunkState {
    added_range: Option<(usize, usize)>,
    removed: bool,
    added: bool,
    old_remaining: usize,
    new_remaining: usize,
}

#[derive(Clone, Debug)]
struct ChangedFile {
    status: String,
    path: String,
    old_path: Option<String>,
    ranges: Vec<ChangedRange>,
}

struct ChangeSnapshot {
    mode: String,
    files: Vec<ChangedFile>,
    ranges: Vec<ChangedRange>,
    unmapped: Vec<Value>,
    fingerprint: String,
}

fn append_fingerprint(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn git_bytes(root: &Path, args: &[String], context: &str) -> Result<Vec<u8>> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = git::git_command(root, &args)?;
    if !output.status.success() {
        bail!(
            "Git {context} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git_diff_prefix(mode: &str) -> Vec<String> {
    let mut args = vec!["diff".to_owned()];
    match mode {
        "staged" => args.push("--cached".to_owned()),
        "head" => args.push("HEAD".to_owned()),
        _ => {}
    }
    args
}

fn valid_workspace_path(value: &str) -> bool {
    !value.is_empty()
        && config::safe_relative(Path::new(value))
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn path_is_within(path: &str, scope: Option<&str>) -> bool {
    scope.is_none_or(|scope| path == scope || path.starts_with(&format!("{scope}/")))
}

fn next_nul_field<'a>(bytes: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    if *offset >= bytes.len() {
        return None;
    }
    let rest = &bytes[*offset..];
    let length = rest
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(rest.len());
    *offset += length.saturating_add(1);
    Some(&rest[..length])
}

fn parse_name_status(bytes: &[u8]) -> Result<Vec<(String, String, Option<String>)>> {
    let mut offset = 0;
    let mut changes = Vec::new();
    while let Some(status) = next_nul_field(bytes, &mut offset) {
        if status.is_empty() {
            continue;
        }
        let status = std::str::from_utf8(status).context("Git returned a non-UTF-8 status")?;
        let first = next_nul_field(bytes, &mut offset)
            .context("Git returned an incomplete NUL-delimited change status")?;
        if status.starts_with('R') || status.starts_with('C') {
            let second = next_nul_field(bytes, &mut offset)
                .context("Git returned an incomplete rename status")?;
            let old_path = std::str::from_utf8(first)
                .context("Git change paths must be valid UTF-8")?
                .to_owned();
            let path = std::str::from_utf8(second)
                .context("Git change paths must be valid UTF-8")?
                .to_owned();
            changes.push((status.to_owned(), path, Some(old_path)));
        } else {
            let path = std::str::from_utf8(first)
                .context("Git change paths must be valid UTF-8")?
                .to_owned();
            changes.push((status.to_owned(), path, None));
        }
    }
    Ok(changes)
}

fn parse_hunk_ranges(patch: &[u8]) -> Vec<ParsedHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<HunkState> = None;
    let text = String::from_utf8_lossy(patch);
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("@@ -") {
            if let Some(state) = current.take() {
                hunks.push(ParsedHunk {
                    added_range: state.added.then_some(state.added_range).flatten(),
                    deleted_only: state.removed && !state.added,
                });
            }
            let Some((old_range, new_range)) = header.split_once(" +") else {
                continue;
            };
            let old_range = old_range.split_whitespace().next().unwrap_or_default();
            let (_, old_count) = old_range.split_once(',').unwrap_or((old_range, "1"));
            let Ok(old_remaining) = old_count.parse::<usize>() else {
                continue;
            };
            let new_range = new_range.split_whitespace().next().unwrap_or_default();
            let (start, count) = new_range
                .split_once(',')
                .map_or((new_range, "1"), |(start, count)| (start, count));
            let (Ok(start), Ok(new_remaining)) = (start.parse::<usize>(), count.parse::<usize>())
            else {
                continue;
            };
            current = Some(HunkState {
                added_range: if new_remaining > 0 {
                    start.checked_add(new_remaining - 1).map(|end| (start, end))
                } else {
                    None
                },
                removed: false,
                added: false,
                old_remaining,
                new_remaining,
            });
            if old_remaining == 0 && new_remaining == 0 {
                if let Some(state) = current.take() {
                    hunks.push(ParsedHunk {
                        added_range: state.added.then_some(state.added_range).flatten(),
                        deleted_only: state.removed && !state.added,
                    });
                }
            }
            continue;
        }
        if let Some(state) = current.as_mut() {
            match line.as_bytes().first() {
                Some(b'+') => {
                    state.added = true;
                    state.new_remaining = state.new_remaining.saturating_sub(1);
                }
                Some(b'-') => {
                    state.removed = true;
                    state.old_remaining = state.old_remaining.saturating_sub(1);
                }
                Some(b' ') => {
                    state.old_remaining = state.old_remaining.saturating_sub(1);
                    state.new_remaining = state.new_remaining.saturating_sub(1);
                }
                _ => {}
            }
            if state.old_remaining == 0 && state.new_remaining == 0 {
                if let Some(state) = current.take() {
                    hunks.push(ParsedHunk {
                        added_range: state.added.then_some(state.added_range).flatten(),
                        deleted_only: state.removed && !state.added,
                    });
                }
            }
        }
    }
    if let Some(state) = current {
        hunks.push(ParsedHunk {
            added_range: state.added.then_some(state.added_range).flatten(),
            deleted_only: state.removed && !state.added,
        });
    }
    hunks
}

fn unmapped_change(path: &str, reason: &str, old_path: Option<&str>) -> Value {
    let mut item = json!({"path":path,"reason":reason});
    if let Some(old_path) = old_path {
        item["old_path"] = json!(old_path);
    }
    item
}

fn indexed_file_digest(conn: &Connection, path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT digest FROM files WHERE path=?1", [path], |row| {
            row.get(0)
        })
        .optional()?)
}

fn range_has_indexed_symbol(
    conn: &Connection,
    path: &str,
    start: usize,
    end: usize,
) -> Result<bool> {
    let found: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind NOT IN('module','file') AND n.line<=?3 AND n.end_line>=?2)",
        rusqlite::params![path, i64::try_from(start)?, i64::try_from(end)?],
        |row| row.get(0),
    )?;
    Ok(found != 0)
}

fn worktree_postimage(root: &Path, path: &str) -> Result<Vec<u8>> {
    let adapter = config::load_adapter(root, None)?;
    let resolved = config::resolve_file_path(root, &adapter, path)?;
    let metadata = fs::metadata(&resolved)?;
    anyhow::ensure!(metadata.is_file(), "change postimage is not a regular file");
    anyhow::ensure!(
        metadata.len() <= config::MAX_FILE_BYTES,
        "change postimage exceeds the indexed file byte budget"
    );
    Ok(fs::read(resolved)?)
}

fn collect_change_snapshot(
    conn: &Connection,
    root: &Path,
    mode: &str,
    path_scope: Option<&str>,
) -> Result<ChangeSnapshot> {
    anyhow::ensure!(
        matches!(mode, "staged" | "unstaged" | "head"),
        "changes must be staged, unstaged, or head"
    );
    if let Some(path) = path_scope {
        anyhow::ensure!(
            valid_workspace_path(path),
            "path must be workspace-relative"
        );
    }
    anyhow::ensure!(
        root.join(".git").exists(),
        "Git change views require a Git repository"
    );

    let mut status_args = git_diff_prefix(mode);
    status_args.extend([
        "--name-status".to_owned(),
        "-z".to_owned(),
        "--find-renames".to_owned(),
    ]);
    if let Some(path) = path_scope {
        status_args.push("--".to_owned());
        status_args.push(path.to_owned());
    }
    let status_bytes = git_bytes(root, &status_args, "change status")?;
    let changes = parse_name_status(&status_bytes)?;
    anyhow::ensure!(
        changes.len() <= MAX_CHANGE_FILES,
        "Git change view exceeds the {MAX_CHANGE_FILES}-file work budget"
    );
    let graph_generation: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |row| row.get(0),
    )?;
    let mut fingerprint = Sha256::new();
    fingerprint.update(b"forge-impact-git-change-view-v1");
    append_fingerprint(&mut fingerprint, graph_generation.as_bytes());
    append_fingerprint(&mut fingerprint, mode.as_bytes());
    append_fingerprint(&mut fingerprint, path_scope.unwrap_or("").as_bytes());
    append_fingerprint(&mut fingerprint, &status_bytes);

    let mut files = Vec::with_capacity(changes.len());
    let mut ranges = Vec::new();
    let mut unmapped = Vec::new();
    let mut total_diff_bytes = status_bytes.len();
    for (status, path, old_path) in changes {
        if !path_is_within(&path, path_scope) {
            continue;
        }
        let mut patch_args = git_diff_prefix(mode);
        patch_args.extend([
            "--unified=0".to_owned(),
            "--no-ext-diff".to_owned(),
            "--no-textconv".to_owned(),
            "--no-color".to_owned(),
            "--find-renames".to_owned(),
            "--".to_owned(),
            path.clone(),
        ]);
        if matches!(status.as_bytes().first(), Some(b'R' | b'C')) {
            if let Some(old_path) = &old_path {
                patch_args.push(old_path.clone());
            }
        }
        let patch = git_bytes(root, &patch_args, "change diff")?;
        total_diff_bytes = total_diff_bytes.saturating_add(patch.len());
        anyhow::ensure!(
            total_diff_bytes <= MAX_GIT_DIFF_BYTES,
            "Git change diff exceeds the 8 MiB work budget"
        );
        let hunks = parse_hunk_ranges(&patch);
        let hunk_ranges = hunks
            .iter()
            .filter_map(|hunk| hunk.added_range)
            .collect::<Vec<_>>();
        let is_binary = patch
            .windows(b"GIT binary patch".len())
            .any(|window| window == b"GIT binary patch")
            || patch
                .windows(b"Binary files".len())
                .any(|window| window == b"Binary files");
        let postimage = if status.starts_with('D') {
            None
        } else if mode == "staged" {
            let object = format!(":0:{path}");
            let output = git::git_command(root, &["show", &object])?;
            output.status.success().then_some(output.stdout)
        } else {
            worktree_postimage(root, &path).ok()
        };
        append_fingerprint(&mut fingerprint, status.as_bytes());
        append_fingerprint(&mut fingerprint, path.as_bytes());
        append_fingerprint(
            &mut fingerprint,
            old_path.as_deref().unwrap_or("").as_bytes(),
        );
        append_fingerprint(&mut fingerprint, &patch);
        append_fingerprint(&mut fingerprint, postimage.as_deref().unwrap_or_default());

        let file = ChangedFile {
            status: status.clone(),
            path: path.clone(),
            old_path: old_path.clone(),
            ranges: hunk_ranges
                .iter()
                .map(|(start, end)| ChangedRange {
                    path: path.clone(),
                    start: *start,
                    end: *end,
                })
                .collect(),
        };
        let indexed_digest = indexed_file_digest(conn, &path)?;
        let postimage_digest = postimage.as_deref().map(commitments::hash);
        let reason = if status.starts_with('D') {
            Some("deleted_range")
        } else if is_binary
            || postimage
                .as_ref()
                .is_some_and(|bytes| std::str::from_utf8(bytes).is_err())
        {
            Some("binary")
        } else if status.starts_with('R') && hunks.is_empty() {
            Some("rename_only")
        } else if hunks.is_empty() {
            Some("no_changed_lines")
        } else if postimage.is_none() {
            Some("unavailable_postimage")
        } else if indexed_digest.is_none() {
            Some("unindexed_path")
        } else if indexed_digest != postimage_digest {
            Some("snapshot_mismatch")
        } else {
            None
        };
        if let Some(reason) = reason {
            unmapped.push(unmapped_change(&path, reason, old_path.as_deref()));
        } else {
            for _ in hunks.iter().filter(|hunk| hunk.deleted_only) {
                unmapped.push(unmapped_change(&path, "deleted_range", old_path.as_deref()));
            }
            for range in &file.ranges {
                if range_has_indexed_symbol(conn, &range.path, range.start, range.end)? {
                    ranges.push(range.clone());
                } else {
                    let mut item = unmapped_change(&path, "no_indexed_symbol", old_path.as_deref());
                    item["start"] = json!(range.start);
                    item["end"] = json!(range.end);
                    unmapped.push(item);
                }
            }
        }
        files.push(file);
    }
    Ok(ChangeSnapshot {
        mode: mode.to_owned(),
        files,
        ranges,
        unmapped,
        fingerprint: hex::encode(fingerprint.finalize()),
    })
}

fn change_ranges_json(ranges: &[ChangedRange]) -> Result<String> {
    Ok(serde_json::to_string(
        &ranges
            .iter()
            .map(|range| json!({"path":range.path,"start":range.start,"end":range.end}))
            .collect::<Vec<_>>(),
    )?)
}

fn decode_edge_descriptor(value: Value) -> Result<Value> {
    match value {
        Value::String(encoded) => {
            serde_json::from_str(&encoded).context("invalid persisted impact edge descriptor")
        }
        other => Ok(other),
    }
}

fn changed_range_from_sql() -> &'static str {
    "json_each(?1) r JOIN path_dictionary p ON p.path=json_extract(r.value,'$.path') JOIN nodes n ON n.path_id=p.path_id AND n.kind NOT IN('module','file') AND n.line<=CAST(json_extract(r.value,'$.end') AS INTEGER) AND n.end_line>=CAST(json_extract(r.value,'$.start') AS INTEGER)"
}

fn changed_symbols_page(
    conn: &Connection,
    ranges: &[ChangedRange],
    options: &QueryOptions,
    generation: &str,
) -> Result<(Value, Vec<String>, Vec<i64>)> {
    let ranges_json = change_ranges_json(ranges)?;
    let from = changed_range_from_sql();
    let total = paging::count(
        conn,
        &format!("SELECT count(DISTINCT n.node_hash) FROM {from}"),
        &[&ranges_json],
    )?;
    let limit = i64::try_from(options.limit)?;
    let offset = i64::try_from(options.offset)?;
    let columns = paging::nodes_with_path("n", options.detail, "p.path");
    let page = reader::rows(
        conn,
        &format!(
            "SELECT DISTINCT {columns} FROM {from} ORDER BY p.path,n.line,n.id LIMIT ?2 OFFSET ?3"
        ),
        &[&ranges_json, &limit, &offset],
        options.limit,
    )?;
    let seed_limit = i64::try_from(MAX_CHANGE_SEEDS.saturating_add(1))?;
    let seeds = reader::rows(
        conn,
        &format!("SELECT DISTINCT n.id FROM {from} ORDER BY p.path,n.line,n.id LIMIT ?2"),
        &[&ranges_json, &seed_limit],
        MAX_CHANGE_SEEDS.saturating_add(1),
    )?;
    let mut ids = Vec::with_capacity(seeds.len());
    let mut hashes = Vec::with_capacity(seeds.len());
    for seed in seeds {
        let id = seed["id"]
            .as_str()
            .context("changed symbol has no public id")?
            .to_owned();
        hashes.push(stable_hash64(&id));
        ids.push(id);
    }
    Ok((paging::value(page, total, options, generation), ids, hashes))
}

fn changed_first_frontier(
    conn: &Connection,
    seed_hashes: &[i64],
    inbound: bool,
    forward: &str,
    reverse: &str,
) -> Result<usize> {
    let seed_json = serde_json::to_string(seed_hashes)?;
    let (from, to) = if inbound {
        ("dst_hash", "src_hash")
    } else {
        ("src_hash", "dst_hash")
    };
    let reverse_edges = if reverse.is_empty() {
        String::new()
    } else {
        format!(" UNION ALL SELECT 1 FROM edges e JOIN json_each(?1) s ON e.{to}=CAST(s.value AS INTEGER) WHERE e.kind IN({reverse})")
    };
    paging::count(
        conn,
        &format!("SELECT count(*) FROM (SELECT 1 FROM edges e JOIN json_each(?1) s ON e.{from}=CAST(s.value AS INTEGER) WHERE e.kind IN({forward}){reverse_edges})"),
        &[&seed_json],
    )
}

fn changed_reachability_cte(steps: &str) -> String {
    format!(
        "WITH RECURSIVE walk(id,depth,visited) AS (SELECT CAST(value AS INTEGER),0,printf(',%lld,',CAST(value AS INTEGER)) FROM json_each(?1) UNION {steps} LIMIT {}), reached(id,distance) AS MATERIALIZED (SELECT id,min(depth) FROM walk GROUP BY id)",
        MAX_CHANGE_RESULT_NODES.saturating_add(1)
    )
}

fn changed_dependency_steps(inbound: bool, forward: &str, reverse: &str) -> String {
    let (from, to) = if inbound {
        ("dst_hash", "src_hash")
    } else {
        ("src_hash", "dst_hash")
    };
    let forward_step = format!(
        "SELECT e.{to},w.depth+1,w.visited||printf('%lld,',e.{to}) FROM walk w JOIN edges e ON e.{from}=w.id WHERE w.depth<?2 AND e.kind IN({forward}) AND instr(w.visited,printf(',%lld,',e.{to}))=0"
    );
    if reverse.is_empty() {
        forward_step
    } else {
        format!(
            "{forward_step} UNION SELECT e.{from},w.depth+1,w.visited||printf('%lld,',e.{from}) FROM walk w JOIN edges e ON e.{to}=w.id WHERE w.depth<?2 AND e.kind IN({reverse}) AND instr(w.visited,printf(',%lld,',e.{from}))=0"
        )
    }
}

fn changed_nodes_page(
    conn: &Connection,
    seed_ids: &[String],
    seed_hashes: &[i64],
    request: &TraversalOptions<'_>,
    generation: &str,
) -> Result<Value> {
    if seed_hashes.is_empty() {
        return Ok(paging::value(Vec::new(), 0, request.page, generation));
    }
    let depth = request.depth;
    let (forward, reverse) = resolve_traversal_kinds(request.mode, request.edge_types, true);
    if depth > 0 {
        let immediate =
            changed_first_frontier(conn, seed_hashes, request.inbound, &forward, &reverse)?;
        if immediate > MAX_DEEP_TRAVERSAL_FRONTIER {
            bail!("Git change view has {immediate} immediate graph links; narrow the changed files or use depth=1 before paging");
        }
    }
    let seed_json = serde_json::to_string(seed_hashes)?;
    let steps = changed_dependency_steps(request.inbound, &forward, &reverse);
    let cte = changed_reachability_cte(&steps);
    let walked = paging::count(
        conn,
        &format!("{cte} SELECT count(*) FROM walk"),
        &[&seed_json, &depth],
    )?;
    if walked > MAX_CHANGE_RESULT_NODES {
        bail!("Git change impact exceeds the {MAX_CHANGE_RESULT_NODES}-state work budget; narrow the changed files or reduce depth");
    }
    let total = paging::count(
        conn,
        &format!("{cte} SELECT count(*) FROM reached"),
        &[&seed_json, &depth],
    )?;
    if total > MAX_CHANGE_RESULT_NODES {
        bail!("Git change impact exceeds the {MAX_CHANGE_RESULT_NODES}-node work budget; narrow the changed files or reduce depth");
    }
    let options = request.page;
    let limit = i64::try_from(options.limit)?;
    let offset = i64::try_from(options.offset)?;
    let columns = paging::nodes("n", options.detail);
    let (from, to) = if request.inbound {
        ("dst_hash", "src_hash")
    } else {
        ("src_hash", "dst_hash")
    };
    let reverse_edge = if reverse.is_empty() {
        String::new()
    } else {
        format!(" OR (e.{to} IN (SELECT CAST(value AS INTEGER) FROM json_each(?1)) AND e.{from}=n.node_hash AND e.kind IN({reverse}))")
    };
    let evidence = if options.detail == crate::core::response::Detail::Full {
        ", 'evidence',(SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)"
    } else {
        ""
    };
    let edge = format!(
        "CASE WHEN r.distance=1 THEN (SELECT json_object('src_public_id',(SELECT id FROM nodes WHERE node_hash=e.src_hash),'dst_public_id',(SELECT id FROM nodes WHERE node_hash=e.dst_hash),'kind',e.kind,'path',(SELECT path FROM path_dictionary WHERE path_id=e.path_id),'line',e.line,'confidence',(SELECT evidence FROM coverage_evidence WHERE evidence_id=e.confidence_id),'occurrence_count',e.occurrence_count,'direction',CASE WHEN e.src_hash IN (SELECT CAST(value AS INTEGER) FROM json_each(?1)) THEN 'outbound' ELSE 'inbound' END{evidence}) FROM edges e WHERE ((e.{from} IN (SELECT CAST(value AS INTEGER) FROM json_each(?1)) AND e.{to}=n.node_hash AND e.kind IN({forward})){reverse_edge}) ORDER BY e.kind,e.src_hash,e.dst_hash,e.path_id,e.line,e.evidence_id LIMIT 1) END AS __edge"
    );
    let (forward_current, forward_predecessor, reverse_current, reverse_predecessor) =
        if request.inbound {
            ("src_hash", "dst_hash", "dst_hash", "src_hash")
        } else {
            ("dst_hash", "src_hash", "src_hash", "dst_hash")
        };
    let reverse_via = if reverse.is_empty() {
        String::new()
    } else {
        format!(" UNION ALL SELECT prior_node.id AS predecessor,e.kind AS edge_kind FROM edges e JOIN reached prior ON prior.id=e.{reverse_predecessor} AND prior.distance=r.distance-1 JOIN nodes prior_node ON prior_node.node_hash=prior.id WHERE e.{reverse_current}=n.node_hash AND e.kind IN({reverse})")
    };
    let via = format!(
        "CASE WHEN r.distance>1 THEN (SELECT json_array(predecessor,edge_kind) FROM (SELECT prior_node.id AS predecessor,e.kind AS edge_kind FROM edges e JOIN reached prior ON prior.id=e.{forward_predecessor} AND prior.distance=r.distance-1 JOIN nodes prior_node ON prior_node.node_hash=prior.id WHERE e.{forward_current}=n.node_hash AND e.kind IN({forward}){reverse_via}) ORDER BY predecessor,edge_kind LIMIT 1) END AS __via"
    );
    let sql = format!("{cte} SELECT {columns},r.distance,{edge},{via} FROM reached r JOIN nodes n ON n.node_hash=r.id ORDER BY r.distance,n.id LIMIT ?3 OFFSET ?4");
    let mut items = reader::rows(
        conn,
        &sql,
        &[&seed_json, &depth, &limit, &offset],
        options.limit,
    )?;
    for item in &mut items {
        let fields = item.as_object_mut().context("invalid change impact node")?;
        let id = fields
            .get("id")
            .and_then(Value::as_str)
            .context("change impact node has no id")?;
        let is_seed = seed_ids.iter().any(|seed| seed == id);
        let edge = decode_edge_descriptor(fields.remove("__edge").unwrap_or(Value::Null))?;
        let via = fields
            .remove("__via")
            .and_then(|value| value.as_str().map(str::to_owned));
        fields.insert("is_seed".into(), json!(is_seed));
        if !is_seed && !edge.is_null() {
            fields.insert("edge".into(), edge);
        } else if !is_seed {
            if fields.get("distance").and_then(Value::as_u64) == Some(1) {
                bail!("depth-one change impact node has no persisted edge evidence");
            }
            let via = via
                .map(|encoded| serde_json::from_str::<(String, String)>(&encoded))
                .transpose()?
                .context("reached change impact node has no predecessor")?;
            fields.insert("via".into(), json!({"id":via.0,"kind":via.1}));
        }
    }
    Ok(paging::value(items, total, options, generation))
}

fn changed_tests_page(
    conn: &Connection,
    seed_ids: &[String],
    options: &QueryOptions,
    generation: &str,
) -> Result<Value> {
    let mut tests = BTreeMap::<String, Value>::new();
    for seed in seed_ids {
        let mut offset = 0;
        let mut index_generation: Option<String> = None;
        loop {
            let page_options = QueryOptions {
                limit: MAX_PAGE_SIZE,
                offset,
                detail: options.detail,
                generation: index_generation.clone(),
            };
            let page = symbols::tests_paged_with_path(conn, seed, None, "inbound", &page_options)?;
            let nodes = &page["nodes"];
            let total = nodes["total"]
                .as_u64()
                .context("changed test total is missing")? as usize;
            if total > MAX_CHANGE_RESULT_NODES {
                bail!("Git change test mapping exceeds the {MAX_CHANGE_RESULT_NODES}-test work budget");
            }
            index_generation = nodes["generation"].as_str().map(str::to_owned);
            if let Some(items) = nodes["items"].as_array() {
                for item in items {
                    let id = item["id"]
                        .as_str()
                        .context("changed test result has no id")?
                        .to_owned();
                    match tests.entry(id) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(item.clone());
                        }
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            let new_depth = item["connection"]["call_path"]
                                .as_array()
                                .map_or(usize::MAX, Vec::len);
                            let old_depth = entry.get()["connection"]["call_path"]
                                .as_array()
                                .map_or(usize::MAX, Vec::len);
                            if new_depth < old_depth {
                                entry.insert(item.clone());
                            }
                        }
                    }
                    if tests.len() > MAX_CHANGE_RESULT_NODES {
                        bail!("Git change test mapping exceeds the {MAX_CHANGE_RESULT_NODES}-test work budget");
                    }
                }
            }
            if nodes["has_more"] != true {
                break;
            }
            offset = nodes["next_offset"]
                .as_u64()
                .context("changed test continuation is missing")? as usize;
        }
    }
    let total = tests.len();
    let items = tests
        .into_values()
        .skip(options.offset)
        .take(options.limit)
        .collect();
    Ok(paging::value(items, total, options, generation))
}

fn changed_impact_with_path_options(
    conn: &Connection,
    root: &Path,
    mode: &str,
    path_scope: Option<&str>,
    request: &TraversalOptions<'_>,
) -> Result<Value> {
    if request.depth > 16 {
        bail!("depth must be <=16");
    }
    let snapshot = collect_change_snapshot(conn, root, mode, path_scope)?;
    let mut page = request.page.clone();
    let current_generation: String = conn.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |row| row.get(0),
    )?;
    page.generation = Some(current_generation);
    let graph_generation = paging::generation(conn, &page)?;
    if let Some(previous) = request.page.generation.as_deref() {
        anyhow::ensure!(
            previous.len() >= 8 && snapshot.fingerprint.starts_with(previous),
            "Git change view changed; restart at offset 0 without generation"
        );
    }
    if request.page.offset > 0 && request.page.generation.is_none() {
        bail!("continuation requires generation from the previous page");
    }
    let (changed_symbols, mut seed_ids, mut seed_hashes) =
        changed_symbols_page(conn, &snapshot.ranges, request.page, &snapshot.fingerprint)?;
    let seed_overflow = seed_ids.len() > MAX_CHANGE_SEEDS;
    seed_ids.truncate(MAX_CHANGE_SEEDS);
    seed_hashes.truncate(MAX_CHANGE_SEEDS);
    let mut unmapped = snapshot.unmapped.clone();
    if seed_overflow {
        unmapped.push(json!({"path":"*","reason":"seed_budget_exceeded","limit":MAX_CHANGE_SEEDS}));
    }
    let nodes = changed_nodes_page(
        conn,
        &seed_ids,
        &seed_hashes,
        request,
        &snapshot.fingerprint,
    )?;
    let tests = changed_tests_page(conn, &seed_ids, request.page, &snapshot.fingerprint)?;
    let file_values = snapshot
        .files
        .iter()
        .map(|file| {
            let ranges = file
                .ranges
                .iter()
                .map(|range| json!({"start":range.start,"end":range.end}))
                .collect::<Vec<_>>();
            let mut value = json!({"path":file.path,"status":file.status,"ranges":ranges});
            if let Some(old_path) = &file.old_path {
                value["old_path"] = json!(old_path);
            }
            value
        })
        .collect::<Vec<_>>();
    let unmapped_total = unmapped.len();
    let unmapped_items = unmapped
        .into_iter()
        .skip(request.page.offset)
        .take(request.page.limit)
        .collect();
    let complete = unmapped_total == 0;
    let unmapped_changes = paging::value(
        unmapped_items,
        unmapped_total,
        request.page,
        &snapshot.fingerprint,
    );
    let final_snapshot = collect_change_snapshot(conn, root, mode, path_scope)?;
    anyhow::ensure!(
        final_snapshot.fingerprint == snapshot.fingerprint,
        "Git change view changed while computing impact; restart at offset 0"
    );
    Ok(json!({
        "selector":{"kind":"git_changes","mode":snapshot.mode},
        "direction":if request.inbound {"inbound"} else {"outbound"},
        "depth":request.depth,
        "mode":request.mode.unwrap_or("calls"),
        "generation":snapshot.fingerprint,
        "graph_generation":graph_generation,
        "changes":{"mode":snapshot.mode,"files":file_values},
        "changed_symbols":changed_symbols,
        "nodes":nodes,
        "tests":tests,
        "unmapped_changes":unmapped_changes,
        "complete":complete
    }))
}

/// Impact either one exact selector or the selected Git change view.
pub(crate) fn impact_with_path_options(
    conn: &Connection,
    root: &Path,
    selector: Option<&str>,
    changes: Option<&str>,
    path: Option<&str>,
    request: &TraversalOptions<'_>,
) -> Result<Value> {
    let empty_selector = selector.is_some_and(|value| value.trim().is_empty());
    match (
        selector.filter(|value| !value.trim().is_empty()),
        changes.filter(|value| !value.trim().is_empty()),
    ) {
        (Some(selector), None) => {
            let mut result = traverse_with_path_options(conn, selector, path, request)?;
            let selected_id = result["selector"]["id"]
                .as_str()
                .or_else(|| result["selector"]["node"]["id"].as_str())
                .unwrap_or(selector)
                .to_owned();
            let selected_path = result["selector"]["path"]
                .as_str()
                .or_else(|| result["selector"]["node"]["path"].as_str())
                .unwrap_or(selector)
                .to_owned();
            result["governing_invariants"] = reader::governing_invariants(conn, &selected_id)?;
            let path_is_indexed: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM path_dictionary WHERE path=?1)",
                [&selected_path],
                |row| row.get(0),
            )?;
            result["affected_docs"] = if path_is_indexed {
                paging::query(
                    conn,
                    "SELECT d.doc_id,d.path,d.section_title,d.doc_type,e.line FROM edges e JOIN nodes symbol ON symbol.node_hash=e.src_hash JOIN path_dictionary sp ON sp.path_id=symbol.path_id JOIN nodes dn ON dn.node_hash=e.dst_hash JOIN doc_sections d ON d.doc_id=dn.id WHERE e.kind='references_doc' AND sp.path=?1 UNION SELECT d.doc_id,d.path,d.section_title,d.doc_type,CAST(json_extract(c.value,'$.line') AS INTEGER) FROM nodes dn JOIN doc_sections d ON d.doc_id=dn.id JOIN json_each(dn.details,'$.symbol_claims') c WHERE json_extract(c.value,'$.target_path')=?1 ORDER BY 2,1,5",
                    &[&selected_path],
                    request.page,
                )?
            } else {
                paging::query(
                    conn,
                    "SELECT DISTINCT d.doc_id,d.path,d.section_title,d.doc_type,e.line FROM edges e JOIN nodes dn ON dn.node_hash=e.dst_hash JOIN doc_sections d ON d.doc_id=dn.id WHERE e.kind='references_doc' AND e.src_hash=?1 ORDER BY d.path,d.doc_id,e.line",
                    &[&stable_hash64(&selected_id)],
                    request.page,
                )?
            };
            Ok(result)
        }
        (None, Some(mode)) => changed_impact_with_path_options(conn, root, mode, path, request),
        (Some(_), Some(_)) => bail!("provide exactly one of selector or changes"),
        (None, None) if empty_selector => traverse_with_path_options(
            conn,
            selector.context("empty selector was unexpectedly absent")?,
            path,
            request,
        ),
        (None, None) => bail!("provide exactly one of selector or changes"),
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
        let (from, to) = if inbound {
            ("dst_hash", "src_hash")
        } else {
            ("src_hash", "dst_hash")
        };
        let reverse_edge = if reverse.is_empty() {
            String::new()
        } else {
            format!(" OR (e.{to}=?1 AND e.{from}=n.node_hash AND e.kind IN({reverse}))")
        };
        let evidence = if options.detail == crate::core::response::Detail::Full {
            ", 'evidence',(SELECT evidence FROM coverage_evidence WHERE evidence_id=e.evidence_id)"
        } else {
            ""
        };
        let edge_json = format!(
            "CASE WHEN r.distance=1 THEN (SELECT json_object('src_public_id',(SELECT id FROM nodes WHERE node_hash=e.src_hash),'dst_public_id',(SELECT id FROM nodes WHERE node_hash=e.dst_hash),'kind',e.kind,'path',(SELECT path FROM path_dictionary WHERE path_id=e.path_id),'line',e.line,'confidence',(SELECT evidence FROM coverage_evidence WHERE evidence_id=e.confidence_id),'occurrence_count',e.occurrence_count,'direction',CASE WHEN e.src_hash=?1 THEN 'outbound' ELSE 'inbound' END{evidence}) FROM edges e WHERE ((e.{from}=?1 AND e.{to}=n.node_hash AND e.kind IN({forward})){reverse_edge}) ORDER BY e.kind,e.src_hash,e.dst_hash,e.path_id,e.line,e.evidence_id LIMIT 1) END AS __edge"
        );
        let page_sql = format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION {steps}), reached(id,distance) AS (SELECT id,min(depth) FROM walk GROUP BY id) SELECT {columns},r.distance,{edge_json} FROM reached r JOIN nodes n ON n.node_hash=r.id ORDER BY r.distance,n.id LIMIT ?3 OFFSET ?4");
        let mut items = reader::rows(
            conn,
            &page_sql,
            &[&id_hash, &depth, &limit, &offset],
            options.limit,
        )?;
        for item in &mut items {
            let fields = item.as_object_mut().context("invalid traversal node")?;
            let edge = decode_edge_descriptor(fields.remove("__edge").unwrap_or(Value::Null))?;
            fields.insert(
                "is_seed".into(),
                json!(fields.get("id").and_then(Value::as_str) == Some(id)),
            );
            if !edge.is_null() {
                fields.insert("edge".into(), edge);
            }
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
