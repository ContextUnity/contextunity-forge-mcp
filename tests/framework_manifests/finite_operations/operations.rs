use super::*;
use serde_json::json;

type Edge<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OperationKind {
    SymbolAttachment,
    FileRoute,
    FactoryMembers,
    NamedBinding,
    EndpointProjection,
}

fn operation_kind_name(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::SymbolAttachment => "symbol_attachment",
        OperationKind::FileRoute => "file_route",
        OperationKind::FactoryMembers => "factory_members",
        OperationKind::NamedBinding => "named_binding",
        OperationKind::EndpointProjection => "endpoint_projection",
    }
}

fn operation_kind_from_name(kind: &str) -> OperationKind {
    match kind {
        "symbol_attachment" => OperationKind::SymbolAttachment,
        "file_route" => OperationKind::FileRoute,
        "factory_members" => OperationKind::FactoryMembers,
        "named_binding" => OperationKind::NamedBinding,
        "endpoint_projection" => OperationKind::EndpointProjection,
        _ => panic!("unknown finite operation kind: {kind}"),
    }
}

pub(super) struct Case {
    pub(super) kind: OperationKind,
    pub(super) vector: Option<&'static str>,
    pub(super) pack: Value,
    pub(super) files: &'static [(&'static str, &'static str)],
    pub(super) edges: &'static [Edge<'static>],
    pub(super) negatives: &'static [Edge<'static>],
    pub(super) coverage: &'static [(&'static str, usize, &'static str, &'static str)],
    pub(super) consumer: &'static str,
    pub(super) rename: &'static [(&'static str, &'static str)],
}

impl Case {
    pub(super) fn source(&self, path: &str) -> &'static str {
        self.files
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, source)| *source)
            .unwrap_or_else(|| panic!("{:?} fixture has no source for {path}", self.kind))
    }
}

mod cases;

pub(super) struct OperationPhase<'a> {
    pub conn: Connection,
    pub durable_facts: BTreeMap<String, Value>,
    pub case: &'a Case,
    pub phase: &'a str,
    pub manifest_present: bool,
    pub rewrite: &'a dyn Fn(&str) -> String,
}

pub(super) type ChangedTarget = Option<(&'static str, Option<&'static str>)>;

pub(super) fn check_cold<T: PartialEq + std::fmt::Debug>(
    observed: ColdObservations<T>,
    seam: &str,
) -> (Connection, Connection) {
    assert!(
        observed.incremental_verification.is_ok(),
        "incremental commitment: {:?}",
        observed.incremental_verification
    );
    assert!(
        observed.cold_verification.is_ok(),
        "cold commitment: {:?}",
        observed.cold_verification
    );
    assert_eq!(
        observed.incremental, observed.cold,
        "{seam}: complete edges/coverage/nodes/dependencies/domain commitments cold/delta parity"
    );
    (observed.incremental_connection, observed.cold_connection)
}

