use super::PythonWorkspace as Workspace;
use super::*;
use contextunity_forge_mcp::{
    core::response::{CoverageOptions, QueryOptions, ResponsePolicy},
    db::{reader, traversal},
};
use rusqlite::Connection;

fn coverage(conn: &Connection, line: i64, expression: &str) -> (String, String) {
    coverage_at(conn, "consumer.py", line, expression)
}

fn coverage_at(conn: &Connection, path: &str, line: i64, expression: &str) -> (String, String) {
    conn.query_row(
        "SELECT status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=?2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?3",
        (path, line, expression),
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
            "SELECT kind,qualname FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='aliases.py') AND name=?1", [name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or_else(|error| panic!("missing type {name}: {error}; nodes: {:?}", rows(&conn, "SELECT name || ':' || kind FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='aliases.py') ORDER BY line")));
        assert_eq!(kind, "type", "{name}");
        assert_eq!(qualname, format!("aliases.{name}"));
        assert!(rows(
            &conn,
            &format!("SELECT id FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='aliases.py') AND name='{name}'")
        )[0]
        .starts_with("type:aliases.py:"));
        let statuses = rows(&conn, &format!("SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='consumer.py' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='{name}' ORDER BY line"));
        assert_eq!(statuses, vec!["resolved"; 2], "{name}");
    }
    assert!(rows(&conn, "SELECT name FROM nodes WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path='aliases.py') AND kind='type' AND name IN ('ordinary','annotated','Local','LocalVariable','LocalPep','Nested')").is_empty());
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
    for (source, expected_status) in [
        ("Value = 1\n", "resolved"),
        ("type Value = str | list[Value]\n", "resolved"),
        (
            "from typing import TypeAlias, TYPE_CHECKING\nif TYPE_CHECKING:\n    Value: TypeAlias = str\n",
            "resolved",
        ),
        ("Value = 1\nValue = 2\n", "ambiguous"),
        ("", "unresolved"),
        ("Value = 1\n", "resolved"),
    ] {
        w.write("aliases.py", source);
        w.delta("aliases.py");
        w.assert_cold_equivalent();
        let conn = w.open();
        assert_eq!(coverage(&conn, 1, "Value").0, expected_status, "{source:?}");
        let import_edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.py' AND e.kind='imports' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='aliases.py' AND dst.name='Value'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(import_edges, i64::from(expected_status == "resolved"), "{source:?}");
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

#[test]
fn framework_lineage_and_local_protocol_members_keep_project_provenance() {
    let first = Workspace::new();
    first.write("app/__init__.py", "");
    first.write(
        "app/models.py",
        "from django.db import models\nclass Product(models.Model):\n    pass\n",
    );
    first.write(
        "app/contracts.py",
        "from typing import Protocol\nclass Notifier(Protocol):\n    def notify(self) -> None: ...\n",
    );
    first.write(
        "consumer.py",
        concat!(
            "from django.http import HttpRequest\n",
            "from django.core.management.base import BaseCommand\n",
            "from pydantic import BaseModel\n",
            "from app.models import Product\n",
            "from app.contracts import Notifier\n",
            "class SecuredRequest(HttpRequest):\n",
            "    pass\n",
            "class Command(BaseCommand):\n",
            "    def handle(self) -> None:\n",
            "        self.stdout.write('ok')\n",
            "class Record(BaseModel):\n",
            "    title: str\n",
            "def run(request: SecuredRequest, record: Record, notifier: Notifier) -> None:\n",
            "    request.GET.get('q')\n",
            "    Product.objects.create()\n",
            "    Record.model_validate({'title': 'ok'})\n",
            "    record.model_dump()\n",
            "    notifier.notify()\n",
            "def choose(flag: bool) -> None:\n",
            "    if flag:\n",
            "        model = Product\n",
            "    else:\n",
            "        model = Notifier\n",
            "    model.objects.create()\n",
        ),
    );
    first.build();
    let first_db = first.open();
    for (line, expression, origin) in [
        (10, "self.stdout.write", "django"),
        (14, "request.GET.get", "django"),
        (15, "Product.objects.create", "django"),
        (16, "Record.model_validate", "pydantic"),
        (17, "record.model_dump", "pydantic"),
    ] {
        let (status, evidence) = coverage(&first_db, line, expression);
        assert_eq!(status, "external", "{expression}: {evidence}");
        assert!(evidence.contains(origin), "{expression}: {evidence}");
    }
    assert_eq!(coverage(&first_db, 18, "notifier.notify").0, "resolved");
    assert_eq!(
        coverage(&first_db, 24, "model.objects.create").0,
        "unresolved",
        "conditional alias cannot select Product as a unique provider"
    );
    let local_edge: i64 = first_db.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.py' AND e.kind='calls' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/contracts.py' AND dst.kind='method' AND dst.name='notify'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        local_edge, 1,
        "protocol call retains its exact local method"
    );

    let second = Workspace::new();
    second.write("app/__init__.py", "");
    second.write("app/models.py", "class Product:\n    pass\n");
    second.write(
        "app/contracts.py",
        "from typing import Protocol\nclass Notifier(Protocol):\n    def cancel(self) -> None: ...\n",
    );
    second.write(
        "consumer.py",
        concat!(
            "from app.models import Product\n",
            "from app.contracts import Notifier\n",
            "def run(notifier: Notifier, client) -> None:\n",
            "    Product.objects.create()\n",
            "    notifier.notify()\n",
            "    client.send()\n",
            "def test_injected(db_client) -> None:\n",
            "    db_client.query()\n",
        ),
    );
    second.build();
    let second_db = second.open();
    for (line, expression) in [
        (4, "Product.objects.create"),
        (5, "notifier.notify"),
        (6, "client.send"),
        (8, "db_client.query"),
    ] {
        assert_eq!(
            coverage(&second_db, line, expression).0,
            "unresolved",
            "{expression}"
        );
    }

    let linked = Workspace::new();
    linked.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
            first.0.display(),
            second.0.display()
        ),
    );
    linked.build();
    let linked_db = linked.open();
    assert_eq!(
        coverage_at(&linked_db, "[first]/consumer.py", 14, "request.GET.get").0,
        "external"
    );
    assert_eq!(
        coverage_at(&linked_db, "[first]/consumer.py", 18, "notifier.notify").0,
        "resolved"
    );
    for (line, expression) in [(4, "Product.objects.create"), (5, "notifier.notify")] {
        assert_eq!(
            coverage_at(&linked_db, "[second]/consumer.py", line, expression).0,
            "unresolved",
            "project isolation for {expression}"
        );
    }
}

