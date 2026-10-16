use super::{populate, sqlite_cache_size_kib};
use crate::engine::scanner;
use rusqlite::Connection;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
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
    let scan = scanner::scan_for_build(&root.0, &adapter).unwrap();
    for path in [
        "apps/requirements-dev.txt",
        "apps/Pipfile",
        "apps/api/setup.cfg",
        "apps/api/package.json",
    ] {
        assert!(
            scan.dependency_manifests.contains(&root.0.join(path)),
            "scanner missed ancestor manifest {path} outside source roots"
        );
    }
    let registry = DependencyRegistry::try_collect_with_paths(
        &root.0,
        Some(&adapter),
        &scan.dependency_manifests,
    )
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
