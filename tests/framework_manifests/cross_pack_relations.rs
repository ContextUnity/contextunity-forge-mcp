use super::*;

#[cfg(all(feature = "lang-python", feature = "lang-typescript"))]
#[test]
fn declarative_framework_rules_reach_persisted_receivers_routes_and_dependencies() {
    let workspace = Workspace::new();
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'sample'\ndependencies = ['django', 'fastapi', 'grid-view-spec']\n",
    );
    workspace.write("package.json", r#"{"dependencies":{"@nestjs/common":"*"}}"#);
    workspace.write(
        ".forge/frameworks/django.toml",
        r#"
[[class_hierarchy_receivers]]
bases = ["django.custom.Base"]
provider = "django.custom.Base"
members = ["deliver"]
[[route_decorators]]
callee = "portal.expose"
method = "POST"
path_argument = 0
[[route_calls]]
callee = "register_endpoint"
method = "GET"
path_argument = 0
handler_argument = 1
[[route_calls]]
callee = "register_with_handler"
method = "GET"
path_argument = 0
handler_argument = 2
[[route_calls]]
callee = "register_with_middleware"
method = "GET"
path_argument = 0
handler_argument = 1
[[route_calls]]
callee = "path"
method = "ANY"
path_argument = 0
handler_argument = 1
allow_relative_path = true
name_keyword = "name"
include = { callee = ".include", argument = 0, namespace_keyword = "namespace" }
[[route_calls]]
callee = "publish"
method = "GET"
path_argument = 0
[[route_decorators]]
callee = "Get"
method = "GET"
path_argument = 0
prefixes = [{ kind = "class_decorator", callee = "Controller", argument = 0 }]
[[di_calls]]
callee = "InjectService"
argument = 0
"#,
    );
    workspace.write(
        "app.py",
        r#"from django.custom import Base
class Worker(Base):
    def run(self):
        self.deliver()
def dependency():
    return Worker()
def middleware():
    return None
@portal.expose('/custom')
def endpoint(service = InjectService(dependency)):
    return service
register_endpoint('/registered', endpoint)
register_endpoint('/missing-handler')
register_endpoint('/keyword-handler', callback=endpoint)
register_with_handler('/offset-missing', 'metadata')
register_with_handler('/offset-valid', 'metadata', endpoint)
register_with_middleware('/middleware', middleware, endpoint)
catalog.path('setting', endpoint)
publish('/no-handler')
@catalog.Get('/setting')
def unrelated():
    return None
"#,
    );
    workspace.write(
        "grid_spec.py",
        "from grid_view_spec.types.spec import GridViewSpec\nfrom grid_view_spec.types.block_base import GridViewBlockBase\nclass CustomSpec(GridViewSpec):\n    def config_value(self): return self.config\nclass CustomBlock(GridViewBlockBase):\n    def block_type(self): return self.type\n",
    );
    let gridview_receiver_cases = [
        (
            "GridViewSpec",
            "grid_view_spec.types.spec.GridViewSpec",
            "blocks",
        ),
        (
            "GridViewMeta",
            "grid_view_spec.types.spec_meta.GridViewMeta",
            "subtitle",
        ),
        (
            "GridViewConfig",
            "grid_view_spec.types.spec_meta.GridViewConfig",
            "htmx",
        ),
        (
            "GridViewLayout",
            "grid_view_spec.types.layout.GridViewLayout",
            "root",
        ),
        (
            "GridViewBlockBase",
            "grid_view_spec.types.block_base.GridViewBlockBase",
            "trusted_style",
        ),
        (
            "GridViewTable",
            "grid_view_spec.types.table_v2.GridViewTable",
            "column_source",
        ),
        (
            "GridViewToolbar",
            "grid_view_spec.types.toolbar.GridViewToolbar",
            "counters",
        ),
        (
            "GridViewFilters",
            "grid_view_spec.types.filters_v2.GridViewFilters",
            "drawer_sections",
        ),
        (
            "GridViewActions",
            "grid_view_spec.types.actions.GridViewActions",
            "items",
        ),
        (
            "GridViewCards",
            "grid_view_spec.types.content.GridViewCards",
            "cards",
        ),
        (
            "GridViewCharts",
            "grid_view_spec.types.content.GridViewCharts",
            "charts",
        ),
        (
            "GridViewKpi",
            "grid_view_spec.types.content.GridViewKpi",
            "items",
        ),
        (
            "GridViewTabs",
            "grid_view_spec.types.content.GridViewTabs",
            "tabs",
        ),
        (
            "GridViewNav",
            "grid_view_spec.types.nav.GridViewNav",
            "levels",
        ),
        (
            "GridViewForm",
            "grid_view_spec.types.form.GridViewForm",
            "fieldsets",
        ),
        (
            "GridViewHeader",
            "grid_view_spec.types.header.GridViewHeader",
            "entity",
        ),
        (
            "GridViewTemplate",
            "grid_view_spec.types.content.GridViewTemplate",
            "mode",
        ),
        (
            "GridViewColumn",
            "grid_view_spec.types.table_v2.GridViewColumn",
            "currency_formatter",
        ),
        (
            "GridViewFilter",
            "grid_view_spec.types.filters_v2.GridViewFilter",
            "drawer_control",
        ),
        (
            "GridViewCard",
            "grid_view_spec.types.content.GridViewCard",
            "meta",
        ),
        (
            "DjangoGridViewHost",
            "grid_view_spec.backends.django.host.DjangoGridViewHost",
            "url_for",
        ),
        (
            "GridViewActionBase",
            "grid_view_spec.types.actions.GridViewActionBase",
            "confirm",
        ),
        (
            "GridViewExportAction",
            "grid_view_spec.types.actions.GridViewExportAction",
            "format",
        ),
        (
            "GridViewOverlayAction",
            "grid_view_spec.types.actions.GridViewOverlayAction",
            "overlay",
        ),
        (
            "GridViewLinkAction",
            "grid_view_spec.types.actions.GridViewLinkAction",
            "href",
        ),
        (
            "GridViewButtonAction",
            "grid_view_spec.types.actions.GridViewButtonAction",
            "action",
        ),
        (
            "GridViewSelectAction",
            "grid_view_spec.types.actions.GridViewSelectAction",
            "options",
        ),
        (
            "GridViewMenuAction",
            "grid_view_spec.types.actions.GridViewMenuAction",
            "items",
        ),
    ];
    for &(class_name, provider, member) in &gridview_receiver_cases {
        let (module, symbol) = provider.rsplit_once('.').unwrap();
        let import_module = match class_name {
            "DjangoGridViewHost" | "GridViewActionBase" => module,
            _ => "grid_view_spec.types",
        };
        let missing_member = format!("_forge_missing_{class_name}");
        workspace.write(
            format!("grid_receivers/{class_name}.py"),
            &format!("from {import_module} import {symbol} as ExternalBase\nclass Probe{class_name}(ExternalBase):\n    def known_probe(self): return self.{member}\n    def unknown_probe(self): return self.{missing_member}\n"),
        );
    }
    workspace.write(
        "fast.py",
        r#"from fastapi import APIRouter, Depends
service_api = APIRouter(prefix='/api')
def dependency():
    return 1
def middleware(request):
    return request
@service_api.get('/items')
def items(value = Depends(dependency)):
    return value
api.get('/builtin-missing')
api.get('/keyword-handler', callback=items)
api.get('/builtin-valid', items)
api.get('/middleware', middleware, items)
api.get('/invalid-literal-handler', 'handler')
api.get(items)
"#,
    );
    workspace.write("controller.ts", "import { Controller, Get } from '@nestjs/common';\n@Controller('/nest')\nclass ControllerClass {\n@Get('/items')\nitems() { return 1; }\n}\n");
    workspace.write(
        "urls.py",
        "from django.urls import path, include\ndef endpoint(): return 'parent decoy'\nurlpatterns = [path('shop/', include('shop.urls', namespace='shop')), path('nested/', include('nested.urls', namespace='nested')), path('parent-decoy/', endpoint)]\n",
    );
    workspace.write("shop/urls.py", "from django.urls import path\nfrom app import endpoint\nurlpatterns = [path('detail/', endpoint, name='detail')]\n");
    workspace.write(
        "nested/urls.py",
        "from django.urls import path, include\nurlpatterns = [path('catalog/', include('catalog.endpoints', namespace='catalog'))]\n",
    );
    workspace.write(
        "catalog/endpoints.py",
        "from django.urls import path\nfrom .views import endpoint\nurlpatterns = [path('deep/', endpoint, name='deep')]\n",
    );
    workspace.write(
        "catalog/views.py",
        "def endpoint(): return 'catalog endpoint'\n",
    );
    workspace.write(
        "qualified_urls.py",
        "import django.urls\nfrom django import urls\ndef endpoint(): return None\na = django.urls.path('/qualified', endpoint)\nb = urls.re_path('^regex/$', endpoint)\n",
    );
    let registry = DependencyRegistry::collect_with_scan_config(&workspace.0, None);
    assert!(registry
        .framework_manifest_for_path(LanguageFamily("python"), "app.py", "django", "django")
        .is_some());
    let conn = workspace.build();
    let routes = query_rows(
        &conn,
        "SELECT name FROM nodes WHERE kind='route' ORDER BY name",
    );
    for expected in [
        "POST /custom",
        "GET /registered",
        "GET /no-handler",
        "GET /offset-valid",
        "GET /middleware",
        "GET /builtin-valid",
        "GET /api/items",
        "GET /nest/items",
        "ANY shop/detail/",
        "ANY nested/catalog/deep/",
        "ANY parent-decoy/",
        "ANY /qualified",
        "ANY ^regex/$",
    ] {
        assert!(
            routes.iter().any(|name| name == expected),
            "missing {expected}: {routes:?}"
        );
    }
    let composed_handles: Vec<(String, String, String)> = conn
        .prepare("SELECT route_path.path,target_path.path,target.qualname FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary route_path ON route_path.path_id=route.owner_path_id JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE edge.kind='handles' AND route.kind='route' AND route.name='ANY shop/detail/'")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect();
    let missing_configured_routes = query_count(&conn, "SELECT count(*) FROM nodes WHERE kind='route' AND name IN ('GET /missing-handler','GET /keyword-handler','GET /offset-missing','GET /builtin-missing','GET /invalid-literal-handler')", []);
    assert!(
        composed_handles.iter().any(|(route_path, target_path, target)| {
            route_path == "urls.py" && target_path == "app.py" && target == "app.endpoint"
        }) && missing_configured_routes == 0,
        "both independent contract boundaries must hold: composed_handles={composed_handles:?}, missing_configured_routes={missing_configured_routes}"
    );
    assert!(
        !routes
            .iter()
            .any(|name| name == "ANY setting" || name == "GET /setting"),
        "bare callee rules must not admit arbitrary qualified callees: {routes:?}"
    );
    for absent in [
        "GET /missing-handler",
        "GET /keyword-handler",
        "GET /offset-missing",
        "GET /builtin-missing",
        "GET /invalid-literal-handler",
    ] {
        assert!(
            !routes.iter().any(|name| name == absent),
            "configured handler positions must not create handlerless routes: {absent}: {routes:?}"
        );
    }
    let inherited = coverage_status_and_evidence(&conn, "app.py", "self.deliver");
    assert_eq!(inherited.0, "external", "{inherited:?}");
    assert!(
        inherited
            .1
            .contains("resolved member django.custom.Base.deliver")
            && inherited.1.contains("via external import django.custom"),
        "project receiver must preserve its exact provider identity: {inherited:?}"
    );
    let grid_spec_receivers: Vec<(String, String, String)> = conn
        .prepare("SELECT x.expression,c.status,e.evidence FROM resolution_coverage c JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence e ON e.evidence_id=c.evidence_id JOIN path_dictionary p ON p.path_id=c.path_id WHERE p.path='grid_spec.py' AND x.expression IN ('self.config','self.type') ORDER BY x.expression")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(grid_spec_receivers.len(), 2, "{grid_spec_receivers:?}");
    assert!(grid_spec_receivers.iter().all(|(_, status, evidence)| {
        status == "external" && evidence.starts_with("finite Python framework member ")
    }), "verified GridView dataclass fields must resolve as external members: {grid_spec_receivers:?}");
    assert!(
        grid_spec_receivers.iter().any(|(expression, _, evidence)| {
            expression == "self.config"
                && evidence.contains("grid_view_spec.types.spec.GridViewSpec.config")
        }),
        "GridViewSpec field provenance must be exact: {grid_spec_receivers:?}"
    );
    assert!(
        grid_spec_receivers.iter().any(|(expression, _, evidence)| {
            expression == "self.type"
                && evidence.contains("grid_view_spec.types.block_base.GridViewBlockBase.type")
        }),
        "GridViewBlockBase field provenance must be exact: {grid_spec_receivers:?}"
    );
    assert_eq!(gridview_receiver_cases.len(), 28);
    for &(class_name, provider, member) in &gridview_receiver_cases {
        let source_path = format!("grid_receivers/{class_name}.py");
        let known = coverage_status_and_evidence(&conn, &source_path, &format!("self.{member}"));
        assert_eq!(
            known.0, "external",
            "{class_name}.{member} must resolve as a verified framework member: {known:?}"
        );
        assert!(
            known.1.contains(&format!("{provider}.{member}")),
            "{class_name}.{member} must retain exact provider provenance: {known:?}"
        );
        let missing_status = coverage_status(
            &conn,
            &source_path,
            &format!("self._forge_missing_{class_name}"),
        );
        assert_eq!(
            missing_status, "unresolved",
            "unknown member on {class_name} must fail closed"
        );
    }
    let injection_edges: Vec<(String, String, String)> = conn
        .prepare("SELECT owner.path,target_path.path,target.qualname FROM edge_occurrences edge JOIN path_dictionary owner ON owner.path_id=edge.owner_id JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE edge.kind='injects' ORDER BY owner.path")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect();
    let expected_injections = [
        ("app.py", "app.py", "app.dependency"),
        ("fast.py", "fast.py", "fast.dependency"),
    ];
    let actual_injections: Vec<(&str, &str, &str)> = injection_edges
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
        .collect();
    assert_eq!(
        actual_injections, expected_injections,
        "each declarative DI call must persist its exact provider path and qualname"
    );
    let route_targets: Vec<(String, String, String, String)> = conn
        .prepare("SELECT DISTINCT route.name,route_path.path,target_path.path,target.qualname FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary route_path ON route_path.path_id=route.path_id JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE edge.kind='handles' AND route.kind='route' ORDER BY route.name,route_path.path,target_path.path,target.qualname")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap().map(Result::unwrap).collect();
    let expected_route_targets = [
        (
            "ANY /qualified",
            "qualified_urls.py",
            "qualified_urls.py",
            "qualified_urls.endpoint",
        ),
        (
            "ANY ^regex/$",
            "qualified_urls.py",
            "qualified_urls.py",
            "qualified_urls.endpoint",
        ),
        (
            "ANY catalog/deep/",
            "nested/urls.py",
            "catalog/views.py",
            "catalog.views.endpoint",
        ),
        (
            "ANY deep/",
            "catalog/endpoints.py",
            "catalog/views.py",
            "catalog.views.endpoint",
        ),
        ("ANY detail/", "shop/urls.py", "app.py", "app.endpoint"),
        (
            "ANY nested/catalog/deep/",
            "urls.py",
            "catalog/views.py",
            "catalog.views.endpoint",
        ),
        ("ANY parent-decoy/", "urls.py", "urls.py", "urls.endpoint"),
        ("ANY shop/detail/", "urls.py", "app.py", "app.endpoint"),
        ("GET /api/items", "fast.py", "fast.py", "fast.items"),
        ("GET /builtin-valid", "fast.py", "fast.py", "fast.items"),
        ("GET /middleware", "app.py", "app.py", "app.endpoint"),
        ("GET /middleware", "app.py", "app.py", "app.middleware"),
        ("GET /middleware", "fast.py", "fast.py", "fast.items"),
        ("GET /middleware", "fast.py", "fast.py", "fast.middleware"),
        (
            "GET /nest/items",
            "controller.ts",
            "controller.ts",
            "controller.ControllerClass.items",
        ),
        ("GET /offset-valid", "app.py", "app.py", "app.endpoint"),
        ("GET /registered", "app.py", "app.py", "app.endpoint"),
        ("POST /custom", "app.py", "app.py", "app.endpoint"),
    ];
    let actual_route_targets: Vec<(&str, &str, &str, &str)> = route_targets
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str()))
        .collect();
    assert_eq!(
        actual_route_targets, expected_route_targets,
        "route handlers must persist exact source and target identities"
    );
    assert_eq!(
        query_count(&conn, "SELECT count(*) FROM nodes WHERE kind='route' AND name IN ('GET /missing-handler','GET /keyword-handler','GET /offset-missing','GET /builtin-missing','GET /invalid-literal-handler')", []),
        0,
        "configured handler positions missing positional arguments must suppress route facts"
    );
    let middleware_handles = query_rows(
        &conn,
        "SELECT target.qualname FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash JOIN nodes target ON target.node_hash=edge.dst_hash WHERE edge.kind='handles' AND route.name='GET /middleware' ORDER BY target.qualname",
    );
    assert_eq!(
        middleware_handles,
        [
            "app.endpoint",
            "app.middleware",
            "fast.items",
            "fast.middleware"
        ]
    );
    assert_eq!(
        query_count(&conn, "SELECT count(*) FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash WHERE route.kind='route' AND route.name='GET /no-handler' AND edge.kind='handles'", []),
        0,
        "optional handler rules emit route facts without handles edges"
    );
    let (details, owner_path): (String, String) = conn.query_row("SELECT route.details,owner.path FROM nodes route JOIN path_dictionary owner ON owner.path_id=route.owner_path_id WHERE route.kind='route' AND route.name='ANY shop/detail/'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    let details: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(owner_path, "urls.py");
    assert_eq!(details["route_name"], "detail");
    assert_eq!(details["route_namespace"], "shop");
    assert_eq!(details["source_path"], "shop/urls.py");
    assert!(details["included_from"].is_string());
    assert_eq!(
        details["source_qualname"],
        "shop.urls.urlpatterns.ANY detail/"
    );

    let nested_details: (String, String, String) = conn.query_row("SELECT route.details,owner.path,target.qualname FROM nodes route JOIN path_dictionary owner ON owner.path_id=route.owner_path_id JOIN edge_occurrences edge ON edge.src_hash=route.node_hash AND edge.kind='handles' JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE route.kind='route' AND route.name='ANY nested/catalog/deep/' AND target_path.path='catalog/views.py'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    let nested_route_details: serde_json::Value = serde_json::from_str(&nested_details.0).unwrap();
    assert_eq!(nested_route_details["source_path"], "catalog/endpoints.py");
    assert_eq!(
        nested_route_details["source_qualname"],
        "catalog.endpoints.urlpatterns.ANY deep/"
    );
    assert_eq!(nested_route_details["route_namespace"], "nested:catalog");
    assert_eq!(nested_details.1, "urls.py");
    assert_eq!(nested_details.2, "catalog.views.endpoint");

    let origin_coverage: Vec<(String, i64, String, String)> = conn.prepare("SELECT path.path,coverage.line,expression.expression,coverage.status FROM resolution_coverage coverage JOIN path_dictionary path ON path.path_id=coverage.path_id JOIN coverage_expressions expression ON expression.expression_id=coverage.expression_id WHERE path.path IN ('shop/urls.py','catalog/endpoints.py') AND coverage.line=3 AND expression.expression='endpoint' ORDER BY path.path").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap().map(Result::unwrap).collect();
    let actual_origins: Vec<(&str, i64, &str, &str)> = origin_coverage
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), *b, c.as_str(), d.as_str()))
        .collect();
    assert_eq!(
        actual_origins,
        [
            ("catalog/endpoints.py", 3, "endpoint", "resolved"),
            ("shop/urls.py", 3, "endpoint", "resolved")
        ],
        "route handle coverage must use the deepest physical import context and line"
    );
    workspace.assert_cold_equivalent();

    drop(conn);
    workspace.write(
        "shop/urls.py",
        "from django.urls import path\nfrom app import endpoint\nurlpatterns = [path('updated/', endpoint, name='detail_v2')]\n",
    );
    workspace.cold_parity("shop/urls.py");
    let conn = workspace.open();
    let (details, owner_path): (String, String) = conn.query_row("SELECT route.details,owner.path FROM nodes route JOIN path_dictionary owner ON owner.path_id=route.owner_path_id WHERE route.kind='route' AND route.name='ANY shop/updated/'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    let details: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(
        owner_path, "urls.py",
        "delta must rehydrate the include owner"
    );
    assert_eq!(details["route_name"], "detail_v2");
    assert_eq!(details["route_namespace"], "shop");
    assert_eq!(details["source_path"], "shop/urls.py");
    let updated_handle: (String, String, String) = conn.query_row("SELECT owner.path,target_path.path,target.qualname FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash JOIN path_dictionary owner ON owner.path_id=route.owner_path_id JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE edge.kind='handles' AND route.kind='route' AND route.name='ANY shop/updated/'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(
        updated_handle,
        (
            "urls.py".to_owned(),
            "app.py".to_owned(),
            "app.endpoint".to_owned()
        ),
        "child delta must preserve the imported handler despite a parent decoy"
    );
    drop(conn);

    workspace.write("urls.py", "from django.urls import path, include\ndef endpoint(): return 'parent decoy'\nurlpatterns = [path('market/', include('shop.urls', namespace='market')), path('nested/', include('nested.urls', namespace='nested')), path('parent-decoy/', endpoint)]\n");
    workspace.cold_parity("urls.py");
    let conn = workspace.open();
    let (details, parent_only_handle): (String, (String, String, String)) = conn.query_row("SELECT route.details,owner.path,target_path.path,target.qualname FROM nodes route JOIN path_dictionary owner ON owner.path_id=route.owner_path_id JOIN edge_occurrences edge ON edge.src_hash=route.node_hash AND edge.kind='handles' JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE route.kind='route' AND route.name='ANY market/updated/'", [], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?)))).unwrap();
    let details: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(
        parent_only_handle,
        (
            "urls.py".to_owned(),
            "app.py".to_owned(),
            "app.endpoint".to_owned()
        )
    );
    assert_eq!(details["route_name"], "detail_v2");
    assert_eq!(details["route_namespace"], "market");
    assert_eq!(details["source_path"], "shop/urls.py");
    assert_eq!(
        details["source_qualname"],
        "shop.urls.urlpatterns.ANY updated/"
    );
    let parent_decoy: (String, String) = conn.query_row("SELECT owner.path,target.qualname FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash JOIN path_dictionary owner ON owner.path_id=route.owner_path_id JOIN nodes target ON target.node_hash=edge.dst_hash WHERE edge.kind='handles' AND route.kind='route' AND route.name='ANY parent-decoy/'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(
        parent_decoy,
        ("urls.py".to_owned(), "urls.endpoint".to_owned())
    );
    let market_origin_coverage: (i64, String) = conn.query_row("SELECT coverage.line,coverage.status FROM resolution_coverage coverage JOIN path_dictionary path ON path.path_id=coverage.path_id JOIN coverage_expressions expression ON expression.expression_id=coverage.expression_id WHERE path.path='shop/urls.py' AND expression.expression='endpoint' AND coverage.line=3", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(market_origin_coverage, (3, "resolved".to_owned()));
    assert_eq!(
        query_count(
            &conn,
            "SELECT count(*) FROM nodes WHERE kind='route' AND name LIKE 'ANY shop/%'",
            []
        ),
        0,
        "parent-only prefix update must remove old route refs"
    );
    drop(conn);

    fs::remove_file(workspace.join("shop/urls.py")).unwrap();
    workspace.cold_parity("shop/urls.py");
    let conn = workspace.open();
    assert_eq!(
        query_count(&conn, "SELECT count(*) FROM nodes WHERE kind='route' AND name IN ('ANY shop/detail/','ANY shop/updated/','ANY market/updated/')", []),
        0,
        "deleting an included Django module removes parent route facts"
    );
    assert_eq!(
        query_count(&conn, "SELECT count(*) FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash WHERE edge.kind='handles' AND route.kind='route' AND route.name LIKE 'ANY market/%'", []),
        0,
        "deleted route must leave no handle edge"
    );
    assert_eq!(
        query_count(&conn, "SELECT count(*) FROM resolution_coverage coverage JOIN path_dictionary path ON path.path_id=coverage.path_id WHERE path.path='shop/urls.py'", []),
        0,
        "deleted source must leave no handle coverage"
    );
    drop(conn);
    workspace.assert_cold_equivalent();

    workspace.write(
        ".forge/frameworks/django.toml",
        r#"
[[route_decorators]]
callee = "portal.expose"
method = "PATCH"
path_argument = 0
"#,
    );
    workspace.delta("app.py");
    let conn = workspace.open();
    let routes = query_rows(
        &conn,
        "SELECT name FROM nodes WHERE kind='route' ORDER BY name",
    );
    assert!(
        routes.iter().any(|name| name == "PATCH /custom"),
        "{routes:?}"
    );
    assert!(
        !routes.iter().any(|name| name == "POST /custom"),
        "{routes:?}"
    );

    let cycle_workspace = Workspace::new();
    cycle_workspace.write(
        "pyproject.toml",
        "[project]\nname = 'include-cycle'\ndependencies = ['django']\n",
    );
    cycle_workspace.write("a/__init__.py", "");
    cycle_workspace.write("b/__init__.py", "");
    cycle_workspace.write(
        "a/urls.py",
        "from django.urls import include, path\nfrom a.views import a_endpoint\nurlpatterns = [path('to-b/', include('b.urls')), path('a/', a_endpoint)]\n",
    );
    cycle_workspace.write(
        "b/urls.py",
        "from django.urls import include, path\nfrom b.views import b_endpoint\nurlpatterns = [path('to-a/', include('a.urls')), path('b/', b_endpoint)]\n",
    );
    cycle_workspace.write("a/views.py", "def a_endpoint(request): return None\n");
    cycle_workspace.write("b/views.py", "def b_endpoint(request): return None\n");
    let cycle_conn = cycle_workspace.build();
    let cycle_owners = query_rows(&cycle_conn, "SELECT path.path FROM nodes module JOIN path_dictionary path ON path.path_id=module.path_id WHERE module.kind='module' AND path.path IN ('a/urls.py','b/urls.py') ORDER BY path.path");
    assert_eq!(cycle_owners, ["a/urls.py", "b/urls.py"]);
    let cycle_routes: Vec<(String, String)> = cycle_conn.prepare("SELECT path.path,route.name FROM nodes route JOIN path_dictionary path ON path.path_id=route.path_id WHERE route.kind='route' AND path.path IN ('a/urls.py','b/urls.py') ORDER BY path.path,route.name").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    let actual_cycle_routes: Vec<(&str, &str)> = cycle_routes
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        actual_cycle_routes,
        [
            ("a/urls.py", "ANY a/"),
            ("a/urls.py", "ANY to-b/"),
            ("a/urls.py", "ANY to-b/b/"),
            ("a/urls.py", "ANY to-b/to-a/a/"),
            ("b/urls.py", "ANY b/"),
            ("b/urls.py", "ANY to-a/"),
            ("b/urls.py", "ANY to-a/a/"),
            ("b/urls.py", "ANY to-a/to-b/b/"),
        ],
        "A↔B includes must terminate with the full finite terminal-route set"
    );
    let cycle_handles: Vec<(String, String, String, String)> = cycle_conn.prepare("SELECT owner.path,route.name,target_path.path,target.qualname FROM edge_occurrences edge JOIN nodes route ON route.node_hash=edge.src_hash JOIN path_dictionary owner ON owner.path_id=route.owner_path_id JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id WHERE edge.kind='handles' AND route.kind='route' AND owner.path IN ('a/urls.py','b/urls.py') ORDER BY owner.path,route.name").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap().map(Result::unwrap).collect();
    let actual_cycle_handles: Vec<(&str, &str, &str, &str)> = cycle_handles
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str()))
        .collect();
    assert_eq!(
        actual_cycle_handles,
        [
            ("a/urls.py", "ANY a/", "a/views.py", "a.views.a_endpoint"),
            (
                "a/urls.py",
                "ANY to-b/b/",
                "b/views.py",
                "b.views.b_endpoint"
            ),
            (
                "a/urls.py",
                "ANY to-b/to-a/a/",
                "a/views.py",
                "a.views.a_endpoint"
            ),
            ("b/urls.py", "ANY b/", "b/views.py", "b.views.b_endpoint"),
            (
                "b/urls.py",
                "ANY to-a/a/",
                "a/views.py",
                "a.views.a_endpoint"
            ),
            (
                "b/urls.py",
                "ANY to-a/to-b/b/",
                "b/views.py",
                "b.views.b_endpoint"
            ),
        ],
        "cycle composition must keep terminal handlers at their real source"
    );
    cycle_workspace.assert_cold_equivalent();
}
