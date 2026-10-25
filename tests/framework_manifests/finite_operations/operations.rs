use super::*;
use serde_json::json;

type Edge<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperationKind {
    SymbolAttachment,
    FileRoute,
    FactoryMembers,
    NamedBinding,
    EndpointProjection,
}

impl OperationKind {
    fn label(self) -> &'static str {
        match self {
            Self::SymbolAttachment => "symbol_attachment",
            Self::FileRoute => "file_route",
            Self::FactoryMembers => "factory_members",
            Self::NamedBinding => "named_binding",
            Self::EndpointProjection => "endpoint_projection",
        }
    }
}

struct Case {
    kind: OperationKind,
    vector: Option<&'static str>,
    pack: Value,
    files: &'static [(&'static str, &'static str)],
    edges: &'static [Edge<'static>],
    negatives: &'static [Edge<'static>],
    coverage: &'static [(&'static str, usize, &'static str, &'static str)],
    consumer: &'static str,
    rename: &'static [(&'static str, &'static str)],
}

mod cases;

pub(super) fn run(failures: &mut Vec<String>) {
    // All rows enter through the same real manifest -> writer -> persisted graph seam.
    // Select canonical declaration IDs from the reopened store, including embedded scopes.
    let cases = cases::all();
    for case in &cases {
        for renamed in [false, true] {
            let phase = format!("{} renamed={renamed}", case.kind.label());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rewrite = |text: &str| {
                    case.rename
                        .iter()
                        .fold(text.to_owned(), |value, (from, to)| {
                            if renamed {
                                value.replace(from, to)
                            } else {
                                value
                            }
                        })
                };
                let workspace = Workspace::new();
                if case.kind == OperationKind::NamedBinding {
                    for pack in ["alpinejs", "gridviewspec"] {
                        workspace.write(
                            format!(".forge/frameworks/{pack}.toml"),
                            "enabled = false\nsyntax_rules = []\nsemantic_rules = []\n",
                        );
                    }
                }
                workspace.write(
                    "pyproject.toml",
                    &rewrite(
                        "[project]\nname='finite-proof'\ndependencies=['webkit','tablekit']\n",
                    ),
                );
                workspace.write(
                    "package.json",
                    &rewrite(if case.kind == OperationKind::FactoryMembers {
                        r#"{"name":"finite-proof","dependencies":{"statekit":"*","observer-kit":"*"}}"#
                    } else {
                        r#"{"name":"finite-proof","dependencies":{"statekit":"*"}}"#
                    }),
                );
                workspace.write(
                    "ordinary.py",
                    "def helper(): return 1\ndef ordinary(): return helper()\n",
                );
                for &(path, source) in case.files {
                    workspace.write(rewrite(path), &rewrite(source));
                }
                if case.kind == OperationKind::NamedBinding {
                    let provider = rewrite(
                        case.files
                            .iter()
                            .find(|(path, _)| *path == "provider.js")
                            .unwrap()
                            .1,
                    );
                    workspace.write("provider.js", &format!("{provider}\n{}", rewrite("window.GridView.registerRenderer('over_limit', drawA);\nwindow.GridView.registerRenderer('invalid_fragment', drawA);\n")));
                    let consumer = rewrite(
                        case.files
                            .iter()
                            .find(|(path, _)| *path == "consumer.py")
                            .unwrap()
                            .1,
                    );
                    workspace.write(
                        "consumer.py",
                        &format!(
                            "{consumer}{}",
                            rewrite("def vue_named(): return Column(renderer='vue_named')\n")
                        ),
                    );
                    let vue = rewrite(
                        case.files
                            .iter()
                            .find(|(path, _)| *path == "observed.vue")
                            .unwrap()
                            .1,
                    );
                    workspace.write("observed.vue", &vue.replacen("</script>", &format!("{}\n</script>", rewrite("window.GridView.registerRenderer('vue_named', function vueDraw() { return 1; });")), 1));
                    // Both valid fragments stay below128; their composition exceeds it.
                    // The second shape also exercises an already-invalid first/later island.
                    for (path, counts, key) in [
                        ("over_limit.html", [65, 65], "over_limit"),
                        ("invalid_fragment.html", [129, 1], "invalid_fragment"),
                        ("invalid_later.html", [1, 129], "invalid_fragment"),
                    ] {
                        let html = counts.into_iter().enumerate().map(|(island, count)| {
                            format!("<script>\nfunction bound{island}() {{ return 1; }}\n{}</script>\n", (0..count).map(|_| format!("window.GridView.registerRenderer('{key}', bound{island});\n")).collect::<String>())
                        }).collect::<String>();
                        workspace.write(path, &rewrite(&html));
                    }
                }
                if case.kind == OperationKind::NamedBinding {
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
                        let source = case
                            .files
                            .iter()
                            .find(|(name, _)| *name == source_path)
                            .unwrap()
                            .1;
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
                        assert!(facts.references.iter().all(|reference| facts.nodes.iter().any(|node| node.id == reference.source)),
                            "{phase}: extraction cannot publish another file's member candidates in {path}");
                    }
                }
                let manifest = ".forge/frameworks/finite.toml";
                let source = rewrite(&toml::to_string(&case.pack).unwrap());
                workspace.write(manifest, &source);
                if let Err(error) = writer::build(&workspace.0, &workspace.db(), None) {
                    failures.push(format!(
                        "{phase}: admitted operation rejected by real build: {error:#}"
                    ));
                    return;
                }
                let observe = |active: bool, changed_target: Option<(&str, Option<&str>)>| {
                    let conn = workspace.open();
                    commitments::verify(&conn).unwrap();
                    assert_eq!(
                        query_rows(
                            &conn,
                            "SELECT path||':'||message FROM errors ORDER BY path,line"
                        ),
                        Vec::<String>::new(),
                        "{phase}: valid fixture"
                    );
                    if active && case.kind == OperationKind::NamedBinding {
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
                            let facts = read_facts_json(&conn, path);
                            let definitions: Vec<&Value> = facts["nodes"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .flat_map(|node| {
                                    node["details"]["value_flow"]["named_bindings"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                })
                                .filter(|fact| fact["domain"] == rewrite(domain))
                                .collect();
                            if definitions.len() != count {
                                publication_failures.push(format!("{path}: complete canonical definitions expected {count}, got {}", definitions.len()));
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
                                publication_failures.push(
                                    "uncertain.html: unknown alternative was dropped".to_owned(),
                                );
                            }
                            // The public API uses its bundled declarative rules. Keep its
                            // original spelling and compare the same publication boundary.
                            if path != "handler.html" {
                                let original =
                                    case.files.iter().find(|(name, _)| *name == path).unwrap().1;
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
                                let projected_public =
                                    crate::common::assertions::projected_facts(&public);
                                let public_definitions: Vec<&Value> = projected_public["nodes"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .flat_map(|node| {
                                        node["details"]["value_flow"]["named_bindings"]
                                            .as_array()
                                            .into_iter()
                                            .flatten()
                                    })
                                    .filter(|fact| {
                                        fact["domain"]
                                            == if path == "observed.vue" {
                                                "store"
                                            } else {
                                                "gridviewspec_renderer"
                                            }
                                    })
                                    .collect();
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
                                    let mut legacy =
                                        contextunity_forge_mcp::core::models::Facts::default();
                                    legacy.nodes.push(module);
                                    languages::require("vue")
                                        .unwrap()
                                        .extract_file(path, original, &namespace, &mut legacy)
                                        .unwrap();
                                    let projected_legacy =
                                        crate::common::assertions::projected_facts(&legacy);
                                    let definitions: Vec<&Value> = projected_legacy["nodes"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .flat_map(|node| {
                                            node["details"]["value_flow"]["named_bindings"]
                                                .as_array()
                                                .into_iter()
                                                .flatten()
                                        })
                                        .filter(|fact| fact["domain"] == "store")
                                        .collect();
                                    if definitions.len() != count
                                        || definitions != public_definitions
                                    {
                                        publication_failures.push(format!("{path}: legacy extract_file must retain the same ordered canonical definitions as typed extraction"));
                                    }
                                    for fact in definitions {
                                        if let Some(id) = fact["role"]["target"]["id"].as_str() {
                                            if !legacy.nodes.iter().any(|node| node.id == id) {
                                                publication_failures.push(format!("{path}: legacy target {id} must identify its actual Node"));
                                            }
                                        }
                                    }
                                }
                                for fact in public_definitions {
                                    if let Some(id) = fact["role"]["target"]["id"].as_str() {
                                        if !public.nodes.iter().any(|node| node.id == id) {
                                            publication_failures.push(format!("{path}: public target {id} must identify its actual Node"));
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
                            let facts = read_facts_json(&conn, path);
                            if !facts["nodes"].as_array().unwrap().iter().any(|node| {
                                node["details"]["value_flow"]["fact_limit_exceeded"] == true
                            }) {
                                publication_failures.push(format!("{path}: invalid/aggregate-overflow state must survive composition and serialization"));
                            }
                        }
                        assert!(
                            publication_failures.is_empty(),
                            "{phase}: {}",
                            publication_failures.join("\n")
                        );
                    }
                    for &(source_path, source_name, kind, target_path, target_name) in case.edges {
                        // Route handlers are ordinary configured routes and survive projection removal.
                        let replacement = changed_target.filter(|(old, _)| *old == target_name);
                        let expected = (active
                            || (kind == "handles" && workspace.0.join(manifest).exists()))
                            && replacement.is_none_or(|(_, target)| target.is_some());
                        let selected_target = replacement
                            .and_then(|(_, target)| target)
                            .unwrap_or(target_name);
                        let ids = |path: &str, name: &str| -> Vec<String> {
                            conn.prepare("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND (n.name LIKE ?2 OR n.id=?2) ORDER BY n.id").unwrap()
                            .query_map(params![rewrite(path), rewrite(name)], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
                        };
                        let source_ids = ids(source_path, source_name);
                        let target_ids = ids(target_path, selected_target);
                        if expected {
                            assert_eq!(
                                source_ids.len(),
                                1,
                                "{phase}: source {source_path}:{source_name}"
                            );
                            assert_eq!(
                                target_ids.len(),
                                1,
                                "{phase}: target {target_path}:{target_name}"
                            );
                        }
                        let actual: Vec<(String, String)> = conn.prepare("SELECT s.id,d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path=?1 AND (s.name LIKE ?2 OR s.id=?2) AND e.kind=?3 AND dp.path=?4 AND (d.name LIKE ?5 OR d.name LIKE ?6) ORDER BY s.id,d.id").unwrap()
                        .query_map(params![rewrite(source_path), rewrite(source_name), kind, rewrite(target_path), rewrite(selected_target), rewrite(target_name)], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                        let wanted = if expected {
                            vec![(source_ids[0].clone(), target_ids[0].clone())]
                        } else {
                            vec![]
                        };
                        assert_eq!(
                            actual, wanted,
                            "{phase}: active={active} {source_name} --{kind}--> {target_name}"
                        );
                    }
                    if changed_target.is_none() {
                        for &(source_path, source_name, kind, target_path, target_name) in
                            case.negatives
                        {
                            let count: usize = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path=?1 AND s.name LIKE ?2 AND e.kind=?3 AND dp.path=?4 AND d.name LIKE ?5", params![rewrite(source_path),rewrite(source_name),kind,rewrite(target_path),rewrite(target_name)], |r|r.get(0)).unwrap();
                            assert_eq!(
                                count, 0,
                                "{phase}: reject {source_name} --{kind}--> {target_name}"
                            );
                        }
                        if active {
                            for &(path, line, expression, status) in case.coverage {
                                let states = coverage_statuses(
                                    &conn,
                                    &rewrite(path),
                                    line,
                                    &rewrite(expression),
                                );
                                assert_eq!(states, [status], "{phase}: {path}:{line} {expression}");
                            }
                        }
                    }
                    match case.kind {
                        OperationKind::FileRoute => {
                            cases::assert_file_route_projection(&conn, &phase, &rewrite)
                        }
                        OperationKind::SymbolAttachment => {
                            cases::assert_symbol_attachment_projection(&conn, &phase, active)
                        }
                        OperationKind::FactoryMembers => {
                            cases::assert_factory_member_projection(&conn, &phase, &rewrite)
                        }
                        OperationKind::NamedBinding | OperationKind::EndpointProjection => {}
                    }
                    assert_eq!(query_count_usize(&conn, "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'", []), 1, "{phase}: ordinary language call survives");
                    if let Some(vector) = case.vector {
                        let payloads: Vec<Value> = all_local_facts(&conn)
                            .into_iter()
                            .flat_map(|(_, facts)| {
                                facts["nodes"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .flat_map(|node| {
                                        node["details"]["value_flow"][vector]
                                            .as_array()
                                            .cloned()
                                            .unwrap_or_default()
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect();
                        assert_eq!(!payloads.is_empty(), active, "{phase}: raw canonical compressed {vector}; no public-Facts projection");
                        for fact in &payloads {
                            assert!(
                                fact["origin"].is_object() && fact["position"].is_object(),
                                "{phase}: durable provenance/position {fact}"
                            );
                        }
                        if active && case.kind == OperationKind::EndpointProjection {
                            cases::assert_unknown_endpoint_inputs(&payloads, &phase);
                        }
                    }
                };
                observe(true, None);
                assert_delta_cold_parity(&workspace);
                if case.kind == OperationKind::NamedBinding {
                    cases::run_named_binding_transitions(
                        &workspace, case, &phase, manifest, &source, &rewrite,
                    );
                }
                if case.kind == OperationKind::SymbolAttachment {
                    let absent = case
                        .files
                        .iter()
                        .find(|(path, _)| *path == "attachments.py")
                        .unwrap()
                        .1;
                    workspace.write(
                        "attachments.py",
                        &rewrite(
                            &absent
                                .replace("expose('/absent')", "expose('/absent', output=Record)"),
                        ),
                    );
                    workspace.delta("attachments.py");
                    let conn = workspace.open();
                    commitments::verify(&conn).unwrap();
                    let canonical = find_node_id(&conn, "models.py", "Record");
                    let actual =
                        edge_dst_ids(&conn, "attachments.py", "absent_model", "uses_model");
                    assert_eq!(
                        actual,
                        [canonical],
                        "{phase}: adding the target slot resolves its exact unchanged provider"
                    );
                    drop(conn);
                    assert_delta_cold_parity(&workspace);
                    workspace.write("attachments.py", &rewrite(absent));
                    workspace.delta("attachments.py");
                    observe(true, None);
                    assert_delta_cold_parity(&workspace);
                }
                if case.kind == OperationKind::EndpointProjection {
                    let original = case
                        .files
                        .iter()
                        .find(|(path, _)| *path == "requests.html")
                        .unwrap()
                        .1;
                    workspace.write(
                        "requests.html",
                        &rewrite(&original.replace("/items/${id}/", "/items/7/")),
                    );
                    workspace.delta("requests.html");
                    let conn = workspace.open();
                    commitments::verify(&conn).unwrap();
                    let target = node_id_by_kind(&conn, "backend.py", "route", "GET /items/{id}/");
                    let actual = query_rows(&conn, "SELECT d.id FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='requests.html' AND e.line=3 AND e.kind='calls_endpoint' ORDER BY d.id");
                    assert_eq!(
                        actual,
                        [target],
                        "{phase}: literal request targets the actual unchanged parameter route"
                    );
                    drop(conn);
                    assert_delta_cold_parity(&workspace);
                    workspace.write("requests.html", &rewrite(original));
                    workspace.delta("requests.html");
                    observe(true, None);
                    assert_delta_cold_parity(&workspace);
                }
                // Consumer-only edit forces cached unchanged-provider hydration.
                let consumer_source = case
                    .files
                    .iter()
                    .find(|(path, _)| *path == case.consumer)
                    .unwrap()
                    .1;
                let comment = if case.consumer.ends_with(".py") {
                    "\n# cached provider replay\n"
                } else {
                    "\n// cached provider replay\n"
                };
                workspace.write(
                    rewrite(case.consumer),
                    &rewrite(&format!("{consumer_source}{comment}")),
                );
                workspace.delta(&rewrite(case.consumer));
                observe(true, None);
                assert_delta_cold_parity(&workspace);
                // The named callback row changes only a function-body argument, retaining
                // both callbacks, imports/callees/returns/fields and cross-language consumers.
                // Other rows exercise their existing source or route-provider delta boundary.
                let (provider, from, to, old_target, new_target) = match case.kind {
                    OperationKind::SymbolAttachment => (
                        "app.py",
                        "output=Record",
                        "output=Alternate",
                        "Record",
                        Some("Alternate"),
                    ),
                    OperationKind::FactoryMembers => {
                        ("store.ts", "checkout()", "commit()", "checkout", None)
                    }
                    OperationKind::NamedBinding => (
                        "provider.js",
                        "registerRenderer('stock_cart', drawA)",
                        "registerRenderer('stock_cart', drawB)",
                        "drawA",
                        Some("drawB"),
                    ),
                    OperationKind::EndpointProjection => (
                        "spec.py",
                        "mode='post'",
                        "mode='get'",
                        "POST /api/records/",
                        Some("GET /api/records/"),
                    ),
                    _ => (
                        "screens/catalog/detail.vue",
                        "",
                        "",
                        "GET /catalog/detail",
                        None,
                    ),
                };
                let provider_source = case
                    .files
                    .iter()
                    .find(|(path, _)| *path == provider)
                    .unwrap()
                    .1;
                if case.kind == OperationKind::FileRoute {
                    fs::remove_file(workspace.0.join(rewrite(provider))).unwrap();
                } else {
                    workspace.write(
                        rewrite(provider),
                        &rewrite(&provider_source.replace(from, to)),
                    );
                }
                workspace.delta(&rewrite(provider));
                observe(true, Some((old_target, new_target)));
                assert_delta_cold_parity(&workspace);
                workspace.write(rewrite(provider), &rewrite(provider_source));
                workspace.delta(&rewrite(provider));
                observe(true, None);
                assert_delta_cold_parity(&workspace);
                if case.kind == OperationKind::NamedBinding {
                    let check = |function: &str, callback: Option<&str>, status: &str| {
                        let conn = workspace.open();
                        let actual = query_rows_params(&conn, "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path='consumer.py' AND s.name=?1 AND e.kind='references' AND dp.path='provider.js' ORDER BY d.id", [rewrite(function)]);
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
                    ("registerRenderer('stock_cart', drawA)", "registerRenderer('other_cart', drawA)", Some("drawA")),
                    ("registerRenderer('stock_cart', drawA)", "registerRenderer(dynamicKey, drawA)", None),
                    ("registerRenderer('stock_cart', drawA)", "registerRenderer('stock_cart', drawB); gv.registerRenderer('stock_cart', drawA)", None),
                ] {
                    workspace.write("provider.js", &rewrite(&provider_source.replace(from, to)));
                    workspace.delta("provider.js");
                    check("renderer", None, if to.contains("; gv.") {"ambiguous"} else {"unresolved"});
                    check("missing", missing_callback, if missing_callback.is_some() {"resolved"} else {"unresolved"});
                    assert_delta_cold_parity(&workspace);
                }
                    workspace.write("provider.js", &rewrite(provider_source));
                    workspace.delta("provider.js");
                    let mut wrong_domain = case.pack.clone();
                    wrong_domain["syntax_rules"][0]["operation"]["domain"] = "action".into();
                    workspace.write(manifest, &rewrite(&toml::to_string(&wrong_domain).unwrap()));
                    workspace.delta(manifest);
                    check("renderer", None, "unresolved");
                    assert_delta_cold_parity(&workspace);
                    workspace.write(manifest, &source);
                    workspace.delta(manifest);
                    observe(true, None);
                    assert_delta_cold_parity(&workspace);
                }
                let mut inactive = case.pack.clone();
                for array in ["semantic_rules", "syntax_rules"] {
                    for rule in inactive[array].as_array_mut().unwrap() {
                        rule["activation"] = json!([{"kind":"package","name":"absent-runtime"}]);
                    }
                }
                let mut replaced = case.pack.clone();
                replaced["replace"] = true.into();
                replaced["semantic_rules"] = json!([]);
                replaced["syntax_rules"] = json!([]);
                for (label, value, active) in [
                    ("deactivated", inactive, false),
                    ("restored", case.pack.clone(), true),
                    ("replaced", replaced, false),
                    ("restored-again", case.pack.clone(), true),
                ] {
                    workspace.write(manifest, &rewrite(&toml::to_string(&value).unwrap()));
                    workspace.delta(manifest);
                    observe(active, None);
                    assert_delta_cold_parity(&workspace);
                    assert!(workspace.db().exists(), "{phase}: {label}");
                }
                fs::remove_file(workspace.0.join(manifest)).unwrap();
                workspace.delta(manifest);
                observe(false, None);
                assert_delta_cold_parity(&workspace);
                workspace.write(manifest, &source);
                workspace.delta(manifest);
                observe(true, None);
                assert_delta_cold_parity(&workspace);
                let array = if case.pack["semantic_rules"].as_array().unwrap().is_empty() {
                    "syntax_rules"
                } else {
                    "semantic_rules"
                };
                let mut unknown = case.pack.clone();
                unknown[array][0]["operation"]["unchecked"] = true.into();
                let mut boundary = case.pack.clone();
                let operation = &mut boundary[array][0]["operation"];
                match case.kind {
                    OperationKind::SymbolAttachment => {
                        operation["callee"]["constructors"] = json!(vec!["Server"; 33])
                    }
                    OperationKind::FileRoute => operation["directory"] = "../screens/".into(),
                    OperationKind::FactoryMembers => operation["argument"] = 32.into(),
                    OperationKind::NamedBinding => operation["domain"] = "x".repeat(129).into(),
                    OperationKind::EndpointProjection => {
                        operation["default_method"] = "INVALID".into()
                    }
                }
                let sql = "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id";
                let before = query_rows(&workspace.open(), sql);
                let mut invalids = vec![unknown, boundary];
                if case.kind == OperationKind::NamedBinding {
                    let mut unsupported_definition = case.pack.clone();
                    unsupported_definition["syntax_rules"][0]["language"] = "python".into();
                    unsupported_definition["syntax_rules"][0]["operation"]["binding"]["callee"] =
                        json!({"kind":"imported","module":"registry","export":"register"});
                    invalids.push(unsupported_definition);
                }
                for invalid in invalids {
                    workspace.write(manifest, &rewrite(&toml::to_string(&invalid).unwrap()));
                    assert!(
                        writer::build(&workspace.0, &workspace.db(), None).is_err(),
                        "{phase}: unknown field or invalid boundary must fail closed"
                    );
                    commitments::verify(&workspace.open()).unwrap();
                    assert_eq!(
                        query_rows(&workspace.open(), sql),
                        before,
                        "{phase}: rejected configuration cannot publish a different graph"
                    );
                }
            }));
            if let Err(payload) = result {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic");
                failures.push(format!("{phase}: {message}"));
            }
        }
    }
}
