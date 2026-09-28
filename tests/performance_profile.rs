use contextunity_forge_mcp::{db::writer, engine::scanner};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
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
        let path =
            std::env::temp_dir().join(format!("forge_profile_{}_{nonce}", std::process::id()));
        fs::create_dir_all(path.join("main")).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("main")
    }
    fn file(&self, path: &str) -> PathBuf {
        if let Some((name, rest)) = path.strip_prefix('[').and_then(|p| p.split_once("]/")) {
            self.0.join("linked").join(name).join(rest)
        } else {
            self.root().join(path)
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(
    binary: Option<&Path>,
    root: &Path,
    db: &Path,
    modified: &[String],
) -> anyhow::Result<Value> {
    if let Some(binary) = binary {
        let mut command = Command::new(binary);
        command.arg("--root").arg(root).arg("--db").arg(db);
        if modified.is_empty() {
            command.arg("build").arg(root);
        } else {
            command.arg("delta").arg(root).args(modified);
        }
        let output = command.output().unwrap();
        anyhow::ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    } else if modified.is_empty() {
        writer::build(root, db, None)
    } else {
        writer::delta(
            root,
            db,
            &modified.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )
    }
}

#[test]
#[ignore = "set FORGE_BENCH_ROOT; copies indexed sources into an isolated Workspace"]
fn frozen_workspace_cold_and_delta_profile() {
    let source_root =
        PathBuf::from(std::env::var("FORGE_BENCH_ROOT").expect("FORGE_BENCH_ROOT required"));
    let adapter = scanner::load_adapter(&source_root, None).unwrap();
    let scan = scanner::scan_with_adapter(&source_root, &adapter).unwrap();
    let ws = Workspace::new();
    let mut originals = BTreeMap::new();
    for entry in &scan.entries {
        let bytes =
            fs::read(scanner::resolve_file_path(&source_root, &adapter, &entry.path).unwrap())
                .unwrap();
        let target = ws.file(&entry.path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, &bytes).unwrap();
        originals.insert(entry.path.clone(), bytes);
    }
    let mut config = String::from("roots: ['.']\n");
    let main_config = config.clone();
    if !adapter.linked_workspaces.is_empty() {
        config.push_str("linked_workspaces:\n");
        for linked in &adapter.linked_workspaces {
            config.push_str(&format!(
                "  - name: {}\n    path: {}\n    roots: ['.']\n",
                linked.name,
                ws.0.join("linked").join(&linked.name).display()
            ));
        }
    }
    let mut bulk = Vec::new();
    let live_db = source_root.join(".forge/code-map.sqlite");
    if live_db.exists() {
        let conn = rusqlite::Connection::open_with_flags(
            live_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let raw: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='inventory_snapshot'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let old: Vec<scanner::FileEntry> = serde_json::from_str(&raw).unwrap();
        let old: BTreeMap<_, _> = old.into_iter().map(|e| (e.path, e.digest)).collect();
        for (path, bytes) in &originals {
            if old.get(path) != Some(&scanner::digest(bytes)) {
                bulk.push(path.clone());
            }
        }
    }
    if bulk.is_empty() {
        let changed = Command::new("git")
            .arg("-C")
            .arg(&source_root)
            .args(["diff", "--name-only", "--diff-filter=M"])
            .output()
            .unwrap();
        assert!(changed.status.success());
        bulk = String::from_utf8(changed.stdout)
            .unwrap()
            .lines()
            .filter(|path| originals.contains_key(*path))
            .take(34)
            .map(str::to_owned)
            .collect();
    }
    if let Ok(input_log) = std::env::var("FORGE_BENCH_INPUT_LOG") {
        let input = fs::read_to_string(input_log).unwrap();
        let row = input
            .lines()
            .find_map(|line| line.strip_prefix("PROFILE_INPUT "))
            .unwrap();
        let value: Value = serde_json::from_str(row).unwrap();
        bulk = serde_json::from_value(value["bulk_paths"].clone()).unwrap();
        assert!(bulk.iter().all(|path| originals.contains_key(path)));
    }
    println!("PROFILE_INPUT {}", json!({"bulk_paths":bulk}));
    let single = std::env::var("FORGE_BENCH_DELTA_PATH")
        .unwrap_or_else(|_| "services/shield/src/contextunity/shield/audit.py".into());
    assert!(
        originals.contains_key(&single),
        "single delta fixture path missing"
    );
    let baseline = std::env::var_os("FORGE_BENCH_BASELINE").map(PathBuf::from);
    let mut versions = Vec::new();
    if let Some(path) = baseline.as_deref() {
        versions.push(("baseline", Some(path)));
    }
    versions.push(("candidate", None));
    if std::env::var_os("FORGE_BENCH_REVERSE").is_some() {
        versions.reverse();
    }
    for (label, binary) in versions {
        for (scope, settings) in [("main", &main_config), ("linked", &config)] {
            fs::write(ws.root().join("forge-mcp.yaml"), settings).unwrap();
            let db = ws.root().join(format!(".forge/{label}-{scope}.sqlite"));
            let started = Instant::now();
            let report = run(binary, &ws.root(), &db, &[]).unwrap();
            let conn = rusqlite::Connection::open_with_flags(
                &db,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let counts: (i64,i64,i64) = conn.query_row("SELECT (SELECT count(*) FROM files),(SELECT count(*) FROM nodes),(SELECT count(*) FROM edges)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            drop(conn);
            println!(
                "PROFILE {}",
                json!({"version":label,"scope":scope,"operation":"cold","wall_ms":started.elapsed().as_millis(),"bytes":fs::metadata(&db).unwrap().len(),"counts":counts,"report":report})
            );
            if binary.is_none() {
                let server =
                    contextunity_forge_mcp::mcp::server::Server::new(ws.root(), db.clone());
                for call in 0..2 {
                    let started = Instant::now();
                    let response = server.read(|conn| Ok(json!({"nodes":conn.query_row("SELECT count(*) FROM nodes", [], |r| r.get::<_,i64>(0))?}))).unwrap();
                    println!(
                        "PROFILE {}",
                        json!({"version":label,"scope":scope,"operation":"unchanged_mcp_admission","call":call,"wall_ms":started.elapsed().as_millis(),"response":response})
                    );
                }
            }
            let scoped_bulk: Vec<_> = bulk
                .iter()
                .filter(|p| scope == "linked" || !p.starts_with('['))
                .cloned()
                .collect();
            for (case, paths) in [("single", vec![single.clone()]), ("bulk", scoped_bulk)] {
                if paths.is_empty() {
                    continue;
                }
                for path in &paths {
                    let mut bytes = originals[path].clone();
                    bytes.push(b'\n');
                    fs::write(ws.file(path), bytes).unwrap();
                }
                let result = run(binary, &ws.root(), &db, &paths);
                let succeeded = result.is_ok();
                let report = match result {
                    Ok(report) => report,
                    Err(error) if binary.is_some() => json!({"error":format!("{error:#}")}),
                    Err(error) => panic!("candidate {scope}/{case}: {error:#}"),
                };
                println!(
                    "PROFILE {}",
                    json!({"version":label,"scope":scope,"operation":case,"report":report})
                );
                for path in &paths {
                    fs::write(ws.file(path), &originals[path]).unwrap();
                }
                if succeeded && case != "bulk" {
                    run(binary, &ws.root(), &db, &paths).unwrap();
                }
            }
        }
    }
}
