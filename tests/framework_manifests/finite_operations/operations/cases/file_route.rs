use super::*;

pub(super) fn case() -> Case {
    Case {
                kind: OperationKind::FileRoute,
                vector: None,
                pack: pack(vec![], vec![
                    syntax("pages", "vue", json!({"kind":"file_route","directory":"screens/","extension":".vue","index":"home","method":"GET"})),
                    syntax("request", "typescript", json!({"kind":"http_client_call","callee":"ask","receivers":[],"methods":{},"default_method":"GET","url_key":"url","method_key":"method","relative_prefixes":["/"]})),
                ]),
                files: &[
                    ("screens/home.vue", "<template><div>Home</div></template>\n"),
                    ("screens/catalog/detail.vue", "<template><div>Catalog</div></template>\n"),
                    ("screens/good-name.vue", "<template><div>Hyphen</div></template>\n"),
                    ("screens/catalog/home.vue", "<template><div>Index</div></template>\n"),
                    ("screens/[id].vue", "<template><div>Dynamic</div></template>\n"),
                    ("screens/dot.segment.vue", "<template><div>Unsupported</div></template>\n"),
                    ("screens/....vue", "<template><div>Unsupported</div></template>\n"),
                    ("screens/bad!.vue", "<template><div>Unsupported</div></template>\n"),
                    ("screens/-leading.vue", "<template><div>Unsupported</div></template>\n"),
                    ("screens/1leading.vue", "<template><div>Unsupported</div></template>\n"),
                    ("screens/trailing-.vue", "<template><div>Unsupported</div></template>\n"),
                    ("screens/double--part.vue", "<template><div>Unsupported</div></template>\n"),
                    ("client.ts", "export function load() { return ask('/catalog/detail'); }\nexport function missing() { return ask('/[id]'); }\nexport function hyphen() { return ask('/good-name'); }\nexport function index() { return ask('/catalog'); }\nexport function root() { return ask('/'); }\nexport function dot() { return ask('/dot.segment'); }\nexport function dots() { return ask('/...'); }\nexport function punctuation() { return ask('/bad!'); }\nexport function leading() { return ask('/-leading'); }\nexport function digit() { return ask('/1leading'); }\nexport function trailing() { return ask('/trailing-'); }\nexport function doubled() { return ask('/double--part'); }\n"),
                ],
                edges: &[("client.ts", "load", "calls_endpoint", "screens/catalog/detail.vue", "GET /catalog/detail"), ("client.ts", "hyphen", "calls_endpoint", "screens/good-name.vue", "GET /good-name"), ("client.ts", "index", "calls_endpoint", "screens/catalog/home.vue", "GET /catalog"), ("client.ts", "root", "calls_endpoint", "screens/home.vue", "GET /")],
                negatives: &[("client.ts", "missing", "calls_endpoint", "screens/[id].vue", "%"), ("client.ts", "dot", "calls_endpoint", "screens/dot.segment.vue", "%"), ("client.ts", "dots", "calls_endpoint", "screens/....vue", "%"), ("client.ts", "punctuation", "calls_endpoint", "screens/bad!.vue", "%"), ("client.ts", "leading", "calls_endpoint", "screens/-leading.vue", "%"), ("client.ts", "digit", "calls_endpoint", "screens/1leading.vue", "%"), ("client.ts", "trailing", "calls_endpoint", "screens/trailing-.vue", "%"), ("client.ts", "doubled", "calls_endpoint", "screens/double--part.vue", "%")],
                coverage: &[("client.ts", 2, "%/[id]%", "unresolved"), ("client.ts", 6, "%/dot.segment%", "unresolved"), ("client.ts", 7, "%/...%", "unresolved"), ("client.ts", 8, "%/bad!%", "unresolved"), ("client.ts", 9, "%/-leading%", "unresolved"), ("client.ts", 10, "%/1leading%", "unresolved"), ("client.ts", 11, "%/trailing-%", "unresolved"), ("client.ts", 12, "%/double--part%", "unresolved")],
                consumer: "client.ts",
                rename: &[("screens", "views"), ("home", "landing"), ("ask", "request")],
            }
}

pub(super) fn assert_unsupported_routes(
    conn: &Connection,
    phase: &str,
    rewrite: &impl Fn(&str) -> String,
) {
    for path in [
        "screens/[id].vue",
        "screens/dot.segment.vue",
        "screens/....vue",
        "screens/bad!.vue",
        "screens/-leading.vue",
        "screens/1leading.vue",
        "screens/trailing-.vue",
        "screens/double--part.vue",
    ] {
        assert_eq!(
            query_count_usize(
                conn,
                "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='route'",
                [rewrite(path)],
            ),
            0,
            "{phase}: unsupported file is not a static route: {path}"
        );
    }
}
