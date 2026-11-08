use super::harness::*;
use contextunity_forge_mcp::core::models::stable_hash64;
use rusqlite::params;
use std::collections::BTreeSet;

#[test]
fn parallel_owner_sealing_matches_serial_encoding_and_thread_counts() {
    let (owner_count, nodes_per_owner) = (128, 64);
    let mut conn = connection();
    let mut owners = BTreeSet::new();
    let transaction = conn.transaction().expect("transaction begins");
    {
        let mut insert_owner = transaction
            .prepare_cached("INSERT OR IGNORE INTO path_dictionary(path) VALUES(?1)")
            .unwrap();
        let mut select_owner = transaction
            .prepare_cached("SELECT path_id FROM path_dictionary WHERE path=?1")
            .unwrap();
        let mut insert_node = transaction
            .prepare_cached(
                "INSERT INTO nodes(id,kind,name,qualname,path_id,line,end_line,is_test,language,generated,details,node_hash,owner_path_id) VALUES(?1,'function',?1,?1,?2,1,1,0,'rust',0,'{}',?3,?2)",
            )
            .unwrap();
        let mut insert_edge = transaction
            .prepare_cached("INSERT INTO edges VALUES(?1,'calls',?2,?3,?4,1,2,2)")
            .unwrap();
        let mut insert_occurrence = transaction
            .prepare_cached("INSERT INTO edge_occurrences VALUES(?1,?2,?3,?4,'calls',?5,1,2)")
            .unwrap();
        let mut insert_dependency = transaction
            .prepare_cached("INSERT INTO dependencies VALUES(?1,?2,NULL,'imports',?3,'external')")
            .unwrap();

        for owner_index in (0..owner_count).rev() {
            let path = format!("src/owner_{owner_index:03}.rs");
            owners.insert(path.clone());
            insert_owner.execute([&path]).unwrap();
            let owner: i64 = select_owner.query_row([&path], |row| row.get(0)).unwrap();
            for ordinal in (0..nodes_per_owner).rev() {
                let id = format!("fn:{owner_index}:{ordinal}");
                insert_node
                    .execute(params![id, owner, stable_hash64(&id)])
                    .unwrap();
            }
            for ordinal in (0..nodes_per_owner).rev() {
                let src = stable_hash64(&format!("fn:{owner_index}:{ordinal}"));
                let dst = stable_hash64(&format!("fn:{owner_index}:0"));
                insert_edge
                    .execute(params![src, dst, owner, ordinal + 1])
                    .unwrap();
                insert_occurrence
                    .execute(params![owner, ordinal, src, dst, ordinal + 1])
                    .unwrap();
                insert_dependency
                    .execute(params![owner, ordinal, format!("symbol_{ordinal}")])
                    .unwrap();
            }
        }
    }
    transaction
        .execute_batch(
            "INSERT INTO coverage_expressions(expression) VALUES('item_00');
         INSERT INTO resolution_coverage(path_id,line,expression_id,status,evidence_id)
         SELECT p.path_id, 0, x.expression_id, 'resolved', 1
         FROM path_dictionary p
         CROSS JOIN coverage_expressions x
         WHERE p.path LIKE 'src/owner_%';",
        )
        .unwrap();
    transaction.commit().unwrap();

    assert_eq!(owners.len(), owner_count);

    let single = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let connection = &mut conn;
    let serial_root = single
        .install(move || commitments::seal(connection))
        .expect("serial sealing produces the reference root");

    let owner_domains = [
        "nodes",
        "edges",
        "edge_occurrences",
        "dependencies",
        "resolution_coverage",
    ];
    let mut digest_query = conn
        .prepare_cached(
            "SELECT p.path,d.digest FROM domain_commitments d
         JOIN path_dictionary p ON p.path_id=d.owner_id
         WHERE d.domain=?1 AND p.path LIKE 'src/owner_%' ORDER BY p.path",
        )
        .unwrap();
    for table in owner_domains {
        let rows = digest_query
            .query_map([table], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .unwrap();
        let mut observed_owners = BTreeSet::new();
        let mut count = 0;
        for row in rows {
            let (owner, digest) = row.unwrap();
            assert_eq!(digest.len(), 32, "{table}: {owner}");
            observed_owners.insert(owner);
            count += 1;
        }
        assert_eq!(count, owner_count, "{table}");
        let expected_owners: BTreeSet<&str> = owners.iter().map(String::as_str).collect();
        let observed_owners_ref: BTreeSet<&str> =
            observed_owners.iter().map(String::as_str).collect();
        assert_eq!(observed_owners_ref, expected_owners, "{table}");
    }
    drop(digest_query);

    commitments::verify(&conn).expect("serial verification succeeds");

    for thread_count in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .build()
            .unwrap();
        let connection = &mut conn;
        let full_seal = pool
            .install(move || commitments::seal(connection))
            .expect("full sealing succeeds for every pool");
        assert_eq!(
            full_seal, serial_root,
            "full sealing must match the serial root with {thread_count} threads"
        );

        let connection = &mut conn;
        let owner_paths = &owners;
        let owner_seal = pool
            .install(move || commitments::seal_owners(connection, Some(owner_paths)))
            .expect("selected-owner sealing succeeds for every pool");
        assert_eq!(
            owner_seal, serial_root,
            "owner sealing must match the serial root with {thread_count} threads"
        );

        let connection = &mut conn;
        pool.install(move || commitments::verify(connection))
            .expect("every pool's sealed database verifies");
    }
}