pub(super) fn observe<'a>(
    observed: OperationPhase<'a>,
    active: bool,
    changed_target: ChangedTarget,
) -> OperationPhase<'a> {
    let OperationPhase {
        conn,
        durable_facts,
        case,
        phase,
        manifest_present,
        rewrite,
    } = observed;
    commitments::verify(&conn).unwrap();
    assert_eq!(
        query_rows(
            &conn,
            "SELECT path||':'||message FROM errors ORDER BY path,line",
        ),
        Vec::<String>::new(),
        "{phase}: valid fixture"
    );
    for &(source_path, source_name, kind, target_path, target_name) in case.edges {
        let replacement = changed_target.filter(|(old, _)| *old == target_name);
        let expected = (active || (kind == "handles" && manifest_present))
            && replacement.is_none_or(|(_, target)| target.is_some());
        let selected_target = replacement
            .and_then(|(_, target)| target)
            .unwrap_or(target_name);
        let ids = |path: &str, name: &str| {
            query_rows_params(
                &conn,
                "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND (n.name LIKE ?2 OR n.id=?2) ORDER BY n.id",
                params![rewrite(path), rewrite(name)],
            )
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
        let actual = matching_edge_ids(
            &conn,
            EdgeQuery {
                source: (&rewrite(source_path), &rewrite(source_name)),
                target: (&rewrite(target_path), &rewrite(selected_target)),
                alternate_target: Some(&rewrite(target_name)),
                kind,
                source_id: true,
            },
        );
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
        for &(source_path, source_name, kind, target_path, target_name) in case.negatives {
            let count = matching_edge_ids(
                &conn,
                EdgeQuery {
                    source: (&rewrite(source_path), &rewrite(source_name)),
                    target: (&rewrite(target_path), &rewrite(target_name)),
                    alternate_target: None,
                    kind,
                    source_id: false,
                },
            )
            .len();
            assert_eq!(
                count, 0,
                "{phase}: reject {source_name} --{kind}--> {target_name}"
            );
        }
        if active {
            for &(path, line, expression, status) in case.coverage {
                let states = coverage_statuses(&conn, &rewrite(path), line, &rewrite(expression));
                assert_eq!(states, [status], "{phase}: {path}:{line} {expression}");
            }
        }
    }
    assert_eq!(
        ordinary_call_count(&conn),
        1,
        "{phase}: ordinary language call survives"
    );
    if let Some(vector) = case.vector {
        let payloads = durable_payloads(&durable_facts, vector);
        assert_eq!(
            !payloads.is_empty(),
            active,
            "{phase}: raw canonical compressed {vector}; no public-Facts projection"
        );
        for fact in &payloads {
            assert!(
                fact["origin"].is_object() && fact["position"].is_object(),
                "{phase}: durable provenance/position {fact}"
            );
        }
    }
    OperationPhase {
        conn,
        durable_facts,
        case,
        phase,
        manifest_present,
        rewrite,
    }
}

