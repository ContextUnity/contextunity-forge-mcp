use super::*;

#[cfg(feature = "lang-typescript")]
#[test]
fn javascript_routes_preserve_middleware_and_framework_decorators_with_finite_receivers() {
    let source = "function auth() {}\nfunction view() {}\napp.get('/items', auth, view);\nserver.put('^items$', view);\napi.delete('/items', view);\nblueprint.patch('/items', view);\nroute.options('/items', view);\nrouter.head('/items', view);\napp.get('/asserted', view as Handler);\napp.get('/non-null', (view!));\napp.get('/type-assertion', <Handler>view);\napp.get('/string-as', 'view' as unknown as Handler);\nconst routes = [{path: 'settings', component: view}];\nclass Controller { @Get('/box') show() {} }\ncache.get('/setting', view);\napp.get('setting', view);\nget('/setting', view);\napp.get('/number', 0);\napp.get('/negative', -1);\napp.get('/boolean', true);\napp.get('/null', null);\napp.get('/string', 'view');\napp.get('/template', `view`);\napp.get('/array', [view]);\napp.get('/object', {view});\nconst values = [{path: '/primitive', component: false}];\nfunction configure(options) {}\nconfigure({path: '/settings', component: view});\naxios . get('/users');\nclient .post('/api/orders');\n";
    let facts = ast::extract("routes.ts", "typescript", source).unwrap();
    assert!(facts.errors.is_empty(), "{:?}", facts.errors);
    let mut routes: Vec<_> = facts
        .nodes
        .iter()
        .filter(|node| node.kind == "route")
        .map(|node| node.name.as_str())
        .collect();
    routes.sort_unstable();
    assert!(routes.contains(&"ANY settings"));
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.kind == "calls" && reference.expression == "configure"));
    for expression in ["GET /users", "POST /api/orders"] {
        assert!(
            facts.references.iter().any(|reference| {
                reference.kind == "calls_endpoint" && reference.expression == expression
            }),
            "missing endpoint reference {expression}: {:?}",
            facts.references
        );
    }
    assert_eq!(
        routes,
        [
            "ANY settings",
            "DELETE /items",
            "GET /asserted",
            "GET /box",
            "GET /items",
            "GET /non-null",
            "GET /type-assertion",
            "HEAD /items",
            "OPTIONS /items",
            "PATCH /items",
            "PUT ^items$"
        ]
    );
    let items = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /items")
        .unwrap();
    assert_eq!(
        items.details["handlers"],
        serde_json::json!(["auth", "view"])
    );
    assert!(facts
        .references
        .iter()
        .any(|reference| reference.kind == "calls"
            && reference.expression == "cache.get"
            && reference.line == 15));
    let decorated = facts
        .nodes
        .iter()
        .find(|node| node.name == "GET /box")
        .unwrap();
    let handler = facts.nodes.iter().find(|node| node.name == "show").unwrap();
    assert!(facts
        .edges
        .iter()
        .any(|edge| edge.src == decorated.id && edge.dst == handler.id && edge.kind == "handles"));
    let graph = linker::link(&BTreeMap::from([("routes.ts".to_owned(), facts.clone())]));
    let view = facts.nodes.iter().find(|node| node.name == "view").unwrap();
    for name in ["GET /asserted", "GET /non-null", "GET /type-assertion"] {
        let route = facts.nodes.iter().find(|node| node.name == name).unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|edge| edge.src == route.id && edge.dst == view.id && edge.kind == "handles"));
    }
}
