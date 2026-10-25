use super::*;

pub(in crate::languages::html::manifest_relations) fn configured_router_prefixes_are_static_unique_and_recursive(
) {
    let workspace = Workspace::new();
    workspace.write("pyproject.toml", "[project]\nname='prefix-proof'\n");
    let pack = serde_json::json!({
        "route_decorators":[{"callee":".expose", "method":"GET", "path_argument":0,
            "receiver_names":["leaf"], "prefixes":[
                {"kind":"constructor","callee":"Mount","keyword":"base_path"},
                {"kind":"attachment","callee":".attach","child_argument":0,"prefix_keyword":"under"}
            ]}],
        "syntax_rules":[{"id":"client","language":"python","operation":{
            "kind":"http_client_call","callee":"ask","receivers":[],"methods":{},
            "default_method":"GET","url_key":"destination","method_key":"verb","relative_prefixes":["/"]}}]
    });
    let manifest = ".forge/frameworks/prefix-proof.toml";
    workspace.write(manifest, &toml::to_string(&pack).unwrap());
    let valid = "root = Mount(base_path='/outer')\nmiddle = Mount(base_path='/middle')\nleaf = Mount(base_path='/leaf')\nroot.attach(middle, under='/v1')\nmiddle.attach(leaf, under='/v2')\n@leaf.expose('/item')\ndef endpoint(): return 1\ndef consumer(): return ask('/outer/v1/middle/v2/leaf/item')\ndef helper(): return 1\ndef ordinary(): return helper()\n";
    workspace.write("app.py", valid);
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let assert_routes = |active: bool| {
        let conn = workspace.open();
        assert_valid(&conn);
        let routes: Vec<String> = conn.prepare("SELECT json_extract(details,'$.route_path') FROM nodes WHERE kind='route' ORDER BY id").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(
            routes,
            if active {
                vec!["/outer/v1/middle/v2/leaf/item"]
            } else {
                vec![]
            }
        );
        assert_eq!(
            edge_occurrence_rows(&conn, "app.py", "calls_endpoint").len(),
            usize::from(active)
        );
        let ordinary: i64 = conn.query_row("SELECT count(*) FROM edge_occurrences e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE e.kind='calls' AND s.name='ordinary' AND d.name='helper'",[],|row|row.get(0)).unwrap();
        assert_eq!(ordinary, 1);
    };
    assert_routes(true);
    complete_parity(&workspace);
    for suffix in [
        "def unrelated(leaf):\n    @leaf.expose('/shadow')\n    def shadow_handler(): return 1\n    return shadow_handler\n",
        "def unrelated():\n    leaf = object()\n    @leaf.expose('/shadow')\n    def shadow_handler(): return 1\n    return shadow_handler\n",
        "def unrelated(root, leaf):\n    root.attach(leaf, under='/shadow')\n",
        "def unrelated():\n    root = object()\n    leaf = object()\n    root.attach(leaf, under='/shadow')\n",
    ] {
        workspace.write("app.py", &format!("{valid}\n{suffix}"));
        workspace.delta("app.py");
        assert_routes(true);
        complete_parity(&workspace);
    }
    workspace.write("app.py", &format!("{valid}\ndef independent():\n    leaf = Mount(base_path='/local')\n    @leaf.expose('/own')\n    def local_endpoint(): return 1\n    return local_endpoint\n"));
    workspace.delta("app.py");
    {
        let conn = workspace.open();
        assert_valid(&conn);
        let routes: Vec<String> = conn.prepare("SELECT json_extract(details,'$.route_path') FROM nodes WHERE kind='route' ORDER BY json_extract(details,'$.route_path')").unwrap().query_map([],|row|row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(
            routes,
            ["/local/own", "/outer/v1/middle/v2/leaf/item"],
            "independent constructor keeps its own lexical prefix"
        );
    }
    complete_parity(&workspace);
    for invalid in [
        valid.replace("base_path='/leaf'", "base_path=unknown"),
        valid.replace("under='/v2'", "under=unknown"),
        format!("{valid}\nleaf = Mount(base_path='/rewritten')\n"),
        format!("{valid}\nleaf += object()\n"),
        format!("{valid}\ndel leaf\n"),
        format!("{valid}\nleaf, other = (object(), object())\n"),
        format!("{valid}\nroot.attach(leaf, under='/duplicate')\n"),
        format!("{valid}\nleaf.attach(root, under='/cycle')\n"),
    ] {
        workspace.write("app.py", &invalid);
        workspace.delta("app.py");
        assert_routes(false);
        complete_parity(&workspace);
    }
    workspace.write("app.py", valid);
    workspace.delta("app.py");
    assert_routes(true);
    let mut replacement = pack.clone();
    replacement["replace"] = true.into();
    replacement["route_decorators"] = serde_json::json!([]);
    replacement["syntax_rules"] = serde_json::json!([]);
    workspace.write(manifest, &toml::to_string(&replacement).unwrap());
    workspace.delta(manifest);
    assert_routes(false);
    complete_parity(&workspace);
}
