use super::{commitments, writer, Connection, Workspace};
use rusqlite::ffi;
use std::{
    collections::{BTreeMap, HashSet},
    ffi::{c_char, c_int, c_void, CStr},
    path::PathBuf,
    sync::Mutex,
    thread::ThreadId,
};

struct FtsReads {
    directory: PathBuf,
    published: PathBuf,
    domains: BTreeMap<&'static str, HashSet<ThreadId>>,
    invalid_reader: bool,
    deny_domain: Option<&'static str>,
}

static FTS_READS: Mutex<Option<FtsReads>> = Mutex::new(None);

// Observe actual SQLite reads without adding production telemetry or replacing
// any part of the build. The extension is restricted to this fixture directory.
struct ObserveFts;

impl ObserveFts {
    fn start(directory: PathBuf, published: PathBuf) -> Self {
        *FTS_READS.lock().unwrap() = Some(FtsReads {
            directory,
            published,
            domains: BTreeMap::new(),
            invalid_reader: false,
            deny_domain: None,
        });
        // SAFETY: the callback has SQLite's extension ABI and static lifetime.
        assert_eq!(
            unsafe { ffi::sqlite3_auto_extension(Some(observe_connection)) },
            ffi::SQLITE_OK
        );
        Self
    }
}

impl Drop for ObserveFts {
    fn drop(&mut self) {
        // SAFETY: unregister only this test's callback, leaving other extensions intact.
        unsafe { ffi::sqlite3_cancel_auto_extension(Some(observe_connection)) };
        *FTS_READS.lock().unwrap() = None;
    }
}

unsafe extern "C" fn observe_connection(
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

fn domain_rows(conn: &Connection) -> Vec<(String, i64, Vec<u8>)> {
    conn.prepare("SELECT domain,owner_id,digest FROM domain_commitments ORDER BY domain,owner_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

pub fn assert_disk_build_matches_serial_seal() {
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
    let mut expected = None;
    let mut expected_domains = None;
    for threads in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let db = workspace.path(format!("cold-{threads}.sqlite"));
        let observation = ObserveFts::start(workspace.path(""), db.clone());
        let report = pool.install(|| writer::build(&source, &db, None)).unwrap();
        let reads = FTS_READS.lock().unwrap().take().unwrap();
        drop(observation);
        assert!(report["nodes"].as_u64().unwrap() >= 8192);
        let root = report["output_root"].as_str().unwrap().to_owned();
        if let Some(expected) = &expected {
            assert_eq!(
                &root, expected,
                "cold sealing is independent of thread count"
            );
        } else {
            expected = Some(root.clone());
        }
        let conn = Connection::open(&db).unwrap();
        let domains = domain_rows(&conn);
        if let Some(expected) = &expected_domains {
            assert_eq!(
                &domains, expected,
                "all owner and FTS digests are thread-independent"
            );
        } else {
            expected_domains = Some(domains.clone());
        }
        commitments::verify(&conn).unwrap();
        assert_eq!(commitments::seal(&conn).unwrap(), root);
        assert_eq!(
            domain_rows(&conn),
            domains,
            "projection equals the SQL oracle in every domain"
        );
        drop(conn);
        commitments::verify(&Connection::open(&db).unwrap()).unwrap();
        if threads == 1 {
            // Calibrate the observer against real reads before using its absence
            // as red proof. This control intentionally reads the published file.
            let control = ObserveFts::start(workspace.path(""), db.clone());
            let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
            commitments::verify(&conn).unwrap();
            let control_reads = FTS_READS.lock().unwrap().take().unwrap();
            drop(conn);
            drop(control);
            assert_eq!(
                control_reads.domains.len(),
                4,
                "observer must see every real FTS domain"
            );
            assert!(
                control_reads.invalid_reader,
                "observer must identify the published file"
            );
        }
        if threads > 1 {
            for domain in [
                "node_search_postings",
                "node_search_config",
                "doc_search_postings",
                "doc_search_config",
            ] {
                assert!(reads.domains.contains_key(domain), "assertion failed: cold FTS domain {domain} must be computed through a stage-private read-only connection");
            }
            let workers: HashSet<_> = reads.domains.values().flatten().collect();
            assert!(reads.domains["node_search_postings"]
                .is_disjoint(&reads.domains["doc_search_postings"]));
            assert!(
                (2..=4).contains(&workers.len()),
                "independent FTS domains must use bounded workers: {workers:?}"
            );
            assert!(
                !reads.invalid_reader,
                "digest workers must not read the published database"
            );
        }
    }

    // A real reader failure must leave the previous published generation intact.
    let db = workspace.path("cold-4.sqlite");
    let published = std::fs::read(&db).unwrap();
    let observation = ObserveFts::start(workspace.path(""), db.clone());
    FTS_READS.lock().unwrap().as_mut().unwrap().deny_domain = Some("node_search_config");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let result = pool.install(|| writer::build(&source, &db, None));
    let reads = FTS_READS.lock().unwrap().take().unwrap();
    drop(observation);
    assert!(result.is_err(), "FTS reader failures must abort the build");
    assert!(reads.domains.contains_key("node_search_config"));
    assert!(!reads.invalid_reader);
    assert_eq!(std::fs::read(&db).unwrap(), published);
    commitments::verify(&Connection::open(&db).unwrap()).unwrap();
}
