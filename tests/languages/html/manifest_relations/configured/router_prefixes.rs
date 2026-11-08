use super::*;

#[test]
fn configured_router_prefixes_are_static_unique_and_recursive() {
    let workspace = HtmlWorkspaceHarness::new();
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
    workspace.build();
    let assert_routes = |active: bool| {
        let observations = workspace.observe();
        let validity = &observations.validity();
        validity.assert_valid();
        let routes: Vec<_> = observations
            .nodes()
            .iter()
            .filter(|node| node.kind == "route")
            .map(|node| {
                serde_json::from_str::<serde_json::Value>(&node.details).unwrap()["route_path"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            routes,
            if active {
                vec!["/outer/v1/middle/v2/leaf/item"]
            } else {
                vec![]
            }
        );
        assert_eq!(
            observations
                .edges()
                .iter()
                .filter(|edge| edge.owner_path == "app.py" && edge.kind == "calls_endpoint")
                .count(),
            usize::from(active)
        );
        let ordinary = observations
            .edges()
            .iter()
            .filter(|edge| {
                edge.kind == "calls"
                    && edge.source_name == "ordinary"
                    && edge.target_name == "helper"
            })
            .count();
        assert_eq!(ordinary, 1);
    };
    assert_routes(true);
    workspace.assert_cold_parity();
    for suffix in [
        "def unrelated(leaf):\n    @leaf.expose('/shadow')\n    def shadow_handler(): return 1\n    return shadow_handler\n",
        "def unrelated():\n    leaf = object()\n    @leaf.expose('/shadow')\n    def shadow_handler(): return 1\n    return shadow_handler\n",
        "def unrelated(root, leaf):\n    root.attach(leaf, under='/shadow')\n",
        "def unrelated():\n    root = object()\n    leaf = object()\n    root.attach(leaf, under='/shadow')\n",
    ] {
        workspace.write("app.py", &format!("{valid}\n{suffix}"));
        workspace.delta(&["app.py"]);
        assert_routes(true);
    }
    workspace.write("app.py", &format!("{valid}\ndef independent():\n    leaf = Mount(base_path='/local')\n    @leaf.expose('/own')\n    def local_endpoint(): return 1\n    return local_endpoint\n"));
    workspace.delta(&["app.py"]);
    {
        let observations = workspace.observe();
        let validity = &observations.validity();
        validity.assert_valid();
        let mut routes: Vec<_> = observations
            .nodes()
            .iter()
            .filter(|node| node.kind == "route")
            .map(|node| {
                serde_json::from_str::<serde_json::Value>(&node.details).unwrap()["route_path"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        routes.sort();
        assert_eq!(
            routes,
            ["/local/own", "/outer/v1/middle/v2/leaf/item"],
            "independent constructor keeps its own lexical prefix"
        );
    }
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
        workspace.delta(&["app.py"]);
        assert_routes(false);
    }
    workspace.write("app.py", valid);
    workspace.delta(&["app.py"]);
    assert_routes(true);
    let mut replacement = pack.clone();
    replacement["replace"] = true.into();
    replacement["route_decorators"] = serde_json::json!([]);
    replacement["syntax_rules"] = serde_json::json!([]);
    workspace.write(manifest, &toml::to_string(&replacement).unwrap());
    workspace.delta(&[manifest]);
    assert_routes(false);
    workspace.assert_cold_parity();
}
