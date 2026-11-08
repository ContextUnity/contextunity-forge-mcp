use super::harness::*;
use crate::common::Workspace;
use contextunity_forge_mcp::engine::pipeline;
use rusqlite::{ffi, Connection};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    ffi::{c_char, c_int, c_void, CStr},
    path::PathBuf,
    sync::Mutex,
    thread::ThreadId,
};

pub(super) struct FtsReads {
    pub(super) directory: PathBuf,
    pub(super) published: PathBuf,
    pub(super) domains: BTreeMap<&'static str, HashSet<ThreadId>>,
    pub(super) invalid_reader: bool,
    pub(super) deny_domain: Option<&'static str>,
}

pub(super) static FTS_READS: Mutex<Option<FtsReads>> = Mutex::new(None);

// Observe actual SQLite reads without adding production telemetry or replacing
// any part of the build. The extension is restricted to this fixture directory.
pub(super) struct ObserveFts;

impl Drop for ObserveFts {
    fn drop(&mut self) {
        // SAFETY: unregister only this test's callback, leaving other extensions intact.
        unsafe { ffi::sqlite3_cancel_auto_extension(Some(observe_connection)) };
        *FTS_READS.lock().unwrap() = None;
    }
}

pub(super) unsafe extern "C" fn observe_connection(
    db: *mut ffi::sqlite3,
    _: *mut *const c_char,
    _: *const ffi::sqlite3_api_routines,
) -> c_int {
    // SAFETY: SQLite supplies a live connection; the filename is borrowed only
    // during this callback. No Rust references escape into SQLite.
    let filename = unsafe { ffi::sqlite3_db_filename(db, c"main".as_ptr()) };
    if filename.is_null() {
        return ffi::SQLITE_OK;
    }
    let filename = PathBuf::from(
        unsafe { CStr::from_ptr(filename) }
            .to_string_lossy()
            .as_ref(),
    );
    let matches = FTS_READS.lock().ok().is_some_and(|state| {
        state
            .as_ref()
            .is_some_and(|reads| filename.starts_with(&reads.directory))
    });
    if matches {
        // SAFETY: SQLite keeps db alive throughout authorizer calls; the user
        // pointer is the same connection handle, never owned or dereferenced here.
        unsafe { ffi::sqlite3_set_authorizer(db, Some(observe_read), db.cast()) }
    } else {
        ffi::SQLITE_OK
    }
}

unsafe extern "C" fn observe_read(
    data: *mut c_void,
    action: c_int,
    table: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    if action != ffi::SQLITE_READ || table.is_null() {
        return ffi::SQLITE_OK;
    }
    // SAFETY: SQLite supplies NUL-terminated authorizer arguments and the live
    // connection pointer registered above. These calls neither execute SQL nor panic.
    let domain = match unsafe { CStr::from_ptr(table) }.to_bytes() {
        b"node_search_commit_vocab" => "node_search_postings",
        b"doc_search_commit_vocab" => "doc_search_postings",
        b"node_search_config" => "node_search_config",
        b"doc_search_config" => "doc_search_config",
        _ => return ffi::SQLITE_OK,
    };
    let db = data.cast::<ffi::sqlite3>();
    // Ingest also reads FTS configuration on its writable connection. Only
    // digest readers are relevant to the independent-domain contract.
    if unsafe { ffi::sqlite3_db_readonly(db, c"main".as_ptr()) } != 1 {
        return ffi::SQLITE_OK;
    }
    let filename = unsafe { ffi::sqlite3_db_filename(db, c"main".as_ptr()) };
    if !filename.is_null() {
        let filename = PathBuf::from(
            unsafe { CStr::from_ptr(filename) }
                .to_string_lossy()
                .as_ref(),
        );
        if let Ok(mut state) = FTS_READS.lock() {
            if let Some(reads) = state.as_mut() {
                reads.invalid_reader |= filename == reads.published;
                reads
                    .domains
                    .entry(domain)
                    .or_default()
                    .insert(std::thread::current().id());
                if reads.deny_domain == Some(domain) {
                    return ffi::SQLITE_DENY;
                }
            }
        }
    }
    ffi::SQLITE_OK
}

pub const FTS_DOMAINS: [&str; 4] = [
    "node_search_postings",
    "node_search_config",
    "doc_search_postings",
    "doc_search_config",
];

fn start_observer(directory: PathBuf, published: PathBuf) -> Result<ObserveFts, c_int> {
    *FTS_READS.lock().unwrap() = Some(FtsReads {
        directory,
        published,
        domains: BTreeMap::new(),
        invalid_reader: false,
        deny_domain: None,
    });
    let result = unsafe { ffi::sqlite3_auto_extension(Some(observe_connection)) };
    if result == ffi::SQLITE_OK {
        Ok(ObserveFts)
    } else {
        *FTS_READS.lock().unwrap() = None;
        Err(result)
    }
}

