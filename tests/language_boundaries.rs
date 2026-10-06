#[cfg(any(
    feature = "lang-go",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-rust",
    feature = "lang-proto"
))]
use contextunity_forge_mcp::{
    db::{reader, writer},
    engine::ast,
};
#[cfg(any(
    feature = "lang-go",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-rust",
    feature = "lang-proto"
))]
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(
    feature = "lang-go",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-rust",
    feature = "lang-proto"
))]
struct Workspace(PathBuf);

#[cfg(any(
    feature = "lang-go",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-rust",
    feature = "lang-proto"
))]
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge_lang_boundaries_{}_{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) -> rusqlite::Connection {
        writer::build(&self.0, &self.db(), None).unwrap();
        reader::open(&self.db(), &self.0).unwrap()
    }
}

#[cfg(any(
    feature = "lang-go",
    feature = "lang-typescript",
    feature = "lang-vue",
    feature = "lang-rust",
    feature = "lang-proto"
))]
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_web_factory_results_keep_proven_receivers() {
    let w = Workspace::new();
    for (path, source) in [
        ("dom.js", "function run() { const element = document.querySelector('#root'); element.addEventListener('click', () => {}); const child = element.querySelector('.child'); child.getAttribute('id'); }"),
        ("fetch.js", "async function run() { const response = await fetch('/api'); response.json(); response.text(); }"),
        ("fetch_bad_property_call.js", "async function run() { const response = await fetch('/api'); response.status(); }"),
        ("dom_shadow.js", "function run(document) { const element = document.querySelector('#root'); element.addEventListener('click', () => {}); }"),
        ("dom_reassigned.js", "function run() { document = {}; const element = document.querySelector('#root'); element.addEventListener('click', () => {}); }"),
        ("fetch_shadow.js", "async function run(fetch) { const response = await fetch('/api'); response.json(); }"),
        ("fetch_local_function.js", "async function fetch() { return {}; } async function run() { const response = await fetch('/api'); response.json(); }"),
        ("fetch_import.js", "import { fetch } from './transport.js'; async function run() { const response = await fetch('/api'); response.json(); }"),
        ("transport.js", "export async function fetch() { return {}; }"),
        ("fetch_rebound.js", "async function run() { fetch = async () => ({}); const response = await fetch('/api'); response.json(); }"),
        ("fetch_plain.js", "function run() { const pending = fetch('/api'); pending.json(); }"),
        ("fetch_reassigned.js", "async function run() { let response = await fetch('/api'); response = {}; response.json(); }"),
        ("fetch_unknown.js", "async function run() { const response = await fetch('/api'); response.unsupported(); }"),
        ("fetch_local.js", "class Response { json() {} } async function run() { const response = new Response(); response.json(); }"),
        ("nested_page.js", "function outer() { const page = {}; function inner() { page.locator('button'); } inner(); }"),
        ("nested_page_late.js", "function outer() { function inner() { page.locator('button'); } const page = {}; inner(); }"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    for (path, expression) in [
        ("dom.js", "element.addEventListener"),
        ("dom.js", "child.getAttribute"),
        ("fetch.js", "response.json"),
        ("fetch.js", "response.text"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(row.0, "external", "{path}:{expression}: {row:?}");
        assert!(
            row.1.contains("builtin:web_api"),
            "{path}:{expression}: {row:?}"
        );
    }
    for (path, expression) in [
        ("dom_shadow.js", "element.addEventListener"),
        ("dom_reassigned.js", "element.addEventListener"),
        ("fetch_shadow.js", "response.json"),
        ("fetch_local_function.js", "response.json"),
        ("fetch_import.js", "response.json"),
        ("fetch_rebound.js", "response.json"),
        ("fetch_plain.js", "pending.json"),
        ("fetch_reassigned.js", "response.json"),
        ("fetch_unknown.js", "response.unsupported"),
        ("fetch_bad_property_call.js", "response.status"),
        ("nested_page.js", "page.locator"),
        ("nested_page_late.js", "page.locator"),
    ] {
        let row = coverage(path, expression);
        assert_eq!(row.0, "unresolved", "{path}:{expression}: {row:?}");
    }
    assert_eq!(coverage("fetch_local.js", "response.json").0, "resolved");
}

#[cfg(feature = "lang-go")]
#[test]
fn go_sibling_functions_in_same_package_link() {
    let w = Workspace::new();
    w.write(
        "pkg/helper.go",
        r#"package mypkg

func Helper() int {
    return 42
}
"#,
    );
    w.write(
        "pkg/main.go",
        r#"package mypkg

func Main() int {
    return Helper()
}
"#,
    );
    let conn = w.build();

    let nodes: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, name, qualname FROM nodes")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("NODES: {:?}", nodes);
    let cov: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id), status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("COVERAGE: {:?}", cov);

    let calls: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='calls'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(
        calls.len(),
        1,
        "expected 1 call edge between sibling files in same package: {:?}",
        calls
    );
    assert!(calls[0].0.contains("Main"));
    assert!(calls[0].1.contains("Helper"));

    let resolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE status='resolved' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='Helper'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved, 1, "expected Helper to be resolved");
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_lexical_this_nested_arrows() {
    let w = Workspace::new();
    w.write(
        "counter.ts",
        r#"class Counter {
  ready = false;
  static active = true;
  mark() {}
  static reset() {}
  run() {
    const read = () => this.ready;
    const call = () => this.mark();
    const ordinary = function() { return this.ready; };
    const unknown = () => this.missing;
    read(); call(); ordinary(); unknown();
  }
  static check() {
    const read = () => this.active;
    const call = () => this.reset();
    const bad = () => this.ready;
    read(); call(); bad();
  }
  constructor() {
    const initialize = () => this.ready;
    initialize();
  }
  nested() {
    const outer = () => () => this.mark();
    outer()();
  }
  paired() {
    const both = () => { this.mark(); const handler = this.mark; };
    both();
  }
}
function outside() { const arrow = () => this.mark(); arrow(); }"#,
    );
    w.write(
        "other.ts",
        "class Counter { ready = true; missing = 1; mark() {} }",
    );
    w.write(
        "counter.js",
        "class Counter { ready = false; run() { const read = () => this.ready; read(); } }",
    );
    let conn = w.build();
    let coverage = |path: &str, line: i64, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=?2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?3",
            rusqlite::params![path, line, expression],
            |row| row.get(0),
        ).unwrap()
    };
    let cov_ev = |path: &str, line: i64, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND line=?2 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?3",
            rusqlite::params![path, line, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };
    for (path, line, expression) in [
        ("counter.ts", 7, "this.ready"),
        ("counter.ts", 8, "this.mark"),
        ("counter.ts", 14, "this.active"),
        ("counter.ts", 15, "this.reset"),
        ("counter.ts", 20, "this.ready"),
        ("counter.ts", 24, "this.mark"),
        ("counter.js", 1, "this.ready"),
    ] {
        let (st, ev) = cov_ev(path, line, expression);
        assert_eq!(
            st, "resolved",
            "{path}:{line} {expression} (evidence: {ev})"
        );
    }
    for (line, expression) in [
        (9, "this.ready"),
        (10, "this.missing"),
        (16, "this.ready"),
        (32, "this.mark"),
    ] {
        assert_eq!(
            coverage("counter.ts", line, expression),
            "unresolved",
            "counter.ts:{line} {expression}"
        );
    }
    for (path, line, target, kind) in [
        ("counter.ts", 7, "ready", "field"),
        ("counter.ts", 8, "mark", "method"),
        ("counter.ts", 14, "active", "field"),
        ("counter.ts", 15, "reset", "method"),
        ("counter.ts", 20, "ready", "field"),
        ("counter.ts", 24, "mark", "method"),
        ("counter.js", 1, "ready", "field"),
    ] {
        let edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.line=?2 AND e.kind IN ('references','calls') AND dst.path=?1 AND dst.name=?3 AND dst.kind=?4",
            rusqlite::params![path, line, target, kind],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(
            edges, 1,
            "arrow must link to exact same-file {kind} {path}:{line} {target}"
        );
    }
    for kind in ["calls", "references"] {
        let edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='counter.ts' AND e.line=28 AND e.kind=?1 AND dst.path='counter.ts' AND dst.kind='method' AND dst.name='mark'",
            [kind], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            edges, 1,
            "same-line captured arrow needs an exact {kind} edge to Counter.mark"
        );
    }
    let sibling_edges: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='counter.ts' AND dst.path='other.ts'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        sibling_edges, 0,
        "same-name Counter in sibling file cannot provide this members"
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_typescript_mjs_cjs_and_package_exports() {
    let w = Workspace::new();
    w.write(
        "packages/math/package.json",
        r#"{
  "name": "@acme/math",
  "exports": {
    ".": "./src/index.mjs",
    "./calculator": "./src/calc.cjs"
  }
}
"#,
    );
    w.write(
        "packages/math/src/index.mjs",
        r#"export function add(a, b) {
    return a + b;
}
"#,
    );
    w.write(
        "packages/math/src/calc.cjs",
        r#"exports.multiply = function(a, b) {
    return a * b;
};
"#,
    );
    w.write(
        "packages/app/src/main.ts",
        r#"import { add } from "@acme/math";
import { multiply } from "@acme/math/calculator";

export function run() {
    return add(2, 3);
}
"#,
    );

    let conn = w.build();

    let modules: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, qualname, path FROM nodes WHERE kind='module'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("MODULES: {:?}", modules);
    let cov: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id), status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap()))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    println!("COVERAGE: {:?}", cov);

    // Verify .mjs and .cjs were scanned and extracted
    let mjs_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE path LIKE '%.mjs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mjs_count, 1);

    let cjs_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE path LIKE '%.cjs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cjs_count, 1);

    // Verify import edges resolved via package.json exports
    let import_edges: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='imports'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert!(
        import_edges
            .iter()
            .any(|(_, dst)| dst.contains("index.mjs")),
        "expected import edge to index.mjs: {:?}",
        import_edges
    );
    assert!(
        import_edges.iter().any(|(_, dst)| dst.contains("calc.cjs")),
        "expected import edge to calc.cjs: {:?}",
        import_edges
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_bindings() {
    let w = Workspace::new();
    w.write(
        "MyButton.vue",
        r#"<template>
  <button>{{ label }}</button>
</template>
<script setup lang="ts">
defineProps<{ label: string }>();
</script>
"#,
    );
    w.write(
        "App.vue",
        r#"<template>
  <div>
    <MyButton :label="buttonText" @click="onSubmit" />
    <p>{{ user.name }}</p>
  </div>
</template>
<script setup lang="ts">
import MyButton from './MyButton.vue';
const buttonText = "Click me";
function onSubmit() {
    return 42;
}

const user = { name: "Antigravity" };
ref(); computed(); reactive(); shallowRef(); shallowReactive(); toRef(); toRefs(); unref(); isRef();
watch(); watchEffect(); onMounted(); onUnmounted(); onUpdated(); onBeforeMount(); onBeforeUnmount();
nextTick(); inject(); provide(); useSlots(); useAttrs(); useI18n(); t();
</script>
"#,
    );

    let facts = ast::extract(
        "App.vue",
        "vue",
        &fs::read_to_string(w.0.join("App.vue")).unwrap(),
    )
    .unwrap();

    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();
    assert!(
        expressions.contains(&"MyButton"),
        "missing template custom component MyButton: {:?}",
        expressions
    );
    assert!(
        expressions.contains(&"buttonText"),
        "missing template bound prop buttonText: {:?}",
        expressions
    );
    assert!(
        expressions.contains(&"onSubmit"),
        "missing template event handler onSubmit: {:?}",
        expressions
    );
    assert!(
        expressions.contains(&"user"),
        "missing template interpolation user: {:?}",
        expressions
    );
    let conn = w.build();
    for name in "ref computed reactive shallowRef shallowReactive toRef toRefs unref isRef watch watchEffect onMounted onUnmounted onUpdated onBeforeMount onBeforeUnmount nextTick inject provide useSlots useAttrs useI18n".split_ascii_whitespace() {
        let (status, evidence): (String, String) = conn
            .query_row(
                "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or_else(|error| panic!("missing Vue runtime call {name}: {error}"));
        assert_eq!(status, "external", "Vue runtime call {name}");
        assert!(evidence.contains("builtin:vue_runtime"), "Vue runtime call {name}: {evidence}");
    }
    let unbound_t: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='t'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(
        unbound_t, "unresolved",
        "t requires a proven callable setup binding"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_compiler_macros_are_setup_scoped_and_lexically_shadowable() {
    let w = Workspace::new();
    w.write(
        "Macro.vue",
        r#"<script>defineProps();</script><script data-note="a setup b">defineEmits();</script><script setup>defineEmits(); function defineModel() {} defineModel();</script>"#,
    );
    w.write("plain.js", "defineProps();");
    let conn = w.build();

    let count = |expression: &str, status: &str| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='Macro.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?1 AND status=?2",
            rusqlite::params![expression, status],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(count("defineProps", "unresolved"), 1);
    let plain_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='plain.js' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='defineProps'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(plain_status, "unresolved");
    assert_eq!(count("defineEmits", "unresolved"), 1);
    assert_eq!(count("defineEmits", "external"), 1);
    let macro_evidence: String = conn
        .query_row(
            "SELECT (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='Macro.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='defineEmits' AND status='external'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        macro_evidence.contains("builtin:vue_macro"),
        "{macro_evidence}"
    );
    assert_eq!(count("defineModel", "resolved"), 1);
    let local_call: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.path_id)='Macro.vue' AND e.kind='calls' AND n.name='defineModel'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(local_call, 1);
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_setup_composable_callable_provenance() {
    let w = Workspace::new();
    w.write(
        "App.vue",
        r#"<template>
  <button @click="emit('save')">Save</button>
</template>
<script setup lang="ts">
const emit = defineEmits(['save']);
emit('save');
</script>"#,
    );
    w.write(
        "Unproven.vue",
        r#"<script setup lang="ts">
function run(emit: (event: string) => void) {
  emit('local');
}
const local = () => {};
let changed = defineEmits(['change']);
changed = local;
changed('change');
emit('unbound');
</script>"#,
    );
    w.write("Other.vue", "<script setup>emit('other');</script>");
    w.write(
        "MacroAlias.vue",
        "<script setup>const customEmit = defineEmits(['save']); customEmit('save');</script>",
    );
    w.write("Reassigned.vue", "<script setup>const emit = defineEmits(['save']); const local = () => {}; emit = local; emit('save');</script>");
    w.write("Duplicate.vue", "<script setup>const emit = defineEmits(['one']); const emit = defineEmits(['two']); emit('two');</script>");
    w.write(
        "Script.vue",
        "<script>const emit = defineEmits(['save']); emit('ordinary');</script>",
    );
    w.write("plain.ts", "emit('plain');");
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> Vec<(i64, String, String)> {
        let mut statement = conn.prepare(
            "SELECT line,status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 ORDER BY line",
        ).unwrap();
        statement
            .query_map(rusqlite::params![path, expression], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };

    let rows = coverage("App.vue", "defineEmits");
    assert_eq!(rows.len(), 1, "producer defineEmits: {rows:?}");
    assert_eq!(rows[0].1, "external", "producer defineEmits: {rows:?}");
    assert!(
        rows[0].2.contains("builtin:vue_macro"),
        "producer defineEmits: {rows:?}"
    );

    let emit_rows = coverage("App.vue", "emit");
    assert_eq!(
        emit_rows.len(),
        2,
        "script and template calls of emit: {emit_rows:?}"
    );
    assert!(
        emit_rows.iter().any(|row| row.0 == 2),
        "template call of emit: {emit_rows:?}"
    );
    assert!(
        emit_rows.iter().any(|row| row.0 >= 6),
        "script call of emit: {emit_rows:?}"
    );
    for row in emit_rows {
        assert_eq!(row.1, "external", "emit: {row:?}");
        assert!(row.2.contains("builtin:vue_macro"), "emit: {row:?}");
    }

    let macro_alias = coverage("MacroAlias.vue", "customEmit");
    assert_eq!(macro_alias.len(), 1);
    assert_eq!(macro_alias[0].1, "external");
    assert!(
        macro_alias[0].2.contains("builtin:vue_macro"),
        "{macro_alias:?}"
    );

    for (path, expression) in [
        ("Unproven.vue", "emit"),
        ("Unproven.vue", "changed"),
        ("Other.vue", "emit"),
        ("Script.vue", "emit"),
        ("plain.ts", "emit"),
        ("Reassigned.vue", "emit"),
        ("Duplicate.vue", "emit"),
    ] {
        let rows = coverage(path, expression);
        assert!(!rows.is_empty(), "missing callable {path}:{expression}");
        assert!(
            rows.iter().all(|row| !row.2.contains("builtin:vue_macro")),
            "unproven callable acquired builtin:vue_macro: {path}:{expression} {rows:?}"
        );
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_setup_inherits_javascript_web_builtins_as_external() {
    let source = r#"<script setup lang="ts">
function run() {
    fetch('/api');
    const items: Array<string> = [];
    items.map(item => item);
}
</script>"#;
    let facts = ast::extract("Platform.vue", "vue", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "Platform.vue".to_owned(),
            facts,
        )]));
    for expression in ["items.map", "fetch"] {
        let coverage = graph
            .coverage
            .iter()
            .find(|row| row.expression == expression)
            .unwrap_or_else(|| panic!("missing Vue coverage for {expression}: {graph:#?}"));
        assert_eq!(coverage.status, "external", "{expression}: {coverage:#?}");
        assert!(
            coverage.evidence.contains("JavaScript/Web platform"),
            "{expression}: {coverage:#?}"
        );
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_typed_defineprops_template_members() {
    let w = Workspace::new();
    w.write(
        "Props.vue",
        r#"<template>
  <p>{{ props.label }}</p>
  <p>{{ props.missing }}</p>
</template>
<script setup lang="ts">
const props = defineProps<{ label: string }>();
</script>"#,
    );
    w.write(
        "Shadow.vue",
        r#"<template><div v-for="props in items">{{ props.label }}</div></template>
<script setup lang="ts">const props = defineProps<{ label: string }>(); const items = [{}];</script>"#,
    );
    w.write(
        "Sibling.vue",
        "<template><p>{{ props.label }}</p></template><script setup>const other = 1;</script>",
    );
    w.write(
        "Duplicate.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">const props = defineProps<{ label: string; label: string; label: string }>();</script>",
    );
    w.write(
        "Reassigned.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">const props = defineProps<{ label: string }>(); props = { label: 'local' };</script>",
    );
    w.write(
        "DuplicateBinding.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">const props = defineProps<{ label: string }>(); const props = defineProps<{ label: string }>();</script>",
    );
    w.write(
        "ShadowedProducer.vue",
        "<template><p>{{ props.label }}</p></template><script setup lang=\"ts\">function defineProps() { return { label: 'local' }; } const props = defineProps<{ label: string }>();</script>",
    );
    w.write(
        "Ordinary.vue",
        "<template><p>{{ props.label }}</p></template><script lang=\"ts\">const props = defineProps<{ label: string }>();</script>",
    );
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(coverage("Props.vue", "props.label"), "resolved");
    assert_eq!(coverage("Props.vue", "props.missing"), "unresolved");
    assert_eq!(coverage("Shadow.vue", "props.label"), "unresolved");
    assert_eq!(coverage("Sibling.vue", "props.label"), "unresolved");
    for path in [
        "Duplicate.vue",
        "Reassigned.vue",
        "DuplicateBinding.vue",
        "ShadowedProducer.vue",
        "Ordinary.vue",
    ] {
        assert_eq!(
            coverage(path, "props.label"),
            "unresolved",
            "unproven typed props in {path}"
        );
    }
    let exact_field_edges: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='Props.vue' AND e.kind='references' AND e.line=2 AND src.path='Props.vue' AND dst.path='Props.vue' AND dst.kind='field' AND dst.name='label'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        exact_field_edges, 1,
        "template member must link to the same-file indexed label field"
    );
    for path in [
        "Duplicate.vue",
        "Reassigned.vue",
        "DuplicateBinding.vue",
        "ShadowedProducer.vue",
        "Ordinary.vue",
    ] {
        let field_edges: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND dst.path=?1 AND dst.name='label'",
            [path], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            field_edges, 0,
            "unproven typed props cannot provide an exact template target in {path}"
        );
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_nested_and_optional_defineprops_template_members() {
    let w = Workspace::new();
    w.write(
        "Nested.vue",
        r#"<template>
  <p>{{ props.user.profile.name }}</p>
  <p>{{ props.user?.profile?.name }}</p>
  <p>{{ props.user.profile.missing }}</p>
</template>
<script setup lang="ts">
const props = defineProps<{ user?: { profile: { name?: string } } }>();
</script>"#,
    );
    w.write(
        "Shadow.vue",
        r#"<template><div v-for="props in items">{{ props.user.profile.name }}</div></template>
<script setup lang="ts">const props = defineProps<{ user: { profile: { name: string } } }>(); const items = [];</script>"#,
    );
    w.write(
        "Sibling.vue",
        r#"<template>{{ props.user.profile.name }}</template><script setup lang="ts">const other = 1;</script>"#,
    );
    w.write(
        "SameLine.vue",
        r#"<template>{{ props.left.name }} {{ props.right.name }}</template><script setup lang="ts">const props = defineProps<{ left: { name: string }; right: { name: string } }>();</script>"#,
    );
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2",
            rusqlite::params![path, expression],
            |row| row.get(0),
        ).unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    for expression in ["props.user.profile.name", "props.user?.profile?.name"] {
        assert_eq!(
            coverage("Nested.vue", expression),
            "resolved",
            "{expression}"
        );
    }
    assert_eq!(
        coverage("Nested.vue", "props.user.profile.missing"),
        "unresolved"
    );
    assert_eq!(
        coverage("Shadow.vue", "props.user.profile.name"),
        "unresolved"
    );
    assert_eq!(
        coverage("Sibling.vue", "props.user.profile.name"),
        "unresolved"
    );
    for expression in ["props.left.name", "props.right.name"] {
        assert_eq!(
            coverage("SameLine.vue", expression),
            "unresolved",
            "ambiguous type-literal field {expression}"
        );
    }
    let exact_field_edges: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='Nested.vue' AND e.kind='references' AND dst.path='Nested.vue' AND dst.kind='field' AND dst.name='name'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        exact_field_edges, 2,
        "both template expressions link to their same-file typed field"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_composable_return_members_follow_setup_aliases() {
    let w = Workspace::new();
    w.write(
        "Good.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { function save() { return 1; } return { save }; } const actions = makeActions(); const alias = actions;</script>",
    );
    w.write(
        "Arrow.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { const save = () => 1; return { save }; } const actions = makeActions(); const alias = actions;</script>",
    );
    w.write(
        "Unknown.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>const alias = unknownFactory();</script>",
    );
    w.write(
        "Shadow.vue",
        "<template><div v-for=\"alias in aliases\"><button @click=\"alias.save\" /></div></template>\n<script setup>function makeActions() { function save() { return 1; } return { save }; } const actions = makeActions(); const alias = actions; const aliases = [];</script>",
    );
    w.write(
        "Rebound.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { function save() { return 1; } return { save }; } const actions = makeActions(); let alias = actions; alias = {};</script>",
    );
    w.write(
        "MutatedMember.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup>function makeActions() { function save() { return 1; } save = () => 2; return { save }; } const actions = makeActions(); const alias = actions;</script>",
    );
    let conn = w.build();
    let status = |path: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='alias.save' AND line=1",
            [path], |row| row.get(0),
        ).unwrap_or_else(|error| panic!("missing alias.save in {path}: {error}"))
    };
    for path in ["Good.vue", "Arrow.vue"] {
        assert_eq!(status(path), "resolved", "{path}");
        let exact_save_edge: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='calls' AND e.line=1 AND dst.path=?1 AND dst.kind='function' AND dst.name='save'",
            [path], |row| row.get(0),
        ).unwrap();
        assert_eq!(
            exact_save_edge, 1,
            "{path} template alias.save points to its returned local function"
        );
    }
    for path in [
        "Unknown.vue",
        "Shadow.vue",
        "Rebound.vue",
        "MutatedMember.vue",
    ] {
        assert_eq!(status(path), "unresolved", "{path}");
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_imported_composable_return_members_follow_unique_provider() {
    let w = Workspace::new();
    w.write(
        "app/actions.ts",
        "export function buildActions() { function save() { return 1; } return { save }; }",
    );
    w.write(
        "app/View.vue",
        "<template><button @click=\"alias.save\" /></template>\n<script setup lang=\"ts\">import { buildActions } from './actions'; const actions = buildActions(); const alias = actions;</script>",
    );
    w.write(
        "other/actions.ts",
        "export function buildActions() { function save() { return 2; } return { save }; }",
    );
    let conn = w.build();
    let status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='app/View.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='alias.save' AND line=1",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(status, "resolved");
    let exact_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/View.vue' AND e.kind='calls' AND e.line=1 AND dst.path='app/actions.ts' AND dst.kind='function' AND dst.name='save'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        exact_edge, 1,
        "template call points to the uniquely imported provider"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_nuxt_components_directory_resolves_with_project_scope() {
    let w = Workspace::new();
    for (path, source) in [
        ("apps/nuxt/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/nuxt/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/nuxt/components/Base/WidgetCard.vue", "<template><article /></template>"),
        ("apps/nuxt/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/other/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/other/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/other/components/Base/WidgetCard.vue", "<template><aside /></template>"),
        ("apps/other/pages/index.vue", "<template><base-widget-card /></template>"),
        ("apps/plain/package.json", r#"{"dependencies":{"vue":"^3"}}"#),
        ("apps/plain/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/plain/components/Base/WidgetCard.vue", "<template><footer /></template>"),
        ("apps/plain/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/disabled/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/disabled/nuxt.config.ts", "export default defineNuxtConfig({ components: false });"),
        ("apps/disabled/components/Base/WidgetCard.vue", "<template><header /></template>"),
        ("apps/disabled/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/collision/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/collision/nuxt.config.ts", "export default defineNuxtConfig({});"),
        ("apps/collision/components/Base/WidgetCard.vue", "<template><article /></template>"),
        ("apps/collision/components/BaseWidgetCard.vue", "<template><aside /></template>"),
        ("apps/collision/pages/index.vue", "<template><BaseWidgetCard /></template>"),
        ("apps/dynamic/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/dynamic/nuxt.config.ts", "const options = { components: false }; export default defineNuxtConfig({ ...options });"),
        ("apps/dynamic/components/Base/WidgetCard.vue", "<template><nav /></template>"),
        ("apps/dynamic/pages/index.vue", "<template><BaseWidgetCard /></template>"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let status = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND line=1",
            rusqlite::params![path, expression],
            |row| row.get(0),
        )
        .unwrap_or_else(|error| panic!("missing Nuxt template tag {path}:{expression}: {error}"))
    };
    for (page, tag, component) in [
        (
            "apps/nuxt/pages/index.vue",
            "BaseWidgetCard",
            "apps/nuxt/components/Base/WidgetCard.vue",
        ),
        (
            "apps/other/pages/index.vue",
            "base-widget-card",
            "apps/other/components/Base/WidgetCard.vue",
        ),
    ] {
        assert_eq!(status(page, tag), "resolved", "{page}");
        let exact_edge: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND e.line=1 AND dst.path=?2",
            rusqlite::params![page, component],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(exact_edge, 1, "{page} must point to its own Nuxt component");
    }
    for page in [
        "apps/plain/pages/index.vue",
        "apps/disabled/pages/index.vue",
        "apps/dynamic/pages/index.vue",
    ] {
        assert_eq!(status(page, "BaseWidgetCard"), "unresolved", "{page}");
    }
    assert_eq!(
        status("apps/collision/pages/index.vue", "BaseWidgetCard"),
        "ambiguous"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_nuxt_builtin_components_require_project_provenance() {
    let w = Workspace::new();
    for (path, source) in [
        ("apps/nuxt/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/nuxt/nuxt.config.ts", "export default defineNuxtConfig({ components: false });"),
        ("apps/nuxt/app.vue", "<template><NuxtLayout><NuxtPage /></NuxtLayout></template>"),
        ("apps/nuxt/pages/index.vue", "<template><ClientOnly><span /></ClientOnly><UnknownWidget /></template>"),
        ("apps/nuxt/pages/local.vue", "<template><ClientOnly /></template><script setup>import ClientOnly from '../widgets/ClientOnly.vue';</script>"),
        ("apps/nuxt/widgets/ClientOnly.vue", "<template><div /></template>"),
        ("apps/plain/package.json", r#"{"dependencies":{"vue":"^3"}}"#),
        ("apps/plain/pages/index.vue", "<template><ClientOnly /></template>"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path=?1 AND x.expression=?2 AND r.line=1",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    let (status, evidence) = coverage("apps/nuxt/pages/index.vue", "ClientOnly");
    assert_eq!(status, "external");
    assert!(evidence.contains("builtin:nuxt"), "{evidence}");
    for tag in ["NuxtLayout", "NuxtPage"] {
        let (status, evidence) = coverage("apps/nuxt/app.vue", tag);
        assert_eq!(status, "external", "{tag}");
        assert!(evidence.contains("builtin:nuxt"), "{tag}: {evidence}");
    }
    assert_eq!(
        coverage("apps/nuxt/pages/index.vue", "UnknownWidget").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/plain/pages/index.vue", "ClientOnly").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/nuxt/pages/local.vue", "ClientOnly").0,
        "resolved"
    );
    let local_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/nuxt/pages/local.vue' AND e.kind='references' AND e.line=1 AND dst.path='apps/nuxt/widgets/ClientOnly.vue'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        local_edge, 1,
        "explicit local component takes priority over Nuxt built-in"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_nuxt_module_components_require_registered_provider() {
    let w = Workspace::new();
    for (path, source) in [
        ("apps/radix/package.json", r#"{"dependencies":{"nuxt":"^3","radix-vue":"^1"}}"#),
        ("apps/radix/nuxt.config.ts", "export default defineNuxtConfig({ modules: ['radix-vue/nuxt'] });"),
        ("apps/radix/pages/index.vue", "<template><DialogRoot><DialogPortal><DialogOverlay /><DialogContent><DialogTitle /><DialogDescription /><DialogClose /></DialogContent></DialogPortal></DialogRoot><UnknownRadixTag /></template>"),
        ("apps/radix/pages/local.vue", "<template><DialogTitle /></template><script setup>import DialogTitle from '../widgets/DialogTitle.vue';</script>"),
        ("apps/radix/widgets/DialogTitle.vue", "<template><h1 /></template>"),
        ("apps/disabled/package.json", r#"{"dependencies":{"nuxt":"^3","radix-vue":"^1"}}"#),
        ("apps/disabled/nuxt.config.ts", "export default defineNuxtConfig({ modules: [] });"),
        ("apps/disabled/pages/index.vue", "<template><DialogTitle /></template>"),
        ("apps/undeclared/package.json", r#"{"dependencies":{"nuxt":"^3"}}"#),
        ("apps/undeclared/nuxt.config.ts", "export default defineNuxtConfig({ modules: ['radix-vue/nuxt'] });"),
        ("apps/undeclared/pages/index.vue", "<template><DialogTitle /></template>"),
    ] {
        w.write(path, source);
    }
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path=?1 AND x.expression=?2 AND r.line=1",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|error| panic!("missing {path}:{expression}: {error}"))
    };
    for tag in [
        "DialogRoot",
        "DialogPortal",
        "DialogOverlay",
        "DialogContent",
        "DialogTitle",
        "DialogDescription",
        "DialogClose",
    ] {
        let (status, evidence) = coverage("apps/radix/pages/index.vue", tag);
        assert_eq!(status, "external", "{tag}");
        assert!(evidence.contains("radix-vue/nuxt"), "{tag}: {evidence}");
    }
    assert_eq!(
        coverage("apps/radix/pages/index.vue", "UnknownRadixTag").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/disabled/pages/index.vue", "DialogTitle").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/undeclared/pages/index.vue", "DialogTitle").0,
        "unresolved"
    );
    assert_eq!(
        coverage("apps/radix/pages/local.vue", "DialogTitle").0,
        "resolved"
    );
    let local_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='apps/radix/pages/local.vue' AND e.kind='references' AND e.line=1 AND dst.path='apps/radix/widgets/DialogTitle.vue'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        local_edge, 1,
        "explicit local component takes priority over registered Nuxt module"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_options_api_registered_components_use_imported_aliases() {
    let w = Workspace::new();
    w.write("LeafCard.vue", "<template><article /></template>");
    w.write(
        "Plain.vue",
        "<template><card-alias /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { components: { CardAlias: LeafCard } };</script>",
    );
    w.write(
        "Wrapped.vue",
        "<template><WrappedAlias /></template>\n<script lang=\"ts\">import { defineComponent } from 'vue'; import LeafCard from './LeafCard.vue'; export default defineComponent({ components: { WrappedAlias: LeafCard } });</script>",
    );
    w.write(
        "Unregistered.vue",
        "<template><orphan-card /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { components: {} };</script>",
    );
    w.write(
        "UnregisteredImport.vue",
        "<template><LeafCard /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { data() { return {}; } };</script>",
    );
    w.write(
        "NoVueWrapper.vue",
        "<template><SpoofAlias /></template>\n<script>import LeafCard from './LeafCard.vue'; function defineComponent(options) { return options; } export default defineComponent({ components: { SpoofAlias: LeafCard } });</script>",
    );
    w.write(
        "Duplicate.vue",
        "<template><DuplicateAlias /></template>\n<script>import LeafCard from './LeafCard.vue'; export default { components: { DuplicateAlias: LeafCard, DuplicateAlias: LeafCard } };</script>",
    );
    let conn = w.build();
    let status = |path: &str, expression: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND line=1",
            rusqlite::params![path, expression],
            |row| row.get(0),
        )
        .unwrap_or_else(|error| panic!("missing template component {path}:{expression}: {error}"))
    };
    for path in ["Plain.vue", "Wrapped.vue"] {
        assert_eq!(status(path, "LeafCard"), "resolved", "{path}");
        let template_edge: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.kind='references' AND e.line=1 AND dst.path='LeafCard.vue'",
            [path],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(
            template_edge, 1,
            "{path} template tag points to the imported component file"
        );
    }
    assert_eq!(status("Unregistered.vue", "orphan-card"), "unresolved");
    assert_eq!(status("UnregisteredImport.vue", "LeafCard"), "unresolved");
    assert_eq!(status("NoVueWrapper.vue", "SpoofAlias"), "unresolved");
    assert_eq!(status("Duplicate.vue", "DuplicateAlias"), "unresolved");
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_sfc_template_bridge_resolves_setup_bindings_and_component_imports() {
    let w = Workspace::new();
    w.write(
        "MyButton.vue",
        "<template><button /></template><script setup>const label = 'ok';</script>",
    );
    w.write(
        "App.vue",
        r#"<template>
  <my-button @click="worker.run" />
  <button @click="worker.run()" />
  <p>{{ buttonText }}</p>
  <p>{{ hidden }}</p>
</template>
<script setup lang="ts">
import MyButton from './MyButton.vue';
class Worker { run() { return 1; } }
const worker = new Worker();
const buttonText = ref('Ready');
function onlyInside() { const hidden = 'private'; return hidden; }
</script>
"#,
    );
    let facts = ast::extract(
        "App.vue",
        "vue",
        &fs::read_to_string(w.0.join("App.vue")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        facts
            .references
            .iter()
            .filter(|reference| reference.kind == "calls" && reference.expression == "worker.run")
            .count(),
        2,
        "{facts:#?}"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| { reference.expression == "buttonText" && reference.line == 4 }),
        "template interpolation retains its setup binding reference: {facts:#?}"
    );
    let conn = w.build();
    let calls: i64 = conn.query_row(
        "SELECT COALESCE(sum(e.occurrence_count),0) FROM edges e JOIN nodes n ON n.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.path_id)='App.vue' AND e.kind='calls' AND n.name='run'",
        [], |row| row.get(0),
    ).unwrap();
    let mut statement = conn.prepare("SELECT (SELECT id FROM nodes WHERE node_hash=edge_occurrences.src_hash),(SELECT id FROM nodes WHERE node_hash=edge_occurrences.dst_hash),line,(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.evidence_id) FROM edge_occurrences WHERE (SELECT path FROM path_dictionary WHERE path_id=edge_occurrences.owner_id)='App.vue' AND kind='calls'").unwrap();
    let occurrences: Vec<(String, String, usize, String)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(calls, 2, "{occurrences:#?}");
    let components: i64 = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='MyButton' AND status='resolved'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(components, 1);
    let component_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edges e JOIN nodes src ON src.node_hash=e.src_hash JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE e.kind='imports' AND src.id='module:App.vue' AND dst.id='module:MyButton.vue'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        component_edge, 1,
        "local component import links to MyButton.vue"
    );
    let mut binding_statement = conn.prepare(
        "SELECT status, (SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='buttonText' AND line=4",
    ).unwrap();
    let binding_coverages: Vec<(String, String)> = binding_statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        binding_coverages
            .iter()
            .any(|(status, _)| status == "resolved"),
        "template interpolation resolves to the local setup binding: {binding_coverages:#?}"
    );
    let hidden_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='App.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='hidden' AND line=5",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        hidden_status, "unresolved",
        "nested script local is outside template scope"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_sfc_template_bridge_resolves_large_setup_scope() {
    let declarations = (0..192)
        .map(|index| format!("const item{index} = {index};"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        "<template><p>{{{{ item191 }}}}</p></template><script setup>{declarations}</script>"
    );
    let facts = ast::extract("Large.vue", "vue", &source).unwrap();
    let graph =
        contextunity_forge_mcp::engine::linker::link(&std::collections::BTreeMap::from([(
            "Large.vue".to_owned(),
            facts,
        )]));
    assert!(
        graph
            .coverage
            .iter()
            .any(|coverage| { coverage.expression == "item191" && coverage.status == "resolved" }),
        "last setup declaration resolves in template: {graph:#?}"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_iteration_and_slot_scopes_keep_all_bindings_and_source_coordinates() {
    let source = r#"<template>
  <div v-for="(item, index) in items" :title="item.name">{{ index }}</div>
  <Panel v-slot="{ entry, count }">{{ entry.title }} {{ count }}</Panel>
</template>
<script setup>const items = [];</script>"#;
    let facts = ast::extract("Scoped.vue", "vue", source).unwrap();
    let iteration = facts
        .nodes
        .iter()
        .find(|node| {
            node.kind == "template_scope"
                && node.details["bindings"]
                    .as_array()
                    .is_some_and(|names| names.iter().any(|name| name == "item"))
        })
        .unwrap();
    assert!(iteration.details["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "index"));
    let index = facts
        .references
        .iter()
        .find(|reference| reference.expression == "index")
        .unwrap();
    assert_eq!(index.source, iteration.id);
    assert_eq!(index.line, 2);
    let line = source.lines().nth(index.line - 1).unwrap();
    assert!(line[index.column..].starts_with("index"));
    let slot = facts
        .nodes
        .iter()
        .find(|node| {
            node.kind == "template_scope"
                && node.details["bindings"]
                    .as_array()
                    .is_some_and(|names| names.iter().any(|name| name == "entry"))
        })
        .unwrap();
    assert!(slot.details["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "count"));
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.source == slot.id && reference.expression == "entry.title"));
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_typed_iteration_members_resolve_to_declared_element_fields() {
    let w = Workspace::new();
    let source = "<template><ul><li v-for=\"(item, index) in items\">{{ item.title }} {{ index }}</li></ul></template>\n<script setup lang=\"ts\">interface Item { title: string } const items: Item[] = [];</script>";
    w.write("TypedList.vue", source);
    w.write(
        "TypedCall.vue",
        "<template><button v-for=\"item in items\" @click=\"item.title()\" /></template>\n<script setup lang=\"ts\">interface Item { title: string } const items: Item[] = [];</script>",
    );
    w.write(
        "ShadowedIterable.vue",
        "<template><div v-for=\"items in groups\"><span v-for=\"item in items\">{{ item.title }}</span></div></template>\n<script setup lang=\"ts\">interface Item { title: string } const items: Item[] = []; const groups = [];</script>",
    );
    let conn = w.build();
    let (status, evidence): (String, String) = conn.query_row(
        "SELECT r.status, e.evidence FROM resolution_coverage r JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE (SELECT path FROM path_dictionary WHERE path_id=r.path_id)='TypedList.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=r.expression_id)='item.title' AND r.line=1",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(status, "resolved", "{evidence}");
    let field_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='TypedList.vue' AND e.kind='references' AND e.line=1 AND dst.path='TypedList.vue' AND dst.kind='field' AND dst.name='title'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(field_edge, 1);
    let call_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='TypedCall.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='item.title' AND line=1",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        call_status, "unresolved",
        "a typed field is not a callable method"
    );
    let shadowed_status: String = conn.query_row(
        "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)='ShadowedIterable.vue' AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='item.title' AND line=1",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        shadowed_status, "unresolved",
        "outer loop binding shadows the setup iterable"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_typed_slot_members_follow_imported_component_provider() {
    let w = Workspace::new();
    w.write(
        "app/Panel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/View.vue",
        "<template><Panel v-slot=\"{ entry }\">{{ entry.title }}</Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue';</script>",
    );
    w.write(
        "app/NestedSlot.vue",
        "<template><Panel><template #default=\"{ entry }\">{{ entry.title }}</template></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue';</script>",
    );
    w.write("app/Plain.vue", "<template><slot /></template>");
    w.write(
        "app/NestedComponent.vue",
        "<template><Panel><Plain><template #default=\"{ entry }\">{{ entry.title }}</template></Plain></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue'; import Plain from './Plain.vue';</script>",
    );
    w.write(
        "app/Unknown.vue",
        "<template><MissingPanel v-slot=\"{ entry }\">{{ entry.title }}</MissingPanel></template>",
    );
    w.write(
        "app/Shadow.vue",
        "<template><Panel v-slot=\"{ entry }\"><span v-for=\"entry in entries\">{{ entry.title }}</span></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue'; const entries = [];</script>",
    );
    w.write(
        "app/FieldCall.vue",
        "<template><Panel v-slot=\"{ entry }\"><button @click=\"entry.title()\" /></Panel></template>\n<script setup lang=\"ts\">import Panel from './Panel.vue';</script>",
    );
    w.write(
        "app/macro.ts",
        "export function other() {} export function defineSlots() {}",
    );
    w.write(
        "app/ShadowedMacroPanel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">import { other as defineSlots } from './macro'; interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/ShadowedMacro.vue",
        "<template><ShadowedMacroPanel v-slot=\"{ entry }\">{{ entry.title }}</ShadowedMacroPanel></template>\n<script setup lang=\"ts\">import ShadowedMacroPanel from './ShadowedMacroPanel.vue';</script>",
    );
    w.write(
        "app/UnrelatedImportPanel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">import { defineSlots as other } from './macro'; interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    w.write(
        "app/UnrelatedImport.vue",
        "<template><UnrelatedImportPanel v-slot=\"{ entry }\">{{ entry.title }}</UnrelatedImportPanel></template>\n<script setup lang=\"ts\">import UnrelatedImportPanel from './UnrelatedImportPanel.vue';</script>",
    );
    w.write(
        "other/Panel.vue",
        "<template><slot /></template>\n<script setup lang=\"ts\">interface Item { title: string } defineSlots<{ default(props: { entry: Item }): any }>();</script>",
    );
    let conn = w.build();
    let status = |path: &str| -> String {
        conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='entry.title' AND line=1",
            [path], |row| row.get(0),
        ).unwrap_or_else(|error| panic!("missing slot member in {path}: {error}"))
    };
    assert_eq!(status("app/View.vue"), "resolved");
    assert_eq!(status("app/NestedSlot.vue"), "resolved");
    assert_eq!(status("app/UnrelatedImport.vue"), "resolved");
    let field_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/View.vue' AND e.kind='references' AND e.line=1 AND dst.path='app/Panel.vue' AND dst.kind='field' AND dst.name='title'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        field_edge, 1,
        "slot member points to the uniquely imported provider field"
    );
    let nested_edge: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/NestedSlot.vue' AND e.kind='references' AND e.line=1 AND dst.path='app/Panel.vue' AND dst.kind='field' AND dst.name='title'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        nested_edge, 1,
        "nested slot follows the imported parent component"
    );
    for path in [
        "app/Unknown.vue",
        "app/Shadow.vue",
        "app/FieldCall.vue",
        "app/ShadowedMacro.vue",
        "app/NestedComponent.vue",
    ] {
        assert_eq!(status(path), "unresolved", "{path}");
    }
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_unicode_does_not_interrupt_bindings() {
    let source = "<template>\n  <div>⏱ {{ timestamp }}</div>\n  <button @click=\"onSubmit\">Go</button>\n</template>\n";
    let facts = ast::extract("Timer.vue", "vue", source).unwrap();
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(expressions.contains(&"timestamp"), "{expressions:?}");
    assert!(expressions.contains(&"onSubmit"), "{expressions:?}");
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_string_literals_do_not_produce_bogus_references() {
    let source = r#"
<template>
  <div :class="['bg-white/5', active && 'text-blue-500', { 'hover:opacity-100': isHovered }]">
    <p>{{ user.name + ' - anonymous' }}</p>
  </div>
</template>
<script setup lang="ts">
const active = true;
const isHovered = false;
const user = { name: "User" };
</script>
"#;
    let facts = ast::extract("Card.vue", "vue", source).unwrap();
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(
        expressions.contains(&"active"),
        "must contain active: {expressions:?}"
    );
    assert!(
        expressions.contains(&"isHovered"),
        "must contain isHovered: {expressions:?}"
    );
    assert!(
        expressions.contains(&"user"),
        "must contain user: {expressions:?}"
    );
    assert!(
        expressions.contains(&"user.name"),
        "must contain the qualified receiver member user.name: {expressions:?}"
    );

    // CSS class string tokens MUST NOT be extracted as references
    assert!(
        !expressions.contains(&"bg"),
        "must NOT contain 'bg': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"white"),
        "must NOT contain 'white': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"text"),
        "must NOT contain 'text': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"blue"),
        "must NOT contain 'blue': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"hover"),
        "must NOT contain 'hover': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"opacity"),
        "must NOT contain 'opacity': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"anonymous"),
        "must NOT contain 'anonymous': {expressions:?}"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_literal_interpolations_preserve_references() {
    let source = r#"
<template>
  <div :title="`User: ${user.name} (${role})`" :class="`prefix ${active ? 'is-active' : ''}`">
    <p>{{ `Hello ${target.title}!` }}</p>
  </div>
</template>
<script setup lang="ts">
const user = { name: "Antigravity" };
const role = "Admin";
const active = true;
const target = { title: "World" };
</script>
"#;
    let facts = ast::extract("TemplateLiteral.vue", "vue", source).unwrap();
    let expressions: Vec<&str> = facts
        .references
        .iter()
        .map(|r| r.expression.as_str())
        .collect();

    assert!(
        expressions.contains(&"user"),
        "must contain user: {expressions:?}"
    );
    assert!(
        expressions.contains(&"user.name"),
        "must contain the qualified receiver member user.name: {expressions:?}"
    );
    assert!(
        expressions.contains(&"role"),
        "must contain role: {expressions:?}"
    );
    assert!(
        expressions.contains(&"active"),
        "must contain active: {expressions:?}"
    );
    assert!(
        expressions.contains(&"target"),
        "must contain target: {expressions:?}"
    );
    assert!(
        expressions.contains(&"target.title"),
        "must contain the qualified receiver member target.title: {expressions:?}"
    );

    // Literal string parts must NOT be extracted
    assert!(
        !expressions.contains(&"User"),
        "must NOT contain 'User': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"prefix"),
        "must NOT contain 'prefix': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"Hello"),
        "must NOT contain 'Hello': {expressions:?}"
    );
    assert!(
        !expressions.contains(&"World"),
        "must NOT contain 'World': {expressions:?}"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_macros_and_cargo_workspace() {
    let w = Workspace::new();
    w.write(
        "crates/core/src/macros.rs",
        r#"#[macro_export]
macro_rules! log_info {
    ($msg:expr) => {
        println!("{}", $msg)
    };
}

"#,
    );
    w.write(
        "crates/core/src/lib.rs",
        r#"pub mod macros;

use crate::macros::log_info;

pub fn execute() {
    log_info!("running");
    println!("standard macro");
}
"#,
    );

    let conn = w.build();

    // Verify macro node is indexed
    let macro_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes WHERE kind='macro' AND name='log_info'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        macro_count, 1,
        "expected log_info macro to be extracted as kind='macro'"
    );

    // Verify standard-library println retains external provenance.
    let println_external: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='println' AND status='external'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        println_external, 1,
        "expected println macro to retain standard-library provenance"
    );

    // Verify crate:: import resolved to crates.core.src prefix
    let import_resolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id) LIKE '%log_info%' AND status='resolved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        import_resolved >= 1,
        "expected crate:: import of log_info to be resolved"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_manifest_types_preserve_receiver_origin_for_aliases_and_qualified_names() {
    let w = Workspace::new();
    w.write("Cargo.toml", "[package]\nname = \"receiver-provenance\"\nversion = \"0.1.0\"\n[dependencies]\nstorage-api = \"1\"\n");
    w.write("src/lib.rs", "use storage_api::Connection as DbConnection;\nfn imported(conn: &DbConnection) { conn.query(); }\nfn qualified(conn: &storage_api::Connection) { conn.query(); }\nfn generic(conn: &storage_api::Connection<u8>) { conn.query(); }\nfn associated(conn: &storage_api::Connection<u8>::Item<u8>) { conn.query(); }\nstruct Local;\nfn local(value: &Local) { value.query(); }\nfn undeclared(value: &missing_api::Connection) { value.query(); }\n");
    let conn = w.build();
    let mut statement = conn.prepare("SELECT c.line, e.expression, c.status, v.evidence FROM resolution_coverage c JOIN coverage_expressions e ON e.expression_id = c.expression_id JOIN coverage_evidence v ON v.evidence_id = c.evidence_id WHERE e.expression IN ('conn.query', 'value.query') ORDER BY c.line").unwrap();
    let rows: Vec<(i64, String, String, String)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for line in [2, 3, 4] {
        assert!(
            rows.iter()
                .any(|(at, expression, status, evidence)| *at == line
                    && expression == "conn.query"
                    && status == "external"
                    && evidence.contains("storage_api")),
            "{rows:#?}"
        );
    }
    assert!(
        rows.iter().any(|(at, expression, status, _)| *at == 5
            && expression == "conn.query"
            && status == "unresolved"),
        "{rows:#?}"
    );
    for line in [7, 8] {
        assert!(
            rows.iter().any(|(at, expression, status, _)| *at == line
                && expression == "value.query"
                && status == "unresolved"),
            "{rows:#?}"
        );
    }
}

#[cfg(feature = "lang-proto")]
#[test]
fn proto_package_qualified_types_and_fields() {
    let w = Workspace::new();
    w.write(
        "protos/auth.proto",
        r#"syntax = "proto3";

package mycompany.auth;

message Credentials {
    string username = 1;
    string token = 2;
}

message Session {
    mycompany.auth.Credentials creds = 1;
}

service AuthService {
    rpc Login(mycompany.auth.Credentials) returns (Session);
}
"#,
    );

    let facts = ast::extract(
        "protos/auth.proto",
        "proto",
        &fs::read_to_string(w.0.join("protos/auth.proto")).unwrap(),
    )
    .unwrap();

    // Verify module qualname is package
    let module_node = facts.nodes.iter().find(|n| n.kind == "module").unwrap();
    assert_eq!(module_node.qualname, "mycompany.auth");

    // Verify field references was extracted
    let field_refs: Vec<&str> = facts
        .references
        .iter()
        .filter(|r| r.kind == "references")
        .map(|r| r.expression.as_str())
        .collect();
    assert!(
        field_refs.contains(&"mycompany.auth.Credentials"),
        "expected field reference to mycompany.auth.Credentials: {:?}",
        field_refs
    );

    // Verify build & link
    let conn = w.build();
    let ref_edges: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT (SELECT id FROM nodes WHERE node_hash=src_hash), (SELECT id FROM nodes WHERE node_hash=dst_hash) FROM edges WHERE kind='references'")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert!(
        ref_edges.iter().any(|(_, dst)| dst.contains("Credentials")),
        "expected reference edge to Credentials: {:?}",
        ref_edges
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_transitive_nested_defineprops_interfaces() {
    let w = Workspace::new();
    w.write(
        "app/types.ts",
        r#"export interface Citation {
  url: string;
  summary: string;
}
export interface BookmarkItem {
  id: string;
  type: string;
  title: string;
  citation: Citation;
}
export interface Props {
  bookmark: BookmarkItem;
}
"#,
    );
    w.write(
        "app/InlineProps.vue",
        r#"<template>
  <p>{{ props.bookmark.title }}</p>
  <p>{{ props.bookmark.citation.url }}</p>
  <p>{{ props.bookmark.citation?.summary }}</p>
  <p>{{ props.bookmark.missing }}</p>
</template>
<script setup lang="ts">
import type { BookmarkItem } from './types';
const props = defineProps<{
  bookmark: BookmarkItem;
}>();
</script>
"#,
    );
    w.write(
        "app/NamedProps.vue",
        r#"<template>
  <p>{{ props.bookmark.citation.url }}</p>
</template>
<script setup lang="ts">
import type { Props } from './types';
const props = defineProps<Props>();
</script>
"#,
    );
    let conn = w.build();
    let coverage = |path: &str, expression: &str| -> (String, String) {
        conn.query_row(
            "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path=?1 AND x.expression=?2 AND r.line=2",
            rusqlite::params![path, expression],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap()
    };

    let title_status = coverage("app/InlineProps.vue", "props.bookmark.title").0;
    assert_eq!(title_status, "resolved", "props.bookmark.title");

    let url_cov = conn.query_row(
        "SELECT r.status,e.evidence FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id JOIN coverage_evidence e ON e.evidence_id=r.evidence_id WHERE p.path='app/InlineProps.vue' AND x.expression='props.bookmark.citation.url'",
        [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ).unwrap();
    assert_eq!(url_cov.0, "resolved", "props.bookmark.citation.url");

    let summary_cov = conn.query_row(
        "SELECT r.status FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id WHERE p.path='app/InlineProps.vue' AND x.expression='props.bookmark.citation?.summary'",
        [], |row| row.get::<_, String>(0),
    ).unwrap();
    assert_eq!(summary_cov, "resolved", "props.bookmark.citation?.summary");

    let missing_status = conn.query_row(
        "SELECT r.status FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id WHERE p.path='app/InlineProps.vue' AND x.expression='props.bookmark.missing'",
        [], |row| row.get::<_, String>(0),
    ).unwrap();
    assert_eq!(
        missing_status, "unresolved",
        "props.bookmark.missing must remain unresolved"
    );

    let named_url_status = conn.query_row(
        "SELECT r.status FROM resolution_coverage r JOIN path_dictionary p ON p.path_id=r.path_id JOIN coverage_expressions x ON x.expression_id=r.expression_id WHERE p.path='app/NamedProps.vue' AND x.expression='props.bookmark.citation.url'",
        [], |row| row.get::<_, String>(0),
    ).unwrap();
    assert_eq!(
        named_url_status, "resolved",
        "props.bookmark.citation.url in NamedProps.vue"
    );

    let edge_count: i64 = conn.query_row(
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='app/InlineProps.vue' AND e.kind='references' AND dst.path='app/types.ts' AND dst.kind='field' AND dst.name='url'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(
        edge_count, 1,
        "reference edge to Citation.url field in app/types.ts"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_destructured_composable_call_uses_indexed_return_member() {
    let w = Workspace::new();
    w.write(
        "app/i18n.ts",
        "export function useI18n() { function t(key: string) { return key; } return { t }; }",
    );
    w.write(
        "app/View.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">import { useI18n } from './i18n'; const { t } = useI18n();</script>",
    );
    w.write(
        "app/Alias.vue",
        "<template><p>{{ translate('save') }}</p></template>\n<script setup lang=\"ts\">import { useI18n } from './i18n'; const { t: translate } = useI18n();</script>",
    );
    w.write(
        "app/Unknown.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">const { t } = useI18n();</script>",
    );
    w.write(
        "app/Rebound.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">import { useI18n } from './i18n'; let { t } = useI18n(); t = () => 'changed';</script>",
    );
    w.write(
        "app/TypeOnly.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">\nimport type { useI18n } from './i18n';\nconst { t } = useI18n();\n</script>",
    );
    w.write(
        "app/SpecifierTypeOnly.vue",
        "<template><p>{{ t('save') }}</p></template>\n<script setup lang=\"ts\">\nimport { type useI18n } from './i18n';\nconst { t } = useI18n();\n</script>",
    );
    w.write(
        "other/i18n.ts",
        "export function useI18n() { function t(key: string) { return key; } return { t }; }",
    );
    let conn = w.build();
    for (path, expression) in [("app/View.vue", "t"), ("app/Alias.vue", "translate")] {
        let status: String = conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)=?2 AND line=1",
            rusqlite::params![path, expression], |row| row.get(0),
        ).unwrap();
        assert_eq!(status, "resolved", "{path} {expression}");
        let edge_count: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND dst.path=?2 AND e.kind='calls' AND dst.name='t'",
            rusqlite::params![path, "app/i18n.ts"], |row| row.get(0),
        ).unwrap();
        assert_eq!(edge_count, 1, "{path} call edge count to t");
    }
    for path in [
        "app/Unknown.vue",
        "app/Rebound.vue",
        "app/TypeOnly.vue",
        "app/SpecifierTypeOnly.vue",
    ] {
        let status: String = conn.query_row(
            "SELECT status FROM resolution_coverage WHERE (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)=?1 AND (SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)='t' AND line=1",
            rusqlite::params![path], |row| row.get(0),
        ).unwrap();
        assert!(
            matches!(status.as_str(), "unresolved" | "ambiguous"),
            "{path} t status={status}"
        );
        let edge_count: i64 = conn.query_row(
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND dst.path='app/i18n.ts' AND e.kind='calls' AND dst.name='t'",
            rusqlite::params![path], |row| row.get(0),
        ).unwrap();
        assert_eq!(edge_count, 0, "{path} negative call edge count to t");
    }
}
