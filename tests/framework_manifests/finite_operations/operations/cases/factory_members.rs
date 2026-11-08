use super::*;

pub(super) fn case() -> Case {
    Case {
        kind: OperationKind::FactoryMembers,
        vector: Some("factory_results"),
        pack: pack(
            vec![],
            vec![syntax(
                "factory",
                "typescript",
                json!({"kind":"factory_members","callee":{"kind":"imported","module":"statekit","export":"makeStore"},"argument":1,"state_member":"initial","actions_member":"commands"}),
            )],
        ),
        files: &[
            (
                "store.ts",
                "import {makeStore} from 'statekit';\nexport const useCart = makeStore('cart', {initial: () => ({count: 0}), commands: {checkout() { return 1; }}});\nexport const other = makeStore('other', dynamicOptions);\nexport function shadow(useCart) { const cart = useCart(); return cart.checkout(); }\nexport function readLocal() { const cart = useCart(); return cart.count; }\nexport function runLocal() { const cart = useCart(); return cart.checkout(); }\n",
            ),
            (
                "client.ts",
                "import {useCart, other} from './store';\nexport function read() { const cart = useCart(); return cart.count; }\nexport function run() { const cart = useCart(); return cart.checkout(); }\nexport function missing() { const cart = useCart(); return cart.absent(); }\nexport function dynamic() { const cart = other(); return cart.checkout(); }\nexport function missingRead() { const cart = useCart(); return cart.absent; }\nexport function dynamicRead() { const cart = other(); return cart.count; }\nexport function readArgument() { const cart = useCart(); return consume(consume(cart.count)); }\nexport function readStatement() { const cart = useCart(); cart.count; return 1; }\nexport function missingArgument() { const cart = useCart(); return consume(cart.absent); }\nfunction consume(value) { return value; }\n",
            ),
            (
                "consumer.js",
                "import {useCart} from './store';\nexport function readArgument() { const cart = useCart(); return consume(consume(cart.count)); }\nexport function readStatement() { const cart = useCart(); cart.count; return 1; }\nexport function missingArgument() { const cart = useCart(); return consume(cart.absent); }\nfunction consume(value) { return value; }\n",
            ),
            (
                "reassigned.ts",
                "import {useCart} from './store';\nuseCart = replacement;\nexport function rebound() { const cart = useCart(); return cart.checkout(); }\n",
            ),
            (
                "duplicate.ts",
                "import {useCart} from './store';\nimport {useCart} from './store';\nexport function duplicate() { const cart = useCart(); return cart.checkout(); }\n",
            ),
            (
                "consumer.vue",
                "<script setup lang=\"ts\">\nimport {useCart} from './store';\nconst cart = useCart();\n</script>\n<template>{{ cart.count }}<button @click=\"cart.checkout()\">Go</button></template>\n",
            ),
            (
                "shadow.vue",
                "<script setup lang=\"ts\">\nimport {useCart} from './store';\nconst cart = useCart();\nconst rows = [];\n</script>\n<template><button v-for=\"cart in rows\" @click=\"cart.checkout()\">{{ cart.count }}</button></template>\n",
            ),
            (
                "ordinary.ts",
                "class Gauge {\n  ledgerValue = 1;\n  readField() { return this.ledgerValue; }\n}\nexport function unrelated() { const item = {payload: 1}; item.payload = 2; return item.payload; }\nexport function unrelatedUnknown(input) { const local = input; return local.missing; }\n",
            ),
            (
                "ordinary.js",
                "class Gauge {\n  ledgerValue = 1;\n  readField() { return this.ledgerValue; }\n}\nexport function unrelated() { const item = {payload: 1}; item.payload = 2; return item.payload; }\nexport function unrelatedUnknown(input) { const local = input; return local.missing; }\n",
            ),
            (
                "ordinary.html",
                "<script>function view() { const self = this; return self.draft; }</script>\n",
            ),
            (
                "prior-grammar.vue",
                "<script setup lang=\"ts\">\nimport { EVENTS } from 'observer-kit';\nenum Phase { Ready }\nfunction consume(value) { return value; }\nfunction readArgument() { return consume(EVENTS.CHANGED); }\nconst selected = EVENTS.CHANGED;\nfunction readEnum() { return Phase.Ready; }\n</script>\n<template><div>Prior grammar</div></template>\n",
            ),
        ],
        edges: &[
            ("client.ts", "read", "references", "store.ts", "count"),
            ("client.ts", "run", "calls", "store.ts", "checkout"),
            ("store.ts", "readLocal", "references", "store.ts", "count"),
            (
                "client.ts",
                "readArgument",
                "references",
                "store.ts",
                "count",
            ),
            (
                "client.ts",
                "readStatement",
                "references",
                "store.ts",
                "count",
            ),
            (
                "consumer.js",
                "readArgument",
                "references",
                "store.ts",
                "count",
            ),
            (
                "consumer.js",
                "readStatement",
                "references",
                "store.ts",
                "count",
            ),
            ("store.ts", "runLocal", "calls", "store.ts", "checkout"),
            (
                "consumer.vue",
                "template_scope:consumer.vue:5:0",
                "references",
                "store.ts",
                "count",
            ),
            (
                "consumer.vue",
                "template_scope:consumer.vue:5:0",
                "calls",
                "store.ts",
                "checkout",
            ),
        ],
        negatives: &[
            ("client.ts", "missing", "calls", "store.ts", "checkout"),
            ("client.ts", "dynamic", "calls", "store.ts", "checkout"),
            ("store.ts", "shadow", "calls", "store.ts", "checkout"),
            ("reassigned.ts", "rebound", "calls", "store.ts", "checkout"),
            ("duplicate.ts", "duplicate", "calls", "store.ts", "checkout"),
            (
                "shadow.vue",
                "template@%",
                "references",
                "store.ts",
                "count",
            ),
            ("shadow.vue", "template@%", "calls", "store.ts", "checkout"),
        ],
        coverage: &[
            ("client.ts", 4, "%absent%", "unresolved"),
            ("client.ts", 6, "%absent%", "unresolved"),
            ("client.ts", 10, "%cart.absent%", "unresolved"),
            ("consumer.js", 4, "%cart.absent%", "unresolved"),
        ],
        consumer: "client.ts",
        rename: &[
            ("statekit", "basketkit"),
            ("makeStore", "makeBasket"),
            ("initial", "seed"),
            ("commands", "operations"),
            ("observer-kit", "signal-kit"),
            ("EVENTS", "SIGNALS"),
            ("CHANGED", "UPDATED"),
        ],
    }
}
