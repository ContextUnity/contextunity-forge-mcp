use super::*;

enum NamedStage {
    WorkerOrder,
    Initial,
    Provider,
}

#[test]
fn named_binding_preserves_canonical_targets_and_delta() {
    assert_named_binding_preserves_canonical_targets_and_delta(false);
}

#[test]
fn named_binding_preserves_canonical_targets_and_delta_when_renamed() {
    assert_named_binding_preserves_canonical_targets_and_delta(true);
}

fn assert_named_binding_preserves_canonical_targets_and_delta(renamed: bool) {
    let public_extraction_checked = std::cell::Cell::new(false);
    let observe_specific = |observed: &operations::OperationPhase<'_>, active| {
        let durable_facts = &observed.durable_facts;
        let case = observed.case;
        let phase = observed.phase;
        let rewrite = observed.rewrite;
        if active {
            let mut publication_failures = Vec::new();
            for (path, count) in [
                ("classic.html", 2),
                ("module.html", 1),
                ("handler.html", 1),
                ("uncertain.html", 2),
                ("observed.vue", 1),
            ] {
                let domain = if path == "observed.vue" {
                    "store"
                } else {
                    "renderer"
                };
                let facts = &durable_facts[path];
                let definitions = named_bindings_for_domain(facts, &rewrite(domain));
                if definitions.len() != count {
                    publication_failures.push(format!(
                        "{path}: complete canonical definitions expected {count}, got {}",
                        definitions.len()
                    ));
                }
                for fact in &definitions {
                    if fact["role"]["target"]["kind"] == "node" {
                        let id = fact["role"]["target"]["id"].as_str().unwrap();
                        if !facts["nodes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|node| node["id"] == id)
                        {
                            publication_failures.push(format!("{path}: explicit target {id} must be an actual retained island Node"));
                        }
                    }
                }
                if path == "uncertain.html"
                    && !definitions
                        .iter()
                        .any(|fact| fact["role"]["target"]["kind"] == "unknown")
                {
                    publication_failures
                        .push("uncertain.html: unknown alternative was dropped".to_owned());
                }
            }
            // Bundled-rule extraction uses immutable fixture text and does not
            // depend on the persisted phase or the renamed manifest variant.
            // Prove that pure seam once; keep graph assertions phase-specific.
            if !public_extraction_checked.replace(true) {
                for (path, count) in [
                    ("classic.html", 2),
                    ("module.html", 1),
                    ("handler.html", 1),
                    ("uncertain.html", 2),
                    ("observed.vue", 1),
                ] {
                    // The public API uses its bundled declarative rules. Keep
                    // its original spelling at the same publication boundary.
                    if path == "handler.html" {
                        continue;
                    }
                    let original = case.source(path);
                    let public = contextunity_forge_mcp::engine::ast::extract(
                        path,
                        if path.ends_with(".vue") {
                            "vue"
                        } else {
                            "html"
                        },
                        original,
                    )
                    .unwrap();
                    let projected_public = crate::common::assertions::projected_facts(&public);
                    let public_definitions = named_bindings_for_domain(
                        &projected_public,
                        if path == "observed.vue" {
                            "store"
                        } else {
                            "gridviewspec_renderer"
                        },
                    );
                    if public_definitions.len() != count {
                        publication_failures.push(format!(
                            "{path}: public definitions expected {count}, got {}",
                            public_definitions.len()
                        ));
                    }
                    if path == "observed.vue" {
                        let mut module = public
                            .nodes
                            .iter()
                            .find(|node| node.kind == "module")
                            .unwrap()
                            .clone();
                        module.details = json!({});
                        let namespace = module.qualname.clone();
                        let mut legacy = contextunity_forge_mcp::core::models::Facts::default();
                        legacy.nodes.push(module);
                        languages::require("vue")
                            .unwrap()
                            .extract_file(path, original, &namespace, &mut legacy)
                            .unwrap();
                        let projected_legacy = crate::common::assertions::projected_facts(&legacy);
                        let definitions = named_bindings_for_domain(&projected_legacy, "store");
                        if definitions.len() != count || definitions != public_definitions {
                            publication_failures.push(format!("{path}: legacy extract_file must retain the same ordered canonical definitions as typed extraction"));
                        }
                        for fact in definitions {
                            if let Some(id) = fact["role"]["target"]["id"].as_str() {
                                if !legacy.nodes.iter().any(|node| node.id == id) {
                                    publication_failures.push(format!(
                                        "{path}: legacy target {id} must identify its actual Node"
                                    ));
                                }
                            }
                        }
                    }
                    for fact in public_definitions {
                        if let Some(id) = fact["role"]["target"]["id"].as_str() {
                            if !public.nodes.iter().any(|node| node.id == id) {
                                publication_failures.push(format!(
                                    "{path}: public target {id} must identify its actual Node"
                                ));
                            }
                        }
                    }
                }
            }
            for path in [
                "over_limit.html",
                "invalid_fragment.html",
                "invalid_later.html",
            ] {
                let Some(facts) = durable_facts.get(path) else {
                    continue;
                };
                if !facts["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["details"]["value_flow"]["fact_limit_exceeded"] == true)
                {
                    publication_failures.push(format!("{path}: invalid/aggregate-overflow state must survive composition and serialization"));
                }
            }
            assert!(
                publication_failures.is_empty(),
                "{phase}: {}",
                publication_failures.join("\n")
            );
        }
    };
    let check_cold = operations::check_cold;
    let observe = |observed: operations::OperationPhase<'_>,
                   active: bool,
                   changed_target: operations::ChangedTarget| {
        let observed = operations::observe(observed, active, changed_target);
        observe_specific(&observed, active);
    };
    let named = |stage: NamedStage, fixture: &operations::OperationFixture, phase: &str| {
        let case = &fixture.case;
        let rewrite = |text: &str| (fixture.rewrite)(text);
        match stage {
            NamedStage::WorkerOrder => {
                // Exercise the same worker's embedded -> native extraction order
                // before the persisted matrix checks every cold/delta transition.
                for (path, copies) in [
                    ("observed.html", 1),
                    ("observed-copy.html", 1),
                    ("observed.html", 2),
                    ("imported.js", 1),
                ] {
                    let source_path = if path.ends_with(".html") {
                        "observed.html"
                    } else {
                        path
                    };
                    let source = case.source(source_path);
                    let facts = contextunity_forge_mcp::engine::ast::extract(
                        path,
                        if path.ends_with(".html") {
                            "html"
                        } else {
                            "javascript"
                        },
                        &rewrite(&source.repeat(copies)),
                    )
                    .unwrap();
                    assert!(
                        facts.references.iter().all(|reference| facts
                            .nodes
                            .iter()
                            .any(|node| node.id == reference.source)),
                        "{phase}: extraction cannot publish another file's member candidates in {path}"
                    );
                }
            }
            NamedStage::Initial => {
                let frozen = fixture.read_bytes("consumer.py");
                for (paths, function, line, target_path, target_name) in [
                    (
                        &["over_limit.html"][..],
                        "over_limit",
                        13,
                        "provider.js",
                        "drawA",
                    ),
                    (
                        &["invalid_fragment.html", "invalid_later.html"][..],
                        "invalid_fragment",
                        14,
                        "provider.js",
                        "drawA",
                    ),
                    (
                        &["observed.vue"][..],
                        "vue_named",
                        15,
                        "observed.vue",
                        "vueDraw",
                    ),
                ] {
                    let originals: Vec<_> = paths
                        .iter()
                        .map(|path| (*path, fixture.read_source(path)))
                        .collect();
                    for present in [false, true] {
                        for (path, original) in &originals {
                            fixture.apply_physical_source_edit(
                                path,
                                if present { original.as_str() } else { "" },
                            );
                        }
                        let conn = fixture.read_connection();
                        let actual = edge_dst_ids(&conn, "consumer.py", function, "references");
                        let resolved = if function == "vue_named" {
                            present
                        } else {
                            !present
                        };
                        let expected = if !resolved {
                            vec![]
                        } else {
                            vec![find_node_id(&conn, target_path, target_name)]
                        };
                        assert_eq!(
                            actual, expected,
                            "{phase}: invalid competitor {function} present={present}"
                        );
                        let states =
                            coverage_statuses(&conn, "consumer.py", line, &format!("%{function}%"));
                        assert_eq!(
                            states,
                            [if resolved { "resolved" } else { "unresolved" }],
                            "{phase}: scoped invalid competitor coverage"
                        );
                        assert_eq!(
                            fixture.read_bytes("consumer.py"),
                            frozen,
                            "{phase}: invalid provider-only delta"
                        );
                        drop(conn);
                        check_cold(fixture.observe_cold(), "filesystem cold");
                    }
                }
                // Distinct supported origins exhaust the carrier even though every
                // rule and call is individually valid. Keep policy fixed while only
                // the provider changes, including the previously unresolved state.
                let mut overflow_pack = case.pack.clone();
                for i in 0..129 {
                    overflow_pack["syntax_rules"].as_array_mut().unwrap().push(syntax(
                    &format!("overflow-origin-{i}"), "javascript",
                    json!({"kind":"named_binding","domain":"renderer","binding":{"kind":"call_definition","callee":{"kind":"global_member","receiver":"window.GridView","member":"registerOverflow"},"key":{"kind":"argument","index":0},"target":{"kind":"argument","index":1},"target_kind":"callable"}}),
                ));
                }
                fixture.apply_source_edits(&[
                    (
                        "unrelated/pyproject.toml",
                        "[project]\nname='unrelated'\ndependencies=['tablekit']\n",
                    ),
                    ("unrelated/package.json", "{\"name\":\"unrelated\"}\n"),
                    (
                        "unrelated/provider.js",
                        "export function isolatedDraw() { return 1; }\nwindow.GridView.registerRenderer('stock_cart', isolatedDraw);\n",
                    ),
                    (
                        "unrelated/consumer.py",
                        "from tablekit import Column\ndef unrelated(): return Column(renderer='stock_cart')\n",
                    ),
                ]);
                let unrelated_consumer = fixture.read_bytes("unrelated/consumer.py");
                fixture.apply_manifest(&overflow_pack);
                let vue_source = fixture.read_source("observed.vue");
                let overflowing_vue = vue_source.replacen("</script>", &format!("{}\n</script>", rewrite("window.GridView.registerOverflow('stock_cart', function overflowDraw() { return 1; });")), 1);
                for overflow in [true, false, true, false] {
                    fixture.apply_physical_source_edit(
                        "observed.vue",
                        if overflow {
                            &overflowing_vue
                        } else {
                            &vue_source
                        },
                    );
                    let conn = fixture.read_connection();
                    let actual =
                        edge_dst_ids(&conn, "consumer.py", &rewrite("renderer"), "references");
                    let expected = if overflow {
                        vec![]
                    } else {
                        vec![find_node_id(&conn, "provider.js", "drawA")]
                    };
                    assert_eq!(
                        actual, expected,
                        "{phase}: distinct-origin provider fault overflow={overflow}"
                    );
                    let states = coverage_statuses(&conn, "consumer.py", 2, "%stock_cart%");
                    assert_eq!(
                        states,
                        [if overflow { "unresolved" } else { "resolved" }],
                        "{phase}: distinct-origin fault coverage"
                    );
                    let blob: Vec<u8> = conn
                        .query_row(
                            "SELECT facts_blob FROM local_facts WHERE path=?1",
                            ["observed.vue"],
                            |row| row.get(0),
                        )
                        .unwrap();
                    let facts =
                        serde_json::to_value(reader::decode_local_facts(&blob).unwrap()).unwrap();
                    assert_eq!(
                        facts["nodes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|node| node["details"]["named_definition_uncertainty_fault"]
                                == true),
                        overflow,
                        "{phase}: valid origin exhaustion persists explicit owner fault"
                    );
                    assert_eq!(
                        operations::ordinary_call_count(&conn),
                        1,
                        "{phase}: named carrier exhaustion preserves ordinary calls"
                    );
                    let isolated_target =
                        find_node_id(&conn, "unrelated/provider.js", "isolatedDraw");
                    let isolated_edges =
                        edge_dst_ids(&conn, "unrelated/consumer.py", "unrelated", "references");
                    assert_eq!(
                        isolated_edges,
                        [isolated_target],
                        "{phase}: nonadmitted project retains its canonical named target"
                    );
                    assert!(
                        error_messages(&conn).is_empty(),
                        "{phase}: supported origin exhaustion is not a whole-index error"
                    );
                    assert_eq!(
                        fixture.read_bytes("consumer.py"),
                        frozen,
                        "{phase}: carrier fault provider-only mutation"
                    );
                    assert_eq!(
                        fixture.read_bytes("unrelated/consumer.py"),
                        unrelated_consumer,
                        "{phase}: unrelated consumer unchanged"
                    );
                    drop(conn);
                    check_cold(fixture.observe_cold(), "filesystem cold");
                }
                fixture.apply_manifest(&case.pack);
                let original = case.source("classic.html");
                let blocks: Vec<&str> = original.split_inclusive("</script>\n").collect();
                for (label, html, target, status) in [
                    (
                        "remove second",
                        original
                            .replace("window.GridView.registerRenderer('classic', classicB);", ""),
                        Some("classicA"),
                        "resolved",
                    ),
                    (
                        "remove first",
                        original
                            .replace("window.GridView.registerRenderer('classic', classicA);", ""),
                        Some("classicB"),
                        "resolved",
                    ),
                    (
                        "reverse islands",
                        format!("{}{}", blocks[1], blocks[0]),
                        None,
                        "ambiguous",
                    ),
                    (
                        "unknown alternative",
                        original.replace(
                            "registerRenderer('classic', classicB)",
                            "registerRenderer('classic', chooseRenderer())",
                        ),
                        None,
                        "ambiguous",
                    ),
                    (
                        "restore alternatives",
                        original.to_owned(),
                        None,
                        "ambiguous",
                    ),
                ] {
                    fixture.apply_source_edit("classic.html", &html);
                    let conn = fixture.read_connection();
                    let actual = edge_dst_ids(&conn, "consumer.py", "classic", "references");
                    let expected: Vec<String> = target
                        .map(|name| find_node_id(&conn, "classic.html", name))
                        .into_iter()
                        .collect();
                    assert_eq!(
                        actual, expected,
                        "{phase}: {label} retains actual classic alternatives"
                    );
                    let states = coverage_statuses(&conn, "consumer.py", 9, "%classic%");
                    assert_eq!(states, [status], "{phase}: {label} coverage");
                    assert_eq!(
                        fixture.read_bytes("consumer.py"),
                        frozen,
                        "classic provider-only mutation"
                    );
                    drop(conn);
                    check_cold(fixture.observe_cold(), "filesystem cold");
                }
                for (path, caller, from, to) in [
                    ("module.html", "module", "moduleDraw", "moduleOther"),
                    ("handler.html", "handler", "handlerDraw", "handlerOther"),
                ] {
                    let original = case.source(path);
                    for (html, name) in [
                        (original.replace(from, to), to),
                        (original.to_owned(), from),
                    ] {
                        fixture.apply_source_edit(path, &html);
                        let conn = fixture.read_connection();
                        let target = find_node_id(&conn, path, name);
                        let actual = edge_dst_ids(&conn, "consumer.py", caller, "references");
                        assert_eq!(
                            actual,
                            [target],
                            "{phase}: provider-only {path} uses the actual remapped Node"
                        );
                        assert_eq!(fixture.read_bytes("consumer.py"), frozen);
                        drop(conn);
                        check_cold(fixture.observe_cold(), "filesystem cold");
                    }
                }
            }
            NamedStage::Provider => {
                let provider_source = case.source("provider.js");
                let check = |function: &str, callback: Option<&str>, status: &str| {
                    let conn = fixture.read_connection();
                    let actual = query_rows_params(
                        &conn,
                        "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path='consumer.py' AND s.name=?1 AND e.kind='references' AND dp.path='provider.js' ORDER BY d.id",
                        [rewrite(function)],
                    );
                    let expected: Vec<String> = callback
                        .map(|name| find_node_id(&conn, "provider.js", name))
                        .into_iter()
                        .collect();
                    assert_eq!(actual, expected, "{phase}: provider-only {function}");
                    let line = if function == "renderer" { 2 } else { 4 };
                    let states = coverage_statuses(
                        &conn,
                        "consumer.py",
                        line,
                        if function == "renderer" {
                            "%stock_cart%"
                        } else {
                            "%other_cart%"
                        },
                    );
                    assert_eq!(
                        states,
                        [status],
                        "{phase}: provider-only {function} coverage"
                    );
                };
                for (from, to, missing_callback) in [
                    (
                        "registerRenderer('stock_cart', drawA)",
                        "registerRenderer('other_cart', drawA)",
                        Some("drawA"),
                    ),
                    (
                        "registerRenderer('stock_cart', drawA)",
                        "registerRenderer(dynamicKey, drawA)",
                        None,
                    ),
                    (
                        "registerRenderer('stock_cart', drawA)",
                        "registerRenderer('stock_cart', drawB); gv.registerRenderer('stock_cart', drawA)",
                        None,
                    ),
                ] {
                    fixture.apply_source_edit("provider.js", &provider_source.replace(from, to));
                    check(
                        "renderer",
                        None,
                        if to.contains("; gv.") {
                            "ambiguous"
                        } else {
                            "unresolved"
                        },
                    );
                    check(
                        "missing",
                        missing_callback,
                        if missing_callback.is_some() {
                            "resolved"
                        } else {
                            "unresolved"
                        },
                    );
                    check_cold(fixture.observe_cold(), "filesystem cold");
                }
                fixture.apply_source_edit("provider.js", provider_source);
                let mut wrong_domain = case.pack.clone();
                wrong_domain["syntax_rules"][0]["operation"]["domain"] = "action".into();
                fixture.apply_manifest(&wrong_domain);
                check("renderer", None, "unresolved");
                check_cold(fixture.observe_cold(), "filesystem cold");
                fixture.apply_manifest(&case.pack);
                observe(fixture.read_phase(phase), true, None);
                check_cold(fixture.observe_cold(), "filesystem cold");
            }
        }
    };
    operations::for_fixture_variant("named_binding", "query_hook", renamed, |mut f| {
        let label = f.label.clone();
        let fixture = &mut f;
        let mut verify = |phase: &str, active: bool, changed_target: operations::ChangedTarget| {
            let phase = format!("{label}: {phase}");
            observe(fixture.read_phase(&phase), active, changed_target);
            check_cold(fixture.observe_cold(), "filesystem cold");
        };

        named(NamedStage::WorkerOrder, fixture, &label);

        fixture.install_manifest(&fixture.case.pack);
        fixture.build();
        verify("initial", true, None);
        named(NamedStage::Initial, fixture, &label);

        // These large extraction-limit inputs are exercised above. Keep later
        // provider/manifest cold checks focused on the remaining named-binding
        // graph instead of reparsing 390 synthetic registrations each time.
        for path in [
            "over_limit.html",
            "invalid_fragment.html",
            "invalid_later.html",
        ] {
            fixture.remove_input(path);
        }
        // These temporary providers only existed to test that the rejected
        // overflow fragments cannot win resolution. Remove them with the
        // fragments so the reduced graph keeps those references unresolved.
        fixture.apply_source_edit("provider.js", fixture.case.source("provider.js"));
        let phase = format!("{label}: extraction-limit inputs removed");
        observe(fixture.read_phase(&phase), true, None);
        check_cold(
            fixture.observe_cold(),
            "filesystem cold after fixture reduction",
        );

        fixture.append_noop_comment(fixture.case.consumer);
        verify("consumer-only edit", true, None);

        let provider = "provider.js";
        let original = fixture.read_source(provider);
        let from = (fixture.rewrite)("registerRenderer('stock_cart', drawA)");
        let to = (fixture.rewrite)("registerRenderer('stock_cart', drawB)");
        fixture.apply_physical_source_edit(provider, &original.replace(&from, &to));
        verify(
            "provider callback changed",
            true,
            Some(("drawA", Some("drawB"))),
        );
        fixture.apply_source_edit(provider, &original);
        verify("provider restored", true, None);
        named(NamedStage::Provider, fixture, &label);

        operations::manifest_lifecycle(fixture, &mut verify);
        let mut unsupported_definition = fixture.case.pack.clone();
        unsupported_definition["syntax_rules"][0]["language"] = "python".into();
        unsupported_definition["syntax_rules"][0]["operation"]["binding"]["callee"] =
            json!({"kind":"imported","module":"registry","export":"register"});
        operations::rejected(
            fixture,
            &label,
            |operation| operation["domain"] = "x".repeat(129).into(),
            vec![unsupported_definition],
        );
    });
}
