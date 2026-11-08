use super::*;

fn first_workspace() -> PersistedWorkspaceHarness {
    let first = fixture_workspace(&[]);
    first.write("app/__init__.py", "");
    first.write(
        "app/models.py",
        "from django.db import models\nclass Product(models.Model):\n    pass\n",
    );
    first.write(
        "app/contracts.py",
        "from typing import Protocol\nclass Notifier(Protocol):\n    def notify(self) -> None: ...\n",
    );
    first.write(
        "consumer.py",
        concat!(
            "from django.http import HttpRequest\n",
            "from django.core.management.base import BaseCommand\n",
            "from pydantic import BaseModel\n",
            "from app.models import Product\n",
            "from app.contracts import Notifier\n",
            "class SecuredRequest(HttpRequest):\n",
            "    pass\n",
            "class Command(BaseCommand):\n",
            "    def handle(self) -> None:\n",
            "        self.stdout.write('ok')\n",
            "class Record(BaseModel):\n",
            "    title: str\n",
            "def run(request: SecuredRequest, record: Record, notifier: Notifier) -> None:\n",
            "    request.GET.get('q')\n",
            "    Product.objects.create()\n",
            "    Record.model_validate({'title': 'ok'})\n",
            "    record.model_dump()\n",
            "    notifier.notify()\n",
            "def choose(flag: bool) -> None:\n",
            "    if flag:\n",
            "        model = Product\n",
            "    else:\n",
            "        model = Notifier\n",
            "    model.objects.create()\n",
        ),
    );
    first.build();
    first
}

fn second_workspace() -> PersistedWorkspaceHarness {
    let second = fixture_workspace(&[]);
    second.write("app/__init__.py", "");
    second.write("app/models.py", "class Product:\n    pass\n");
    second.write(
        "app/contracts.py",
        "from typing import Protocol\nclass Notifier(Protocol):\n    def cancel(self) -> None: ...\n",
    );
    second.write(
        "consumer.py",
        concat!(
            "from app.models import Product\n",
            "from app.contracts import Notifier\n",
            "def run(notifier: Notifier, client) -> None:\n",
            "    Product.objects.create()\n",
            "    notifier.notify()\n",
            "    client.send()\n",
            "def test_injected(db_client) -> None:\n",
            "    db_client.query()\n",
        ),
    );
    second.build();
    second
}

#[test]
fn imported_framework_lineage_keeps_external_providers_and_exact_protocol_calls() {
    let first = first_workspace();
    let first_db = first.open();
    for (line, expression, origin) in [
        (10, "self.stdout.write", "django"),
        (14, "request.GET.get", "django"),
        (15, "Product.objects.create", "django"),
        (16, "Record.model_validate", "pydantic"),
        (17, "record.model_dump", "pydantic"),
    ] {
        let (status, evidence) = persisted_coverage(&first_db, "consumer.py", line, expression);
        assert_eq!(status, "external", "{expression}: {evidence}");
        assert!(evidence.contains(origin), "{expression}: {evidence}");
    }
    assert_eq!(
        persisted_coverage(&first_db, "consumer.py", 18, "notifier.notify").0,
        "resolved"
    );
    assert_eq!(
        persisted_coverage(&first_db, "consumer.py", 24, "model.objects.create").0,
        "unresolved",
        "conditional alias cannot select Product as a unique provider"
    );
    let local_edge: i64 = first_db.query_row(
    "SELECT count(*) FROM edge_occurrences e JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE (SELECT path FROM path_dictionary WHERE path_id=e.owner_id)='consumer.py' AND e.kind='calls' AND (SELECT path FROM path_dictionary WHERE path_id=dst.path_id)='app/contracts.py' AND dst.kind='method' AND dst.name='notify'",
    [], |row| row.get(0),
).unwrap();
    assert_eq!(local_edge, 1, "protocol call retains its exact local method");
}

#[test]
fn unproved_local_members_and_injected_names_remain_unresolved() {
    let second = second_workspace();
    let second_db = second.open();
    for (line, expression) in [
        (4, "Product.objects.create"),
        (5, "notifier.notify"),
        (6, "client.send"),
        (8, "db_client.query"),
    ] {
        assert_eq!(
            persisted_coverage(&second_db, "consumer.py", line, expression).0,
            "unresolved",
            "{expression}"
        );
    }
}

#[test]
fn linked_projects_preserve_framework_lineage_and_protocol_identity_isolation() {
    let first = first_workspace();
    let second = second_workspace();
    let linked = fixture_workspace(&[]);
    linked.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: first\n    path: '{}'\n    roots: [.]\n  - name: second\n    path: '{}'\n    roots: [.]\n",
            first.root().display(),
            second.root().display()
        ),
    );
    linked.build();
    let linked_db = linked.open();
    assert_eq!(
        persisted_coverage(&linked_db, "[first]/consumer.py", 14, "request.GET.get").0,
        "external"
    );
    assert_eq!(
        persisted_coverage(&linked_db, "[first]/consumer.py", 18, "notifier.notify").0,
        "resolved"
    );
    for (line, expression) in [(4, "Product.objects.create"), (5, "notifier.notify")] {
        assert_eq!(
            persisted_coverage(&linked_db, "[second]/consumer.py", line, expression).0,
            "unresolved",
            "project isolation for {expression}"
        );
    }
}
