use super::{commitments, writer, Connection, Workspace};

pub fn assert_disk_build_matches_serial_seal() {
    let workspace = Workspace::new();
    let source = workspace.path("source");
    let code: String = (0..8192)
        .map(|index| format!("pub fn symbol_{index:04}() {{}}\n"))
        .collect();
    workspace.write("source/lib.rs", &code);
    let mut expected = None;
    for threads in [1, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let db = workspace.path(format!("cold-{threads}.sqlite"));
        let report = pool.install(|| writer::build(&source, &db, None)).unwrap();
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
        commitments::verify(&conn).unwrap();
        assert_eq!(commitments::seal(&conn).unwrap(), root);
        drop(conn);
        commitments::verify(&Connection::open(&db).unwrap()).unwrap();
    }
}
