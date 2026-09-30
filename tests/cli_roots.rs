use contextunity_forge_mcp::db::reader;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("forge_cli_root_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("sample.py"), "def sample():\n    return 1\n").unwrap();
        Self(path)
    }

    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(args)
        .output()
        .unwrap()
}

#[derive(Clone, Copy)]
enum RebuildReason {
    MissingMetadata,
    SchemaVersion,
    SemanticsVersion,
    CommitmentAlgorithm,
}

#[derive(Clone, Copy)]
enum Corruption {
    FreelistCount,
    TablePage,
    FtsPage,
}

fn build_workspace() -> Workspace {
    let workspace = Workspace::new();
    let root = workspace.0.to_str().unwrap();
    let build = cli(&["--root", root, "build"]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    workspace
}

fn apply_rebuild_reason(workspace: &Workspace, reason: RebuildReason) {
    let conn = rusqlite::Connection::open(workspace.db()).unwrap();
    match reason {
        RebuildReason::MissingMetadata => conn
            .execute_batch(
                "CREATE INDEX valid_partial_legacy_idx ON files(path) WHERE path IS NOT NULL; DROP TABLE metadata;",
            )
            .unwrap(),
        RebuildReason::SchemaVersion => {
            conn.execute(
                "UPDATE metadata SET value='0' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        }
        RebuildReason::SemanticsVersion => {
            conn.execute(
                "UPDATE metadata SET value='0' WHERE key='index_semantics_version'",
                [],
            )
            .unwrap();
        }
        RebuildReason::CommitmentAlgorithm => {
            conn.execute(
                "UPDATE metadata SET value='0' WHERE key='commitment_algorithm'",
                [],
            )
            .unwrap();
        }
    }
}

fn corrupt_database(workspace: &Workspace, corruption: Corruption) {
    let mut bytes = fs::read(workspace.db()).unwrap();
    match corruption {
        Corruption::FreelistCount => {
            assert_eq!(u32::from_be_bytes(bytes[32..36].try_into().unwrap()), 0);
            assert_eq!(u32::from_be_bytes(bytes[36..40].try_into().unwrap()), 0);
            bytes[36..40].copy_from_slice(&1u32.to_be_bytes());
        }
        Corruption::TablePage | Corruption::FtsPage => {
            let table = match corruption {
                Corruption::TablePage => "files",
                Corruption::FtsPage => "doc_search_data",
                Corruption::FreelistCount => unreachable!(),
            };
            let (page_size, root_page) = {
                let conn = rusqlite::Connection::open(workspace.db()).unwrap();
                let page_size = conn
                    .query_row("PRAGMA page_size", [], |row| row.get::<_, usize>(0))
                    .unwrap();
                let root_page = conn
                    .query_row(
                        "SELECT rootpage FROM sqlite_schema WHERE name=?1",
                        [table],
                        |row| row.get::<_, usize>(0),
                    )
                    .unwrap();
                (page_size, root_page)
            };
            bytes[(root_page - 1) * page_size] = 0xff;
        }
    }
    fs::write(workspace.db(), bytes).unwrap();
}

fn assert_no_integrity_snapshot_for_current_process() {
    let prefix = format!("contextunity-forge-integrity-{}-", std::process::id());
    let leftovers: Vec<_> = fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
                && path.is_dir()
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "integrity snapshot leftovers: {leftovers:?}"
    );
}

#[test]
fn build_uses_global_root_when_positional_root_is_omitted() {
    let workspace = Workspace::new();
    let root = workspace.0.to_str().unwrap();
    let result = cli(&["--root", root, "build"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(workspace.db().exists());
    let inspect = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(String::from_utf8_lossy(&inspect.stdout).contains("sample.py"));
}

#[test]
fn positional_root_build_remains_supported() {
    let workspace = Workspace::new();
    let result = cli(&["build", workspace.0.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(workspace.db().exists());
}

#[cfg(unix)]
#[test]
fn cli_rejects_fifo_database_without_blocking() {
    let workspace = Workspace::new();
    let fifo = workspace.0.join("database.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());

    let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
        .args(["--root", workspace.0.to_str().unwrap(), "--db"])
        .arg(&fifo)
        .args(["query", "inspect", "sample.py:sample"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(!status.success());
                break;
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("CLI blocked while opening a FIFO database");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("failed waiting for CLI process: {error}");
            }
        }
    }
}

#[test]
fn conflicting_roots_are_rejected_for_build_scan_and_delta() {
    let first = Workspace::new();
    let second = Workspace::new();
    for command in ["build", "scan", "delta"] {
        let mut args = vec![
            "--root",
            first.0.to_str().unwrap(),
            command,
            second.0.to_str().unwrap(),
        ];
        if command == "delta" {
            args.push("sample.py");
        }
        let result = cli(&args);
        assert!(
            !result.status.success(),
            "{command} accepted conflicting roots"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("conflicting workspace roots"));
    }
    assert!(!first.db().exists());
    assert!(!second.db().exists());
}

#[test]
fn scan_uses_global_root_when_positional_root_is_omitted() {
    let workspace = Workspace::new();
    let result = cli(&["--root", workspace.0.to_str().unwrap(), "scan"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("sample.py"));
}

#[test]
fn delta_uses_global_root_with_named_modified_file() {
    let workspace = Workspace::new();
    let root = workspace.0.to_str().unwrap();
    let build = cli(&["--root", root, "build"]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    fs::write(
        workspace.0.join("sample.py"),
        "def updated():\n    return 2\n",
    )
    .unwrap();
    let delta = cli(&["--root", root, "delta", "--modified", "sample.py"]);
    assert!(
        delta.status.success(),
        "{}",
        String::from_utf8_lossy(&delta.stderr)
    );
    let inspect = cli(&["--root", root, "query", "inspect", "sample.py:updated"]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(String::from_utf8_lossy(&inspect.stdout).contains("updated"));
}

#[test]
fn query_rebuilds_typed_incompatible_indexes_but_preserves_corrupt_files() {
    let reasons = [
        RebuildReason::MissingMetadata,
        RebuildReason::SchemaVersion,
        RebuildReason::SemanticsVersion,
        RebuildReason::CommitmentAlgorithm,
    ];

    for reason in reasons {
        let workspace = build_workspace();
        apply_rebuild_reason(&workspace, reason);
        let root = workspace.0.to_str().unwrap();
        let error = reader::open(&workspace.db(), &workspace.0).unwrap_err();
        assert!(
            error.to_string().contains("rebuild") || matches!(reason, RebuildReason::SchemaVersion)
        );
        assert_no_integrity_snapshot_for_current_process();

        let rebuilt = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
        assert!(
            rebuilt.status.success(),
            "{}",
            String::from_utf8_lossy(&rebuilt.stderr)
        );
        assert!(reader::open(&workspace.db(), &workspace.0).is_ok());
    }

    let corruption_cases = [
        (RebuildReason::MissingMetadata, Corruption::TablePage),
        (RebuildReason::SchemaVersion, Corruption::FreelistCount),
        (RebuildReason::SemanticsVersion, Corruption::FtsPage),
        (
            RebuildReason::CommitmentAlgorithm,
            Corruption::FreelistCount,
        ),
    ];
    for (reason, corruption) in corruption_cases {
        let workspace = build_workspace();
        apply_rebuild_reason(&workspace, reason);
        corrupt_database(&workspace, corruption);
        let before = fs::read(workspace.db()).unwrap();
        let error = reader::open(&workspace.db(), &workspace.0).unwrap_err();
        if matches!(corruption, Corruption::FreelistCount) {
            assert!(
                error
                    .to_string()
                    .contains("Freelist: size is 0 but should be 1"),
                "{error:#}"
            );
        }
        assert_eq!(fs::read(workspace.db()).unwrap(), before);
        assert_no_integrity_snapshot_for_current_process();

        let root = workspace.0.to_str().unwrap();
        let rejected = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
        assert!(
            !rejected.status.success(),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert_eq!(fs::read(workspace.db()).unwrap(), before);
    }

    let unreadable_workspace = Workspace::new();
    fs::create_dir_all(unreadable_workspace.0.join(".forge")).unwrap();
    let corrupt = b"not a sqlite database";
    fs::write(unreadable_workspace.db(), corrupt).unwrap();
    let root = unreadable_workspace.0.to_str().unwrap();
    let rejected = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
    assert!(!rejected.status.success());
    assert_eq!(fs::read(unreadable_workspace.db()).unwrap(), corrupt);

    let empty_workspace = Workspace::new();
    fs::create_dir_all(empty_workspace.db().parent().unwrap()).unwrap();
    fs::write(empty_workspace.db(), []).unwrap();
    let before = fs::read(empty_workspace.db()).unwrap();
    let root = empty_workspace.0.to_str().unwrap();
    let rejected = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
    assert!(!rejected.status.success());
    assert_eq!(fs::read(empty_workspace.db()).unwrap(), before);

    let initialized_workspace = Workspace::new();
    fs::create_dir_all(initialized_workspace.db().parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(initialized_workspace.db()).unwrap();
    conn.execute_batch("PRAGMA user_version=1").unwrap();
    drop(conn);
    let before = fs::read(initialized_workspace.db()).unwrap();
    assert_eq!(&before[..16], b"SQLite format 3\0");
    let root = initialized_workspace.0.to_str().unwrap();
    let rebuilt = cli(&["--root", root, "query", "inspect", "sample.py:sample"]);
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    assert_ne!(fs::read(initialized_workspace.db()).unwrap(), before);
    assert!(reader::open(&initialized_workspace.db(), &initialized_workspace.0).is_ok());
}
