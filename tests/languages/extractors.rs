#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::ast;

#[cfg(feature = "lang-rust")]
#[test]
fn test_rust_ast_extractor() {
    let source = r#"
use std::collections::HashMap;

pub struct Config {
    pub host: String,
    pub port: u16,
}

pub enum Status {
    Active,
    Inactive,
}

impl Config {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}
"#;
    let facts = ast::extract("test.rs", "rust", source).expect("rust extraction failed");
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "struct" && n.name == "Config"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "enum" && n.name == "Status"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "new"));
    let constructor = facts
        .nodes
        .iter()
        .find(|n| n.name == "new")
        .expect("new method missing");
    assert_eq!(constructor.details["receiver_type"], "Config");
    assert!(constructor.details.get("is_method").is_none());
    assert_eq!(constructor.details["is_static"], true);
    assert!(ast::parser("sql", "schema.sql").is_err());
    let mut public_facts = contextunity_forge_mcp::core::models::Facts::default();
    contextunity_forge_mcp::engine::languages::parse_file(
        &contextunity_forge_mcp::engine::languages::sql::SQL,
        "public.sql",
        "CREATE TABLE public_test (id INT)",
        "public",
        &mut public_facts,
    )
    .unwrap();
    assert!(public_facts
        .nodes
        .iter()
        .any(|node| node.name == "public_test"));
    for (sql, name, relation) in [
        (
            "SELECT * FROM public.orders",
            "public.orders",
            "queries_table",
        ),
        (
            "SELECT * FROM orders, customers",
            "customers",
            "queries_table",
        ),
        (
            "BEGIN; PRAGMA x=1; SELECT * FROM orders",
            "orders",
            "queries_table",
        ),
        ("SET search_path TO shop; CREATE TABLE batch_table (id INT)", "batch_table", "defines_table"),
        (
            "INSERT OR REPLACE INTO orders VALUES (1)",
            "orders",
            "mutates_table",
        ),
        ("UPDATE orders SET id = 1", "orders", "mutates_table"),
        ("DELETE FROM orders", "orders", "mutates_table"),
        (
            "CREATE TABLE IF NOT EXISTS orders (id INT)",
            "orders",
            "defines_table",
        ),
    ] {
        assert!(
            contextunity_forge_mcp::engine::languages::sql::extract_embedded_sql_tables(sql)
                .contains(&(name.to_owned(), relation))
        );
    }

    let workspace = super::support::Workspace::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = 'fixture'\nversion = '0.1.0'\n[dependencies]\nsqlx = '0.8'\n",
    );
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE customers (id INTEGER);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE shop_override (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id) ON UPDATE CASCADE, PRIMARY KEY (id));\nCREATE TABLE a.same (id INTEGER); CREATE TABLE b.same (id INTEGER); CREATE TABLE \"x.y\" (id INT); CREATE TABLE x.y (id INT); CREATE TABLE SET (id INT); CREATE TABLE CASCADE (id INT);\nCREATE VIEW order_view AS SELECT * FROM orders;\n");
    workspace.write(
        "migration.sql",
        "ALTER TABLE orders ADD COLUMN note TEXT;\n",
    );
    workspace.write("queries.rs", "#[derive(sqlx::FromRow)] struct OrderRow { id: i64 }\nfn load() { let _ = \"SELECT * FROM orders\"; }\nfn save() { let _ = \"INSERT INTO orders (id) VALUES (1)\"; }\nfn remove() { let _ = \"DELETE FROM orders WHERE id = 1\"; }\nfn customers() { let _ = \"SELECT * FROM customers\"; }\nfn macro_load() { let _ = sqlx::query!(\"SELECT * FROM orders\"); }\nfn typed_load() { let _ = sqlx::query_as!(OrderRow, \"SELECT * FROM orders\"); }\nfn inline_schema() { let _ = \"CREATE TABLE local_data (id INTEGER)\"; }\n");
    workspace.write("foreign.rs", "struct Imported;\nuse external::Imported;\nfn foreign() { let _ = sqlx::query_as!(Imported, \"SELECT * FROM orders\"); }\n");
    workspace.write("qualified.rs", "fn exact() { let _ = \"SELECT * FROM a.same\"; }\nfn ambiguous_short() { let _ = \"SELECT * FROM same\"; }\nfn batch() { let _ = \"BEGIN; PRAGMA x=1; SELECT * FROM orders, customers\"; }\nfn inline_create() { let _ = \"CREATE TABLE c.inline_qual (id INT)\"; }\nfn inline_query() { let _ = \"SELECT * FROM inline_qual\"; }\n");
    workspace.write("upsert.rs", "fn upsert() { let _ = \"INSERT INTO orders VALUES (1) ON CONFLICT DO UPDATE SET id=1\"; }\n");
    workspace.write("ambiguous.rs", "mod a { struct Shared; }\nmod b { struct Shared; }\nfn ambiguous() { let _ = sqlx::query_as!(Shared, \"SELECT * FROM orders\"); }\n");
    workspace.write("shadow.rs", "mod sqlx {}\nstruct Shadow;\nfn shadow() { let _ = sqlx::query_as!(Shadow, \"SELECT * FROM orders\"); }\n");
    workspace.write("other.rs", "struct Other;\nfn other() { let _ = custom::query_as!(Other, \"SELECT * FROM orders\"); }\n");
    workspace.write(
        "derive_only.rs",
        "#[derive(sqlx::FromRow)] struct DeriveOnly { id: i64 }\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("queries.py", "from django.db import models\nfrom sqlalchemy.orm import DeclarativeBase\ndef py_load():\n    return \"SELECT * FROM orders\"\n\nclass Order(DeclarativeBase):\n    __tablename__ = 'orders'\n\nclass FalseSqlAlchemy:\n    __tablename__ = 'orders'\n\nclass Item(models.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'shop'\nversion = '0.1'\ndependencies = ['django', 'sqlalchemy']\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/models.py",
        "from django.db import models as dbmodels\nclass Cart(dbmodels.Model):\n    pass\nclass Override(dbmodels.Model):\n    class Meta:\n        db_table = 'orders'\nclass NonModel:\n    class Meta:\n        db_table = 'orders'\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("shop/reassigned.py", "from django.db import models as dbmodels\ndbmodels = object()\nclass Reassigned(dbmodels.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/fake.py",
        "from local import models\nclass Fake(models.Model):\n    pass\n",
    );
    #[cfg(feature = "lang-typescript")]
    workspace.write(
        "queries.ts",
        "function ts_load() { return \"SELECT * FROM orders\"; }\n",
    );
    let db = workspace.build();
    let qualified: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('a.same','b.same')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(qualified, 2, "qualified tables retain distinct identities");
    let dotted: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('\"x.y\"','x.y')", [], |r| r.get(0)).unwrap();
    assert_eq!(dotted, 2, "quoted and qualified identities differ");
    let fake_dml: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name IN ('upsert','item') AND d.name IN ('SET','CASCADE') AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(fake_dml, 0, "UPDATE clauses are not standalone table writes");
    for (source, target, expected) in [
        ("exact", "a.same", 1),
        ("exact", "b.same", 0),
        ("ambiguous_short", "a.same", 0),
        ("ambiguous_short", "b.same", 0),
        ("batch", "customers", 1),
        ("batch", "orders", 1),
        ("inline_query", "c.inline_qual", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind = 'table' AND name = 'orders'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1, "SQL schema tables must be indexed");
    let inline: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND name='local_data'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inline, 1, "inline CREATE must synthesize a table");
    let inline_link: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='inline_schema' AND d.name='local_data' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(inline_link, 1, "inline CREATE must link its owner");
    let model: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='OrderRow' AND d.name='orders' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        model, 1,
        "query_as must link its unambiguous local result type"
    );
    let invalid: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('Shared','Imported','Shadow','Other','DeriveOnly') AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(
        invalid, 0,
        "ambiguous, imported, shadowed, unconfigured and derive-only identities must remain unresolved"
    );
    let mut expected = vec![
        ("order_view", "queries_table"),
        ("load", "queries_table"),
        ("save", "mutates_table"),
        ("remove", "mutates_table"),
        ("macro_load", "queries_table"),
        ("typed_load", "queries_table"),
        ("item", "references_table"),
    ];
    #[cfg(feature = "lang-python")]
    expected.push(("py_load", "queries_table"));
    #[cfg(feature = "lang-python")]
    expected.extend([("Order", "defines_table"), ("Item", "defines_table")]);
    #[cfg(feature = "lang-typescript")]
    expected.push(("ts_load", "queries_table"));
    for (source, kind) in expected {
        let count: i64 = db.query_row(
            "SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name='orders' AND e.kind=?2",
            [source, kind], |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1, "missing {kind} edge from {source}");
    }
    let columns: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='column' AND qualname IN ('orders.id', 'item.order_id')", [], |row| row.get(0)).unwrap();
    assert_eq!(columns, 2);
    let altered: i64 = db.query_row("SELECT count(*) FROM nodes WHERE kind='column' AND qualname='orders.note' AND path_id=(SELECT path_id FROM path_dictionary WHERE path='migration.sql')", [], |row| row.get(0)).unwrap();
    assert_eq!(altered, 1, "ALTER TABLE must index the added column");
    #[cfg(feature = "lang-python")]
    {
        let defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Cart' AND d.name='shop_cart' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(defaults, 1, "Django default table should link");
        let false_defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Fake' AND d.name='shop_fake' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(false_defaults, 0, "local models must not imply Django ORM");
        let override_edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Override' AND d.name='shop_override' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(override_edges, 0, "explicit Meta table overrides default");
        let invalid_models: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('NonModel','Reassigned','FalseSqlAlchemy') AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            invalid_models, 0,
            "unproven Django identities remain unresolved"
        );
    }
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &contextunity_forge_mcp::core::response::ResponsePolicy::default(),
        Some(100),
        0,
        None,
        None,
    )
    .unwrap();
    let impact = contextunity_forge_mcp::db::traversal::traverse_with_options(
        &db,
        "schema.sql:table:orders",
        &contextunity_forge_mcp::db::traversal::TraversalOptions {
            depth: 1,
            inbound: true,
            mode: None,
            edge_types: None,
            page: &page,
        },
    )
    .unwrap();
    let reached = impact["nodes"]["items"].as_array().unwrap();
    assert!(
        reached.iter().any(|node| node["name"] == "order_view"),
        "SQL impact must include views: {impact}"
    );
    for name in ["load", "save", "remove", "item"] {
        assert!(
            reached.iter().any(|node| node["name"] == name),
            "SQL impact must include {name}: {impact}"
        );
    }
    #[cfg(feature = "lang-python")]
    for name in ["Order", "Item", "py_load"] {
        assert!(
            reached.iter().any(|node| node["name"] == name),
            "SQL impact must include {name}: {impact}"
        );
    }
    drop(db);
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id), PRIMARY KEY (id));\nCREATE VIEW order_view AS SELECT * FROM orders;\nCREATE TABLE customers (id INTEGER);\n");
    workspace.cold_parity("schema.sql");
    let delta = workspace.open();
    let cold = contextunity_forge_mcp::db::reader::open(
        &workspace.0.join(".forge/cold.sqlite"),
        &workspace.0,
    )
    .unwrap();
    let root = |conn: &rusqlite::Connection| -> String {
        conn.query_row(
            "SELECT value FROM metadata WHERE key='output_root'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        root(&delta),
        root(&cold),
        "SQL cold and delta commitments must agree"
    );
    let without_sqlx = super::support::Workspace::new();
    without_sqlx.write("schema.sql", "CREATE TABLE orders (id INTEGER);\n");
    without_sqlx.write("query.rs", "struct NotActive;\nfn load() { let _ = sqlx::query_as!(NotActive, \"SELECT * FROM orders\"); }\n");
    let db = without_sqlx.build();
    let edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='NotActive' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(edges, 0, "undeclared SQLx must not activate its model rule");
}

#[cfg(feature = "lang-go")]
#[test]
fn test_go_ast_extractor() {
    let source = r#"
package main

import "fmt"

type Server struct {
    port int
}

func (s *Server) Start() {
    fmt.Println(s.port)
}

func Run() {
    s := &Server{port: 8080}
    s.Start()
}
"#;
    let facts = ast::extract("main.go", "go", source).expect("go extraction failed");
    assert!(facts.nodes.iter().any(|n| n.name == "Server"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "Start"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "Run"));
}

#[cfg(feature = "lang-python")]
#[test]
fn python_member_access_callee_does_not_emit_duplicate_reference() {
    let source = r#"
class ServiceClient:
    def fetch(self, endpoint: str) -> dict:
        return self._send_request(endpoint)

    def _send_request(self, endpoint: str) -> dict:
        return {"status": "ok"}
"#;
    let facts = ast::extract("service.py", "python", source).expect("python extraction failed");
    let send_request_refs: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.expression == "self._send_request")
        .collect();
    assert_eq!(
        send_request_refs.len(),
        1,
        "Expected exactly 1 relation for self._send_request, got {:?}",
        send_request_refs
    );
    assert_eq!(send_request_refs[0].kind, "calls");
}