pub(super) fn verify(
    fixture: &OperationFixture,
    phase: &str,
    active: bool,
    changed_target: ChangedTarget,
    specific: &mut impl FnMut(&OperationPhase<'_>, bool),
) {
    let observed = observe(fixture.read_phase(phase), active, changed_target);
    specific(&observed, active);
    check_cold(fixture.observe_cold(), "filesystem cold");
}

pub(super) fn for_each_fixture(
    kind: OperationKind,
    name: &str,
    mut scenario: impl FnMut(&OperationFixture, &str),
) {
    for renamed in [false, true] {
        for_fixture_variant(operation_kind_name(kind), name, renamed, |fixture| {
            scenario(&fixture, &fixture.label);
        });
    }
}

pub(super) fn for_fixture_variant(
    kind: &str,
    name: &str,
    renamed: bool,
    scenario: impl FnOnce(OperationFixture),
) {
    let kind = operation_kind_from_name(kind);
    let workspace = FrameworkHarness::persisted(&[]);
    let case = cases::for_kind(kind);
    let rename_pairs = case.rename;
    let rewrite: Box<dyn Fn(&str) -> String> = Box::new(move |text| {
        rename_pairs
            .iter()
            .fold(text.to_owned(), |value, (from, to)| {
                if renamed {
                    value.replace(from, to)
                } else {
                    value
                }
            })
    });

    if kind == OperationKind::NamedBinding {
        for pack in ["alpinejs", "gridviewspec"] {
            workspace.write(
                format!(".forge/frameworks/{pack}.toml"),
                "enabled = false\nsyntax_rules = []\nsemantic_rules = []\n",
            );
        }
    }
    workspace.write(
        "pyproject.toml",
        &rewrite("[project]\nname='finite-proof'\ndependencies=['webkit','tablekit']\n"),
    );
    workspace.write(
        "package.json",
        &rewrite(if kind == OperationKind::FactoryMembers {
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
        if kind == OperationKind::NamedBinding
            && matches!(path, "provider.js" | "consumer.py" | "observed.vue")
        {
            continue;
        }
        workspace.write(rewrite(path), &rewrite(source));
    }

    if kind == OperationKind::NamedBinding {
        let provider = case.source("provider.js");
        workspace.write(
            "provider.js",
            &format!(
                "{}\n{}",
                rewrite(provider),
                rewrite("window.GridView.registerRenderer('over_limit', drawA);\nwindow.GridView.registerRenderer('invalid_fragment', drawA);\n")
            ),
        );
        let consumer = case.source("consumer.py");
        workspace.write(
            "consumer.py",
            &format!(
                "{}{}",
                rewrite(consumer),
                rewrite("def vue_named(): return Column(renderer='vue_named')\n")
            ),
        );
        let vue = case.source("observed.vue");
        let vue = rewrite(vue).replacen(
            "</script>",
            &format!(
                "{}\n</script>",
                rewrite("window.GridView.registerRenderer('vue_named', function vueDraw() { return 1; });")
            ),
            1,
        );
        workspace.write("observed.vue", &vue);

        // Each valid HTML fragment stays below the extraction limit; two together exceed it.
        for (path, counts, key) in [
            ("over_limit.html", [65, 65], "over_limit"),
            ("invalid_fragment.html", [129, 1], "invalid_fragment"),
            ("invalid_later.html", [1, 129], "invalid_fragment"),
        ] {
            let html = counts
                .into_iter()
                .enumerate()
                .map(|(island, count)| {
                    format!(
                        "<script>\nfunction bound{island}() {{ return 1; }}\n{}</script>\n",
                        (0..count)
                            .map(|_| format!(
                                "window.GridView.registerRenderer('{key}', bound{island});\n"
                            ))
                            .collect::<String>()
                    )
                })
                .collect::<String>();
            workspace.write(path, &rewrite(&html));
        }
    }

    let fixture = OperationFixture {
        workspace,
        cold_references: ColdFilesystemReferenceCache::default(),
        case,
        rewrite,
        manifest: ".forge/frameworks/finite.toml",
        label: format!("{name} renamed={renamed}"),
    };
    scenario(fixture);
}

pub(super) fn manifest_lifecycle(
    fixture: &OperationFixture,
    mut verify: impl FnMut(&str, bool, ChangedTarget),
) {
    let mut inactive = fixture.case.pack.clone();
    for array in ["semantic_rules", "syntax_rules"] {
        for rule in inactive[array].as_array_mut().unwrap() {
            rule["activation"] = json!([{ "kind": "package", "name": "absent-runtime" }]);
        }
    }
    let mut replaced = fixture.case.pack.clone();
    replaced["replace"] = true.into();
    replaced["semantic_rules"] = json!([]);
    replaced["syntax_rules"] = json!([]);
    for (phase, manifest, active) in [
        ("deactivated", inactive, false),
        ("restored", fixture.case.pack.clone(), true),
        ("replaced", replaced, false),
        ("restored again", fixture.case.pack.clone(), true),
    ] {
        fixture.apply_manifest(&manifest);
        verify(phase, active, None);
    }

    fixture.remove_input(".forge/frameworks/finite.toml");
    verify("manifest removed", false, None);
    fixture.apply_manifest(&fixture.case.pack);
    verify("manifest restored", true, None);
}

pub(super) fn rejected(
    fixture: &OperationFixture,
    phase: &str,
    set_invalid_boundary: impl FnOnce(&mut Value),
    additional_invalids: Vec<Value>,
) {
    let case = &fixture.case;
    let array = if case.pack["semantic_rules"].as_array().unwrap().is_empty() {
        "syntax_rules"
    } else {
        "semantic_rules"
    };
    let mut unknown = case.pack.clone();
    unknown[array][0]["operation"]["unchecked"] = true.into();
    let mut boundary = case.pack.clone();
    set_invalid_boundary(&mut boundary[array][0]["operation"]);
    let before = publication_commitments(&fixture.read_connection());
    for invalid in [unknown, boundary].into_iter().chain(additional_invalids) {
        fixture.install_manifest(&invalid);
        assert!(
            contextunity_forge_mcp::engine::pipeline::build_with_cache_dir(
                fixture.workspace.root(),
                &fixture.workspace.db(),
                None,
                &fixture.workspace.path(".forge/extraction-cache"),
            )
            .is_err(),
            "{phase}: unknown field or invalid boundary must fail closed"
        );
        let conn = fixture.read_connection();
        commitments::verify(&conn).unwrap();
        assert_eq!(
            publication_commitments(&conn),
            before,
            "{phase}: rejected configuration cannot publish a different graph"
        );
    }
}

pub(super) fn durable_payloads<'a>(
    durable_facts: &'a BTreeMap<String, Value>,
    vector: &str,
) -> Vec<&'a Value> {
    durable_facts
        .values()
        .flat_map(|facts| facts["nodes"].as_array().unwrap().iter())
        .flat_map(|node| {
            node["details"]["value_flow"][vector]
                .as_array()
                .into_iter()
                .flatten()
        })
        .collect()
}

pub(super) struct EdgeQuery<'a> {
    pub source: (&'a str, &'a str),
    pub target: (&'a str, &'a str),
    pub alternate_target: Option<&'a str>,
    pub kind: &'a str,
    pub source_id: bool,
}

