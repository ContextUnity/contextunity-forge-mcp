use anyhow::Result;
use contextunity_forge_benchmarks::BenchmarkWorkspace;
use contextunity_forge_mcp::{engine::scanner, mcp::server::Server};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

fn run(binary: Option<&Path>, root: &Path, db: &Path, modified: &[String]) -> Result<Value> {
    if let Some(binary) = binary {
        let mut command = Command::new(binary);
        command.arg("--root").arg(root).arg("--db").arg(db);
        if modified.is_empty() {
            command.arg("build").arg(root);
        } else {
            command.arg("delta").arg(root).args(modified);
        }
        let output = command.output()?;
        anyhow::ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    } else if modified.is_empty() {
        Ok(contextunity_forge_mcp::db::writer::build(root, db, None)?)
    } else {
        Ok(contextunity_forge_mcp::db::writer::delta(
            root,
            db,
            &modified.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )?)
    }
}

fn profile() -> Result<()> {
    let source_root =
        PathBuf::from(std::env::var("FORGE_BENCH_ROOT").expect("FORGE_BENCH_ROOT required"));
    let adapter = scanner::load_adapter(&source_root, None)?;
    let scan = scanner::scan_with_adapter(&source_root, &adapter)?;
    let workspace = BenchmarkWorkspace::new();
    let mut originals = BTreeMap::new();
    for entry in &scan.entries {
        let bytes = fs::read(scanner::resolve_file_path(
            &source_root,
            &adapter,
            &entry.path,
        )?)?;
        let target = workspace.file(&entry.path);
        fs::create_dir_all(target.parent().expect("scanned file has a parent"))?;
        fs::write(&target, &bytes)?;
        originals.insert(entry.path.clone(), bytes);
    }

    let mut linked_config = String::from("roots: ['.']\n");
    let main_config = linked_config.clone();
    if !adapter.linked_workspaces.is_empty() {
        linked_config.push_str("linked_workspaces:\n");
        for linked in &adapter.linked_workspaces {
            linked_config.push_str(&format!(
                "  - name: {}\n    path: {}\n    roots: ['.']\n",
                linked.name,
                workspace
                    .parent()
                    .join("linked")
                    .join(&linked.name)
                    .display()
            ));
        }
    }

    let mut bulk = Vec::new();
    let live_db = source_root.join(".forge/code-map.sqlite");
    if live_db.exists() {
        let conn =
            Connection::open_with_flags(live_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let raw: String = conn.query_row(
            "SELECT value FROM metadata WHERE key='inventory_snapshot'",
            [],
            |row| row.get(0),
        )?;
        let old: Vec<scanner::FileEntry> = serde_json::from_str(&raw)?;
        let old: BTreeMap<_, _> = old
            .into_iter()
            .map(|entry| (entry.path, entry.digest))
            .collect();
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
            .output()?;
        anyhow::ensure!(
            changed.status.success(),
            "could not inspect changed source files"
        );
        bulk = String::from_utf8(changed.stdout)?
            .lines()
            .filter(|path| originals.contains_key(*path))
            .take(34)
            .map(str::to_owned)
            .collect();
    }
    if let Ok(input_log) = std::env::var("FORGE_BENCH_INPUT_LOG") {
        let input = fs::read_to_string(input_log)?;
        let row = input
            .lines()
            .find_map(|line| line.strip_prefix("PROFILE_INPUT "))
            .expect("input log must contain PROFILE_INPUT JSON");
        let value: Value = serde_json::from_str(row)?;
        bulk = serde_json::from_value(value["bulk_paths"].clone())?;
        assert!(bulk.iter().all(|path| originals.contains_key(path)));
    }
    println!("PROFILE_INPUT {}", json!({"bulk_paths":bulk}));

    let single = std::env::var("FORGE_BENCH_DELTA_PATH")
        .unwrap_or_else(|_| "services/shield/src/contextunity/shield/audit.py".into());
    anyhow::ensure!(
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
        for (scope, settings) in [("main", &main_config), ("linked", &linked_config)] {
            fs::write(workspace.root().join("forge-mcp.yaml"), settings)?;
            let db = workspace
                .root()
                .join(format!(".forge/{label}-{scope}.sqlite"));
            let started = Instant::now();
            let report = run(binary, workspace.root(), &db, &[])?;
            let conn =
                Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let counts: (i64, i64, i64) = conn.query_row(
                "SELECT (SELECT count(*) FROM files),(SELECT count(*) FROM nodes),(SELECT count(*) FROM edges)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            drop(conn);
            println!(
                "PROFILE {}",
                json!({"version":label,"scope":scope,"operation":"cold","wall_ms":started.elapsed().as_millis(),"bytes":fs::metadata(&db)?.len(),"counts":counts,"report":report})
            );

            if binary.is_none() {
                let server = Server::new(workspace.root().to_path_buf(), db.clone());
                for call in 0..2 {
                    let started = Instant::now();
                    let response = server.read(|conn| {
                        Ok(json!({"nodes":conn.query_row("SELECT count(*) FROM nodes", [], |row| row.get::<_, i64>(0))?}))
                    })?;
                    println!(
                        "PROFILE {}",
                        json!({"version":label,"scope":scope,"operation":"unchanged_mcp_admission","call":call,"wall_ms":started.elapsed().as_millis(),"response":response})
                    );
                }
            }

            let scoped_bulk: Vec<_> = bulk
                .iter()
                .filter(|path| scope == "linked" || !path.starts_with('['))
                .cloned()
                .collect();
            for (case, paths) in [("single", vec![single.clone()]), ("bulk", scoped_bulk)] {
                if paths.is_empty() {
                    continue;
                }
                for path in &paths {
                    let mut bytes = originals[path].clone();
                    bytes.push(b'\n');
                    fs::write(workspace.file(path), bytes)?;
                }
                let result = run(binary, workspace.root(), &db, &paths);
                let succeeded = result.is_ok();
                let report = match result {
                    Ok(report) => report,
                    Err(error) if binary.is_some() => json!({"error":format!("{error:#}")}),
                    Err(error) => return Err(error),
                };
                println!(
                    "PROFILE {}",
                    json!({"version":label,"scope":scope,"operation":case,"report":report})
                );
                for path in &paths {
                    fs::write(workspace.file(path), &originals[path])?;
                }
                if succeeded && case != "bulk" {
                    run(binary, workspace.root(), &db, &paths)?;
                }
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = profile() {
        eprintln!("frozen workspace profile failed: {error:#}");
        std::process::exit(1);
    }
}
