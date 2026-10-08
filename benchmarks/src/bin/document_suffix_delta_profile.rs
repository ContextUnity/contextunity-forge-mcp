use anyhow::{ensure, Result};
use contextunity_forge_benchmarks::BenchmarkWorkspace;
use rusqlite::Connection;
use serde_json::json;
use std::{path::Path, process::Command, time::Instant};

fn doc_edges(db: &Path) -> Result<Vec<(String, String, String)>> {
    let conn = Connection::open(db)?;
    let mut statement = conn.prepare(
        "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind FROM edges WHERE kind IN ('documents','references_doc') ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn profile() -> Result<()> {
    let baseline = std::env::var("FORGE_BASELINE_BIN")?;
    let old = BenchmarkWorkspace::new();
    let new = BenchmarkWorkspace::new();
    for workspace in [&old, &new] {
        for file in 0..1_600 {
            let mut source = String::from("[server]\n");
            for key in 0..10 {
                source.push_str(&format!("limit_{file}_{key}.cpu = {file}\n"));
            }
            workspace.write(format!("config/settings_{file:04}.toml"), &source);
        }
        workspace.write(
            "docs/guide.md",
            "# Configuration\nUse `server.limit_0_0.cpu`.\n",
        );
    }

    let output = Command::new(&baseline)
        .arg("--db")
        .arg(old.db())
        .arg("build")
        .arg(old.root())
        .output()?;
    ensure!(
        output.status.success(),
        "baseline cold build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    new.build();

    for workspace in [&old, &new] {
        workspace.write(
            "docs/guide.md",
            "# Configuration\nUse `server.limit_0_0.cpu`.\n\nDetails.\n",
        );
    }

    let old_wall = Instant::now();
    let output = Command::new(&baseline)
        .arg("--db")
        .arg(old.db())
        .arg("delta")
        .arg(old.root())
        .arg("docs/guide.md")
        .output()?;
    ensure!(
        output.status.success(),
        "baseline delta failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let old_wall_ms = old_wall.elapsed().as_secs_f64() * 1000.0;
    let old_report: serde_json::Value = serde_json::from_slice(&output.stdout)?;

    let new_wall = Instant::now();
    let new_report = new.delta(&["docs/guide.md"]);
    let new_wall_ms = new_wall.elapsed().as_secs_f64() * 1000.0;
    println!(
        "DELTA_DOC_PROFILE {}",
        json!({
            "files": 1601,
            "baseline": {"wall_ms": old_wall_ms, "report": old_report, "doc_edges": doc_edges(&old.db())?.len()},
            "candidate_fix": {"wall_ms": new_wall_ms, "report": new_report, "doc_edges": doc_edges(&new.db())?.len()}
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = profile() {
        eprintln!("document suffix delta profile failed: {error:#}");
        std::process::exit(1);
    }
}
