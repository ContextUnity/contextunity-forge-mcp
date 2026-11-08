use super::*;

#[test]
fn factory_members_preserve_canonical_targets_and_delta() {
    let mut observe_specific = |observed: &operations::OperationPhase<'_>, _active| {
        let conn = &observed.conn;
        let durable_facts = &observed.durable_facts;
        let phase = observed.phase;
        let rewrite = observed.rewrite;
        let mut member_failures = Vec::new();
        for (path, expression, status, expected) in [
            ("ordinary.html", "self.draft", "unresolved", 1),
            ("prior-grammar.vue", "EVENTS.CHANGED", "external", 2),
        ] {
            let actual = query_count_usize(
                conn,
                "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2 AND c.status=?3",
                params![path, rewrite(expression), status],
            );
            if actual != expected {
                member_failures.push(format!(
                    "{path}: ordinary grammar {expression} must retain {expected} {status} rows, got {actual}"
                ));
            }
        }
        let canonical = node_id_by_kind(conn, "prior-grammar.vue", "field", "Ready");
        let enum_targets = edge_dst_ids(conn, "prior-grammar.vue", "readEnum", "references");
        if enum_targets.as_slice() != std::slice::from_ref(&canonical) {
            member_failures.push(format!(
                "prior-grammar.vue: ordinary enum target {canonical}, got {enum_targets:?}"
            ));
        }
        for path in ["ordinary.html", "prior-grammar.vue"] {
            let guessed = query_count_usize(
                conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path=?1 AND e.kind='references' AND s.name IN ('view','readArgument')",
                [path],
            );
            if guessed != 0 {
                member_failures.push(format!(
                    "{path}: unknown/external grammar reads gained {guessed} guessed exact edges"
                ));
            }
        }
        assert_eq!(
            query_count_usize(
                conn,
                "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='client.ts' AND c.line=7 AND x.expression='cart.count'",
                [],
            ),
            0,
            "{phase}: fact-absent dynamic provider creates no member coverage"
        );
        assert_eq!(
            query_count_usize(
                conn,
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='dynamicRead' AND e.kind='references'",
                [],
            ),
            0,
            "{phase}: fact-absent dynamic provider creates no exact member edge"
        );
        for path in ["ordinary.ts", "ordinary.js"] {
            let canonical = node_id_by_kind(conn, path, "field", "ledgerValue");
            let targets: Vec<String> = query_rows_params(
                conn,
                "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path=?1 AND s.name='readField' AND e.kind='references' AND d.name='ledgerValue' ORDER BY d.id",
                [path],
            );
            if targets != [canonical.clone()] {
                member_failures.push(format!(
                    "{path}: ordinary exact field target {canonical}, got {targets:?}"
                ));
            }
            let facts = &durable_facts[path];
            assert!(
                !facts["references"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reference| reference["kind"] == "references"
                        && reference["line"] == 5
                        && reference["column"] == 57),
                "{phase}: write-only LHS is not a read candidate"
            );
            let coverage = query_count_usize(
                conn,
                "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression IN ('item.payload','local.missing')",
                [path],
            );
            if coverage != 0 {
                member_failures.push(format!(
                    "{path}: unconfigured local reads/write-only LHS added {coverage} coverage rows"
                ));
            }
        }
        assert!(
            member_failures.is_empty(),
            "{phase}: {}",
            member_failures.join("\n")
        );
    };

    operations::for_each_fixture(
        operations::OperationKind::FactoryMembers,
        "factory_members",
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

            fixture.append_noop_comment(fixture.case.consumer);
            verify("consumer-only edit", true, None);

            let provider = "store.ts";
            let original = fixture.case.source(provider);
            fixture.apply_source_edit(provider, &original.replace("checkout()", "commit()"));
            verify("factory member changed", true, Some(("checkout", None)));
            fixture.apply_source_edit(provider, original);
            verify("factory member restored", true, None);

            operations::manifest_lifecycle(fixture, &mut verify);
            operations::rejected(
                fixture,
                label,
                |operation| operation["argument"] = 32.into(),
                vec![],
            );
        },
    );
}
