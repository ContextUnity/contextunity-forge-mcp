use super::*;

#[test]
fn persisted_imported_factory_return_reaches_only_its_own_project_provider() {
    let provider = r#"class Base:
    def base(self): pass
class Service(Base):
    def execute(self): pass
    def replay(self):
        super().base()
def make_service() -> Service:
    return Service()
"#;
    let consumer = r#"from provider import make_service
service = make_service()
changed = make_service()
changed = unknown_factory()
mystery = unknown_factory()
if condition:
    conditional = make_service()
class Runner:
    def run(self):
        service.execute()
        make_service().execute()
        service.absent()
        changed.execute()
        mystery.execute()
        conditional.execute()
    def shadow(self, service):
        service.execute()
"#;
    let (_workspace, db) = build_workspace(&[("provider.py", provider), ("consumer.py", consumer)]);
    assert!(persisted_edge(
        &db,
        "consumer.py",
        "imports",
        "make_service",
        "provider.py",
        "provider.make_service"
    ));
    let service = persisted_coverage(&db, "consumer.py", "service.execute");
    assert_eq!(service.len(), 2, "{service:?}");
    assert_eq!(service[0].0, "resolved", "{service:?}");
    assert_eq!(service[1].0, "unresolved", "{service:?}");
    assert!(persisted_edge(
        &db,
        "consumer.py",
        "calls",
        "service.execute",
        "provider.py",
        "provider.Service.execute"
    ));
    let computed = persisted_coverage(&db, "consumer.py", "make_service().execute");
    assert_eq!(computed.len(), 1, "{computed:?}");
    assert_eq!(computed[0].0, "resolved", "{computed:?}");
    assert!(persisted_edge(
        &db,
        "consumer.py",
        "calls",
        "make_service().execute",
        "provider.py",
        "provider.Service.execute"
    ));
    let inherited = persisted_coverage(&db, "provider.py", "super().base");
    assert_eq!(inherited.len(), 1, "{inherited:?}");
    assert_eq!(inherited[0].0, "resolved", "{inherited:?}");
    assert!(persisted_edge(
        &db,
        "provider.py",
        "calls",
        "super().base",
        "provider.py",
        "provider.Base.base"
    ));
    for expression in [
        "service.absent",
        "changed.execute",
        "mystery.execute",
        "conditional.execute",
    ] {
        let coverage = persisted_coverage(&db, "consumer.py", expression);
        assert_eq!(coverage.len(), 1, "{expression}: {coverage:?}");
        assert_eq!(coverage[0].0, "unresolved", "{expression}: {coverage:?}");
    }
}
#[test]
fn annotated_python_receivers_follow_declared_inheritance_and_protocol_members() {
    let source = "from django.http import HttpRequest\nfrom typing import Protocol\nclass Request(HttpRequest): pass\nclass Authenticated(Request): pass\nclass Connection(Protocol):\n    def execute(self, sql): ...\ndef use(request: Authenticated, conn: Connection):\n    request.GET.get('key')\n    conn.execute('select 1')\n    request.GET.missing('key')\n";
    let facts = ast::extract("typed_members.py", "python", source).unwrap();
    let execute_node = facts
        .nodes
        .iter()
        .find(|node| node.qualname.ends_with("Connection.execute"))
        .unwrap();
    let execute = execute_node.id.clone();
    let execute_qualname = execute_node.qualname.clone();
    let graph = linker::link(&BTreeMap::from([("typed_members.py".to_owned(), facts)]));
    for (line, expression, expected) in [
        (8, "request.GET.get", "external"),
        (9, "conn.execute", "resolved"),
        (10, "request.GET.missing", "unresolved"),
    ] {
        let coverage = graph
            .coverage
            .iter()
            .find(|coverage| coverage.line == line && coverage.expression == expression)
            .unwrap();
        assert_eq!(
            coverage.status, expected,
            "{expression} at {line}: {}",
            coverage.evidence
        );
        if expression == "request.GET.get" {
            assert!(
                coverage.evidence.contains("django.http.HttpRequest"),
                "{}",
                coverage.evidence
            );
        }
    }
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"
        && edge.path == "typed_members.py"
        && edge.line == 9
        && edge.evidence == "conn.execute"
        && edge.dst == execute));
    let (_workspace, db) = build_workspace(&[("typed_members.py", source)]);
    for (expression, expected) in [
        ("request.GET.get", "external"),
        ("conn.execute", "resolved"),
        ("request.GET.missing", "unresolved"),
    ] {
        let coverage = persisted_coverage(&db, "typed_members.py", expression);
        assert_eq!(coverage.len(), 1, "{expression}: {coverage:?}");
        assert_eq!(coverage[0].0, expected, "{expression}: {coverage:?}");
        if expression == "request.GET.get" {
            assert!(
                coverage[0].1.contains("django.http.HttpRequest"),
                "{coverage:?}"
            );
        }
    }
    assert!(persisted_edge(
        &db,
        "typed_members.py",
        "calls",
        "conn.execute",
        "typed_members.py",
        &execute_qualname
    ));
}
