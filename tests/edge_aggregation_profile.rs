use contextunity_forge_mcp::core::{models::Edge, schema::SCHEMA_DDL};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge_edge_profile_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Vec<Edge> {
    let mut edges = Vec::with_capacity(228_000);
    for index in 0..200_000 {
        edges.push(Edge {
            src: format!("py:packages/service/src/domain/consumer_{index:06}.py:10:dispatch"),
            dst: format!("py:packages/service/src/domain/provider_{index:06}.py:20:handle"),
            kind: if index % 5 == 0 { "imports" } else { "calls" }.into(),
            path: format!("packages/service/src/domain/consumer_{index:06}.py"),
            line: 25,
            evidence: format!("first_{index:06}"),
            confidence: "exact".into(),
        });
    }
    for index in 0..28_000 {
        let mut edge = edges[(index * 37) % 200_000].clone();
        edge.path = format!("zzz_duplicate_owners/consumer_{index:06}.py");
        edge.line = 3;
        edge.evidence = format!("later_{index:06}");
        edge.confidence = "heuristic".into();
        edges.push(edge);
    }
    edges
}

fn peak_rss_kib() -> Option<u64> {
    fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
}

#[test]
#[ignore = "worker for the isolated edge persistence benchmark"]
fn edge_persistence_profile_worker() {
    let Ok(algorithm) = std::env::var("FORGE_EDGE_PROFILE_ALGORITHM") else {
        return;
    };
    assert!(matches!(algorithm.as_str(), "legacy" | "aggregate"));
    let workspace = Workspace::new();
    let edges = fixture();
    let mut conn = Connection::open(workspace.0.join("edges.sqlite")).unwrap();
    conn.execute_batch("PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-128000; PRAGMA cache_spill=OFF;").unwrap();
    let table = SCHEMA_DDL
        .split_once("CREATE TABLE IF NOT EXISTS edges (")
        .unwrap()
        .1
        .split_once("\n);")
        .unwrap()
        .0;
    conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS edges ({table}\n);"))
        .unwrap();
    let tx = conn.transaction().unwrap();
    let started = Instant::now();
    let mut map_capacity = 0usize;
    let mut vector_capacity = 0usize;
    if algorithm == "legacy" {
        let mut statement = tx.prepare("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count)VALUES(?1,?2,?3,?4,?5,?6,?7,1)ON CONFLICT(src_public_id,dst_public_id,kind)DO UPDATE SET occurrence_count=occurrence_count+1").unwrap();
        for edge in &edges {
            statement
                .execute(params![
                    edge.src,
                    edge.dst,
                    edge.kind,
                    edge.path,
                    edge.line,
                    edge.evidence,
                    edge.confidence
                ])
                .unwrap();
        }
    } else {
        let mut unique: Vec<(&Edge, i64)> = Vec::new();
        let mut indices: hashbrown::HashMap<(&str, &str, &str), usize> = hashbrown::HashMap::new();
        for edge in &edges {
            let key = (edge.src.as_str(), edge.dst.as_str(), edge.kind.as_str());
            if let Some(&index) = indices.get(&key) {
                unique[index].1 = unique[index].1.checked_add(1).unwrap();
            } else {
                indices.try_reserve(1).unwrap();
                unique.try_reserve(1).unwrap();
                indices.insert(key, unique.len());
                unique.push((edge, 1));
            }
        }
        map_capacity = indices.capacity();
        vector_capacity = unique.capacity();
        drop(indices);
        let mut statement = tx.prepare("INSERT INTO edges(src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count)VALUES(?1,?2,?3,?4,?5,?6,?7,?8)").unwrap();
        for (edge, count) in unique {
            statement
                .execute(params![
                    edge.src,
                    edge.dst,
                    edge.kind,
                    edge.path,
                    edge.line,
                    edge.evidence,
                    edge.confidence,
                    count
                ])
                .unwrap();
        }
    }
    let write_ms = started.elapsed().as_secs_f64() * 1000.0;
    let committing = Instant::now();
    tx.commit().unwrap();
    let commit_ms = committing.elapsed().as_secs_f64() * 1000.0;
    let rss = peak_rss_kib();
    let (unique_count, occurrences): (usize, usize) = conn
        .query_row(
            "SELECT count(*),sum(occurrence_count) FROM edges",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((unique_count, occurrences), (200_000, edges.len()));
    let mut hash = Sha256::new();
    let mut statement = conn.prepare("SELECT json_array(edge_id,src_public_id,dst_public_id,kind,path,line,evidence,confidence,occurrence_count) FROM edges ORDER BY edge_id").unwrap();
    for row in statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
    {
        let row = row.unwrap();
        hash.update((row.len() as u64).to_le_bytes());
        hash.update(row.as_bytes());
    }
    println!(
        "EDGE_PROFILE_SAMPLE {}",
        json!({
            "algorithm":algorithm,"write_ms":write_ms,"commit_ms":commit_ms,
            "unique_edges":unique_count,"occurrences":occurrences,
            "digest":hex::encode(hash.finalize()),"peak_rss_kib":rss,
            "map_capacity":map_capacity,"vector_capacity":vector_capacity,
        })
    );
}

fn quantiles(samples: &[Value], field: &str) -> Value {
    let mut values: Vec<_> = samples
        .iter()
        .filter_map(|sample| sample[field].as_f64())
        .collect();
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return Value::Null;
    }
    let p95_index = (values.len() * 95).div_ceil(100) - 1;
    json!({"median":values[values.len()/2],"p95":values[p95_index],"samples":values.len()})
}

