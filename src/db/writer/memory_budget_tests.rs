use super::{build, populate, sqlite_cache_size_kib};
use crate::engine::scanner;
use rusqlite::Connection;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct TemporaryRoot(PathBuf);

impl TemporaryRoot {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge_writer_cache_test_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sqlite_cache_size_is_clamped_in_kibibytes() {
    assert_eq!(sqlite_cache_size_kib(0), 4_096);
    assert_eq!(sqlite_cache_size_kib(64 * 1024 * 1024), 8_192);
    assert_eq!(sqlite_cache_size_kib(u64::MAX), 128_000);

    let root = TemporaryRoot::new();
    let adapter = scanner::load_adapter(&root.0, None).unwrap();
    let mut connection = Connection::open_in_memory().unwrap();
    connection.pragma_update(None, "cache_size", 1).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA cache_size", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );

    let dependencies = crate::engine::languages::dependency_registry::DependencyRegistry::default();
    populate(
        &mut connection,
        &root.0,
        &adapter,
        &[],
        &BTreeMap::new(),
        None,
        super::BuildInputs {
            dependency_registry: &dependencies,
            memory_budget: scanner::memory_budget_bytes(),
            schema_ready: false,
        },
    )
    .unwrap();

    let cache_size = connection
        .query_row("PRAGMA cache_size", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert!(cache_size < 0, "cache_size must use negative KiB units");
    let cache_size_kib = cache_size
        .checked_neg()
        .and_then(|value| u64::try_from(value).ok())
        .expect("negative cache size must fit KiB range");
    assert!((4_096..=128_000).contains(&cache_size_kib));
}

#[test]
fn transient_memory_budget_does_not_change_persisted_adapter_policy() {
    let root = TemporaryRoot::new();
    let mut adapter = scanner::load_adapter(&root.0, None).unwrap();
    let default_policy = super::policy(&root.0, &adapter);

    adapter.limits.memory_budget_mb = Some(50);
    let configured_policy = super::policy(&root.0, &adapter);

    assert_eq!(default_policy, configured_policy);
    assert!(default_policy["limits"].get("memory_budget_mb").is_none());
}

#[cfg(all(
    target_os = "linux",
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript"
))]
fn assert_variant_manifests_above_source_roots() {
    use crate::engine::languages::{by_id, dependency_registry::DependencyRegistry};

    let root = TemporaryRoot::new();
    fs::create_dir_all(root.0.join("apps/api/src")).unwrap();
    for (path, contents) in [
        ("forge-mcp.yaml", "roots: [apps/api/src]\ndocs: []\n"),
        ("apps/api/src/unit.py", "def indexed(): pass\n"),
        ("requirements-root.txt", "root-dependency==1\n"),
        (
            "Cargo.toml",
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nroot-crate='1'\n",
        ),
        ("apps/requirements-dev.txt", "ancestor-requirement==1\n"),
        ("apps/Pipfile", "[packages]\nancestor-pipfile='*'\n"),
        (
            "apps/api/setup.cfg",
            "[options]\ninstall_requires = ancestor-setup==1\n",
        ),
        (
            "apps/api/package.json",
            r#"{"dependencies":{"ancestor-js":"1"}}"#,
        ),
    ] {
        fs::write(root.0.join(path), contents).unwrap();
    }

    let adapter = scanner::load_adapter(&root.0, None).unwrap();
    let dependency_manifests = scanner::dependency_manifests_for_build(&root.0, &adapter).unwrap();
    for path in [
        "apps/requirements-dev.txt",
        "apps/Pipfile",
        "apps/api/setup.cfg",
        "apps/api/package.json",
    ] {
        assert!(
            dependency_manifests.contains(&root.0.join(path)),
            "scanner missed ancestor manifest {path} outside source roots"
        );
    }
    let registry =
        DependencyRegistry::try_collect_with_paths(&root.0, Some(&adapter), &dependency_manifests)
            .unwrap();
    for (profile, path, package) in [
        ("python", "root_probe.py", "root_dependency"),
        ("rust", "root_probe.rs", "root_crate"),
        ("python", "apps/probe.py", "ancestor_requirement"),
        ("python", "apps/probe.py", "ancestor_pipfile"),
        ("python", "apps/api/src/unit.py", "ancestor_setup"),
        ("typescript", "apps/api/src/unit.ts", "ancestor-js"),
    ] {
        assert!(
            registry.declares_for_path(by_id(profile).unwrap().family(), path, package),
            "cold-build registry lost {package} for {path}"
        );
    }
}

