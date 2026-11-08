use super::*;

#[test]
fn endpoint_projection_preserves_canonical_targets_and_delta() {
    let mut observe_specific = |observed: &operations::OperationPhase<'_>, active| {
        if active {
            if let Some(vector) = observed.case.vector {
                let payloads = operations::durable_payloads(&observed.durable_facts, vector);
                assert!(
                    payloads.iter().any(|fact| {
                        fact["url"]["kind"] == "unknown" && fact["method"]["kind"] == "unknown"
                    }),
                    "{}: explicit uncertain endpoint inputs survive persistence",
                    observed.phase
                );
            }
        }
    };

    operations::for_each_fixture(
        operations::OperationKind::EndpointProjection,
        "endpoint_projection",
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

            let original = fixture.read_source("requests.html");
            fixture.apply_physical_source_edit(
                "requests.html",
                &original.replace("/items/${id}/", "/items/7/"),
            );
            let conn = fixture.read_connection();
            commitments::verify(&conn).unwrap();
            let target = node_id_by_kind(&conn, "backend.py", "route", "GET /items/{id}/");
            let actual = operations::targets_on_line(&conn, "requests.html", 3, "calls_endpoint");
            assert_eq!(
                actual,
                [target],
                "{label}: literal request targets the actual unchanged parameter route"
            );
            drop(conn);
            operations::check_cold(fixture.observe_cold(), "filesystem cold");
            fixture.apply_physical_source_edit("requests.html", &original);
            verify("request restored", true, None);

            fixture.append_noop_comment(fixture.case.consumer);
            verify("consumer-only edit", true, None);

            let provider = "spec.py";
            let original = fixture.case.source(provider);
            fixture.apply_source_edit(provider, &original.replace("mode='post'", "mode='get'"));
            verify(
                "endpoint method changed",
                true,
                Some(("POST /api/records/", Some("GET /api/records/"))),
            );
            fixture.apply_source_edit(provider, original);
            verify("endpoint method restored", true, None);

            operations::manifest_lifecycle(fixture, &mut verify);
            operations::rejected(
                fixture,
                label,
                |operation| operation["default_method"] = "INVALID".into(),
                vec![],
            );
        },
    );
}