#[test]
#[ignore = "seven paired subprocess samples, run explicitly in release without competing builds"]
fn alternating_edge_persistence_profile() {
    let binary = std::env::current_exe().unwrap();
    let mut legacy = Vec::new();
    let mut aggregate = Vec::new();
    let mut expected_digest = None;
    for repetition in 0..7 {
        let order = if repetition % 2 == 0 {
            ["legacy", "aggregate"]
        } else {
            ["aggregate", "legacy"]
        };
        for algorithm in order {
            let output = Command::new(&binary)
                .args([
                    "--ignored",
                    "--exact",
                    "edge_persistence_profile_worker",
                    "--nocapture",
                ])
                .env("FORGE_EDGE_PROFILE_ALGORITHM", algorithm)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            let raw = stdout
                .lines()
                .find_map(|line| {
                    line.split_once("EDGE_PROFILE_SAMPLE ")
                        .map(|(_, json)| json)
                })
                .unwrap();
            let mut sample: Value = serde_json::from_str(raw).unwrap();
            if let Some(expected) = &expected_digest {
                assert_eq!(
                    &sample["digest"], expected,
                    "first evidence, counts, or insertion order differ"
                );
            } else {
                expected_digest = Some(sample["digest"].clone());
            }
            sample["repetition"] = json!(repetition);
            println!("EDGE_PROFILE {sample}");
            if algorithm == "legacy" {
                legacy.push(sample);
            } else {
                aggregate.push(sample);
            }
        }
    }
    println!(
        "EDGE_PROFILE_SUMMARY {}",
        json!({
            "scope":"modeled cold edge-table materialization; actual edges schema, secondary indexes deferred; excludes parsing, occurrences, dependencies, commitments and final publication",
            "rss_scope":"fresh subprocess high-water RSS through commit, including fixture and SQLite cache; not production writer RSS",
            "legacy":{"write_ms":quantiles(&legacy,"write_ms"),"commit_ms":quantiles(&legacy,"commit_ms"),"peak_rss_kib":quantiles(&legacy,"peak_rss_kib")},
            "aggregate":{"write_ms":quantiles(&aggregate,"write_ms"),"commit_ms":quantiles(&aggregate,"commit_ms"),"peak_rss_kib":quantiles(&aggregate,"peak_rss_kib")},
            "digest":expected_digest,
        })
    );
}
