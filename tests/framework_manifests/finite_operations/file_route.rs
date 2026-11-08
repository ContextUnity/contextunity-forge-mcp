use super::*;

#[test]
fn file_route_preserves_canonical_targets_and_delta() {
    operations::for_each_fixture(
        operations::OperationKind::FileRoute,
        "file_route",
        |fixture, label| {
            let mut observe_specific = |observed: &operations::OperationPhase<'_>, _active| {
                for path in [
                    "screens/[id].vue",
                    "screens/dot.segment.vue",
                    "screens/....vue",
                    "screens/bad!.vue",
                    "screens/-leading.vue",
                    "screens/1leading.vue",
                    "screens/trailing-.vue",
                    "screens/double--part.vue",
                ] {
                    assert_eq!(
                        query_count_usize(
                            &observed.conn,
                            "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='route'",
                            [(observed.rewrite)(path)],
                        ),
                        0,
                        "{}: unsupported file is not a static route: {path}",
                        observed.phase
                    );
                }
            };
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

            let provider = "screens/catalog/detail.vue";
            let original = fixture.case.source(provider);
            fixture.remove_input(provider);
            verify(
                "route file removed",
                true,
                Some(("GET /catalog/detail", None)),
            );
            fixture.apply_source_edit(provider, original);
            verify("route file restored", true, None);

            operations::manifest_lifecycle(fixture, &mut verify);
            operations::rejected(
                fixture,
                label,
                |operation| operation["directory"] = "../screens/".into(),
                vec![],
            );
        },
    );
}