pub(super) fn matching_edge_ids(conn: &Connection, query: EdgeQuery<'_>) -> Vec<(String, String)> {
    conn.prepare_cached("SELECT s.id,d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path=?1 AND (s.name LIKE ?2 OR (?7 AND s.id=?2)) AND e.kind=?3 AND dp.path=?4 AND (d.name LIKE ?5 OR d.name LIKE ?6) ORDER BY s.id,d.id")
        .unwrap()
        .query_map(params![query.source.0, query.source.1, query.kind, query.target.0, query.target.1, query.alternate_target.unwrap_or(query.target.1), query.source_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

pub(super) fn ordinary_call_count(conn: &Connection) -> usize {
    query_count_usize(
        conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",
        [],
    )
}

pub(super) fn targets_on_line(
    conn: &Connection,
    path: &str,
    line: usize,
    kind: &str,
) -> Vec<String> {
    query_rows_params(
        conn,
        "SELECT d.id FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes d ON d.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=?2 AND e.kind=?3 ORDER BY d.id",
        params![path, line, kind],
    )
}

pub(super) struct OperationFixture {
    workspace: Workspace,
    cold_references: ColdFilesystemReferenceCache,
    pub case: Case,
    pub rewrite: Box<dyn Fn(&str) -> String>,
    manifest: &'static str,
    pub label: String,
}

impl OperationFixture {
    pub(super) fn install_manifest(&self, value: &Value) {
        self.workspace.write(
            (self.rewrite)(self.manifest),
            &(self.rewrite)(&toml::to_string(value).unwrap()),
        );
    }

    pub(super) fn build(&self) -> Value {
        self.workspace.build()
    }

    pub(super) fn apply_manifest(&self, value: &Value) -> Value {
        self.install_manifest(value);
        self.workspace.delta(&[(self.rewrite)(self.manifest)])
    }

    pub(super) fn apply_source_edit(&self, path: &str, source: &str) -> Value {
        self.apply_source_edits(&[(path, source)])
    }

    pub(super) fn apply_physical_source_edit(&self, path: &str, source: &str) -> Value {
        let path = (self.rewrite)(path);
        self.workspace.write(&path, source);
        self.workspace.delta(&[path])
    }

    pub(super) fn append_noop_comment(&self, path: &str) -> Value {
        let source = self.read_source(path);
        let comment = if path.ends_with(".py") {
            "\n# cached provider replay\n"
        } else {
            "\n// cached provider replay\n"
        };
        self.apply_physical_source_edit(path, &format!("{source}{comment}"))
    }

    pub(super) fn apply_source_edits(&self, edits: &[(&str, &str)]) -> Value {
        let paths: Vec<_> = edits
            .iter()
            .map(|(path, source)| {
                let path = (self.rewrite)(path);
                self.workspace.write(&path, &(self.rewrite)(source));
                path
            })
            .collect();
        self.workspace.delta(&paths)
    }

    pub(super) fn remove_input(&self, path: &str) -> Value {
        let path = (self.rewrite)(path);
        self.workspace.remove(&path);
        self.workspace.delta(&[path])
    }

    pub(super) fn read_source(&self, path: &str) -> String {
        std::fs::read_to_string(self.workspace.path((self.rewrite)(path))).unwrap()
    }

    pub(super) fn read_bytes(&self, path: &str) -> Vec<u8> {
        std::fs::read(self.workspace.path((self.rewrite)(path))).unwrap()
    }

    pub(super) fn read_connection(&self) -> Connection {
        self.workspace.open()
    }

    pub(super) fn read_phase<'a>(&'a self, phase: &'a str) -> OperationPhase<'a> {
        let conn = self.read_connection();
        let durable_facts = self
            .case
            .vector
            .map(|_| all_local_facts(&conn).into_iter().collect())
            .unwrap_or_default();
        OperationPhase {
            conn,
            durable_facts,
            case: &self.case,
            phase,
            manifest_present: self.workspace.path((self.rewrite)(self.manifest)).exists(),
            rewrite: self.rewrite.as_ref(),
        }
    }

    pub(super) fn observe_cold(&self) -> ColdObservations<PersistedRows> {
        self.cold_references
            .observe(&self.workspace, self.workspace.open(), persisted_rows)
    }
}
