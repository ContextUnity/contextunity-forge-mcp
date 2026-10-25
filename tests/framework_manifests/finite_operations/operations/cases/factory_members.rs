use super::*;

pub(super) fn case() -> Case {
    Case {
                kind: OperationKind::FactoryMembers,
                vector: Some("factory_results"),
                pack: pack(vec![], vec![syntax("factory", "typescript", json!({"kind":"factory_members","callee":{"kind":"imported","module":"statekit","export":"makeStore"},"argument":1,"state_member":"initial","actions_member":"commands"}))]),
                files: &[
                    ("store.ts", "import {makeStore} from 'statekit';\nexport const useCart = makeStore('cart', {initial: () => ({count: 0}), commands: {checkout() { return 1; }}});\nexport const other = makeStore('other', dynamicOptions);\nexport function shadow(useCart) { const cart = useCart(); return cart.checkout(); }\nexport function readLocal() { const cart = useCart(); return cart.count; }\nexport function runLocal() { const cart = useCart(); return cart.checkout(); }\n"),
                    ("client.ts", "import {useCart, other} from './store';\nexport function read() { const cart = useCart(); return cart.count; }\nexport function run() { const cart = useCart(); return cart.checkout(); }\nexport function missing() { const cart = useCart(); return cart.absent(); }\nexport function dynamic() { const cart = other(); return cart.checkout(); }\nexport function missingRead() { const cart = useCart(); return cart.absent; }\nexport function dynamicRead() { const cart = other(); return cart.count; }\nexport function readArgument() { const cart = useCart(); return consume(consume(cart.count)); }\nexport function readStatement() { const cart = useCart(); cart.count; return 1; }\nexport function missingArgument() { const cart = useCart(); return consume(cart.absent); }\nfunction consume(value) { return value; }\n"),
                    ("consumer.js", "import {useCart} from './store';\nexport function readArgument() { const cart = useCart(); return consume(consume(cart.count)); }\nexport function readStatement() { const cart = useCart(); cart.count; return 1; }\nexport function missingArgument() { const cart = useCart(); return consume(cart.absent); }\nfunction consume(value) { return value; }\n"),
                    ("reassigned.ts", "import {useCart} from './store';\nuseCart = replacement;\nexport function rebound() { const cart = useCart(); return cart.checkout(); }\n"),
                    ("duplicate.ts", "import {useCart} from './store';\nimport {useCart} from './store';\nexport function duplicate() { const cart = useCart(); return cart.checkout(); }\n"),
                    ("consumer.vue", "<script setup lang=\"ts\">\nimport {useCart} from './store';\nconst cart = useCart();\n</script>\n<template>{{ cart.count }}<button @click=\"cart.checkout()\">Go</button></template>\n"),
                    ("shadow.vue", "<script setup lang=\"ts\">\nimport {useCart} from './store';\nconst cart = useCart();\nconst rows = [];\n</script>\n<template><button v-for=\"cart in rows\" @click=\"cart.checkout()\">{{ cart.count }}</button></template>\n"),
                    ("ordinary.ts", "class Gauge {\n  ledgerValue = 1;\n  readField() { return this.ledgerValue; }\n}\nexport function unrelated() { const item = {payload: 1}; item.payload = 2; return item.payload; }\nexport function unrelatedUnknown(input) { const local = input; return local.missing; }\n"),
                    ("ordinary.js", "class Gauge {\n  ledgerValue = 1;\n  readField() { return this.ledgerValue; }\n}\nexport function unrelated() { const item = {payload: 1}; item.payload = 2; return item.payload; }\nexport function unrelatedUnknown(input) { const local = input; return local.missing; }\n"),
                    ("ordinary.html", "<script>function view() { const self = this; return self.draft; }</script>\n"),
                    ("prior-grammar.vue", "<script setup lang=\"ts\">\nimport { EVENTS } from 'observer-kit';\nenum Phase { Ready }\nfunction consume(value) { return value; }\nfunction readArgument() { return consume(EVENTS.CHANGED); }\nconst selected = EVENTS.CHANGED;\nfunction readEnum() { return Phase.Ready; }\n</script>\n<template><div>Prior grammar</div></template>\n"),
                ],
                edges: &[("client.ts", "read", "references", "store.ts", "count"), ("client.ts", "run", "calls", "store.ts", "checkout"), ("store.ts", "readLocal", "references", "store.ts", "count"), ("client.ts", "readArgument", "references", "store.ts", "count"), ("client.ts", "readStatement", "references", "store.ts", "count"), ("consumer.js", "readArgument", "references", "store.ts", "count"), ("consumer.js", "readStatement", "references", "store.ts", "count"), ("store.ts", "runLocal", "calls", "store.ts", "checkout"), ("consumer.vue", "template_scope:consumer.vue:5:0", "references", "store.ts", "count"), ("consumer.vue", "template_scope:consumer.vue:5:0", "calls", "store.ts", "checkout")],
                negatives: &[("client.ts", "missing", "calls", "store.ts", "checkout"), ("client.ts", "dynamic", "calls", "store.ts", "checkout"), ("store.ts", "shadow", "calls", "store.ts", "checkout"), ("reassigned.ts", "rebound", "calls", "store.ts", "checkout"), ("duplicate.ts", "duplicate", "calls", "store.ts", "checkout"), ("shadow.vue", "template@%", "references", "store.ts", "count"), ("shadow.vue", "template@%", "calls", "store.ts", "checkout")],
                coverage: &[("client.ts", 4, "%absent%", "unresolved"), ("client.ts", 6, "%absent%", "unresolved"), ("client.ts", 10, "%cart.absent%", "unresolved"), ("consumer.js", 4, "%cart.absent%", "unresolved")],
                consumer: "client.ts",
                rename: &[("statekit", "basketkit"), ("makeStore", "makeBasket"), ("initial", "seed"), ("commands", "operations"), ("observer-kit", "signal-kit"), ("EVENTS", "SIGNALS"), ("CHANGED", "UPDATED")],
            }
}

