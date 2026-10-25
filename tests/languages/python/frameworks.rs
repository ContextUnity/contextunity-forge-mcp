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
    ).unwrap_or_else(|error| panic!("missing coverage at {path}:{line} for {expression}: {error}"))
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
fn django_orm_and_drf_receiver_packs_are_finite_and_router_handlers_are_unique() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='django-drf-receivers'\ndependencies=['django', 'djangorestframework']\n",
    );
    let models = concat!(
        "from django.db.models import QuerySet, Manager, ForeignKey, Model\n\nclass ProductQuerySet(QuerySet):\n    def enabled(self):\n        return self.filter(active=True)\n\nclass ProductManager(Manager):\n    def visible(self):\n        return self.get_queryset()\n\nclass ProductRelation(ForeignKey):\n    def related_type(self):\n        return self.get_internal_type()\n\nclass Unrelated:\n    def query(self):\n        return self.filter(active=True)\n",
        r#"
class Product(Model):
    pass

class LocalSelection(QuerySet):
    def own(self):
        pass

class OverriddenSelection(QuerySet):
    def filter(self):
        return unknown()

class LocalObjects(Model):
    objects = unknown()

def finite_returns(selection: LocalSelection, manager: ProductManager, overridden: OverriddenSelection):
    filtered = Product.objects.filter(active=True)
    filtered.exclude(active=False)
    Product.objects.all().filter(active=True)
    retained = selection.filter(active=True)
    retained.own()
    selection.filter(active=True).own()
    selected = manager.filter(active=True)
    selected.exclude(active=False)
    selected.visible()
    filtered.invented()
    counted = Product.objects.count()
    counted.exclude(active=False)
    replaced = overridden.filter()
    replaced.exclude(active=False)
    LocalObjects.objects.filter(active=True)
    unproved = unknown()
    unproved.exclude(active=False)
    changed = Product.objects.all()
    changed = unknown()
    changed.exclude(active=False)
    manager.visible()
    ordered = Product.objects.order_by('name')
    ordered.exclude(active=False)
    Product.objects.order_by('name').filter(active=True)
    related = Product.objects.select_related('parent')
    related.exclude(active=False)
    Product.objects.select_related('parent').filter(active=True)
    prefetched = Product.objects.prefetch_related('children')
    prefetched.exclude(active=False)
    Product.objects.prefetch_related('children').filter(active=True)
    local_ordered = selection.order_by('name')
    local_ordered.own()
    selection.order_by('name').own()
    local_related = selection.select_related('parent')
    local_related.own()
    selection.select_related('parent').own()
    local_prefetched = selection.prefetch_related('children')
    local_prefetched.own()
    selection.prefetch_related('children').own()
    manager_ordered = manager.order_by('name')
    manager_ordered.exclude(active=False)
    manager_ordered.visible()
    manager_related = manager.select_related('parent')
    manager_related.exclude(active=False)
    manager_related.visible()
    manager_prefetched = manager.prefetch_related('children')
    manager_prefetched.exclude(active=False)
    manager_prefetched.visible()
    LocalObjects.objects.order_by('name')
    LocalObjects.objects.select_related('parent')
    LocalObjects.objects.prefetch_related('children')
    got = Product.objects.get()
    got.exclude(active=False)
    created = Product.objects.create()
    created.exclude(active=False)
    first = Product.objects.first()
    first.exclude(active=False)
"#
    );
    workspace.write("app/models.py", models);
    workspace.write(
        "app/api.py",
        "from rest_framework.serializers import Serializer, ModelSerializer\nfrom rest_framework.views import APIView\nfrom rest_framework.generics import GenericAPIView\nfrom rest_framework.viewsets import ViewSet, ModelViewSet\n\nclass RequestSerializer(Serializer):\n    def validate(self):\n        return self.is_valid()\n\nclass ProductSerializer(ModelSerializer):\n    def valid(self):\n        return self.is_valid()\n\nclass ProductAPIView(APIView):\n    def get(self, request):\n        return self.get_permissions()\n\nclass ProductGenericAPIView(GenericAPIView):\n    def list(self, request):\n        return self.get_serializer()\n\nclass ProductViewSet(ModelViewSet):\n    def list(self, request):\n        return self.get_queryset()\n\nclass BasicViewSet(ViewSet):\n    def detail(self):\n        return self.reverse_action('detail')\n",
    );
    workspace.write(
        "app/routes.py",
        "from rest_framework.routers import DefaultRouter\nfrom app.api import ProductViewSet\nrouter = DefaultRouter()\nrouter.register('products', ProductViewSet, basename='product')\nrouter.register(prefix, ProductViewSet, basename='dynamic')\nrouter.register('duplicate', ProductViewSet, basename='duplicate-a')\nrouter.register('duplicate', ProductViewSet, basename='duplicate-b')\nother.register('unrelated', ProductViewSet, basename='unrelated')\n",
    );

    workspace.build();
    let conn = workspace.open();
    for (path, line, expression, provider) in [
        (
            "app/models.py",
            5,
            "self.filter",
            "django.db.models.QuerySet",
        ),
        (
            "app/models.py",
            9,
            "self.get_queryset",
            "django.db.models.Manager",
        ),
        (
            "app/models.py",
            13,
            "self.get_internal_type",
            "django.db.models.ForeignKey",
        ),
        (
            "app/api.py",
            8,
            "self.is_valid",
            "rest_framework.serializers.Serializer",
        ),
        (
            "app/api.py",
            12,
            "self.is_valid",
            "rest_framework.serializers.ModelSerializer",
        ),
        (
            "app/api.py",
            16,
            "self.get_permissions",
            "rest_framework.views.APIView",
        ),
        (
            "app/api.py",
            20,
            "self.get_serializer",
            "rest_framework.generics.GenericAPIView",
        ),
        (
            "app/api.py",
            24,
            "self.get_queryset",
            "rest_framework.viewsets.ModelViewSet",
        ),
        (
            "app/api.py",
            28,
            "self.reverse_action",
            "rest_framework.viewsets.ViewSet",
        ),
    ] {
        let (status, evidence) = coverage_at(&conn, path, line, expression);
        assert_eq!(status, "external", "{path}:{line} {expression}: {evidence}");
        assert!(evidence.contains(provider), "{path}:{line} {evidence}");
    }
    assert_eq!(
        coverage_at(&conn, "app/models.py", 17, "self.filter").0,
        "unresolved"
    );

    for (expression, expected) in [
        ("Product.objects.filter", "external"),
        ("filtered.exclude", "external"),
        ("Product.objects.all().filter", "external"),
        ("retained.own", "resolved"),
        ("selection.filter(active=True).own", "resolved"),
        ("selected.exclude", "external"),
        ("selected.visible", "unresolved"),
        ("filtered.invented", "unresolved"),
        ("counted.exclude", "unresolved"),
        ("replaced.exclude", "unresolved"),
        ("LocalObjects.objects.filter", "unresolved"),
        ("unproved.exclude", "unresolved"),
        ("changed.exclude", "unresolved"),
        ("manager.visible", "resolved"),
        ("ordered.exclude", "external"),
        ("Product.objects.order_by('name').filter", "external"),
        ("related.exclude", "external"),
        (
            "Product.objects.select_related('parent').filter",
            "external",
        ),
        ("prefetched.exclude", "external"),
        (
            "Product.objects.prefetch_related('children').filter",
            "external",
        ),
        ("local_ordered.own", "resolved"),
        ("selection.order_by('name').own", "resolved"),
        ("local_related.own", "resolved"),
        ("selection.select_related('parent').own", "resolved"),
        ("local_prefetched.own", "resolved"),
        ("selection.prefetch_related('children').own", "resolved"),
        ("manager_ordered.exclude", "external"),
        ("manager_ordered.visible", "unresolved"),
        ("manager_related.exclude", "external"),
        ("manager_related.visible", "unresolved"),
        ("manager_prefetched.exclude", "external"),
        ("manager_prefetched.visible", "unresolved"),
        ("LocalObjects.objects.order_by", "unresolved"),
        ("LocalObjects.objects.select_related", "unresolved"),
        ("LocalObjects.objects.prefetch_related", "unresolved"),
        ("got.exclude", "unresolved"),
        ("created.exclude", "unresolved"),
        ("first.exclude", "unresolved"),
    ] {
        let statuses: Vec<(String, String)> = conn
            .prepare("SELECT c.status,v.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE p.path='app/models.py' AND x.expression=?1 ORDER BY c.line")
            .unwrap()
            .query_map([expression], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            !statuses.is_empty() && statuses.iter().all(|row| row.0 == expected),
            "finite method return {expression}: expected {expected}, observed {statuses:?}"
        );
        if expected == "external" {
            assert!(
                statuses.iter().all(|row| row.1.contains("django")),
                "{expression}: {statuses:?}"
            );
            for (_, evidence) in &statuses {
                assert!(
                    !evidence.contains("configured_receiver")
                        && !evidence.contains("external import  at line 0"),
                    "finite receiver evidence must retain its actual authority and origin: {expression}: {evidence}"
                );
            }
            let origin_class = match expression {
                "selected.exclude"
                | "manager_ordered.exclude"
                | "manager_related.exclude"
                | "manager_prefetched.exclude" => Some("ProductManager"),
                "filtered.exclude"
                | "Product.objects.all().filter"
                | "ordered.exclude"
                | "Product.objects.order_by('name').filter"
                | "related.exclude"
                | "Product.objects.select_related('parent').filter"
                | "prefetched.exclude"
                | "Product.objects.prefetch_related('children').filter" => Some("Product"),
                _ => None,
            };
            if let Some(origin_class) = origin_class {
                let class_id: String = conn.query_row(
                    "SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='app/models.py' AND n.kind='class' AND n.name=?1",
                    [origin_class], |row| row.get(0),
                ).unwrap();
                let prefix = format!(
                    "finite python configured member {expression}; pack django; hierarchy provider django.db.models.QuerySet; ordered bases django.db.models.QuerySet,django.db.models.query.QuerySet; receiver origin external import django.db.models at line 1; configuration "
                );
                let source_identity = format!(":app/models.py:django:{class_id}");
                for (_, evidence) in &statuses {
                    let configuration = evidence.strip_prefix(&prefix).unwrap_or_else(|| {
                        panic!("finite receiver must report its Python hierarchy authority and retained import: {expression}: {evidence}")
                    });
                    assert!(
                        configuration.len() > source_identity.len()
                            && configuration.ends_with(&source_identity),
                        "finite receiver must retain its actual originating class separately from target rule: {expression}: {evidence}"
                    );
                }
            }
            let calls: i64 = conn.query_row(
                "SELECT count(*) FROM edge_occurrences e JOIN coverage_evidence v ON v.evidence_id=e.evidence_id WHERE e.kind='calls' AND v.evidence=?1",
                [expression], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                calls, 0,
                "external return never invents a local call: {expression}"
            );
        }
    }

    let router_edges: Vec<String> = conn
        .prepare(
            "SELECT json_array(json_extract(src.details,'$.route_path'),dst.path_id,dst.name) FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path='app/routes.py' AND e.kind='handles' ORDER BY src.line",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        router_edges.len(),
        1,
        "only one proven ViewSet registration has a handler edge: {router_edges:?}"
    );
    assert!(router_edges[0].contains("products"));
    assert!(router_edges[0].contains("ProductViewSet"));
    let route_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='route' AND path_id=(SELECT path_id FROM path_dictionary WHERE path='app/routes.py')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        route_count, 1,
        "dynamic and duplicate registrations do not create route handler facts"
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

