use super::*;
#[test]
fn oversized_python_flow_keeps_type_only_imports_fail_closed_after_persistence() {
    use forge_core::typed_facts::{NodeView, TypedFacts};

    let model = "class Client:\n    pass\n";
    let mut small = String::from(
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\n",
    );
    small.push_str("x = 0\n");
    small.push_str("Client()\n");
    let small_line = small.lines().count() as i64;
    let workspace = fixture_workspace(&[]);
    workspace.write("src/pkg/model.py", model);
    workspace.write("src/consumer.py", &small);
    let check = |conn: &rusqlite::Connection| {
        assert_eq!(
            persisted_status(conn, "src/consumer.py", small_line, "Client"),
            "unresolved"
        );
        let blob: Vec<u8> = query_scalar(
            conn,
            "SELECT facts_blob FROM local_facts WHERE path='src/consumer.py'",
            [],
        );
        let facts = TypedFacts::from_public(
            contextunity_forge_mcp::db::reader::decode_local_facts(&blob).unwrap(),
        );
        let module = facts
            .nodes
            .iter()
            .find(|node| node.kind == "module")
            .unwrap();
        let module = serde_json::to_value(NodeView::new(module, &facts.flows, None)).unwrap();
        let flow = &module["details"]["value_flow"];
        assert_ne!(flow["fact_limit_exceeded"], true);
        assert_eq!(flow["type_only_imports"][0]["alias"], "Client");
    };
    workspace.build();
    let cold = workspace.open();
    check(&cold);
    drop(cold);
    workspace.write(
        "src/pkg/model.py",
        "class Client:\n    pass\nclass Added:\n    pass\n",
    );
    let delta_report = workspace.delta(&["src/pkg/model.py"]);
    assert_eq!(delta_report["reparsed_files"].as_u64(), Some(1));
    assert_eq!(delta_report["loaded_fact_files"].as_u64(), Some(1));
    let delta = workspace.open();
    check(&delta);

    let mut oversized = String::from(
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\n",
    );
    for _ in 0..65_536 {
        oversized.push_str("x = 0\n");
    }
    let call_line = oversized.lines().count() as i64 + 1;
    oversized.push_str("Client()\n");
    let sources = [
        ("src/pkg/model.py", model),
        ("src/consumer.py", oversized.as_str()),
    ];
    let facts = sources
        .iter()
        .map(|(path, source)| {
            let parsed = contextunity_forge_mcp::engine::ast::extract(path, "python", source)
                .unwrap_or_else(|error| panic!("{path} (python): {error}"));
            ((*path).to_owned(), parsed)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let graph = contextunity_forge_mcp::engine::linker::link(&facts);
    let catalog = crate::languages::harness::LinkedCatalog { facts, graph };
    let facts = TypedFacts::from_public(catalog.facts["src/consumer.py"].clone());
    let module = facts
        .nodes
        .iter()
        .find(|node| node.kind == "module")
        .unwrap();
    let module = serde_json::to_value(NodeView::new(module, &facts.flows, None)).unwrap();
    let flow = &module["details"]["value_flow"];
    assert_eq!(flow["fact_limit_exceeded"], true);
    assert!(flow.get("type_only_imports").is_none());
    assert!(flow.get("annotation_references").is_none());
    let client = catalog
        .graph
        .coverage
        .iter()
        .find(|row| {
            row.path == "src/consumer.py"
                && row.expression == "Client"
                && row.line == call_line as usize
        })
        .unwrap_or_else(|| panic!("missing Client coverage at {call_line}"));
    assert_eq!(client.status, "unresolved", "{client:#?}");
}

#[test]
fn type_checking_imports_resolve_annotations_without_runtime_calls() {
    let (primary, second) = linked_python_roots();
    for workspace in [&primary, &second] {
        workspace.write(
            "src/pkg/model.py",
            "class Client:\n    @classmethod\n    def make(cls):\n        return cls()\n",
        );
        workspace.write(
            "src/consumer.py",
            "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client = Client.make()) -> None:\n    Client()\n    Client.make()\n    value.make()\ndef alias_use() -> None:\n    alias = Client\n    alias.make()\n",
        );
        workspace.write(
            "src/unproven.py",
            "if TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n",
        );
        workspace.write("src/nested.py", "from typing import TYPE_CHECKING\ndef use() -> None:\n    if TYPE_CHECKING:\n        from pkg.model import Client\n    Client()\n");
        workspace
            .write("src/rebound_guard.py", "from typing import TYPE_CHECKING\ndel TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        workspace.write(
            "src/runtime.py",
            "from pkg.model import Client\nClient()\nClient.make()\n",
        );
        workspace.write("src/aliased_import.py", "import typing as aliases\nif aliases.TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        workspace.write("src/spoofed_typing.py", "import os as typing\nif typing.TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        workspace.write("src/renamed_guard.py", "from typing import TYPE_CHECKING as TC\nif TC:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
        let rebound_guard_source = "from typing import TYPE_CHECKING as TC\nTC = False\nif TC:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n";
        workspace.write("src/renamed_rebound_guard.py", rebound_guard_source);
        let extracted = ExtractorHarness::for_lang("python")
            .at_path("src/renamed_rebound_guard.py")
            .extract(rebound_guard_source);
        let rebound_import = extracted
            .references
            .iter()
            .find(|reference| reference.module.as_deref() == Some("pkg.model"))
            .expect("guarded import remains indexed after alias rebinding");
        assert!(!matches!(
            rebound_import.receiver_hint.as_ref(),
            Some(ReceiverHint::TypeOnlyImport)
        ));
        assert_eq!(rebound_import.alias.as_deref(), Some("Client"));
    }
    primary.build();
    let conn = primary.open();
    for (path, provider) in [
        ("src/consumer.py", "src/pkg/model.py:Client"),
        (
            "[second]/src/consumer.py",
            "[second]/src/pkg/model.py:Client",
        ),
    ] {
        assert_eq!(persisted_status(&conn, path, 4, "Client"), "resolved");
        assert_eq!(
            persisted_status(&conn, path, 4, "Client.make"),
            "unresolved"
        );
        assert_eq!(persisted_status(&conn, path, 5, "Client"), "unresolved");
        assert_eq!(
            persisted_status(&conn, path, 6, "Client.make"),
            "unresolved"
        );
        assert_eq!(persisted_status(&conn, path, 7, "value.make"), "resolved");
        assert_eq!(
            persisted_status(&conn, path, 10, "alias.make"),
            "unresolved"
        );
        let targets: Vec<String> = query_strings(
            &conn,
            "SELECT (SELECT path FROM path_dictionary WHERE path_id=dst.path_id) || ':' || dst.name FROM edge_occurrences e JOIN path_dictionary p ON p.path_id=e.owner_id JOIN nodes dst ON dst.node_hash=e.dst_hash WHERE p.path=?1 AND e.line=4 AND e.kind='references' ORDER BY (SELECT path FROM path_dictionary WHERE path_id=dst.path_id),dst.name",
            [path],
        );
        assert_eq!(targets, vec![provider]);
    }
    for prefix in ["", "[second]/"] {
        let unproven = format!("{prefix}src/unproven.py");
        assert_eq!(
            persisted_status(&conn, &unproven, 3, "Client"),
            "unresolved"
        );
        assert_eq!(
            persisted_status(&conn, &unproven, 4, "Client"),
            "unresolved"
        );
        let nested = format!("{prefix}src/nested.py");
        assert_eq!(
            persisted_status_rows(&conn, &nested, 5, "Client"),
            ["unresolved"]
        );
        let rebound = format!("{prefix}src/rebound_guard.py");
        assert_eq!(
            persisted_status_rows(&conn, &rebound, 5, "Client"),
            ["unresolved"]
        );
        assert_eq!(
            persisted_status_rows(&conn, &rebound, 6, "Client"),
            ["unresolved"]
        );
        let runtime = format!("{prefix}src/runtime.py");
        assert_eq!(persisted_status(&conn, &runtime, 2, "Client"), "resolved");
        assert_eq!(
            persisted_status(&conn, &runtime, 3, "Client.make"),
            "resolved"
        );
        let aliased = format!("{prefix}src/aliased_import.py");
        assert_eq!(persisted_status(&conn, &aliased, 4, "Client"), "resolved");
        assert_eq!(persisted_status(&conn, &aliased, 5, "Client"), "unresolved");
        let spoofed = format!("{prefix}src/spoofed_typing.py");
        assert_eq!(persisted_status(&conn, &spoofed, 4, "Client"), "unresolved");
        assert_eq!(persisted_status(&conn, &spoofed, 5, "Client"), "unresolved");
        let renamed = format!("{prefix}src/renamed_guard.py");
        assert_eq!(persisted_status(&conn, &renamed, 4, "Client"), "resolved");
        assert_eq!(persisted_status(&conn, &renamed, 5, "Client"), "unresolved");
    }
    let (shadow_primary, shadow_second) = linked_python_roots();
    for workspace in [&shadow_primary, &shadow_second] {
        workspace.write("src/pkg/model.py", "class Client:\n    pass\n");
        workspace.write("src/typing.py", "TYPE_CHECKING = bool(input())\n");
        workspace.write("src/shadow_consumer.py", "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n");
    }
    shadow_primary.build();
    let shadow_conn = shadow_primary.open();
    for prefix in ["", "[second]/"] {
        let path = format!("{prefix}src/shadow_consumer.py");
        assert_eq!(
            persisted_status_rows(&shadow_conn, &path, 4, "Client"),
            ["unresolved"]
        );
        assert_eq!(
            persisted_status_rows(&shadow_conn, &path, 5, "Client"),
            ["unresolved"]
        );
    }
    for (declared, annotation_status) in [(false, "unresolved"), (true, "resolved")] {
        let (extension_primary, extension_second) = linked_python_roots();
        for workspace in [&extension_primary, &extension_second] {
            workspace.write("src/pkg/model.py", "class Client:\n    pass\n");
            if declared {
                workspace.write(
                    "pyproject.toml",
                    "[project]\ndependencies = [\"typing-extensions>=4\"]\n",
                );
            }
            workspace.write(
                "src/extension_consumer.py",
                "from typing_extensions import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg.model import Client\ndef use(value: Client) -> None:\n    Client()\n",
            );
        }
        extension_primary.build();
        let extension_conn = extension_primary.open();
        for prefix in ["", "[second]/"] {
            let path = format!("{prefix}src/extension_consumer.py");
            assert_eq!(
                persisted_status_rows(&extension_conn, &path, 4, "Client"),
                [annotation_status]
            );
            assert_eq!(
                persisted_status_rows(&extension_conn, &path, 5, "Client"),
                ["unresolved"]
            );
        }
    }
}
