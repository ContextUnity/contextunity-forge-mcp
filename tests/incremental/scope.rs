#![cfg(any(feature = "lang-python", feature = "lang-rust", feature = "lang-vue"))]

use contextunity_forge_mcp::{
    core::commitments,
    db::{reader, writer::delta},
};
use rusqlite::Connection;
use std::fs;

use crate::common::Workspace;

type FactObservations = Vec<(String, Vec<u8>)>;
type ParityObservations = Vec<(&'static str, Vec<String>, Vec<String>)>;

fn cold_comparison(
    workspace: &Workspace,
) -> (bool, bool, i64, FactObservations, ParityObservations) {
    let receipt = workspace.db().with_extension("sqlite.verified.json");
    if receipt.exists() {
        fs::remove_file(receipt).unwrap();
    }
    let incremental = workspace.open();
    let incremental_verified = commitments::verify(&incremental).is_ok();
    let dangling: i64 = incremental.query_row("SELECT count(*) FROM edges e LEFT JOIN nodes s ON s.node_hash=e.src_hash LEFT JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.id IS NULL OR d.id IS NULL", [], |r| r.get(0)).unwrap();

    let cold_path = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold_path);
    let cold = reader::open(&cold_path, workspace.root()).unwrap();
    let cold_verified = commitments::verify(&cold).is_ok();
    let mut facts = Vec::new();
    for conn in [&incremental, &cold] {
        let mut statement = conn
            .prepare("SELECT path,facts_blob FROM local_facts")
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .unwrap();
        for row in rows {
            let (path, facts_blob) = row.unwrap();
            facts.push((path, facts_blob));
        }
    }
    let mut parity = Vec::new();
    for sql in [
        "SELECT p.path||'|'||n.id||'|'||n.kind||'|'||np.path FROM nodes n JOIN path_dictionary p ON p.path_id=n.owner_path_id JOIN path_dictionary np ON np.path_id=n.path_id ORDER BY p.path,n.id",
        "SELECT path||'|'||hex(facts_blob) FROM local_facts ORDER BY path",
        "SELECT p.path||'|'||o.ordinal||'|'||s.id||'|'||d.id||'|'||o.kind||'|'||o.line||'|'||v.evidence FROM edge_occurrences o JOIN path_dictionary p ON p.path_id=o.owner_id JOIN nodes s ON s.node_hash=o.src_hash JOIN nodes d ON d.node_hash=o.dst_hash JOIN coverage_evidence v ON v.evidence_id=o.confidence_id ORDER BY p.path,o.ordinal",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        "SELECT p.path||'|'||c.domain||'|'||hex(c.digest) FROM domain_commitments c JOIN path_dictionary p ON p.path_id=c.owner_id ORDER BY p.path,c.domain",
    ] {
        parity.push((sql, strings(&incremental, sql), strings(&cold, sql)));
    }
    drop(cold);
    drop(incremental);
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = std::path::PathBuf::from(format!("{}{suffix}", cold_path.display()));
        if sidecar.exists() {
            fs::remove_file(sidecar).unwrap();
        }
    }
    fs::remove_file(&cold_path).unwrap();
    let receipt = cold_path.with_extension("sqlite.verified.json");
    if receipt.exists() {
        fs::remove_file(receipt).unwrap();
    }
    (incremental_verified, cold_verified, dangling, facts, parity)
}
fn strings(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
#[cfg(feature = "lang-rust")]
fn outside_inventory_paths_are_not_admitted_as_deletions() {
    let w = Workspace::new();
    w.write("source.rs", "pub fn source() {}\n");
    w.write("notes.txt", "excluded\n");
    w.build();
    let error = delta(w.root(), &w.db(), &["notes.txt".into()]).unwrap_err();
    assert!(
        format!("{error:#}").contains("modified path outside admitted inventory: notes.txt"),
        "{error:#}"
    );
    w.write(".gitignore", "source.rs\n");
    let error = delta(w.root(), &w.db(), &["source.rs".into()]).unwrap_err();
    assert!(
        format!("{error:#}").contains("modified path outside admitted inventory: source.rs"),
        "{error:#}"
    );
}

#[test]
#[cfg(feature = "lang-rust")]
fn delta_exact_spelling_and_alias_renames_match_cold_inventory() {
    let w = Workspace::new();
    let linked = Workspace::new();
    let linked_alias_root = Workspace::new();
    let cases = [
        ("case/Foo.rs", "case/foo.rs"),
        ("unicode/caf\u{e9}.rs", "unicode/cafe\u{301}.rs"),
        ("reserved/a #%.rs", "reserved/b #%.rs"),
        ("old.rs", "a #%.rs"),
        ("Parent/source.rs", "parent/source.rs"),
        ("alias/Foo.rs", "alias/foo.rs"),
    ];
    for (old, _) in cases {
        w.write(old, "pub fn renamed() {}\n");
    }
    linked.write("Parent/Foo.rs", "pub fn parent() {}\n");
    linked.write("Foo.rs", "pub fn leaf() {}\n");
    linked_alias_root.write("Foo.rs", "pub fn linked() {}\n");
    fs::create_dir_all(w.path(".forge")).unwrap();
    #[cfg(unix)]
    let (root_alias, linked_alias) = {
        use std::os::unix::fs::symlink;
        let root_alias = w.path(".forge/root-alias");
        let linked_alias = w.path(".forge/linked-alias");
        symlink(w.root(), &root_alias).unwrap();
        symlink(linked_alias_root.root(), &linked_alias).unwrap();
        (root_alias, linked_alias)
    };
    #[cfg(not(unix))]
    let linked_alias = linked_alias_root.root().to_path_buf();
    w.write(
        "forge-mcp.yaml",
        &format!(
        "linked_workspaces:\n  - name: library\n    path: {}\n  - name: aliased\n    path: {}\n",
        linked.root().display(), linked_alias.display()
    ),
    );
    w.build();

    let mut renamed_files = Vec::new();
    for (old, new) in cases {
        let old_dir = std::path::Path::new(old).parent().unwrap();
        let new_dir = std::path::Path::new(new).parent().unwrap();
        let old_name = fs::read_dir(w.path(old_dir))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter_map(|name| name.into_string().ok())
            .find(|name| name.ends_with(".rs"))
            .unwrap();
        if old.starts_with("Parent/") {
            fs::rename(w.path("Parent"), w.path("parent")).unwrap();
        } else {
            fs::rename(w.path(old), w.path(new)).unwrap();
        }
        let new_key = if old.starts_with("Parent/") {
            new.to_owned()
        } else {
            let name = fs::read_dir(w.path(new_dir))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter_map(|name| name.into_string().ok())
                .find(|name| name.ends_with(".rs"))
                .unwrap();
            new_dir.join(name).to_string_lossy().into_owned()
        };
        let old_key = old_dir.join(old_name).to_string_lossy().into_owned();
        if old_key != new_key {
            #[cfg(unix)]
            if old.starts_with("alias/") {
                delta(
                    &root_alias,
                    &w.db(),
                    &[old_key.clone().into(), new_key.clone().into()],
                )
                .unwrap();
            } else {
                w.delta(&[&old_key, &new_key]);
            }
            #[cfg(not(unix))]
            w.delta(&[&old_key, &new_key]);
            let paths = strings(&w.open(), "SELECT path FROM files");
            assert!(paths.contains(&new_key), "{paths:?}");
            assert!(!paths.contains(&old_key), "{paths:?}");
        }
        if new == "a #%.rs" {
            let uri = w.file_uri("a #%.rs", Some(1));
            assert!(uri.ends_with("/a%20%23%25.rs#L1"), "{uri}");
        }
        if old != "Parent/source.rs" && !old.starts_with("alias/") {
            renamed_files.push(new_key);
        }
    }
    fs::rename(linked.path("Parent"), linked.path("parent")).unwrap();
    fs::rename(linked.path("Foo.rs"), linked.path("foo.rs")).unwrap();
    fs::rename(
        linked_alias_root.path("Foo.rs"),
        linked_alias_root.path("foo.rs"),
    )
    .unwrap();
    w.delta(&[
        "[library]/Parent/Foo.rs",
        "[library]/parent/Foo.rs",
        "[library]/Foo.rs",
        "[library]/foo.rs",
        "[aliased]/Foo.rs",
        "[aliased]/foo.rs",
    ]);
    let paths = strings(&w.open(), "SELECT path FROM files ORDER BY path");
    for expected in [
        "parent/source.rs",
        "alias/foo.rs",
        "[library]/foo.rs",
        "[library]/parent/Foo.rs",
        "[aliased]/foo.rs",
    ] {
        assert!(paths.iter().any(|path| path == expected), "{paths:?}");
    }
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
    for path in &renamed_files {
        fs::remove_file(w.path(path)).unwrap();
    }
    let removed: Vec<_> = renamed_files.iter().map(String::as_str).collect();
    w.delta(&removed);
    let paths = strings(&w.open(), "SELECT path FROM files");
    for path in &renamed_files {
        assert!(!paths.contains(path), "{paths:?}");
    }
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
    #[cfg(unix)]
    {
        fs::remove_file(root_alias).unwrap();
        fs::remove_file(linked_alias).unwrap();
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn distinct_filesystem_spellings_coexist_through_delta() {
    let w = Workspace::new();
    w.write("Foo.rs", "pub fn upper() {}\n");
    w.write("caf\u{e9}.rs", "pub fn nfc() {}\n");
    w.build();
    if contextunity_forge_mcp::core::fs::is_case_insensitive_volume(w.root()) {
        eprintln!("case coexistence unavailable on case-insensitive volume");
    } else {
        w.write("foo.rs", "pub fn lower() {}\n");
        w.delta(&["foo.rs"]);
        assert_eq!(
            strings(
                &w.open(),
                "SELECT path FROM files WHERE path IN ('Foo.rs', 'foo.rs') ORDER BY path"
            ),
            ["Foo.rs", "foo.rs"]
        );
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
    w.write("cafe\u{301}.rs", "pub fn nfd() {}\n");
    let names: Vec<_> = fs::read_dir(w.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("caf"))
        .collect();
    if names.len() == 1 {
        eprintln!("Unicode coexistence unavailable on normalization-folding volume");
        return;
    }
    assert_eq!(names.len(), 2);
    w.delta(&["cafe\u{301}.rs"]);
    let paths = strings(&w.open(), "SELECT path FROM files ORDER BY path");
    for path in ["caf\u{e9}.rs", "cafe\u{301}.rs"] {
        assert!(paths.iter().any(|p| p == path), "{paths:?}");
    }
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}

#[test]
#[cfg(all(feature = "lang-rust", unix))]
fn delta_rejects_unlisted_changes_and_symlink_boundaries() {
    use std::os::unix::fs::symlink;
    let w = Workspace::new();
    w.write("source.rs", "pub fn before() {}\n");
    w.build();
    w.write("source.rs", "pub fn changed() {}\n");
    w.write("other.rs", "pub fn other() {}\n");
    let error = delta(w.root(), &w.db(), &["other.rs".into()]).unwrap_err();
    assert!(format!("{error:#}").contains("unlisted source change: source.rs"));
    fs::remove_file(w.path("other.rs")).unwrap();
    w.write("source.rs", "pub fn before() {}\n");
    symlink(w.path("source.rs"), w.path("leaf.rs")).unwrap();
    let error = delta(w.root(), &w.db(), &["leaf.rs".into()]).unwrap_err();
    assert!(format!("{error:#}").contains("leaf.rs"), "{error:#}");
    fs::remove_file(w.path("leaf.rs")).unwrap();
    symlink(std::env::temp_dir(), w.path("escape")).unwrap();
    let error = delta(w.root(), &w.db(), &["escape/out.rs".into()]).unwrap_err();
    assert!(format!("{error:#}").contains("escape/out.rs"), "{error:#}");
    fs::remove_file(w.path("escape")).unwrap();
}

#[test]
fn document_mtime_json_roundtrip_preserves_commitment_input() {
    // An observed timestamp that parsed one ULP lower without float_roundtrip.
    let mtime = 1_790_494_491.067_507_7_f64;
    let encoded = serde_json::to_string(&mtime).unwrap();
    let decoded: f64 = serde_json::from_str(&encoded).unwrap();
    assert_eq!(mtime.to_bits(), decoded.to_bits(), "{encoded}");
}

#[test]
#[cfg(feature = "lang-python")]
fn one_file_edit_does_not_rewrite_unrelated_component_owners() {
    let w = Workspace::new();
    for component in 0..48 {
        w.write(
            format!("part{component:02}/a.py"),
            &format!("def value_{component}():\n    return {component}\n"),
        );
        w.write(
            format!("part{component:02}/b.py"),
            &format!("def extra_{component}():\n    return {component}\n"),
        );
    }
    w.build();
    let sql =
        "SELECT id||'|'||node_id FROM nodes WHERE path_id NOT IN(SELECT path_id FROM path_dictionary WHERE path IN('part23','part23/b.py')) ORDER BY id";
    let before = strings(&w.open(), sql);
    w.write("part23/b.py", "def extra_23():\n    return 999\n");
    let report = w.delta(&["part23/b.py"]);
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(before, strings(&w.open(), sql));
    assert_eq!(report["reparsed_files"], 1);
    assert_eq!(report["loaded_fact_files"], 0);
    assert_eq!(report["rewritten_files"], 1);
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn src_namespace_edit_relinks_its_consumer_without_invalidating_siblings() {
    let w = Workspace::new();
    let provider = "services/shield/src/contextunity/shield/audit.py";
    w.write(provider, "def log_event(): return 1\n");
    w.write(
        "services/client/src/contextunity/client/main.py",
        "from contextunity.shield.audit import log_event\ndef run(): return log_event()\n",
    );
    for n in 0..20 {
        w.write(
            format!("services/other/src/contextunity/other/helper_{n}.py"),
            &format!("def helper_{n}(): return {n}\n"),
        );
        w.write(
            format!("services/other/src/contextunity/other/view_{n}.py"),
            &format!("from contextunity.other.helper_{n} import helper_{n}\ndef view(): return helper_{n}()\n"),
        );
    }
    w.build();
    w.write(provider, "def log_event_new(): return 2\n");
    let report = w.delta(&[provider]);
    assert_eq!(report["reparsed_files"], 1, "{report}");
    assert_eq!(report["rewritten_files"], 1, "{report}");
    assert_eq!(report["affected_owners"], 2, "{report}");
    assert_eq!(report["loaded_fact_files"], 1, "{report}");
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn python_import_frontier_uses_semicolon_and_continuation_facts() {
    let w = Workspace::new();
    w.write(
        "provider.py",
        "def api(): return 1\ndef service(): return 1\n",
    );
    w.write("pkg/__init__.py", "");
    w.write("pkg/nested.py", "def nested_api(): return 1\n");
    w.write(
        "middle_module.py",
        "import provider; marker = 0\nimport provider as renamed\nimport pkg.nested\n",
    );
    w.write(
        "module_consumer.py",
        "from middle_module import provider, renamed\ndef use_module(): return provider.api()\ndef use_renamed_module(): return renamed.api()\n",
    );
    w.write(
        "dotted_consumer.py",
        "from middle_module import pkg\ndef use_dotted_module(): return pkg.nested.nested_api()\n",
    );
    w.write(
        "middle_alias.py",
        "# facts should preserve this logical import across its line continuation\nfrom provider import api as chosen, \\\n    service as other\nmarker = 0\n",
    );
    w.write(
        "alias_consumer.py",
        "from middle_alias import chosen\ndef use_alias(): return chosen()\n",
    );
    w.build();

    w.write(
        "provider.py",
        "def api(): return 2\ndef service(): return 3\ndef added(): return 4\n",
    );
    let report = w.delta(&["provider.py"]);
    assert!(
        report["loaded_fact_files"].as_u64().unwrap_or_default() >= 2,
        "both intermediate import owners should be evaluated from indexed facts: {report}"
    );
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }

    w.write(
        "pkg/nested.py",
        "def nested_api(): return 2\ndef added(): return 3\n",
    );
    let dotted_report = w.delta(&["pkg/nested.py"]);
    assert!(
        dotted_report["loaded_fact_files"]
            .as_u64()
            .unwrap_or_default()
            >= 2,
        "plain dotted module import should expand from indexed facts: {dotted_report}"
    );
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn indexed_reexport_frontier_matches_cold_build() {
    let w = Workspace::new();
    w.write("provider.py", "class Api: pass\n");
    w.write("middle.py", "import provider\nExportedApi = provider.Api\n");
    w.write(
        "consumer.py",
        "from middle import ExportedApi\ndef consume() -> ExportedApi: return ExportedApi()\n",
    );
    w.build();

    w.write(
        "provider.py",
        "class Api:\n    def value(self): return 1\nclass Added: pass\n",
    );
    let report = w.delta(&["provider.py"]);
    assert_eq!(
        report["loaded_fact_files"], 2,
        "the direct import owner and its indexed re-export consumer should be evaluated: {report}"
    );
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn python_missing_module_export_facts_expand_conservatively() {
    let w = Workspace::new();
    w.write("provider.py", "def value(): return 1\n");
    w.write(
        "unexposed_alias.py",
        "import provider as _provider\nmarker = 0\n",
    );
    w.write(
        "unexposed_consumer.py",
        "from unexposed_alias import marker\ndef call_constant(): return marker\n",
    );
    w.build();
    w.write(
        "provider.py",
        "def value(): return 2\ndef added(): return 3\n",
    );
    let report = w.delta(&["provider.py"]);
    assert_eq!(
        report["loaded_fact_files"],
        2,
        "the direct import owner and its consumer should be evaluated because module export facts are unavailable: {report}"
    );
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn linked_component_transfer_preserves_unmodified_containment_edges() {
    let w = Workspace::new();
    let linked = Workspace::new();
    w.write("local/a.py", "def local(): pass\n");
    linked.write("src/b.py", "def bravo(): pass\n");
    linked.write("src/c.py", "def charlie(): pass\n");
    w.write("forge-mcp.yaml", &format!("roots: [local]\nlinked_workspaces:\n  - name: linked\n    path: {}\n    roots: [src]\n", linked.root().display()));
    w.build();
    let untouched_sql = "SELECT id||'|'||node_id FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='local/a.py') ORDER BY id";
    let before = strings(&w.open(), untouched_sql);
    linked.write("src/a.py", "def alpha(): pass\n");
    let report = w.delta(&["[linked]/src/a.py"]);
    assert_eq!(report["rewritten_files"], 2, "{report}");
    assert_eq!(report["loaded_fact_files"], 1, "{report}");
    assert_eq!(before, strings(&w.open(), untouched_sql));
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
    for path in ["src/a.py", "src/b.py", "src/c.py"] {
        linked.remove(path);
        w.delta(&[&format!("[linked]/{path}")]);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
}

#[test]
#[cfg(feature = "lang-python")]
fn provider_addition_ambiguity_and_removal_relink_consumers_and_docs() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "from lib import provide\ndef consume():\n    return provide()\n",
    );
    w.write("docs/api.md", "# API\nUse `provide` to read the value.\n");
    w.write("other/a.py", "def untouched(): pass\n");
    w.build();
    for (path, source) in [
        ("lib.py", "def provide(): return 1\n"),
        ("lib/__init__.py", "def provide(): return 2\n"),
    ] {
        w.write(path, source);
        w.delta(&[path]);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
    assert_eq!(strings(&w.open(), "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='provide' ORDER BY line"), ["ambiguous", "unresolved"]);
    for path in ["lib/__init__.py", "lib.py"] {
        w.remove(path);
        w.delta(&[path]);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
    assert_eq!(
        strings(
            &w.open(),
            "SELECT (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='documents'"
        ),
        Vec::<String>::new()
    );
}

#[test]
#[cfg(feature = "lang-python")]
fn component_representative_mutations_match_cold_builds() {
    let w = Workspace::new();
    w.write("part/b.py", "def bravo(): pass\n");
    w.write("part/c.py", "def charlie(): pass\n");
    w.write("other/a.py", "def unrelated(): pass\n");
    w.build();
    for (changed, source) in [
        ("part/c.py", "def charlie(): return 1\n"),
        ("part/b.py", "def bravo(): return 2\n"),
        ("part/a.py", "def alpha(): pass\n"),
    ] {
        w.write(changed, source);
        w.delta(&[changed]);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
    fs::rename(w.path("part/a.py"), w.path("part/d.py")).unwrap();
    w.delta(&["part/a.py", "part/d.py"]);
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
    for path in ["part/b.py", "part/c.py", "part/d.py"] {
        w.remove(path);
        w.delta(&[path]);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
    let conn = w.open();
    assert_eq!(
        strings(
            &conn,
            "SELECT id FROM nodes WHERE kind='component' ORDER BY id"
        ),
        ["component:other"]
    );
}

#[cfg(feature = "lang-rust")]
fn shared_aggregate_workspace() -> Workspace {
    let w = Workspace::new();
    w.write("src/defs.rs", "pub struct Thing; pub trait Trait {}\n");
    w.write(
        "impl_a/a.rs",
        "use src::defs as d; use src::defs::Trait as X; impl X for d::Thing {}\n",
    );
    w.write(
        "impl_b/b.rs",
        "use src::defs as e; use src::defs::Trait as Y; impl Y for e::Thing {}\n",
    );
    w.build();
    assert_aggregate(&w, "impl_a/a.rs", 2);
    w
}

#[test]
#[cfg(feature = "lang-rust")]
fn dyn_trait_dispatch_add_change_and_remove_impls_match_cold_builds() {
    let candidates = "SELECT d.id FROM edges e JOIN nodes s ON s.node_hash=e.src_hash \
        JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='caller' \
        AND e.kind='possible_calls' ORDER BY d.id";
    for (operation, initial, replacement, expected) in [
        ("add", None, Some("use crate::defs::Work; pub struct Beta; impl Work for Beta { fn run(&self) {} }\n"), 2),
        ("change", Some("use crate::defs::Work; pub struct Beta; impl Work for Beta { fn run(&self) {} }\n"), Some("use crate::defs::Other; pub struct Beta; impl Other for Beta { fn run(&self) {} }\n"), 1),
        ("remove", Some("use crate::defs::Work; pub struct Beta; impl Work for Beta { fn run(&self) {} }\n"), None, 1),
    ] {
        let w = Workspace::new();
        w.write("src/defs.rs", "pub trait Work { fn run(&self); } pub trait Other { fn run(&self); }\n");
        w.write("src/alpha.rs", "use crate::defs::Work; pub struct Alpha; impl Work for Alpha { fn run(&self) {} }\n");
        w.write("src/caller.rs", "use crate::defs::Work; pub fn caller(value: &dyn Work) { value.run(); }\n");
        if let Some(source) = initial {
            w.write("src/beta.rs", source);
        }
        w.build();
        let initial_count = if initial.is_some() { 2 } else { 1 };
        assert_eq!(strings(&w.open(), candidates).len(), initial_count, "{operation}: fixture must resolve cross-file trait candidates");
        match replacement {
            Some(source) => w.write("src/beta.rs", source),
            None => w.remove("src/beta.rs"),
        }
        let report = delta(w.root(), &w.db(), &["src/beta.rs".into()]).unwrap();
        let cold_path = w.path(".forge/cold-dispatch.sqlite");
        w.build_to(&cold_path);
        let cold = reader::open(&cold_path, w.root()).unwrap();
        let expected_candidates = strings(&cold, candidates);
        assert_eq!(expected_candidates.len(), expected, "{operation}: cold build fixture must retain bounded dispatch");
        assert_eq!(strings(&w.open(), candidates), expected_candidates, "{operation}: unchanged caller must match cold-build candidate edges; delta={report}");
        {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"), "{path}: newly written local facts use the typed binary format");
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
    }
}

#[cfg(feature = "lang-rust")]
fn assert_aggregate(w: &Workspace, owner: &str, count: usize) {
    let actual = strings(
        &w.open(),
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||occurrence_count FROM edges WHERE kind='implements'",
    );
    let expected = if count == 0 {
        Vec::new()
    } else {
        vec![format!("{owner}|{count}")]
    };
    assert_eq!(actual, expected);
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_deletion_reseals_transferred_and_unchanged_owners() {
    for (deleted, surviving) in [
        ("impl_a/a.rs", "impl_b/b.rs"),
        ("impl_b/b.rs", "impl_a/a.rs"),
    ] {
        let w = shared_aggregate_workspace();
        w.remove(deleted);
        let report = w.delta(&[deleted]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["reparsed_files"], 0, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 0, "{report}");
        assert_aggregate(&w, surviving, 1);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
        w.remove(surviving);
        w.delta(&[surviving]);
        assert_aggregate(&w, "", 0);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_addition_reseals_prior_and_resulting_owners() {
    for (added, owner) in [
        ("impl_0/new.rs", "impl_0/new.rs"),
        ("impl_z/new.rs", "impl_a/a.rs"),
    ] {
        let w = shared_aggregate_workspace();
        w.write(
            added,
            "use src::defs as f; use src::defs::Trait as Z; impl Z for f::Thing {}\n",
        );
        let report = w.delta(&[added]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["reparsed_files"], 1, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, owner, 3);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_edit_reseals_transferred_and_unchanged_owners() {
    for (edited, surviving) in [
        ("impl_a/a.rs", "impl_b/b.rs"),
        ("impl_b/b.rs", "impl_a/a.rs"),
    ] {
        let w = shared_aggregate_workspace();
        w.write(edited, "pub fn placeholder() {}\n");
        let report = w.delta(&[edited]);
        assert_eq!(report["affected_owners"], 1, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, surviving, 1);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
}

#[test]
#[cfg(feature = "lang-rust")]
fn shared_aggregate_rename_reseals_prior_and_resulting_owners() {
    for (old, new, owner) in [
        ("impl_a/a.rs", "impl_z/z.rs", "impl_b/b.rs"),
        ("impl_b/b.rs", "impl_0/z.rs", "impl_0/z.rs"),
    ] {
        let w = shared_aggregate_workspace();
        fs::create_dir_all(w.path(new).parent().unwrap()).unwrap();
        fs::rename(w.path(old), w.path(new)).unwrap();
        let report = w.delta(&[old, new]);
        assert_eq!(report["affected_owners"], 2, "{report}");
        assert_eq!(report["loaded_fact_files"], 0, "{report}");
        assert_eq!(report["rewritten_files"], 1, "{report}");
        assert_aggregate(&w, owner, 2);
        {
            let (incremental_verified, cold_verified, dangling, facts, parity) =
                cold_comparison(&w);
            assert!(incremental_verified);
            assert!(cold_verified);
            assert_eq!(dangling, 0);
            for (path, facts_blob) in facts {
                assert!(
                    facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                    "{path}: newly written local facts use the typed binary format"
                );
                reader::decode_local_facts(&facts_blob).unwrap();
                assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
            }
            for (sql, incremental, cold) in parity {
                assert_eq!(incremental, cold, "{sql}");
            }
        }
    }
}

#[test]
#[cfg(feature = "lang-vue")]
fn vue_setup_body_edits_refresh_calls_and_preserve_consumer_contracts() {
    let w = Workspace::new();
    w.write(
        "Provider.vue",
        "<script setup>\nexport function run() { defineProps(); return 1; }\n</script>\n",
    );
    w.write(
        "consumer.ts",
        "import { run } from './Provider.vue';\nrun();\n",
    );
    w.build();
    let consumer_hashes = |conn: &Connection| {
        strings(conn,
        "SELECT domain||'|'||hex(digest) FROM domain_commitments WHERE owner_id=(SELECT path_id FROM path_dictionary WHERE path='consumer.ts') ORDER BY domain")
    };
    let before = consumer_hashes(&w.open());
    assert_eq!(
        strings(
            &w.open(),
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.ts' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='run' AND line=2"
        ),
        ["resolved"]
    );
    w.write(
        "Provider.vue",
        "<script setup>\nexport function run() {\n  defineOptions();\n  return 1;\n}\n</script>\n",
    );
    let report = w.delta(&["Provider.vue"]);
    assert_eq!(report["affected_owners"], 1, "{report}");
    assert_eq!(report["reparsed_files"], 1, "{report}");
    assert_eq!(consumer_hashes(&w.open()), before);
    assert_eq!(strings(&w.open(), "SELECT status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='Provider.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='defineOptions'"), ["external|builtin:vue_macro; rule:vue-runtime:component-macros built-in: defineOptions"]);
    {
        let (incremental_verified, cold_verified, dangling, facts, parity) = cold_comparison(&w);
        assert!(incremental_verified);
        assert!(cold_verified);
        assert_eq!(dangling, 0);
        for (path, facts_blob) in facts {
            assert!(
                facts_blob.starts_with(b"FORGE-LOCAL-FACTS\0\x03"),
                "{path}: newly written local facts use the typed binary format"
            );
            reader::decode_local_facts(&facts_blob).unwrap();
            assert!(!commitments::hash(&facts_blob).is_empty(), "{path}");
        }
        for (sql, incremental, cold) in parity {
            assert_eq!(incremental, cold, "{sql}");
        }
    }
}
