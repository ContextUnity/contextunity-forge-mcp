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
            .prepare("SELECT expression, status, evidence FROM resolution_coverage")
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
            .prepare("SELECT src_public_id, dst_public_id FROM edges WHERE kind='calls'")
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
            "SELECT count(*) FROM resolution_coverage WHERE status='resolved' AND expression='Helper'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved, 1, "expected Helper to be resolved");
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
            .prepare("SELECT expression, status, evidence FROM resolution_coverage")
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
            .prepare("SELECT src_public_id, dst_public_id FROM edges WHERE kind='imports'")
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
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_compiler_macros_are_setup_scoped_and_lexically_shadowable() {
    let w = Workspace::new();
    w.write(
        "Macro.vue",
        r#"<script>defineProps();</script><script data-note="a setup b">defineEmits();</script><script setup>defineEmits(); function defineModel() {} defineModel();</script>"#,
    );
    let conn = w.build();

    let count = |expression: &str, status: &str| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM resolution_coverage WHERE path='Macro.vue' AND expression=?1 AND status=?2",
            rusqlite::params![expression, status],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(count("defineProps", "unresolved"), 1);
    assert_eq!(count("defineEmits", "unresolved"), 1);
    assert_eq!(count("defineEmits", "resolved"), 1);
    assert_eq!(count("defineModel", "resolved"), 1);
    let local_call: i64 = conn
        .query_row(
            "SELECT count(*) FROM edges e JOIN nodes n ON n.id=e.dst_public_id WHERE e.path='Macro.vue' AND e.kind='calls' AND n.name='defineModel'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(local_call, 1);
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_template_receiver_calls_use_setup_types_and_component_exports() {
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
</template>
<script setup lang="ts">
import MyButton from './MyButton.vue';
class Worker { run() { return 1; } }
const worker = new Worker();
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
    let conn = w.build();
    let calls: i64 = conn.query_row(
        "SELECT COALESCE(sum(e.occurrence_count),0) FROM edges e JOIN nodes n ON n.id=e.dst_public_id WHERE e.path='App.vue' AND e.kind='calls' AND n.name='run'",
        [], |row| row.get(0),
    ).unwrap();
    let mut statement = conn.prepare("SELECT src,dst,line,evidence FROM edge_occurrences WHERE owner='App.vue' AND kind='calls'").unwrap();
    let occurrences: Vec<(String, String, usize, String)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(calls, 2, "{occurrences:#?}");
    let components: i64 = conn.query_row(
        "SELECT count(*) FROM resolution_coverage WHERE path='App.vue' AND expression='MyButton' AND status='resolved'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(components, 1);
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

    // Verify builtin println is resolved
    let println_resolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE expression='println' AND status='resolved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        println_resolved, 1,
        "expected println macro to be resolved as builtin"
    );

    // Verify crate:: import resolved to crates.core.src prefix
    let import_resolved: i64 = conn
        .query_row(
            "SELECT count(*) FROM resolution_coverage WHERE expression LIKE '%log_info%' AND status='resolved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        import_resolved >= 1,
        "expected crate:: import of log_info to be resolved"
    );
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
            .prepare("SELECT src_public_id, dst_public_id FROM edges WHERE kind='references'")
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
