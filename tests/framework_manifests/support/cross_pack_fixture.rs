use super::*;

pub(in super::super) const GRIDVIEW_RECEIVER_CASES: &[(&str, &str, &str)] = &[
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

pub(in super::super) fn django_include_sources() -> VirtualWorkspace {
    let mut sources = BTreeMap::new();
    macro_rules! put {
        ($path:expr, $source:expr $(,)?) => {{
            sources.insert(($path).to_string(), ($source).to_string());
        }};
    }
    put!(
        "pyproject.toml",
        "[project]\nname = 'django-sample'\ndependencies = ['django']\n",
    );
    put!(
        ".forge/frameworks/django.toml",
        r#"
[[route_decorators]]
callee = "portal.expose"
method = "POST"
path_argument = 0
[[route_calls]]
callee = "path"
method = "ANY"
path_argument = 0
handler_argument = 1
allow_relative_path = true
name_keyword = "name"
include = { callee = ".include", argument = 0, namespace_keyword = "namespace" }
"#,
    );
    put!(
        "app.py",
        "@portal.expose('/custom')\ndef endpoint(): return None\n",
    );
    put!(
        "urls.py",
        "from django.urls import path, include\ndef endpoint(): return 'parent decoy'\nurlpatterns = [path('shop/', include('shop.urls', namespace='shop')), path('nested/', include('nested.urls', namespace='nested')), path('parent-decoy/', endpoint)]\n",
    );
    put!("shop/urls.py", "from django.urls import path\nfrom app import endpoint\nurlpatterns = [path('detail/', endpoint, name='detail')]\n");
    put!(
        "nested/urls.py",
        "from django.urls import path, include\nurlpatterns = [path('catalog/', include('catalog.endpoints', namespace='catalog'))]\n",
    );
    put!(
        "catalog/endpoints.py",
        "from django.urls import path\nfrom .views import endpoint\nurlpatterns = [path('deep/', endpoint, name='deep')]\n",
    );
    put!(
        "catalog/views.py",
        "def endpoint(): return 'catalog endpoint'\n",
    );
    VirtualWorkspace::from_map(sources)
}

pub(in super::super) fn cross_pack_sources() -> VirtualWorkspace {
    let mut sources = django_include_sources().files;
    macro_rules! put {
        ($path:expr, $source:expr $(,)?) => {{
            sources.insert(($path).to_string(), ($source).to_string());
        }};
    }
    put!(
        "pyproject.toml",
        "[project]\nname = 'sample'\ndependencies = ['django', 'fastapi', 'grid-view-spec']\n",
    );
    put!("package.json", r#"{"dependencies":{"@nestjs/common":"*"}}"#);
    put!(
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
    put!(
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
    put!(
        "grid_spec.py",
        "from grid_view_spec.types.spec import GridViewSpec\nfrom grid_view_spec.types.block_base import GridViewBlockBase\nclass CustomSpec(GridViewSpec):\n    def config_value(self): return self.config\nclass CustomBlock(GridViewBlockBase):\n    def block_type(self): return self.type\n",
    );

    for &(class_name, provider, member) in GRIDVIEW_RECEIVER_CASES {
        let (module, symbol) = provider.rsplit_once('.').unwrap();
        let import_module = match class_name {
            "DjangoGridViewHost" | "GridViewActionBase" => module,
            _ => "grid_view_spec.types",
        };
        let missing_member = format!("_forge_missing_{class_name}");
        put!(
            format!("grid_receivers/{class_name}.py"),
            &format!("from {import_module} import {symbol} as ExternalBase\nclass Probe{class_name}(ExternalBase):\n    def known_probe(self): return self.{member}\n    def unknown_probe(self): return self.{missing_member}\n"),
        );
    }
    put!(
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
    put!("controller.ts", "import { Controller, Get } from '@nestjs/common';\n@Controller('/nest')\nclass ControllerClass {\n@Get('/items')\nitems() { return 1; }\n}\n");
    put!(
        "qualified_urls.py",
        "import django.urls\nfrom django import urls\ndef endpoint(): return None\na = django.urls.path('/qualified', endpoint)\nb = urls.re_path('^regex/$', endpoint)\n",
    );
    VirtualWorkspace::from_map(sources)
}