#[test]
fn imported_cursor_methods_are_finite_and_untyped_logger_names_stay_unknown() {
    let w = Workspace::new();
    w.write(
        "consumer.py",
        "import sqlite3\ndef run(cursor: sqlite3.Cursor, logger):\n    cursor.execute('select 1')\n    cursor.unsupported_method()\n    logger.info('unknown')\n",
    );
    w.build();
    let conn = w.open();
    let (status, evidence) = coverage(&conn, 3, "cursor.execute");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("sqlite3.Cursor"), "{evidence}");
    assert!(evidence.contains("line 1"), "{evidence}");
    for (line, expression) in [(4, "cursor.unsupported_method"), (5, "logger.info")] {
        assert_eq!(
            coverage(&conn, line, expression).0,
            "unresolved",
            "{expression}"
        );
    }
}

#[test]
fn module_qualified_local_subclass_inherits_only_its_own_imported_framework_members() {
    let first = Workspace::new();
    first.write("pkg/__init__.py", "");
    first.write(
        "pkg/models.py",
        "from pydantic import BaseModel\nclass Record(BaseModel):\n    title: str\n",
    );
    first.write(
        "pkg/consumer.py",
        "from pkg import models\ndef run():\n    models.Record.model_validate({'title': 'ok'})\n",
    );

    let second = Workspace::new();
    second.write("pkg/__init__.py", "");
    second.write(
        "pkg/models.py",
        "from pydantic import BaseModel\nclass Record(BaseModel):\n    title: str\nRecord = object\n",
    );
    second.write(
        "pkg/consumer.py",
        "from pkg import models\ndef run():\n    models.Record.model_validate({'title': 'unknown'})\n",
    );

    let linked = Workspace::new();
    linked.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
            first.0.display(),
            second.0.display()
        ),
    );
    linked.build();
    let conn = linked.open();
    let (status, evidence) = coverage_at(
        &conn,
        "[first]/pkg/consumer.py",
        3,
        "models.Record.model_validate",
    );
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("pydantic"), "{evidence}");
    assert_eq!(
        coverage_at(
            &conn,
            "[second]/pkg/consumer.py",
            3,
            "models.Record.model_validate",
        )
        .0,
        "unresolved"
    );
}