#[cfg(all(target_os = "linux", feature = "lang-rust"))]
#[test]
fn cold_build_reads_source_once_and_preserves_durable_facts() {
    use super::build;
    use crate::core::{commitments, models::Facts};
    use std::{
        io::Read,
        os::fd::{AsRawFd, FromRawFd, OwnedFd},
    };

    let root = TemporaryRoot::new();
    fs::create_dir(root.0.join("src")).unwrap();
    let source = "/// Preserve λ and a cold-build commitment.\npub fn indexed_once() {}\n";
    let source_path = root.0.join("src/unit.rs");
    fs::write(&source_path, source).unwrap();

    // Observe the real filesystem boundary. CLOSE separates repeated OPEN
    // events so the kernel cannot coalesce two reads into one notification.
    let descriptor = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    assert!(descriptor >= 0, "{}", std::io::Error::last_os_error());
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let path = std::ffi::CString::new(source_path.as_os_str().as_encoded_bytes()).unwrap();
    let watch = unsafe {
        libc::inotify_add_watch(
            descriptor.as_raw_fd(),
            path.as_ptr(),
            libc::IN_OPEN | libc::IN_CLOSE_NOWRITE,
        )
    };
    assert!(watch >= 0, "{}", std::io::Error::last_os_error());
    let mut source_events = std::fs::File::from(descriptor);

    let database = root.0.join(".forge/code-map.sqlite");
    build(&root.0, &database, None).unwrap();
    let connection = Connection::open(&database).unwrap();
    let (inventory_digest, facts_digest, blob): (String, String, Vec<u8>) = connection
        .query_row(
            "SELECT i.digest,f.source_digest,f.facts_blob FROM source_inventory i JOIN local_facts f USING(path) WHERE i.path='src/unit.rs' AND i.status='indexed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let digest = scanner::digest(source.as_bytes());
    assert_eq!(inventory_digest, digest);
    assert_eq!(facts_digest, digest);
    let durable_json = zstd::stream::decode_all(blob.as_slice()).unwrap();
    let facts: Facts = serde_json::from_slice(&durable_json).unwrap();
    assert!(facts
        .nodes
        .iter()
        .any(|node| node.name == "indexed_once" && node.kind == "function"));
    assert!(facts.errors.is_empty());
    commitments::verify(&connection).unwrap();

    let mut buffer = [0u8; 4096];
    let count = source_events.read(&mut buffer).unwrap();
    let mut offset = 0;
    let mut opens = 0;
    while offset < count {
        assert!(count - offset >= std::mem::size_of::<libc::inotify_event>());
        let event = unsafe {
            std::ptr::read_unaligned(buffer[offset..].as_ptr().cast::<libc::inotify_event>())
        };
        assert_eq!(event.mask & libc::IN_Q_OVERFLOW, 0);
        opens += usize::from(event.mask & libc::IN_OPEN != 0);
        offset += std::mem::size_of::<libc::inotify_event>() + event.len as usize;
    }
    assert_eq!(
        opens, 1,
        "cold build must read source bytes once across scanner and extraction"
    );

    #[cfg(all(feature = "lang-python", feature = "lang-typescript"))]
    assert_variant_manifests_above_source_roots();
}

#[cfg(all(target_os = "linux", feature = "lang-rust"))]
#[test]
fn cold_build_reports_the_sqlite_bulk_ingestion_strategy() {
    let root = TemporaryRoot::new();
    fs::create_dir(root.0.join("src")).unwrap();
    fs::write(root.0.join("src/lib.rs"), "pub fn indexed_symbol() {}\n").unwrap();

    let report = build(&root.0, &root.0.join("cold-ingestion.sqlite"), None).unwrap();
    let ingestion = report
        .get("sqlite_ingestion")
        .and_then(serde_json::Value::as_object)
        .expect("cold build reports its SQLite ingestion strategy");

    assert_eq!(
        ingestion.get("mode").and_then(serde_json::Value::as_str),
        Some("deferred-index-batched")
    );
    assert_eq!(
        ingestion
            .get("secondary_indexes_deferred")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(ingestion
        .get("batch_rows")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|rows| rows > 0));
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_pipeline_accounts_for_in_flight_bytes_and_preserves_commitments() {
    use super::build;
    use crate::core::commitments;

    let workspace = TemporaryRoot::new();
    let source = workspace.0.join("source");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(
        source.join("forge-mcp.yaml"),
        "roots: [src]\nlimits:\n  memory_budget_mb: 1\n",
    )
    .unwrap();
    for index in 0..16 {
        fs::write(
            source.join(format!("src/unit_{index:02}.rs")),
            format!("pub fn unit_{index}() {{}}\n"),
        )
        .unwrap();
    }
    // A single legal file exceeds the entire transient queue budget. It must
    // make progress in isolation instead of deadlocking behind its own permit.
    fs::write(
        source.join("src/oversized.rs"),
        format!(
            "/*{}*/\npub fn oversized() {{}}\n",
            "x".repeat(2 * 1024 * 1024)
        ),
    )
    .unwrap();
    let mut expected_root = None;
    for threads in [1, 4] {
        let database = workspace.0.join(format!("pipeline-{threads}.sqlite"));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let report = pool.install(|| build(&source, &database, None)).unwrap();
        let pipeline = &report["pipeline"];
        let metric = |name: &str| {
            pipeline[name]
                .as_u64()
                .unwrap_or_else(|| panic!("cold build must report pipeline.{name}: {report}"))
        };
        let budget = metric("budget_bytes");
        let peak = metric("peak_in_flight_bytes");
        let largest = metric("max_item_bytes");
        assert!(budget > 0 && budget <= 1024 * 1024);
        assert!(
            largest > budget,
            "fixture must exercise oversized admission"
        );
        assert!(peak > 0 && peak <= budget.max(largest));
        assert_eq!(metric("oversized_admissions"), 1);
        assert_eq!(metric("extracted_files"), 17);
        assert_eq!(metric("ingested_files"), 17);

        let conn = Connection::open(&database).unwrap();
        commitments::verify(&conn).unwrap();
        let functions: i64 = conn
            .query_row(
                "SELECT count(*) FROM nodes WHERE kind='function'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(functions, 17, "streaming must retain every extracted file");
        let root = report["output_root"].as_str().unwrap().to_owned();
        if let Some(expected) = &expected_root {
            assert_eq!(&root, expected, "worker count must not change commitments");
        } else {
            expected_root = Some(root);
        }
    }
}

#[cfg(feature = "lang-rust")]
fn write_pipeline_fixture(root: &Path, files: usize) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("forge-mcp.yaml"),
        "roots: [src]\nlimits:\n  memory_budget_mb: 1\n",
    )
    .unwrap();
    for index in 0..files {
        fs::write(
            root.join(format!("src/unit_{index:02}.rs")),
            format!("pub fn unit_{index}() {{}}\n"),
        )
        .unwrap();
    }
}

#[cfg(feature = "lang-rust")]
fn assert_no_pipeline_stage(parent: &Path) {
    let stages: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".forge-stage-")
        })
        .map(|entry| entry.path())
        .collect();
    assert!(
        stages.is_empty(),
        "failed pipeline left stage paths: {stages:?}"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_pipeline_overlaps_scan_extraction_and_sqlite_ingestion() {
    use super::{build, build_with_test_hooks, PipelineTestHooks};
    use crate::core::commitments;
    use std::sync::atomic::Ordering;

    let workspace = TemporaryRoot::new();
    let source = workspace.0.join("source");
    write_pipeline_fixture(&source, 12);

    let baseline = workspace.0.join("baseline.sqlite");
    let baseline_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let baseline_report = baseline_pool
        .install(|| build(&source, &baseline, None))
        .unwrap();

    let database = workspace.0.join("overlap.sqlite");
    let hooks = PipelineTestHooks::overlap();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let report = pool
        .install(|| build_with_test_hooks(&source, &database, None, hooks.clone()))
        .unwrap();

    assert_eq!(hooks.overlap_participants.load(Ordering::Acquire), 3);
    assert_eq!(report["pipeline"]["extracted_files"], 12);
    assert_eq!(report["pipeline"]["ingested_files"], 12);
    assert_eq!(
        report["output_root"], baseline_report["output_root"],
        "overlapping stages must preserve the canonical commitments"
    );

    let connection = Connection::open(&database).unwrap();
    commitments::verify(&connection).unwrap();
    let (rows, unique_paths): (i64, i64) = connection
        .query_row(
            "SELECT count(*),count(DISTINCT path) FROM local_facts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((rows, unique_paths), (12, 12));
    let (inventory_rows, unique_inventory_paths): (i64, i64) = connection
        .query_row(
            "SELECT count(*),count(DISTINCT path) FROM source_inventory",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((inventory_rows, unique_inventory_paths), (12, 12));
    let temp_triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_temp_master WHERE type='trigger'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        temp_triggers, 0,
        "pipeline trigger must be removed before sealing"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_pipeline_worker_error_cancels_stages_and_preserves_previous_database() {
    use super::build;
    use crate::core::commitments;

    let workspace = TemporaryRoot::new();
    let source = workspace.0.join("source");
    write_pipeline_fixture(&source, 16);
    let database = workspace.0.join(".forge/code-map.sqlite");
    let first_report = build(&source, &database, None).unwrap();
    let original_root = first_report["output_root"].as_str().unwrap().to_owned();

    fs::write(source.join("src/unit_00.rs"), [0xff, 0xfe, 0xfd]).unwrap();
    let error = build(&source, &database, None).unwrap_err();
    assert!(error
        .to_string()
        .contains("stream did not contain valid UTF-8"));

    let connection = Connection::open(&database).unwrap();
    commitments::verify(&connection).unwrap();
    let persisted_root = commitments::seal_snapshot(&connection).unwrap();
    assert_eq!(persisted_root, original_root);
    assert_no_pipeline_stage(database.parent().unwrap());
}

#[cfg(feature = "lang-rust")]
#[test]
fn cold_pipeline_consumer_error_cancels_stages_and_preserves_previous_database() {
    use super::{build, build_with_test_hooks, PipelineTestHooks};
    use crate::core::commitments;

    let workspace = TemporaryRoot::new();
    let source = workspace.0.join("source");
    write_pipeline_fixture(&source, 16);
    let database = workspace.0.join(".forge/code-map.sqlite");
    let first_report = build(&source, &database, None).unwrap();
    let original_root = first_report["output_root"].as_str().unwrap().to_owned();

    let error = build_with_test_hooks(&source, &database, None, PipelineTestHooks::fail_consumer())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("injected pipeline SQLite consumer failure"));

    let connection = Connection::open(&database).unwrap();
    commitments::verify(&connection).unwrap();
    let verified_root = commitments::seal_snapshot(&connection).unwrap();
    assert_eq!(verified_root, original_root);
    assert_no_pipeline_stage(database.parent().unwrap());
}
