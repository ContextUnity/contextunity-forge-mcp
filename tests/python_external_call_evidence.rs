#![cfg(feature = "lang-python")]

use contextunity_forge_mcp::{
    core::{
        commitments,
        response::{CoverageOptions, QueryOptions, ResponsePolicy},
    },
    db::{reader, traversal, writer},
};
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forge_external_calls_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) {
        writer::build(&self.0, &self.db(), None).unwrap();
    }
    fn delta(&self, path: &str) {
        writer::delta(&self.0, &self.db(), &[PathBuf::from(path)]).unwrap();
    }
    fn open(&self) -> Connection {
        reader::open(&self.db(), &self.0).unwrap()
    }

    fn assert_cold_equivalent(&self) {
        let incremental = self.open();
        commitments::verify(&incremental).unwrap();
        let cold_path = self.0.join(".forge/cold.sqlite");
        writer::build(&self.0, &cold_path, None).unwrap();
        let cold = reader::open(&cold_path, &self.0).unwrap();
        commitments::verify(&cold).unwrap();
        for sql in [
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
            "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
        ] {
            assert_eq!(rows(&incremental, sql), rows(&cold, sql), "{sql}");
        }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn coverage(conn: &Connection, line: i64, expression: &str) -> (String, String) {
    conn.query_row(
        "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND line=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
        (line, expression),
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap()
}

fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn known_external_alias_is_external_without_claiming_a_callable_target() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "import requests as http\ndef run(): return http.get('https://example.test')\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 1, "requests").0, "external");
    let (status, evidence) = coverage(&conn, 2, "http.get");
    assert_eq!(status, "external");
    assert_eq!(
        evidence,
        "call through external import requests; callable target unverified"
    );
    let edges: i64 = conn
        .query_row("SELECT count(*) FROM edges WHERE kind='calls'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(edges, 0);
    let page = QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    for overview in [
        reader::overview_with_options(
            &conn,
            &reader::OverviewOptions {
                aspects: None,
                page: &page,
            },
        )
        .unwrap(),
        traversal::query_with_options(
            &conn,
            "overview",
            None,
            &traversal::GraphQueryOptions {
                depth: 1,
                direction: None,
                coverage: CoverageOptions::default(),
                page: &page,
            },
        )
        .unwrap(),
    ] {
        assert_eq!(overview["counts"]["unresolved"], 0);
        assert_eq!(overview["counts"]["external_imports"], 2);
    }
    let removal = traversal::removal_paged(&conn, "consumer.py", &page).unwrap();
    assert_eq!(removal["unresolved_references"], 0);
    assert_eq!(removal["unresolved_references"], 0);
}

#[test]
fn module_type_aliases_resolve_imports_and_annotation_references() {
    let w = Workspace::new();
    w.write("aliases.py", "from typing import TypeAlias, TypeVar, NewType\nfrom typing_extensions import TypeAliasType\nimport typing\ntype JsonValue = str | list[JsonValue] | dict[str, JsonValue]\ntype GenericAlias[T] = list[T]\nJsonPrimitive: TypeAlias = str | int\nJsonDict: typing.TypeAlias = dict[str, JsonValue]\nExplicit: TypeAliasType = TypeAliasType('Explicit', str)\nVariable = TypeVar('Variable')\nUserId = NewType('UserId', int)\nConstructed = TypeAliasType('Constructed', list[str])\nordinary = 1\nannotated: int = 2\ndef local():\n    Local: TypeAlias = str\n    LocalVariable = TypeVar('LocalVariable')\n    type LocalPep = int\nclass Container:\n    Nested: TypeAlias = str\n");
    let names = [
        "JsonValue",
        "GenericAlias",
        "JsonPrimitive",
        "JsonDict",
        "Explicit",
        "Variable",
        "UserId",
        "Constructed",
    ];
    let mut consumer = format!("from aliases import {}\n", names.join(", "));
    for name in names {
        consumer.push_str(&format!(
            "def use_{name}(value: {name}) -> {name}: return value\n"
        ));
    }
    consumer.push_str("from aliases import Missing\ndef missing(value: Missing): return value\n");
    w.write("consumer.py", &consumer);
    w.build();
    let conn = w.open();
    for name in names {
        let (kind, qualname): (String, String) = conn.query_row(
            "SELECT kind,qualname FROM nodes WHERE path='aliases.py' AND name=?1", [name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or_else(|error| panic!("missing type {name}: {error}; nodes: {:?}", rows(&conn, "SELECT name || ':' || kind FROM nodes WHERE path='aliases.py' ORDER BY line")));
        assert_eq!(kind, "type", "{name}");
        assert_eq!(qualname, format!("aliases.{name}"));
        assert!(rows(
            &conn,
            &format!("SELECT id FROM nodes WHERE path='aliases.py' AND name='{name}'")
        )[0]
        .starts_with("type:aliases.py:"));
        let statuses = rows(&conn, &format!("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='{name}' ORDER BY line"));
        assert_eq!(statuses, vec!["resolved"; 2], "{name}");
    }
    assert!(rows(&conn, "SELECT name FROM nodes WHERE path='aliases.py' AND kind='type' AND name IN ('ordinary','annotated','Local','LocalVariable','LocalPep','Nested')").is_empty());
    assert_eq!(rows(&conn, "SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND status='unresolved' ORDER BY line"), vec!["Missing", "Missing"]);
}

#[test]
fn module_alias_conversion_delta_matches_cold_build() {
    let w = Workspace::new();
    w.write("aliases.py", "Value = 1\n");
    w.write(
        "consumer.py",
        "from aliases import Value\ndef use(value: Value) -> Value: return value\n",
    );
    w.build();
    assert_eq!(coverage(&w.open(), 1, "Value").0, "unresolved");
    for source in ["type Value = str | list[Value]\n", "from typing import TypeAlias, TYPE_CHECKING\nif TYPE_CHECKING:\n    Value: TypeAlias = str\n", "Value = 1\n"] {
        w.write("aliases.py", source);
        w.delta("aliases.py");
        w.assert_cold_equivalent();
        assert_eq!(coverage(&w.open(), 1, "Value").0, if source == "Value = 1\n" { "unresolved" } else { "resolved" });
    }
}

#[test]
fn absolute_external_import_and_shadowed_alias_have_distinct_evidence() {
    let w = Workspace::new();
    w.write("consumer.py", "import unknown_package as missing\nimport requests as http\ndef unknown(): return missing.get()\ndef shadowed(http): return http.get()\n");
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 3, "missing.get").0, "external");
    assert!(coverage(&conn, 3, "missing.get")
        .1
        .starts_with("call through external import unknown_package;"));
    assert!(rows(
        &conn,
        "SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id) FROM edges WHERE kind='calls' AND (SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id)='missing.get'"
    )
    .is_empty());
    assert_eq!(coverage(&conn, 4, "http.get").0, "unresolved");
    assert!(!coverage(&conn, 4, "http.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn duplicate_aliases_and_local_provider_do_not_gain_external_provenance() {
    let w = Workspace::new();
    w.write("provider.py", "def get(): return 1\n");
    w.write("consumer.py", "import requests as client\nimport httpx as client\nimport provider as local\ndef run(): return client.get(), local.get()\n");
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 4, "client.get").0, "unresolved");
    assert!(!coverage(&conn, 4, "client.get")
        .1
        .starts_with("call through external import "));
    assert_eq!(coverage(&conn, 4, "local.get").0, "resolved");
    assert!(!coverage(&conn, 4, "local.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn local_import_is_scoped_to_its_function() {
    let w = Workspace::new();
    w.write("consumer.py", "def first():\n    import requests as http\n    return http.get('https://example.test')\ndef second(): return http.get('https://example.test')\n");
    w.build();
    let conn = w.open();
    assert_eq!(
        coverage(&conn, 3, "http.get").1,
        "call through external import requests; callable target unverified"
    );
    assert!(!coverage(&conn, 4, "http.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn call_before_external_import_has_no_provenance() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "def run(): return http.get('https://example.test')\nimport requests as http\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 1, "http.get").0, "unresolved");
    assert!(!coverage(&conn, 1, "http.get")
        .1
        .starts_with("call through external import "));
}

#[test]
fn assignment_rebind_has_no_external_provenance() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "import json\njson = object()\ndef run(): return json.loads('{}')\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(coverage(&conn, 3, "json.loads").0, "unresolved");
    assert!(!coverage(&conn, 3, "json.loads")
        .1
        .starts_with("call through external import "));
}

#[test]
fn changing_import_provenance_preserves_cold_delta_equivalence() {
    let w = Workspace::new();
    let external =
        "import requests as client\ndef run(): return client.get('https://example.test')\n";
    let unknown =
        "import unknown_package as client\ndef run(): return client.get('https://example.test')\n";
    w.write("consumer.py", external);
    w.build();
    w.write("consumer.py", unknown);
    w.delta("consumer.py");
    w.assert_cold_equivalent();
    assert_eq!(coverage(&w.open(), 2, "client.get").0, "external");
    assert_eq!(
        coverage(&w.open(), 2, "client.get").1,
        "call through external import unknown_package; callable target unverified"
    );
    w.write("consumer.py", external);
    w.delta("consumer.py");
    w.assert_cold_equivalent();
    assert_eq!(
        coverage(&w.open(), 2, "client.get").1,
        "call through external import requests; callable target unverified"
    );
}
