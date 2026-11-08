use super::*;
#[cfg(feature = "lang-python")]
#[test]
fn python_routes_preserve_http_decorators_and_django_patterns_as_mapping_calls_remain_calls() {
    let source = "def auth(): pass\ndef handler(): pass\napp.get('/items', auth, handler)\nrouter.post('^item$', handler)\npath('items/', handler)\nre_path('^items/', handler)\ndjango.urls.path('', handler)\n@bp.route('/multi', methods=['GET', 'PATCH'])\ndef decorated(): pass\n@mapping.get('/lookup')\ndef mapped(): pass\nconfig.get('section', handler)\napp.get('setting', handler)\nconfig.get('/setting', handler)\napp.get('/number', 0)\napp.get('/negative', -1)\napp.get('/boolean', True)\napp.get('/none', None)\napp.get('/string', 'handler')\napp.get('/list', [handler])\napp.get('/tuple', (handler,))\napp.get('/mapping', {'handler': handler})\n";
    let facts = ExtractorHarness::for_lang("python")
        .at_path("routes.py")
        .extract(source);
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let mut routes: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "route")
        .map(|node| node.name.as_str())
        .collect();
    routes.sort_unstable();
    assert_eq!(
        routes,
        [
            "ANY ",
            "ANY ^items/",
            "ANY items/",
            "GET /items",
            "GET /multi",
            "PATCH /multi",
            "POST ^item$"
        ]
    );
    let items = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /items")
        .unwrap();
    assert_eq!(
        items.details["handlers"],
        serde_json::json!(["auth", "handler"])
    );
    let ordinary_calls: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.kind == "calls" && reference.expression == "config.get")
        .map(|reference| reference.line)
        .collect();
    assert_eq!(ordinary_calls, [12, 14]);
    for name in ["GET /multi", "PATCH /multi"] {
        let route = facts.nodes.iter().find(|node| node.name == name).unwrap();
        let handler = facts
            .nodes
            .iter()
            .find(|node| node.name == "decorated")
            .unwrap();
        assert!(facts
            .edges
            .iter()
            .any(|edge| edge.src == route.id && edge.dst == handler.id && edge.kind == "handles"));
    }
}
