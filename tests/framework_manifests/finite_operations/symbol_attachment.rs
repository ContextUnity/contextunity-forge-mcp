use super::*;

#[test]
fn symbol_attachment_preserves_canonical_targets_and_delta() {
    let mut observe_specific = |observed: &operations::OperationPhase<'_>, active| {
        let conn = &observed.conn;
        let phase = observed.phase;
        assert_eq!(
            query_count_usize(
                conn,
                "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='attachments.py' AND c.line=4 AND x.expression='configured attachment'",
                [],
            ),
            0,
            "{phase}: proven absent target slot creates no attachment coverage"
        );
        assert_eq!(
            query_count_usize(
                conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='attachments.py' AND s.name='absent_model' AND e.kind='uses_model'",
                [],
            ),
            0,
            "{phase}: absent target has no model edge"
        );
        let facts = &observed.durable_facts["attachments.py"];
        let attachments: Vec<&Value> = facts["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|node| {
                node["details"]["value_flow"]["symbol_attachments"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .collect();
        assert!(
            !attachments.iter().any(|fact| fact["position"]["line"] == 4),
            "{phase}: proven absence emits no persisted attachment fact: {attachments:?}"
        );
        if active {
            for line in [6, 8] {
                assert!(
                    attachments.iter().any(|fact| {
                        fact["position"]["line"] == line && fact["target"]["kind"] == "unknown"
                    }),
                    "{phase}: present dynamic or uncertain target remains an unknown fact at {line}"
                );
            }
        }
    };

    operations::for_each_fixture(
        operations::OperationKind::SymbolAttachment,
        "symbol_attachment",
        |fixture, label| {
            let mut verify =
                |phase: &str, active: bool, changed_target: operations::ChangedTarget| {
                    let phase = format!("{label}: {phase}");
                    operations::verify(
                        fixture,
                        &phase,
                        active,
                        changed_target,
                        &mut observe_specific,
                    );
                };

            fixture.install_manifest(&fixture.case.pack);
            fixture.build();
            verify("initial", true, None);

            let absent = fixture.read_source("attachments.py");
            let absent_decorator = (fixture.rewrite)("expose('/absent')");
            let configured_decorator = (fixture.rewrite)("expose('/absent', output=Record)");
            fixture.apply_physical_source_edit(
                "attachments.py",
                &absent.replace(&absent_decorator, &configured_decorator),
            );
            let conn = fixture.read_connection();
            commitments::verify(&conn).unwrap();
            let canonical = find_node_id(&conn, "models.py", "Record");
            let actual = edge_dst_ids(&conn, "attachments.py", "absent_model", "uses_model");
            assert_eq!(
                actual,
                [canonical],
                "{label}: adding the target slot resolves its exact unchanged provider"
            );
            drop(conn);
            operations::check_cold(fixture.observe_cold(), "filesystem cold");
            fixture.apply_physical_source_edit("attachments.py", &absent);
            verify("attachment restored", true, None);

            fixture.append_noop_comment(fixture.case.consumer);
            verify("consumer-only edit", true, None);

            let provider = "app.py";
            let original = fixture.case.source(provider);
            fixture.apply_source_edit(
                provider,
                &original.replace("output=Record", "output=Alternate"),
            );
            verify(
                "attachment provider changed",
                true,
                Some(("Record", Some("Alternate"))),
            );
            fixture.apply_source_edit(provider, original);
            verify("attachment provider restored", true, None);

            operations::manifest_lifecycle(fixture, &mut verify);
            operations::rejected(
                fixture,
                label,
                |operation| operation["callee"]["constructors"] = json!(vec!["Server"; 33]),
                vec![],
            );
        },
    );
}
