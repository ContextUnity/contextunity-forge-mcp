use super::*;
#[test]
fn finite_javascript_static_globals_respect_local_shadowing() {
    let source = "function run() { Object.hasOwn({}, 'x'); Promise.all([]); Date.now(); CSS.escape('x'); encodeURIComponent('x'); }";
    let graph = linked("globals.js", "javascript", source);
    for expression in [
        "Object.hasOwn",
        "Promise.all",
        "Date.now",
        "CSS.escape",
        "encodeURIComponent",
    ] {
        assert!(
            graph.statuses(expression).contains(&"external"),
            "{expression}: {graph:#?}"
        );
    }
    let shadow = "function run(Object, Promise, Date, CSS, encodeURIComponent) { Object.hasOwn({}, 'x'); Promise.all([]); Date.now(); CSS.escape('x'); encodeURIComponent('x'); }";
    let graph = linked("shadow.js", "javascript", shadow);
    for expression in [
        "Object.hasOwn",
        "Promise.all",
        "Date.now",
        "CSS.escape",
        "encodeURIComponent",
    ] {
        assert!(
            graph.statuses(expression).contains(&"unresolved"),
            "shadowed {expression}: {graph:#?}"
        );
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn test_typescript_ast_extractor() {
    let source = r#"
import { Router } from "express";

export interface User {
    id: string;
    name: string;
}

export class UserService {
    getUser(id: string): User {
        return { id, name: "Alice" };
    }
}

export const processUser = (u: User) => {
    console.log(u);
};
"#;
    let facts = ExtractorHarness::for_lang("typescript")
        .at_path("test.ts")
        .extract(source);
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "class" && n.name == "UserService"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "getUser"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "processUser"));
    let process_user = facts
        .nodes
        .iter()
        .find(|n| n.name == "processUser")
        .expect("processUser function missing");
    assert!(process_user.details.get("is_method").is_none());
    assert!(process_user.details.get("is_static").is_none());
    assert!(process_user.details.get("receiver_type").is_none());
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_tsx_parser_slot_reuses_cleanly_across_files() {
    for (path, source, expected_name) in [
        (
            "First.tsx",
            "export const First = () => <main><span /></main>;",
            "First",
        ),
        (
            "Second.tsx",
            "export function Second() { return <button />; }",
            "Second",
        ),
    ] {
        let facts = ExtractorHarness::for_lang("typescript")
            .at_path(path)
            .extract(source);
        assert!(facts.errors.is_empty(), "{path}: {:?}", facts.errors);
        assert!(facts
            .nodes
            .iter()
            .any(|node| node.name == expected_name && node.kind == "function"));
    }
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_lexical_this_nested_arrows() {
    let w = crate::languages::PersistedWorkspaceHarness::new();
    w.write(
        "counter.ts",
        r#"class Counter {
  ready = false;
  static active = true;
  mark() {}
  static reset() {}
  run() {
    const read = () => this.ready;
    const call = () => this?.mark();
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
    w.build();
    let conn = w.open();
    for (path, line, expression) in [
        ("counter.ts", 7, "this.ready"),
        ("counter.ts", 8, "this?.mark"),
        ("counter.ts", 14, "this.active"),
        ("counter.ts", 15, "this.reset"),
        ("counter.ts", 20, "this.ready"),
        ("counter.ts", 24, "this.mark"),
        ("counter.js", 1, "this.ready"),
    ] {
        let (st, ev) = persisted_coverage(&conn, path, Some(line), expression);
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
            persisted_coverage(&conn, "counter.ts", Some(line), expression).0,
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
        let edges: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)=?1 AND e.line=?2 AND e.kind IN ('references','calls') AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)=?1 AND dst.name=?3 AND dst.kind=?4",
            rusqlite::params![path, line, target, kind],
        );
        assert_eq!(
            edges, 1,
            "arrow must link to exact same-file {kind} {path}:{line} {target}"
        );
    }
    for kind in ["calls", "references"] {
        let edges: i64 = query_scalar(
            &conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='counter.ts' AND e.line=28 AND e.kind=?1 AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='counter.ts' AND dst.kind='method' AND dst.name='mark'",
            [kind],
        );
        assert_eq!(
            edges, 1,
            "same-line captured arrow needs an exact {kind} edge to Counter.mark"
        );
    }
    let sibling_edges: i64 = query_scalar(
        &conn,
        "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='counter.ts' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='other.ts'",
        [],
    );
    assert_eq!(
        sibling_edges, 0,
        "same-name Counter in sibling file cannot provide this members"
    );
}

#[cfg(feature = "lang-vue")]
#[test]
fn vue_profile_parser_reuse_tracks_the_selected_grammar() {
    let profile =
        contextunity_forge_mcp::engine::languages::by_id("vue").expect("Vue profile is compiled");
    for (path, source, expected_name) in [
        (
            "First.vue",
            "export function First() { return 1; }",
            "First",
        ),
        (
            "Second.tsx",
            "export function Second() { return <button />; }",
            "Second",
        ),
        ("Third.js", "export function Third() { return 1; }", "Third"),
    ] {
        let mut facts = contextunity_forge_mcp::core::models::Facts::default();
        contextunity_forge_mcp::engine::languages::parse_file(
            profile, path, source, path, &mut facts,
        )
        .expect("Vue profile parse failed");
        assert!(facts.errors.is_empty(), "{path}: {:?}", facts.errors);
        assert!(facts
            .nodes
            .iter()
            .any(|node| node.name == expected_name && node.kind == "function"));
    }
}
