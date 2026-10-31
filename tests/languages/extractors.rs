#![cfg(any(
    feature = "lang-python",
    feature = "lang-rust",
    feature = "lang-typescript",
    feature = "lang-go"
))]

use contextunity_forge_mcp::engine::{ast, linker};
#[cfg(feature = "lang-rust")]
use forge_index::languages::rust::kind;

#[cfg(feature = "lang-python")]
mod scope_allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Probe;

    thread_local! {
        static SELECTED: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    #[global_allocator]
    static ALLOCATOR: Probe = Probe;

    fn record(size: usize, align: usize, pointer: *mut u8) {
        if align == 1 && !pointer.is_null() {
            let _ = SELECTED.try_with(|selected| {
                let (expected, count) = selected.get();
                if expected != 0 && size == expected {
                    selected.set((expected, count + 1));
                }
            });
        }
    }

    unsafe impl GlobalAlloc for Probe {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            record(layout.size(), layout.align(), pointer);
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, size) };
            record(size, layout.align(), pointer);
            pointer
        }
    }

    pub(super) fn measure<T>(size: usize, operation: impl FnOnce() -> T) -> (T, usize) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                SELECTED.with(|selected| selected.set((0, 0)));
            }
        }
        SELECTED.with(|selected| {
            assert_eq!(selected.get().0, 0);
            selected.set((size, 0));
        });
        let reset = Reset;
        let result = operation();
        let count = SELECTED.with(|selected| selected.get().1);
        drop(reset);
        (result, count)
    }
}

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

    #[cfg(feature = "lang-rust")]
    {
        let rust_kind = kind("function_item");
        assert_eq!(rust_kind, Some("function"));
    }

    // Contract baseline for LS-001: Rust std prelude trait resolution and declarative manifests.
    // Standard prelude traits (Clone, Default, Iterator) must resolve or classify as external via rust-std.toml.
    let std_source = r#"
fn run() {
    let _default: Result<i32, ()> = Result::default();
    let text = String::from("hello");
    let _cloned = text.clone();
    let _default_trait: i32 = Default::default();
    let numbers = Vec::new();
    let _iter = numbers.iter();
}
"#;
    let std_facts =
        ast::extract("std_prelude.rs", "rust", std_source).expect("std_prelude extraction failed");
    let std_graph = linker::link(&std::collections::BTreeMap::from([(
        "std_prelude.rs".to_owned(),
        std_facts,
    )]));
    let default_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Result::default")
        .expect("missing coverage for Result::default");
    assert_eq!(
        default_coverage.status, "unresolved",
        "Result does not implement Default: {default_coverage:#?}"
    );
    let string_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "String::from")
        .expect("missing coverage for String::from");
    assert_eq!(
        string_coverage.status, "external",
        "String::from must resolve to external Rust standard library prelude via rust-std.toml: {string_coverage:#?}"
    );
    let clone_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "text.clone")
        .expect("missing coverage for text.clone");
    assert_eq!(
        clone_coverage.status, "external",
        "text.clone must resolve to external Rust standard library prelude via rust-std.toml: {clone_coverage:#?}"
    );
    let default_trait_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Default::default")
        .expect("missing coverage for Default::default");
    assert_eq!(
        default_trait_coverage.status, "external",
        "Default::default must resolve to external Rust standard library prelude via rust-std.toml: {default_trait_coverage:#?}"
    );
    let vec_new_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "Vec::new")
        .expect("missing coverage for Vec::new");
    assert_eq!(
        vec_new_coverage.status, "external",
        "Vec::new must resolve to external: {vec_new_coverage:#?}"
    );
    let iter_coverage = std_graph
        .coverage
        .iter()
        .find(|c| c.expression == "numbers.iter")
        .expect("missing coverage for numbers.iter");
    assert_eq!(
        iter_coverage.status, "external",
        "numbers.iter must resolve to external: {iter_coverage:#?}"
    );

    // Verify Cargo third-party ecosystem crate routing (Task 2 contract)
    let cargo_source = r#"
use rusqlite::Connection;
use tree_sitter::Parser;