#[test]
fn module_qualified_framework_call_requires_an_earlier_import_position() {
    let w = Workspace::new();
    w.write("pkg/__init__.py", "");
    w.write(
        "pkg/models.py",
        "from pydantic import BaseModel\nclass Record(BaseModel):\n    pass\n",
    );
    w.write(
        "before.py",
        "models.Record.model_validate({}); from pkg import models\n",
    );
    w.write(
        "after.py",
        "from pkg import models; models.Record.model_validate({})\n",
    );
    w.build();
    let conn = w.open();
    assert_eq!(
        coverage_at(&conn, "before.py", 1, "models.Record.model_validate").0,
        "unresolved"
    );
    let (status, evidence) = coverage_at(&conn, "after.py", 1, "models.Record.model_validate");
    assert_eq!(status, "external", "{evidence}");
    assert!(evidence.contains("pydantic"), "{evidence}");
}

#[cfg(feature = "lang-python")]
#[test]
fn python_routes_preserve_http_decorators_and_django_patterns_as_mapping_calls_remain_calls() {
    let source = "def auth(): pass\ndef handler(): pass\napp.get('/items', auth, handler)\nrouter.post('^item$', handler)\npath('items/', handler)\nre_path('^items/', handler)\ndjango.urls.path('', handler)\n@bp.route('/multi', methods=['GET', 'PATCH'])\ndef decorated(): pass\n@mapping.get('/lookup')\ndef mapped(): pass\nconfig.get('section', handler)\napp.get('setting', handler)\nconfig.get('/setting', handler)\napp.get('/number', 0)\napp.get('/negative', -1)\napp.get('/boolean', True)\napp.get('/none', None)\napp.get('/string', 'handler')\napp.get('/list', [handler])\napp.get('/tuple', (handler,))\napp.get('/mapping', {'handler': handler})\n";
    let facts = ast::extract("routes.py", "python", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let mut routes: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "route")
        .map(|node| node.name.as_str())
        .collect();
    routes.sort_unstable();
    assert_eq!(
        routes,
        [
            "ANY ",
            "ANY ^items/",
            "ANY items/",
            "GET /items",
            "GET /multi",
            "PATCH /multi",
            "POST ^item$"
        ]
    );
    let items = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /items")
        .unwrap();
    assert_eq!(
        items.details["handlers"],
        serde_json::json!(["auth", "handler"])
    );
    let ordinary_calls: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.kind == "calls" && reference.expression == "config.get")
        .map(|reference| reference.line)
        .collect();
    assert_eq!(ordinary_calls, [12, 14]);
    for name in ["GET /multi", "PATCH /multi"] {
        let route = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let handler = facts
            .nodes
            .iter()
            .find(|node| node.name == "decorated")
            .unwrap();
        assert!(facts
            .edges
            .iter()
            .any(|edge| edge.src == route.id && edge.dst == handler.id && edge.kind == "handles"));
    }
}