pub(super) fn assert_unconfigured_member_reads(
    conn: &Connection,
    phase: &str,
    rewrite: &impl Fn(&str) -> String,
) {
    let mut member_failures = Vec::new();
    for (path, expression, status, expected) in [
        ("ordinary.html", "self.draft", "unresolved", 1),
        ("prior-grammar.vue", "EVENTS.CHANGED", "external", 2),
    ] {
        let actual = query_count_usize(
            conn,
            "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression=?2 AND c.status=?3",
            params![path, rewrite(expression), status],
        );
        if actual != expected {
            member_failures.push(format!(
                "{path}: ordinary grammar {expression} must retain {expected} {status} rows, got {actual}"
            ));
        }
    }
    let canonical = node_id_by_kind(conn, "prior-grammar.vue", "field", "Ready");
    let enum_targets = edge_dst_ids(conn, "prior-grammar.vue", "readEnum", "references");
    if enum_targets.as_slice() != std::slice::from_ref(&canonical) {
        member_failures.push(format!(
            "prior-grammar.vue: ordinary enum target {canonical}, got {enum_targets:?}"
        ));
    }
    for path in ["ordinary.html", "prior-grammar.vue"] {
        let guessed = query_count_usize(
            conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path=?1 AND e.kind='references' AND s.name IN ('view','readArgument')",
            [path],
        );
        if guessed != 0 {
            member_failures.push(format!(
                "{path}: unknown/external grammar reads gained {guessed} guessed exact edges"
            ));
        }
    }
    assert_eq!(
        query_count_usize(
            conn,
            "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path='client.ts' AND c.line=7 AND x.expression='cart.count'",
            [],
        ),
        0,
        "{phase}: fact-absent dynamic provider creates no member coverage"
    );
    assert_eq!(
        query_count_usize(
            conn,
            "SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash WHERE s.name='dynamicRead' AND e.kind='references'",
            [],
        ),
        0,
        "{phase}: fact-absent dynamic provider creates no exact member edge"
    );
    for path in ["ordinary.ts", "ordinary.js"] {
        let canonical = node_id_by_kind(conn, path, "field", "ledgerValue");
        let targets: Vec<String> = query_rows_params(
            conn,
            "SELECT d.id FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash JOIN path_dictionary p ON p.path_id=s.path_id WHERE p.path=?1 AND s.name='readField' AND e.kind='references' AND d.name='ledgerValue' ORDER BY d.id",
            [path],
        );
        if targets != [canonical.clone()] {
            member_failures.push(format!(
                "{path}: ordinary exact field target {canonical}, got {targets:?}"
            ));
        }
        let facts = read_facts_json(conn, path);
        assert!(
            !facts["references"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reference| reference["kind"] == "references"
                    && reference["line"] == 5
                    && reference["column"] == 57),
            "{phase}: write-only LHS is not a read candidate"
        );
        let coverage = query_count_usize(
            conn,
            "SELECT count(*) FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id WHERE p.path=?1 AND x.expression IN ('item.payload','local.missing')",
            [path],
        );
        if coverage != 0 {
            member_failures.push(format!(
                "{path}: unconfigured local reads/write-only LHS added {coverage} coverage rows"
            ));
        }
    }
    assert!(
        member_failures.is_empty(),
        "{phase}: {}",
        member_failures.join("\n")
    );
}