fn run_cargo_crates() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("SELECT 1", []).unwrap();
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
}
"#;
    let cargo_facts = ast::extract("cargo_test.rs", "rust", cargo_source).unwrap();
    let cargo_graph = linker::link(&std::collections::BTreeMap::from([(
        "cargo_test.rs".to_owned(),
        cargo_facts,
    )]));
    let execute_cov = cargo_graph
        .coverage
        .iter()
        .find(|c| c.expression == "conn.execute")
        .expect("missing coverage for conn.execute");
    assert_eq!(
        execute_cov.status, "external",
        "conn.execute must resolve to external via rust-cargo.toml declarative pack: {execute_cov:#?}"
    );
    let parser_cov = cargo_graph
        .coverage
        .iter()
        .find(|c| c.expression == "parser.set_language")
        .expect("missing coverage for parser.set_language");
    assert_eq!(
        parser_cov.status, "external",
        "parser.set_language must resolve to external via rust-cargo.toml declarative pack: {parser_cov:#?}"
    );

    let iso_source = r#"
fn check_string_trait_isolation() {
    let s = String::new();
    s.clone();
    s.next();
}
"#;
    let iso_facts = ast::extract("iso_test.rs", "rust", iso_source).unwrap();
    let iso_graph = linker::link(&std::collections::BTreeMap::from([(
        "iso_test.rs".to_owned(),
        iso_facts,
    )]));
    let next_cov = iso_graph
        .coverage
        .iter()
        .find(|c| c.expression == "s.next")
        .expect("missing coverage for s.next");
    assert_eq!(
        next_cov.status, "unresolved",
        "s.next must remain unresolved because String does not implement Iterator: {next_cov:#?}"
    );
    for (adapter, preserves_item) in [
        ("filter(|_| true)", true),
        ("take(1)", true),
        ("skip(1)", true),
        ("step_by(1)", true),
        ("collect::<Vec<_>>().into_iter()", true),
        ("map(|_| B)", false),
        ("map(convert)", false),
        ("enumerate()", false),
        ("map(|item| item)", false),
    ] {
        let source = format!(
            "struct A; impl A {{ fn finish(&self) {{}} }}
             struct B; impl B {{ fn finish(&self) {{}} }}
             fn convert(_: A) -> B {{ B }}
             trait Finish {{ fn finish(&self); }}
             impl Finish for (usize, A) {{ fn finish(&self) {{}} }}
             fn run(items: Vec<A>) {{
                 let mut adapted = items.into_iter().{adapter};
                 let item = adapted.next().unwrap();
                 item.finish();
             }}"
        );
        let facts = ast::extract("iterator.rs", "rust", &source).unwrap();
        assert!(facts.errors.is_empty(), "{adapter}: {facts:#?}");
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "finish" && node.details["receiver_type"] == "A")
            .unwrap()
            .id
            .clone();
        let graph = linker::link(&std::collections::BTreeMap::from([(
            "iterator.rs".to_owned(),
            facts,
        )]));
        assert_eq!(
            graph.edges.iter().any(|edge| edge.kind == "calls"
                && edge.evidence == "item.finish"
                && edge.dst == target),
            preserves_item,
            "{adapter}: {graph:#?}"
        );
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == "item.finish")
            .unwrap();
        assert_eq!(
            coverage.status,
            if preserves_item { "resolved" } else { "unresolved" },
            "{adapter}: {coverage:#?}"
        );
    }
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
        (
            "SET search_path TO shop; CREATE TABLE batch_table (id INT)",
            "batch_table",
            "defines_table",
        ),
        (
            "INSERT OR REPLACE INTO orders VALUES (1)",
            "orders",
            "mutates_table",
        ),
        ("UPDATE orders SET id = 1", "orders", "mutates_table"),
        ("COMMIT; SELECT * FROM orders", "orders", "queries_table"),
        ("ROLLBACK; UPDATE orders AS o SET id = 1", "orders", "mutates_table"),
        ("UPDATE ONLY orders o SET id = 1", "orders", "mutates_table"),
        ("WITH source AS (SELECT * FROM customers) UPDATE ONLY (orders) AS o SET id=1", "orders", "mutates_table"),
        ("SELECT * FROM orders o JOIN customers c ON c.id=o.id, inventory i", "inventory", "queries_table"),
        ("SELECT * FROM orders o, inventory i JOIN customers c ON c.id=i.id", "customers", "queries_table"),
        ("WITH temporary AS (SELECT * FROM orders) SELECT * FROM temporary JOIN inventory ON 1=1", "orders", "queries_table"),
        ("COMMIT; SELECT * FROM orders WHERE note = $$semi;colon$$", "orders", "queries_table"),
        ("SET x='semi;SELECT fake'; /* ; UPDATE ignored */ SELECT * FROM orders", "orders", "queries_table"),
        ("BEGIN; -- ; UPDATE ignored\nWITH rows AS (SELECT * FROM orders) SELECT * FROM rows", "orders", "queries_table"),
        ("SET x=$tag$; UPDATE ignored$tag$; SELECT * FROM orders", "orders", "queries_table"),
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

    for (sql, expected) in [
        ("WITH temporary(id) AS (SELECT * FROM temporary) SELECT * FROM temporary", vec!["temporary"]),
        ("WITH RECURSIVE temporary(id) AS (SELECT * FROM temporary) SELECT * FROM temporary", vec![]),
        ("WITH temporary(id) AS (SELECT * FROM orders) SELECT * FROM public.temporary", vec!["orders", "public.temporary"]),
        ("WITH first_cte AS (SELECT * FROM orders), second_cte(id) AS (SELECT * FROM first_cte) SELECT * FROM second_cte", vec!["orders"]),
    ] {
        let actual = contextunity_forge_mcp::engine::languages::sql::extract_embedded_sql_tables(sql);
        assert_eq!(actual.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), expected, "{sql}");
    }

    // Exercise the real literal producers, including prose that previously
    // matched SQL keywords inside unrelated words.
    let mut embedded = vec![(
        "embedded.rs",
        "rust",
        r##"fn query() { let _ = r#"SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows"#; }
fn prose() { let _ = "information was updated and transformed"; }"##,
    )];
    #[cfg(feature = "lang-python")]
    embedded.push((
        "embedded.py",
        "python",
        "def query():\n    return \"SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows\"\ndef prose():\n    '''information was updated and transformed'''\n",
    ));
    #[cfg(feature = "lang-typescript")]
    embedded.push((
        "embedded.ts",
        "typescript",
        "function query() { return `SET x=$tag$; UPDATE ignored$tag$; /* ; */ WITH rows AS (SELECT * FROM orders) SELECT * FROM rows`; }\nfunction prose() { return 'information was updated and transformed'; }\n",
    ));
    for (path, language, source) in embedded {
        let facts = ast::extract(path, language, source).unwrap();
        let queries: Vec<_> = facts.references.iter().filter(|r| r.kind == "queries_table").collect();
        assert_eq!(queries.len(), 1, "{path}: {queries:?}");
        assert_eq!(queries[0].expression, "orders", "{path}");
        assert!(!facts.references.iter().any(|r| r.kind == "mutates_table"), "{path}");
    }

    let workspace = super::support::Workspace::new();
    workspace.write("more_tables.sql", "CREATE TABLE inventory (id INTEGER); CREATE TABLE factory_orders (id INTEGER); CREATE TABLE temporary (id INTEGER);");
    workspace.write(
        "quoted_schema.sql",
        "CREATE TABLE \"quoted_table\" (id INTEGER);",
    );
    workspace.write(
        "case_schema.sql",
        "CREATE TABLE \"Foo\" (id INTEGER); CREATE TABLE \"foo\" (id INTEGER);",
    );
    workspace.write(
        "Cargo.toml",
        "[package]\nname = 'fixture'\nversion = '0.1.0'\n[dependencies]\nsqlx = '0.8'\n",
    );
    workspace.write("schema.sql", "CREATE TABLE orders (id INTEGER PRIMARY KEY);\nCREATE TABLE customers (id INTEGER);\nCREATE TABLE shop_cart (id INTEGER);\nCREATE TABLE shop_fake (id INTEGER);\nCREATE TABLE shop_override (id INTEGER);\nCREATE TABLE item (id INTEGER, order_id INTEGER REFERENCES orders(id) ON UPDATE CASCADE, PRIMARY KEY (id));\nCREATE TABLE a.same (id INTEGER); CREATE TABLE b.same (id INTEGER); CREATE TABLE \"x.y\" (id INT); CREATE TABLE x.y (id INT); CREATE TABLE SET (id INT); CREATE TABLE CASCADE (id INT);\nCREATE VIEW order_view AS SELECT * FROM orders;\n");
    workspace.write(
        "extra_schema.sql",
        "CREATE TABLE shop_default (id INTEGER);",
    );
    workspace.write(
        "migration.sql",
        "ALTER TABLE orders ADD COLUMN note TEXT;\n",
    );
    workspace.write("queries.rs", "#[derive(sqlx::FromRow)] struct OrderRow { id: i64 }\nfn load() { let _ = \"SELECT * FROM orders\"; }\nfn save() { let _ = \"INSERT INTO orders (id) VALUES (1)\"; }\nfn remove() { let _ = \"DELETE FROM orders WHERE id = 1\"; }\nfn customers() { let _ = \"SELECT * FROM customers\"; }\nfn macro_load() { let _ = sqlx::query!(\"SELECT * FROM orders\"); }\nfn typed_load() { let _ = sqlx::query_as!(OrderRow, \"SELECT * FROM orders\"); }\nfn inline_schema() { let _ = \"CREATE TABLE local_data (id INTEGER)\"; }\n");
    workspace.write("foreign.rs", "struct Imported;\nuse external::Imported;\nfn foreign() { let _ = sqlx::query_as!(Imported, \"SELECT * FROM orders\"); }\n");
    workspace.write("qualified.rs", "fn exact() { let _ = \"SELECT * FROM a.same\"; }\nfn ambiguous_short() { let _ = \"SELECT * FROM same\"; }\nfn batch() { let _ = \"BEGIN; PRAGMA x=1; SELECT * FROM orders, customers\"; }\nfn inline_create() { let _ = \"CREATE TABLE c.inline_qual (id INT)\"; }\nfn inline_query() { let _ = \"SELECT * FROM inline_qual\"; }\n");
    workspace.write("upsert.rs", "fn upsert() { let _ = \"INSERT INTO orders VALUES (1) ON CONFLICT DO UPDATE SET id=1\"; }\n");
    workspace.write("patterns.rs", "fn rollback_update() { let _ = \"ROLLBACK; UPDATE ONLY orders AS o SET id=1\"; }\nfn mixed_from() { let _ = \"SELECT * FROM orders o JOIN customers c ON c.id=o.id, inventory i\"; }\nfn cte_query() { let _ = \"WITH temporary AS (SELECT * FROM orders) SELECT * FROM temporary JOIN inventory ON 1=1\"; }\n");
    #[cfg(feature = "lang-python")]
    workspace.write("identifiers.py", "def quoted_orders():\n    return 'SELECT * FROM \"orders\"'\ndef unquoted_table():\n    return 'SELECT * FROM quoted_table'\ndef cte_columns():\n    return 'WITH temporary(id) AS (SELECT id FROM orders) SELECT * FROM temporary'\n");
    workspace.write("ambiguous.rs", "mod a { struct Shared; }\nmod b { struct Shared; }\nfn ambiguous() { let _ = sqlx::query_as!(Shared, \"SELECT * FROM orders\"); }\n");
    workspace.write("shadow.rs", "mod sqlx {}\nstruct Shadow;\nfn shadow() { let _ = sqlx::query_as!(Shadow, \"SELECT * FROM orders\"); }\n");
    workspace.write("other.rs", "struct Other;\nfn other() { let _ = custom::query_as!(Other, \"SELECT * FROM orders\"); }\n");
    workspace.write(
        "derive_only.rs",
        "#[derive(sqlx::FromRow)] struct DeriveOnly { id: i64 }\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write("queries.py", "from django.db import models\nfrom sqlalchemy.orm import DeclarativeBase\ndef py_load():\n    return \"SELECT * FROM orders\"\n\nclass Base(DeclarativeBase):\n    pass\nclass Order(Base):\n    __tablename__ = 'orders'\n\nclass FalseSqlAlchemy:\n    __tablename__ = 'orders'\n\nclass Item(models.Model):\n    class Meta:\n        db_table = 'orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("quoted.py", "def quoted():\n    return 'SELECT * FROM \"x.y\"'\ndef qualified():\n    return 'SELECT * FROM x.y'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("case_query.py", "def upper_case():\n    return 'SELECT * FROM \"Foo\"'\ndef lower_case():\n    return 'SELECT * FROM \"foo\"'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory.py", "from sqlalchemy.orm import declarative_base as make_base\nBase = make_base()\nclass FactoryOrder(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory_qualified.py", "import sqlalchemy.orm as orm\nBase = orm.declarative_base()\nclass QualifiedFactory(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write("factory_invalid.py", "from sqlalchemy.orm import declarative_base as make_base\nmake_base = object()\nBase = make_base()\nclass ReboundFactory(Base):\n    __tablename__ = 'factory_orders'\n");
    #[cfg(feature = "lang-python")]
    workspace.write(".forge/frameworks/other-tables.toml", "[[semantic_rules]]\nid = 'other-table'\nnamespace = 'other.tables'\nscope = 'project'\noperation = { kind = 'class_attribute_to_table', language = 'python', attribute = 'table_name', container = 'Config' }\n");
    #[cfg(feature = "lang-python")]
    workspace.write(".forge/frameworks/standalone-default.toml", "[[semantic_rules]]\nid = 'default'\nnamespace = 'other.default'\nscope = 'project'\noperation = { kind = 'class_default_table', language = 'python', bases = ['fixture.CustomBase'] }\n");
    #[cfg(feature = "lang-python")]
    workspace.write(
        "shop/default.py",
        "# class Commented(CustomBase): pass\n\"\"\"class Quoted(CustomBase): pass\"\"\"\nfrom fixture import CustomBase\nclass Empty:\n    pass\nclass Default(CustomBase):\n    pass\n",
    );
    #[cfg(feature = "lang-python")]
    workspace.write(
        "other_model.py",
        "class Generic:\n    class Config:\n        table_name = 'orders'\n",
    );
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
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("quoted_orders", "orders", 1),
        ("unquoted_table", "quoted_table", 1),
        ("cte_columns", "temporary", 0),
        ("cte_columns", "orders", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.name=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("upper_case", "\"Foo\"", 1),
        ("upper_case", "\"foo\"", 0),
        ("lower_case", "\"foo\"", 1),
        ("lower_case", "\"Foo\"", 0),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "quoted case: {source} -> {target}");
    }
    let fake_cte: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='cte_query' AND d.name='temporary' AND e.kind='queries_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        fake_cte, 0,
        "CTE name must not be resolved to a physical table"
    );
    #[cfg(feature = "lang-python")]
    {
        let edge: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='FactoryOrder' AND d.name='factory_orders' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            edge, 1,
            "declarative_base factory must prove model ancestry"
        );
        let qualified: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='QualifiedFactory' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(qualified, 1, "qualified imported factory must be resolved");
        let rebound: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='ReboundFactory' AND e.kind='defines_table'", [], |r| r.get(0)).unwrap();
        assert_eq!(rebound, 0, "rebound factory must fail closed");
    }
    let qualified: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('a.same','b.same')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(qualified, 2, "qualified tables retain distinct identities");
    let dotted: i64 = db
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='table' AND qualname IN ('\"x.y\"','x.y')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dotted, 2, "quoted and qualified identities differ");
    #[cfg(feature = "lang-python")]
    for (source, target, expected) in [
        ("quoted", "\"x.y\"", 1),
        ("quoted", "x.y", 0),
        ("qualified", "x.y", 1),
        ("qualified", "\"x.y\"", 0),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let fake_dml: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name IN ('upsert','item') AND d.name IN ('SET','CASCADE') AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        fake_dml, 0,
        "UPDATE clauses are not standalone table writes"
    );
    for (source, target, expected) in [
        ("exact", "a.same", 1),
        ("exact", "b.same", 0),
        ("ambiguous_short", "a.same", 0),
        ("ambiguous_short", "b.same", 0),
        ("batch", "customers", 1),
        ("batch", "orders", 1),
        ("inline_query", "c.inline_qual", 1),
        ("mixed_from", "inventory", 1),
    ] {
        let count: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name=?1 AND d.qualname=?2 AND e.kind='queries_table'", [source,target], |r| r.get(0)).unwrap();
        assert_eq!(count, expected, "{source} -> {target}");
    }
    let update_edge: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='rollback_update' AND d.name='orders' AND e.kind='mutates_table'", [], |r| r.get(0)).unwrap();
    assert_eq!(update_edge, 1, "batch UPDATE with alias and ONLY must link");
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
    expected.extend([
        ("Order", "defines_table"),
        ("Item", "defines_table"),
        ("Generic", "defines_table"),
    ]);
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
        let other_default: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='Default' AND d.name='shop_default' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            other_default, 1,
            "independent default-only manifest should link"
        );
        let empty_defaults: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='Empty' AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(empty_defaults, 0, "class without bases must not define a table");
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
    #[cfg(feature = "lang-python")]
    without_sqlx.write("inactive.py", "from sqlalchemy.orm import DeclarativeBase\nclass Inactive(DeclarativeBase):\n    __tablename__ = 'orders'\n");
    #[cfg(feature = "lang-python")]
    without_sqlx.write("inactive_factory.py", "from sqlalchemy.orm import declarative_base\nBase = declarative_base()\nclass InactiveFactory(Base):\n    __tablename__ = 'orders'\n");
    without_sqlx.write("query.rs", "struct NotActive;\nfn load() { let _ = sqlx::query_as!(NotActive, \"SELECT * FROM orders\"); }\n");
    let db = without_sqlx.build();
    let edges: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='NotActive' AND e.kind='queries_table'", [], |row| row.get(0)).unwrap();
    assert_eq!(edges, 0, "undeclared SQLx must not activate its model rule");
    #[cfg(feature = "lang-python")]
    {
        let inactive: i64 = db.query_row("SELECT count(*) FROM edges e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name IN ('Inactive','InactiveFactory') AND e.kind='defines_table'", [], |row| row.get(0)).unwrap();
        assert_eq!(
            inactive, 0,
            "undeclared SQLAlchemy must not activate its model rule"
        );
    }
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

    let workspace = super::support::Workspace::new();
    let scope = format!("project_{}", "s".repeat(211));
    workspace.write(
        format!("{scope}/pyproject.toml"),
        "[project]\nname = 'scope-probe'\nversion = '0.1.0'\n",
    );
    let dependencies =
        contextunity_forge_mcp::engine::languages::dependency_registry::DependencyRegistry::collect(
            Some(&workspace),
        );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let mut scope_copies = Vec::new();
    for count in [1, 32] {
        let path = format!("{scope}/service.py");
        let source = format!(
            "class ServiceClient:\n    def fetch(self, endpoint: str) -> dict:\n{}    def _send_request(self, endpoint: str) -> dict:\n        return {{'status': 'ok'}}\n",
            "        self._send_request(endpoint)\n".repeat(count),
        );
        let facts = ast::extract(&path, "python", &source).unwrap();
        assert_eq!(
            facts
                .references
                .iter()
                .filter(|reference| reference.expression == "self._send_request")
                .count(),
            count,
        );
        let target = facts
            .nodes
            .iter()
            .find(|node| node.name == "_send_request")
            .unwrap()
            .id
            .clone();
        let all = std::collections::BTreeMap::from([(path, facts)]);
        let (graph, copies) = pool.install(|| {
            let _ = contextunity_forge_mcp::engine::linker::link_with_registry(
                &all,
                None,
                Some(&workspace),
                &dependencies,
            );
            scope_allocations::measure(scope.len(), || {
                contextunity_forge_mcp::engine::linker::link_with_registry(
                    &all,
                    None,
                    Some(&workspace),
                    &dependencies,
                )
            })
        });
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == "calls" && edge.dst == target)
                .count(),
            count,
            "every member call must retain its exact target",
        );
        assert_eq!(
            graph
                .coverage
                .iter()
                .filter(|coverage| coverage.expression == "self._send_request" && coverage.status == "resolved")
                .count(),
            count,
        );
        scope_copies.push(copies);
    }
    assert!(
        scope_copies[1] <= scope_copies[0],
        "assertion failed: package-scope preparation must be per file, not per member reference: {scope_copies:?}",
    );
}
