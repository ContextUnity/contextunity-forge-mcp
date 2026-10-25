use super::*;

mod configured;
use configured::*;

#[test]
fn framework_manifest_class_attributes_follow_effective_rules_and_delta() {
    configured_syntax_rules_follow_effective_rules_and_delta();
    configured_template_rules_follow_effective_rules_and_delta();
    configured_di_origins_follow_effective_rules_and_delta();
    configured_component_and_receiver_rules_follow_effective_rules_and_delta();
    configured_component_macros_follow_effective_rules_and_delta();
    configured_router_prefixes_are_static_unique_and_recursive();
    for (pack_name, file_attribute, symbol_attribute, directory, module, export) in [
        (
            "kiln",
            "panel_asset",
            "entity_type",
            "screens/",
            "kiln.paint",
            "emit",
        ),
        (
            "orchard",
            "markup_asset",
            "record_kind",
            "presentations/",
            "orchard.ink",
            "draw",
        ),
    ] {
        let workspace = Workspace::new();
        let manifest_path = format!(".forge/frameworks/{pack_name}.toml");
        let project = "[project]\nname='attribute-rules'\n";
        workspace.write("pyproject.toml", project);
        workspace.write(
            "app/models.py",
            "class Record: pass\nclass Duplicate: pass\n",
        );
        workspace.write("other/models.py", "class Duplicate: pass\n");
        let source = format!(
            "from app.models import Record, Missing, Duplicate\nfrom other.models import Duplicate\nfrom {module} import {export} as paint\n\nclass Unique:\n    {file_attribute} = 'catalog/card.html'\n    {symbol_attribute} = Record\n\nclass MissingFile:\n    {file_attribute} = 'absent.html'\n    {symbol_attribute} = Record\n\nclass DuplicateFile:\n    {file_attribute} = 'catalog/duplicate.html'\n    {symbol_attribute} = Record\n\nclass MissingSymbol:\n    {file_attribute} = 'catalog/card.html'\n    {symbol_attribute} = Missing\n\nclass DuplicateSymbol:\n    {file_attribute} = 'catalog/card.html'\n    {symbol_attribute} = Duplicate\n\nclass Dynamic:\n    {file_attribute} = choose_asset()\n    {symbol_attribute} = choose_type()\n\ndef helper(): return 1\ndef ordinary(): return helper()\ndef page(): return paint('catalog/card.html')\ndef shadowed(paint): return paint('catalog/card.html')\n"
        );
        workspace.write("app/views.py", &source);
        let target = format!("app/{directory}catalog/card.html");
        workspace.write(&target, "<p>Unique</p>\n");
        for owner in ["app", "other"] {
            workspace.write(
                format!("{owner}/{directory}catalog/duplicate.html"),
                "<p>Duplicate</p>\n",
            );
        }
        // A suffix within a directory name is not an eligible path segment.
        workspace.write(format!("app/not{directory}absent.html"), "<p>Decoy</p>\n");
        let mut pack = serde_json::json!({
            "semantic_rules": [
                {"id":"asset", "namespace":"unrelated.assets", "scope":"project", "activation":[], "operation":{"kind":"class_attribute_to_file", "language":"python", "attribute":file_attribute, "target":{"kind":"project_path_suffix", "prefix":directory, "language":"html"}, "relation":"renders_template"}},
                {"id":"entity", "namespace":"unrelated.entities", "scope":"project", "activation":[], "operation":{"kind":"class_attribute_to_symbol", "language":"python", "attribute":symbol_attribute, "target":{"kind":"python_symbol", "language":"python"}, "relation":"uses_model"}},
                {"id":"paint", "namespace":"unrelated.assets", "scope":"project", "activation":[], "operation":{"kind":"call_to_file", "language":"python", "callee":{"kind":"imported", "module":module, "export":export}, "argument":0, "target":{"kind":"project_path_suffix", "prefix":directory, "language":"html"}, "relation":"renders_template"}}
            ]
        });
        let write_pack = |value: &serde_json::Value| {
            workspace.write(&manifest_path, &toml::to_string(value).unwrap());
        };
        write_pack(&pack);
        writer::build(&workspace.0, &workspace.db(), None).unwrap_or_else(|error| {
            panic!("configured {pack_name} attributes and directory must build: {error:#}")
        });
        let assert_relations = |active: bool| {
            let conn = workspace.open();
            assert_valid(&conn);
            for (kind, owners, destination) in [
                (
                    "renders_template",
                    vec!["Unique", "MissingSymbol", "DuplicateSymbol", "page"],
                    format!("module:{target}"),
                ),
                (
                    "uses_model",
                    vec!["Unique", "MissingFile", "DuplicateFile"],
                    node_id(&conn, "app/models.py", "Record"),
                ),
            ] {
                let mut expected: Vec<_> = if active {
                    owners
                        .iter()
                        .map(|name| (node_id(&conn, "app/views.py", name), destination.clone()))
                        .collect()
                } else {
                    vec![]
                };
                expected.sort();
                let mut actual: Vec<_> = edge_occurrence_rows(&conn, "app/views.py", kind)
                    .into_iter()
                    .map(|(source, target, _)| (source, target))
                    .collect();
                actual.sort();
                assert_eq!(actual, expected, "{pack_name}: {kind}, active={active}");
            }
            let ordinary_calls: i64 = conn.query_row(
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                ordinary_calls, 1,
                "ordinary language calls survive rule changes"
            );
            let imports: i64 = conn.query_row(
                "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND c.line=1 AND x.expression='Record' AND c.status='resolved'",
                [], |row| row.get(0),
            ).unwrap();
            assert!(imports > 0, "ordinary import facts survive rule changes");
            if active {
                for (expression, status) in [
                    ("'absent.html'", "unresolved"),
                    ("'catalog/duplicate.html'", "ambiguous"),
                    ("Missing", "unresolved"),
                    ("Duplicate", "ambiguous"),
                    ("choose_asset()", "unresolved"),
                    ("choose_type()", "unresolved"),
                ] {
                    let statuses: Vec<String> = conn.prepare(
                        "SELECT c.status FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='app/views.py' AND x.expression=?1",
                    ).unwrap().query_map([expression], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
                    assert!(
                        statuses.iter().any(|actual| actual == status),
                        "{pack_name}: {expression} requires {status}: {statuses:?}"
                    );
                }
            }
        };
        assert_relations(true);
        complete_parity(&workspace);

        // Only effective rules determine semantic behavior, including when
        // the language source and ordinary imports/calls remain untouched.
        for operation in pack["semantic_rules"].as_array_mut().unwrap() {
            operation["activation"] =
                serde_json::json!([{"kind":"package", "name":"unrelated-engine"}]);
        }
        write_pack(&pack);
        workspace.delta(&manifest_path);
        assert_relations(false);
        complete_parity(&workspace);
        workspace.write(
            "pyproject.toml",
            "[project]\nname='attribute-rules'\ndependencies=['unrelated-engine']\n",
        );
        workspace.delta("pyproject.toml");
        assert_relations(true);
        complete_parity(&workspace);

        let mut replacement = pack.clone();
        replacement["replace"] = true.into();
        replacement["semantic_rules"] = serde_json::json!([]);
        write_pack(&replacement);
        workspace.delta(&manifest_path);
        assert_relations(false);
        complete_parity(&workspace);
        write_pack(&pack);
        workspace.delta(&manifest_path);
        assert_relations(true);
        complete_parity(&workspace);
        fs::remove_file(workspace.0.join(&manifest_path)).unwrap();
        workspace.delta(&manifest_path);
        assert_relations(false);
        complete_parity(&workspace);
        write_pack(&pack);
        workspace.delta(&manifest_path);
        assert_relations(true);
        complete_parity(&workspace);
    }
}