#[test]
fn starlette_and_fastapi_pack_relations_are_exact() {
    use contextunity_forge_mcp::{core::commitments, db::writer};

    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname='python-framework-packs'\ndependencies=['fastapi','starlette','shorekit']\n",
    );
    workspace.write(
        ".forge/frameworks/starlette.toml",
        include_str!("../../../src/engine/languages/manifests/starlette.toml"),
    );
    workspace.write(
        "models.py",
        "class Record: pass\nclass Alternate: pass\nclass Fault: pass\nclass Duplicate: pass\nclass Duplicate: pass\n",
    );
    workspace.write(
        "providers.py",
        "def provide(): return 1\ndef repeated(): return 1\ndef repeated(): return 2\n",
    );
    workspace.write(
        "ordinary.py",
        "from providers import provide\ndef ordinary(): return provide()\n",
    );
    workspace.write(
        "routes.py",
        "from fastapi import FastAPI, APIRouter\nfrom starlette.routing import Route, Mount, WebSocketRoute\napp = FastAPI()\nparent = APIRouter(prefix='/api')\nleaf = APIRouter(prefix='/v1')\napp.include_router(parent, prefix='/root')\nparent.include_router(leaf, prefix='/nested')\n@leaf.get('/records')\ndef nested(): pass\n@leaf.get(dynamic_path)\ndef dynamic_route(): pass\ndef http(): pass\ndef socket(): pass\nroutes = [Route('/plain', http), Mount('/mounted'), WebSocketRoute('/socket', socket)]\n",
    );
    workspace.write(
        "responses.py",
        "from starlette.responses import JSONResponse, HTMLResponse\nclass Json(JSONResponse):\n    def known(self): return self.render({})\nclass Html(HTMLResponse):\n    def known(self): return self.render('ok')\n    def unknown(self): return self.unconfigured()\n",
    );
    let consumer = "from fastapi import FastAPI, Depends\nfrom models import Record, Alternate, Fault, Duplicate\nfrom providers import provide, repeated\napp = FastAPI()\n@app.get('/records', response_model=Record)\ndef endpoint(value=Depends(provide)): return value\n@app.exception_handler(Fault)\ndef rescue(): return None\ndef registered(): return None\napp.add_exception_handler(Fault, registered)\n@app.get('/missing', response_model=MissingModel)\ndef missing_model(): return None\n@app.get('/duplicate', response_model=Duplicate)\ndef duplicate_model(): return None\n@app.get('/dynamic', response_model=choose_model())\ndef dynamic_model(): return None\ndef missing_dependency(value=Depends(absent)): return value\ndef duplicate_dependency(value=Depends(repeated)): return value\n@app.exception_handler(MissingFault)\ndef missing_fault(): return None\n@app.exception_handler(Duplicate)\ndef duplicate_fault(): return None\napp.add_exception_handler(Fault, absent_handler)\ndef shadow(app, Depends):\n    @app.get('/shadow', response_model=Record)\n    def blocked(value=Depends(provide)): return value\n    return blocked\n";
    workspace.write("consumer.py", consumer);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();

    let observe = |active: bool| {
        let conn = workspace.open();
        commitments::verify(&conn).unwrap();
        assert!(rows(&conn, "SELECT message FROM errors").is_empty());
        let handles = rows(&conn, "SELECT s.name||'->'||d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='routes.py' AND e.kind='handles' ORDER BY s.name,d.name");
        assert_eq!(
            handles,
            [
                "ANY /plain->http",
                "GET /root/api/nested/v1/records->nested",
                "WEBSOCKET /socket->socket",
            ]
        );
        assert_eq!(
            rows(&conn, "SELECT name FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='routes.py' AND n.kind='route' AND n.name LIKE 'MOUNT %'"),
            ["MOUNT /mounted"]
        );
        for (line, expression, provider) in [
            (3, "self.render", "starlette.responses.JSONResponse"),
            (5, "self.render", "starlette.responses.HTMLResponse"),
        ] {
            let (status, evidence) = coverage_at(&conn, "responses.py", line, expression);
            assert_eq!(status, "external", "{evidence}");
            assert!(evidence.contains(provider), "{evidence}");
        }
        assert_eq!(
            coverage_at(&conn, "responses.py", 6, "self.unconfigured").0,
            "unresolved"
        );
        assert_eq!(
            rows(&conn, "SELECT s.name||'->'||d.name FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='ordinary.py' AND e.kind='calls'"),
            ["ordinary->provide"]
        );
        assert_eq!(
            coverage_at(&conn, "ordinary.py", 1, "provide").0,
            "resolved"
        );

        let mut failures = Vec::new();
        for (source, kind, target_path, target) in [
            ("endpoint", "injects_dependency", "providers.py", "provide"),
            ("endpoint", "uses_model", "models.py", "Record"),
            ("rescue", "references", "models.py", "Fault"),
            ("registered", "references", "models.py", "Fault"),
        ] {
            let count: usize = conn.query_row(
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary sp ON sp.path_id=s.path_id JOIN path_dictionary dp ON dp.path_id=d.path_id WHERE sp.path='consumer.py' AND s.name=?1 AND e.kind=?2 AND dp.path=?3 AND d.name=?4",
                (source, kind, target_path, target), |row| row.get(0),
            ).unwrap();
            if count != usize::from(active) {
                failures.push(format!(
                    "{source} --{kind}--> {target_path}:{target}: expected {}, observed {count}",
                    usize::from(active)
                ));
            }
        }
        for (source, kind) in [
            ("missing_model", "uses_model"),
            ("duplicate_model", "uses_model"),
            ("dynamic_model", "uses_model"),
            ("blocked", "uses_model"),
            ("blocked", "injects_dependency"),
            ("missing_dependency", "injects_dependency"),
            ("duplicate_dependency", "injects_dependency"),
            ("missing_fault", "references"),
            ("duplicate_fault", "references"),
        ] {
            let count: usize = conn.query_row(
                "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='consumer.py' AND s.name=?1 AND e.kind=?2",
                (source, kind), |row| row.get(0),
            ).unwrap();
            assert_eq!(count, 0, "nonexact {source} --{kind}");
        }
        if active {
            for (marker, expression, status) in [
                ("@app.get('/missing'", "%MissingModel%", "unresolved"),
                ("@app.get('/duplicate'", "%Duplicate%", "ambiguous"),
                ("missing_dependency(value", "%absent%", "unresolved"),
                ("duplicate_dependency(value", "%repeated%", "ambiguous"),
                (
                    "@app.exception_handler(MissingFault)",
                    "%MissingFault%",
                    "unresolved",
                ),
                (
                    "@app.exception_handler(Duplicate)",
                    "%Duplicate%",
                    "ambiguous",
                ),
            ] {
                let line = consumer
                    .lines()
                    .position(|line| line.contains(marker))
                    .unwrap()
                    + 1;
                let count: usize = conn.query_row(
                    "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='consumer.py' AND c.line=?1 AND x.expression LIKE ?2 AND c.status=?3",
                    (line, expression, status), |row| row.get(0),
                ).unwrap();
                if count == 0 {
                    failures.push(format!(
                        "consumer.py:{line} {expression}: missing {status} coverage"
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "pack semantic contract:\n{}",
            failures.join("\n")
        );
        drop(conn);
        workspace.assert_cold_equivalent();
        let incremental = workspace.open();
        let cold = reader::open(&workspace.0.join(".forge/cold.sqlite"), &workspace.0).unwrap();
        for sql in [
            "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
            "SELECT json_array(path,hex(facts_blob)) FROM local_facts ORDER BY path",
            "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id",
        ] {
            assert_eq!(rows(&incremental, sql), rows(&cold, sql), "{sql}");
        }
    };
    observe(true);

    let manifest = ".forge/frameworks/fastapi.toml";
    let mut pack: toml::Value = toml::from_str(include_str!(
        "../../../src/engine/languages/manifests/fastapi.toml"
    ))
    .unwrap();
    pack.as_table_mut()
        .unwrap()
        .insert("replace".into(), true.into());
    let mut removed = pack.clone();
    removed.as_table_mut().unwrap().remove("semantic_rules");
    workspace.write(manifest, &toml::to_string(&removed).unwrap());
    workspace.delta(manifest);
    observe(false);

    let rewrite = |source: &str| {
        source
            .replace("fastapi", "shorekit")
            .replace("FastAPI", "Gateway")
            .replace("Depends", "Acquire")
            .replace("response_model", "result_type")
            .replace("add_exception_handler", "install_rescue")
            .replace("exception_handler", "rescue_handler")
    };
    for rule in pack["semantic_rules"].as_array_mut().unwrap() {
        *rule = toml::from_str::<toml::Value>(&rewrite(&toml::to_string(rule).unwrap())).unwrap();
    }
    pack["di_calls"][0]["callee"] = "Acquire".into();
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    workspace.delta(manifest);
    observe(false);
    workspace.write("consumer.py", &rewrite(consumer));
    workspace.delta("consumer.py");
    observe(true);

    std::fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    observe(false);
    workspace.write("consumer.py", consumer);
    workspace.delta("consumer.py");
    observe(true);
}

#[cfg(all(feature = "lang-typescript", feature = "lang-html"))]
#[test]
fn gridviewspec_pack_bindings_follow_unique_targets() {
    use contextunity_forge_mcp::{core::commitments, db::writer};

    let workspace = Workspace::new();
    let families = [
        ("table_v2", "GridViewTable", "datasource"),
        ("toolbar", "GridViewToolbar", "counters"),
        ("filters_v2", "GridViewFilters", "drawer_sections"),
        ("content", "GridViewCards", "cards"),
        ("content", "GridViewCharts", "charts"),
        ("content", "GridViewKpi", "items"),
        ("content", "GridViewTabs", "tabs"),
        ("actions", "GridViewActions", "items"),
    ];
    let mut receivers = String::new();
    for (index, (module, base, member)) in families.iter().enumerate() {
        receivers.push_str(&format!("from grid_view_spec.types.{module} import {base}\nclass Family{index}({base}):\n    def known(self): return self.{member}\n    def unknown(self): return self.unconfigured\n"));
    }
    workspace.write("families.py", &receivers);
    workspace.write("backend.py", "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/api/records/')\ndef records(): return []\n@app.post('/api/records/')\ndef update(): return []\n@app.get('/api/duplicate/')\ndef first(): return []\n@app.get('/api/duplicate/')\ndef second(): return []\n");
    workspace.write(
        "ordinary.py",
        "from backend import records\ndef ordinary(): return records()\n",
    );
    let provider = "export function draw() { return 1; }\nexport function openCart() { return 2; }\nexport function install() {\n    var gv = window.GridView;\n    gv.registerRenderer('stock_cart', draw);\n    gv.registerAction('open_stock_cart', openCart);\n    gv.registerRenderer('duplicate', draw);\n    gv.registerRenderer('duplicate', openCart);\n}\nexport function shadow(window) { window.GridView.registerRenderer('shadow', draw); }\nexport function rebound() { var gv = window.GridView; gv = other; gv.registerRenderer('rebound', draw); }\n";
    let consumer = "from grid_view_spec.types.table_v2 import GridViewTable, GridViewDataSource, GridViewColumn\ndef get_table(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/'))\ndef post_table(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/', method='post'))\ndef renderer(): return GridViewColumn(renderer='stock_cart')\ndef action(): return GridViewColumn(extra={'action':'open_stock_cart'})\ndef missing(): return GridViewColumn(renderer='missing')\ndef duplicate(): return GridViewColumn(renderer='duplicate')\ndef dynamic(): return GridViewColumn(renderer=selected)\ndef shadowed(GridViewColumn): return GridViewColumn(renderer='stock_cart')\ndef shadow(): return GridViewColumn(renderer='shadow')\ndef rebound(): return GridViewColumn(renderer='rebound')\ndef dynamic_url(): return GridViewTable(datasource=GridViewDataSource(endpoint=selected))\ndef dynamic_method(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/', method=selected))\ndef detached(): return GridViewDataSource(endpoint='/api/records/')\ndef missing_route(): return GridViewTable(datasource=GridViewDataSource(endpoint='/absent/'))\ndef duplicate_route(): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/duplicate/'))\ndef shadow_table(GridViewTable): return GridViewTable(datasource=GridViewDataSource(endpoint='/api/records/'))\n";
    workspace.write("provider.js", provider);
    workspace.write("spec.py", consumer);
    workspace.write("request.html", "<button hx-get='/api/records/' hx-target='#records' hx-swap='outerHTML'></button>\n<button hx-get='/api/{{ id }}/'></button>\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();

    let observe = |active: bool, registry: &str, family_active: bool| {
        let conn = workspace.open();
        commitments::verify(&conn).unwrap();
        assert!(rows(&conn, "SELECT message FROM errors").is_empty());
        for (index, (module, base, member)) in families.iter().enumerate() {
            let (status, evidence) = coverage_at(
                &conn,
                "families.py",
                (index * 4 + 3) as i64,
                &format!("self.{member}"),
            );
            assert_eq!(
                status,
                if family_active {
                    "external"
                } else {
                    "unresolved"
                },
                "{base}: {evidence}"
            );
            if family_active {
                assert!(
                    evidence.contains(&format!("grid_view_spec.types.{module}.{base}")),
                    "{evidence}"
                );
            }
            assert_eq!(
                coverage_at(
                    &conn,
                    "families.py",
                    (index * 4 + 4) as i64,
                    "self.unconfigured"
                )
                .0,
                "unresolved"
            );
        }
        let target = |path: &str, name: &str| -> String {
            conn.query_row("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.name=?2", (path, name), |row| row.get(0)).unwrap()
        };
        assert_eq!(rows(&conn, "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='ordinary.py' AND s.name='ordinary' AND e.kind='calls'"), [target("backend.py","records")]);
        assert_eq!(
            coverage_at(&conn, "ordinary.py", 1, "records").0,
            "resolved"
        );
        let mut failures = Vec::new();
        for (source, kind, path, name, expected) in [
            (
                "get_table",
                "calls_endpoint",
                "backend.py",
                "GET /api/records/",
                active,
            ),
            (
                "post_table",
                "calls_endpoint",
                "backend.py",
                "POST /api/records/",
                active,
            ),
            (
                "renderer",
                "references",
                "provider.js",
                "draw",
                active && registry == "resolved",
            ),
            (
                "action",
                "references",
                "provider.js",
                "openCart",
                active && registry == "resolved",
            ),
        ] {
            let actual = conn.prepare("SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='spec.py' AND s.name=?1 AND e.kind=?2 ORDER BY d.id").unwrap().query_map((source,kind), |row| row.get::<_,String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>();
            let expected = if expected {
                vec![target(path, name)]
            } else {
                vec![]
            };
            if actual != expected {
                failures.push(format!(
                    "{source} --{kind}--> {path}:{name}: expected {expected:?}, actual {actual:?}"
                ));
            }
        }
        for (source, kind) in [
            ("missing", "references"),
            ("duplicate", "references"),
            ("dynamic", "references"),
            ("shadowed", "references"),
            ("shadow", "references"),
            ("rebound", "references"),
            ("dynamic_url", "calls_endpoint"),
            ("dynamic_method", "calls_endpoint"),
            ("detached", "calls_endpoint"),
            ("missing_route", "calls_endpoint"),
            ("duplicate_route", "calls_endpoint"),
            ("shadow_table", "calls_endpoint"),
        ] {
            let count: usize = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='spec.py' AND s.name=?1 AND e.kind=?2", (source,kind), |row| row.get(0)).unwrap();
            assert_eq!(count, 0, "nonexact {source} --{kind}");
        }
        assert_eq!(rows(&conn, "SELECT s.name||'->'||d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='backend.py' AND e.kind='handles' AND s.name IN ('GET /api/records/','POST /api/records/') ORDER BY s.name"), [format!("GET /api/records/->{}",target("backend.py","records")),format!("POST /api/records/->{}",target("backend.py","update"))]);
        assert_eq!(rows(&conn,"SELECT d.id FROM edge_occurrences e JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=e.owner_id WHERE p.path='request.html' AND e.kind='calls_endpoint'"), [target("backend.py","GET /api/records/")]);
        if active {
            for (line, expression, status) in [
                (6, "%missing%", "unresolved"),
                (7, "%duplicate%", "ambiguous"),
                (15, "%/absent/%", "unresolved"),
                (16, "%/api/duplicate/%", "ambiguous"),
                (4, "%stock_cart%", registry),
                (5, "%open_stock_cart%", registry),
            ] {
                let count: usize = conn.query_row("SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='spec.py' AND c.line=?1 AND x.expression LIKE ?2 AND c.status=?3",(line,expression,status),|row|row.get(0)).unwrap();
                if count == 0 {
                    failures.push(format!(
                        "spec.py:{line} {expression}: missing {status} coverage"
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "GridViewSpec bindings:\n{}",
            failures.join("\n")
        );
        drop(conn);
        workspace.assert_cold_equivalent();
        let incremental = workspace.open();
        let cold = reader::open(&workspace.0.join(".forge/cold.sqlite"), &workspace.0).unwrap();
        for sql in ["SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id", "SELECT json_array(path,hex(facts_blob)) FROM local_facts ORDER BY path", "SELECT json_array(domain,owner_id,hex(digest)) FROM domain_commitments ORDER BY domain,owner_id"] {
            assert_eq!(rows(&incremental,sql),rows(&cold,sql),"{sql}");
        }
    };
    observe(true, "resolved", true);

    // A workspace extension adds a project API without replacing the verified catalogue.
    let bundled: toml::Value = toml::from_str(include_str!(
        "../../../src/engine/languages/manifests/gridviewspec.toml"
    ))
    .unwrap();
    assert_eq!(
        bundled["class_hierarchy_receivers"]
            .as_array()
            .unwrap()
            .len(),
        28
    );
    workspace.write("extension.py","from fixture_grid import Widget\nclass Extended(Widget):\n    def known(self): return self.expose\n");
    workspace.delta("extension.py");
    assert_eq!(
        coverage_at(&workspace.open(), "extension.py", 3, "self.expose").0,
        "unresolved"
    );
    workspace.write(".forge/frameworks/gridviewspec.toml","[[class_hierarchy_receivers]]\nbases=['fixture_grid.Widget']\nprovider='fixture_grid.Widget'\nmembers=['expose']\n");
    workspace.delta(".forge/frameworks/gridviewspec.toml");
    assert_eq!(
        coverage_at(&workspace.open(), "extension.py", 3, "self.expose").0,
        "external"
    );
    observe(true, "resolved", true);
    std::fs::remove_file(workspace.0.join(".forge/frameworks/gridviewspec.toml")).unwrap();
    workspace.delta(".forge/frameworks/gridviewspec.toml");
    assert_eq!(
        coverage_at(&workspace.open(), "extension.py", 3, "self.expose").0,
        "unresolved"
    );

    workspace.write(
        "provider.js",
        &provider
            .replace("gv.registerRenderer('stock_cart', draw);", "")
            .replace("gv.registerAction('open_stock_cart', openCart);", ""),
    );
    workspace.delta("provider.js");
    observe(true, "unresolved", true);
    workspace.write("provider.js", &provider.replace("gv.registerRenderer('stock_cart', draw);", "gv.registerRenderer('stock_cart', draw); gv.registerRenderer('stock_cart', openCart);").replace("gv.registerAction('open_stock_cart', openCart);", "gv.registerAction('open_stock_cart', openCart); gv.registerAction('open_stock_cart', draw);"));
    workspace.delta("provider.js");
    observe(true, "ambiguous", true);
    workspace.write("provider.js", provider);
    workspace.delta("provider.js");
    observe(true, "resolved", true);

    let manifest = ".forge/frameworks/gridviewspec.toml";
    let mut pack: toml::Value = toml::from_str(include_str!(
        "../../../src/engine/languages/manifests/gridviewspec.toml"
    ))
    .unwrap();
    pack.as_table_mut()
        .unwrap()
        .insert("replace".into(), true.into());
    let mut removed = pack.clone();
    removed.as_table_mut().unwrap().remove("semantic_rules");
    removed.as_table_mut().unwrap().remove("syntax_rules");
    workspace.write(manifest, &toml::to_string(&removed).unwrap());
    workspace.delta(manifest);
    observe(false, "unresolved", true);
    workspace.write(manifest, "replace=true\n");
    workspace.delta(manifest);
    observe(false, "unresolved", false);
    let neighbor = Workspace::new();
    neighbor.write("families.py", &receivers);
    writer::build(&neighbor.0, &neighbor.db(), None).unwrap();
    assert_eq!(
        coverage_at(&neighbor.open(), "families.py", 3, "self.datasource").0,
        "external"
    );
    std::fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    observe(true, "resolved", true);

    let rewrite = |text: &str| {
        text.replace("grid_view_spec.types.table_v2", "atelier.tables")
            .replace("GridViewTable", "Panel")
            .replace("GridViewDataSource", "Feed")
            .replace("GridViewColumn", "Cell")
            .replace("datasource", "sourcefeed")
            .replace("endpoint", "address")
            .replace("method", "mode")
            .replace("renderer", "painter")
            .replace("extra", "options")
            .replace("action", "command")
            .replace("GridView", "Surface")
            .replace("registerRenderer", "bindPainter")
            .replace("registerAction", "bindCommand")
    };
    for section in ["semantic_rules", "syntax_rules"] {
        for rule in pack[section]
            .as_array_mut()
            .expect("pack provides binding rules")
        {
            // Rewrite configured identities only, preserving typed schema keys.
            let mut value = serde_json::to_value(&*rule).unwrap();
            fn rewrite_values(value: &mut serde_json::Value, rewrite: &impl Fn(&str) -> String) {
                match value {
                    serde_json::Value::String(text) => *text = rewrite(text),
                    serde_json::Value::Array(items) => {
                        for item in items {
                            rewrite_values(item, rewrite);
                        }
                    }
                    serde_json::Value::Object(fields) => {
                        for item in fields.values_mut() {
                            rewrite_values(item, rewrite);
                        }
                    }
                    _ => {}
                }
            }
            rewrite_values(&mut value, &|text| {
                if [
                    "endpoint_projection",
                    "named_binding",
                    "call_definition",
                    "call_reference",
                    "keyword_member",
                    "global_member",
                    "imported",
                    "argument",
                    "keyword",
                    "callable",
                    "python",
                    "javascript",
                ]
                .contains(&text)
                {
                    text.into()
                } else {
                    rewrite(text)
                }
            });
            *rule = toml::Value::try_from(&value).unwrap();
        }
    }
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    workspace.delta(manifest);
    observe(false, "unresolved", true);
    let rewritten_consumer = rewrite(consumer)
        .replace("def painter(", "def renderer(")
        .replace("def command(", "def action(")
        .replace("def dynamic_mode(", "def dynamic_method(");
    workspace.write("spec.py", &rewritten_consumer);
    workspace.delta("spec.py");
    workspace.write("provider.js", &rewrite(provider));
    workspace.delta("provider.js");
    observe(true, "resolved", true);
    std::fs::remove_file(workspace.0.join(manifest)).unwrap();
    workspace.delta(manifest);
    observe(false, "unresolved", true);
    workspace.write("spec.py", consumer);
    workspace.delta("spec.py");
    workspace.write("provider.js", provider);
    workspace.delta("provider.js");
    observe(true, "resolved", true);

    // Observed built-in runtime registrations live in a separately indexed
    // frontend project. Their installation-specific workspace identity is local data.
    for renamed in [false, true] {
        let scoped = Workspace::new();
        let runtime = Workspace::new();
        let rewrite = |text: &str| {
            if renamed {
                text.replace("grid_view_spec.types.table_v2", "atelier.tables")
                    .replace("GridViewColumn", "Cell")
                    .replace("registerRenderer", "bindPainter")
                    .replace("gridviewspec_renderer", "atelier_painter")
            } else {
                text.to_owned()
            }
        };
        scoped.write("forge-mcp.yaml", &format!("roots: [.]\nlinked_workspaces:\n  - name: runtime\n    path: '{}'\n    roots: [.]\n", runtime.0.display()));
        scoped.write(
            "pyproject.toml",
            "[project]\nname='consumer'\ndependencies=['grid-view-spec']\n",
        );
        runtime.write("pyproject.toml", "[project]\nname='grid-view-spec'\n");
        runtime.write("frontend/package.json", "{\"name\":\"runtime-provider\"}");
        let provider_path = "frontend/src/runtime/renderers/builtins.ts";
        let runtime_source = rewrite("import {registerRenderer} from '../../grid-view/registry-api';\nfunction moneyRenderer() { return 'money'; }\nfunction linkRenderer() { return 'link'; }\nexport function initBuiltinRenderers() { registerRenderer('money', moneyRenderer); }\n");
        runtime.write(provider_path, &runtime_source);
        let consumer = rewrite("from grid_view_spec.types.table_v2 import GridViewColumn\ndef render(): return GridViewColumn(renderer='money')\ndef missing(): return GridViewColumn(renderer='r')\ndef dynamic(): return GridViewColumn(renderer=selected)\ndef shadow(GridViewColumn): return GridViewColumn(renderer='money')\n");
        scoped.write("consumer.py", &consumer);
        runtime.write("src/consumer.py", &consumer);
        let mut policy: toml::Value = toml::from_str(include_str!(
            "../../../src/engine/languages/manifests/gridviewspec.toml"
        ))
        .unwrap();
        for rule in policy["syntax_rules"].as_array_mut().unwrap() {
            if rule["id"].as_str() == Some("renderer-reference") {
                rule["operation"]["binding"].as_table_mut().unwrap().insert("provider_scopes".into(), vec![toml::Value::try_from(serde_json::json!({"workspace":"runtime","project":"frontend","package":"runtime-provider"})).unwrap()].into());
            }
        }
        let policy = rewrite(&toml::to_string(&policy).unwrap());
        let local_manifest = ".forge/frameworks/gridviewspec.toml";
        scoped.write(local_manifest, &policy);
        runtime.write(local_manifest, &policy);
        writer::build(&scoped.0, &scoped.db(), None).unwrap();
        let check = |expected: Option<&str>, status: &str| {
            let conn = scoped.open();
            commitments::verify(&conn).unwrap();
            assert!(rows(&conn, "SELECT message FROM errors").is_empty());
            let targets: Vec<String> = conn.prepare("SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='consumer.py' AND s.name='render' AND e.kind='references' ORDER BY d.id").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
            let wanted: Vec<String> = expected.map(|name| conn.query_row("SELECT n.id FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path='[runtime]/frontend/src/runtime/renderers/builtins.ts' AND n.name=?1", [name], |row| row.get(0)).unwrap()).into_iter().collect();
            assert_eq!(
                targets, wanted,
                "configured runtime exact canonical callback, renamed={renamed}"
            );
            assert_eq!(coverage_at(&conn, "consumer.py", 2, "money").0, status);
            for function in ["missing", "dynamic", "shadow"] {
                let count: i64 = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path='consumer.py' AND s.name=?1 AND e.kind='references'", [function], |row| row.get(0)).unwrap();
                assert_eq!(count, 0, "no runtime guess for {function}");
            }
            drop(conn);
            scoped.assert_cold_equivalent();
        };
        check(Some("moneyRenderer"), "resolved");
        assert_eq!(
            coverage_at(&scoped.open(), "[runtime]/src/consumer.py", 2, "money").0,
            "resolved",
            "same-workspace Python/frontend boundary also requires explicit import"
        );
        // Unchanged consumers must replay a callback-only provider mutation.
        runtime.write(
            provider_path,
            &runtime_source.replace("('money', moneyRenderer)", "('money', linkRenderer)"),
        );
        scoped.delta("[runtime]/frontend/src/runtime/renderers/builtins.ts");
        check(Some("linkRenderer"), "resolved");
        runtime.write(
            provider_path,
            &format!(
                "{runtime_source}\nexport function duplicate() {{ {}('money', linkRenderer); }}\n",
                if renamed {
                    "bindPainter"
                } else {
                    "registerRenderer"
                }
            ),
        );
        scoped.delta("[runtime]/frontend/src/runtime/renderers/builtins.ts");
        check(None, "ambiguous");
        std::fs::remove_file(runtime.0.join(provider_path)).unwrap();
        scoped.delta("[runtime]/frontend/src/runtime/renderers/builtins.ts");
        check(None, "unresolved");
        runtime.write(provider_path, &runtime_source);
        scoped.delta("[runtime]/frontend/src/runtime/renderers/builtins.ts");
        check(Some("moneyRenderer"), "resolved");
        scoped.write(
            local_manifest,
            &policy.replace(
                "package = \"runtime-provider\"",
                "package = \"unrelated-provider\"",
            ),
        );
        scoped.delta(local_manifest);
        check(None, "unresolved");
        scoped.write(local_manifest, &policy);
        scoped.delta(local_manifest);
        check(Some("moneyRenderer"), "resolved");
        scoped.write("consumer.py", &format!("{consumer}\n# cached provider\n"));
        scoped.delta("consumer.py");
        check(Some("moneyRenderer"), "resolved");
    }
}