#[test]
fn cold_disk_build_sealing_matches_serial_and_preserves_previous_generation() {
    let workspace = Workspace::new();
    let source = workspace.path("source");
    let code: String = (0..8192)
        .map(|index| format!("pub fn symbol_{index:04}() {{}}\n"))
        .collect();
    workspace.write("source/lib.rs", &code);
    workspace.write(
        "source/README.md",
        "# Sealing fixture\n\nCanonical Unicode λ documentation.\n",
    );
    workspace.write(
        "source/calls.py",
        "def target(): pass\ndef caller():\n    target()\n    missing()\n",
    );
    #[cfg(feature = "lang-html")]
    workspace.write(
        "source/page.html",
        "<button onclick=\"missing()\">Go</button>\n<script>missing();</script>\n",
    );

    let domain_rows = |conn: &Connection| -> Vec<(String, i64, Vec<u8>)> {
        conn.prepare(
            "SELECT domain,owner_id,digest FROM domain_commitments ORDER BY domain,owner_id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    };

    let extraction_cache = workspace.path(".forge/extraction-cache-cold");
    let mut expected_disk_root = None;
    let mut expected_domains = None;
    let mut control_domain_workers = BTreeMap::new();
    let mut control_read_published_database = false;

    for threads in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let db = workspace.path(format!("cold-{threads}.sqlite"));
        let observation = start_observer(workspace.path(""), db.clone())
            .expect("SQLite FTS observer registration succeeds");
        let report = pool
            .install(|| pipeline::build_with_cache_dir(&source, &db, None, &extraction_cache))
            .expect("cold production pipeline builds the disk fixture");
        let reads = FTS_READS.lock().unwrap().take().unwrap();
        drop(observation);

        let reported_nodes = report["nodes"].as_u64().unwrap();
        assert!(reported_nodes >= 8192);
        let root = report["output_root"].as_str().unwrap().to_owned();

        let conn = Connection::open(&db).unwrap();
        let domain_rows_before = domain_rows(&conn);
        commitments::verify(&conn).expect("each cold database verifies before resealing");
        let serial_seal =
            commitments::seal(&conn).expect("serial sealing succeeds for each cold database");
        let domain_rows_after = domain_rows(&conn);
        drop(conn);

        assert_eq!(
            serial_seal, root,
            "serial sealing reproduces the published output root for {threads} threads"
        );
        assert_eq!(
            domain_rows_before, domain_rows_after,
            "serial sealing preserves the published domain digests for {threads} threads"
        );

        let reopened = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open read-only succeeds");
        commitments::verify(&reopened).expect("each published database verifies after reopen");
        drop(reopened);

        if let Some(expected_root) = &expected_disk_root {
            assert_eq!(
                &root, expected_root,
                "cold sealing is independent of thread count"
            );
        } else {
            expected_disk_root = Some(root.clone());
        }

        if let Some(expected_doms) = &expected_domains {
            assert_eq!(
                &domain_rows_before, expected_doms,
                "owner and FTS digests are thread-independent"
            );
        } else {
            expected_domains = Some(domain_rows_before.clone());
        }

        if threads == 1 {
            // Calibrate the observer against real reads before using its absence
            // as red proof. This control intentionally reads the published file.
            let control = start_observer(workspace.path(""), db.clone())
                .expect("SQLite FTS observer registration succeeds");
            let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
            commitments::verify(&conn).expect("the read-only observer control verifies");
            let control_reads = FTS_READS.lock().unwrap().take().unwrap();
            drop(conn);
            drop(control);
            control_domain_workers = control_reads.domains;
            control_read_published_database = control_reads.invalid_reader;
        } else {
            for domain in FTS_DOMAINS {
                assert!(
                    reads.domains.contains_key(domain),
                    "cold FTS domain {domain} uses a stage-private read-only connection"
                );
            }
            let node_workers = reads
                .domains
                .get("node_search_postings")
                .expect("node posting readers were observed");
            let doc_workers = reads
                .domains
                .get("doc_search_postings")
                .expect("document posting readers were observed");
            assert!(
                node_workers.is_disjoint(doc_workers),
                "node and document postings use disjoint workers"
            );
            let worker_count = reads
                .domains
                .values()
                .flatten()
                .collect::<HashSet<_>>()
                .len();
            assert!((2..=4).contains(&worker_count));
            assert!(!reads.invalid_reader);
        }
    }

    let control_fts_domains: BTreeSet<_> = control_domain_workers.keys().copied().collect();
    assert_eq!(
        control_fts_domains.len(),
        4,
        "observer sees each real FTS domain"
    );
    assert_eq!(control_fts_domains, FTS_DOMAINS.into_iter().collect());
    assert!(
        control_read_published_database,
        "observer identifies the published database"
    );

    // A real reader failure must leave the previous published generation intact.
    let db = workspace.path("cold-4.sqlite");
    let previous_generation_before = std::fs::read(&db).unwrap();
    let observation = start_observer(workspace.path(""), db.clone())
        .expect("SQLite FTS observer registration succeeds");
    FTS_READS.lock().unwrap().as_mut().unwrap().deny_domain = Some("node_search_config");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let failed_build_result =
        pool.install(|| pipeline::build_with_cache_dir(&source, &db, None, &extraction_cache));
    let reads = FTS_READS.lock().unwrap().take().unwrap();
    drop(observation);

    let previous_generation_after = std::fs::read(&db).unwrap();
    assert_eq!(
        previous_generation_before, previous_generation_after,
        "failed build preserves the exact previous published bytes"
    );
    let conn = Connection::open(&db).unwrap();
    commitments::verify(&conn).expect("the preserved previous generation still verifies");
    drop(conn);

    assert_eq!(
        failed_build_result
            .expect_err("the denied FTS reader must abort the build")
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "authorization denied".to_owned(),
            "Error code 23: Authorization denied".to_owned()
        ],
        "the exact denied-reader error chain is retained"
    );
    assert!(reads.domains.contains_key("node_search_config"));
    assert!(!reads.invalid_reader);
}
