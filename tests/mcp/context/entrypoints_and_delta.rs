use super::*;

const UNTESTED_FILES: &[(&str, &str)] = &[
    (
        "service.py",
        "def entry():\n    return target()\ndef target():\n    return 1\ndef unused(value: int) -> int:\n    return value\nclass Base:\n    pass\nclass Child(Base):\n    pass\nclass GrandChild(Child):\n    pass\n",
    ),
    (
        "tests/test_service.py",
        "from service import entry\ndef test_entry():\n    entry()\n",
    ),
];

const ENTRYPOINT_FILES: &[(&str, &str)] = &[
    (
        "src/main.rs",
        "fn main() { helper(); }\nfn helper() {}\nfn ordinary_unwired() {}\n#[test]\nfn test_root() {}\n",
    ),
    ("requirements.txt", "fastapi\nclick\ncelery\nmcp\ntyper\ndjango\n"),
    ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nclap='4'\n"),
    (
        "src/clap_tool.rs",
        "use clap::{Parser, Subcommand};\n#[derive(Parser)]\nstruct Cli { #[command(subcommand)] command: Commands }\n#[derive(Subcommand)]\nenum Commands { Run }\nfn run_job() {}\nfn dispatch() { let cli = Cli::parse(); match cli.command { Commands::Run => run_job() } }\n",
    ),
    (
        "src/unrelated.rs",
        "enum Local { Run }\nfn unrelated_job() {}\nfn dispatch(value: Local) { match value { Local::Run => unrelated_job() } }\n",
    ),
    (
        "src/clap_rebound.rs",
        "use clap::{Parser, Subcommand};\n#[derive(Parser)]\nstruct Cli { #[command(subcommand)] command: Commands }\n#[derive(Subcommand)]\nenum Commands { Run }\nfn rebound_job() {}\nfn dispatch(other: Cli) { let mut cli = Cli::parse(); cli = other; match cli.command { Commands::Run => rebound_job() } }\n",
    ),
    (
        "src/clap_shadow.rs",
        "use fake::{Parser, Subcommand};\n#[derive(Parser)]\nstruct Cli { command: Commands }\n#[derive(Subcommand)]\nenum Commands { Run }\nfn fake_job() {}\nfn dispatch() { let cli = Cli::parse(); match cli.command { Commands::Run => fake_job() } }\n",
    ),
    (
        "src/clap_local.rs",
        "mod clap {}\nuse clap::{Parser, Subcommand};\n#[derive(Parser)]\nstruct Cli { #[command(subcommand)] command: Commands }\n#[derive(Subcommand)]\nenum Commands { Run }\nfn local_job() {}\nfn dispatch() { let cli = Cli::parse(); match cli.command { Commands::Run => local_job() } }\n",
    ),
    (
        "src/clap_override.rs",
        "use clap::{Parser, Subcommand};\n#[derive(Parser)]\nstruct Cli { #[command(subcommand)] command: Commands }\n#[derive(Subcommand)]\nenum Commands { Run }\nimpl Cli { fn parse() -> Self { todo!() } }\nfn override_job() {}\nfn dispatch() { let cli = Cli::parse(); match cli.command { Commands::Run => override_job() } }\n",
    ),
    (
        "pyproject.toml",
        "[project]\nname = 'entry-fixture'\nversion = '0.1.0'\n[project.scripts]\nfixture = 'package:script_main'\n",
    ),
    (
        "package.py",
        "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/other')\ndef other_route(): pass\ndef script_main(): pass\ndef guarded_main(): pass\ndef not_guarded(): pass\nif __name__ == '__main__':\n    guarded_main()\nif __name__ != '__main__':\n    not_guarded()\n",
    ),
    (
        "command.py",
        "from django.core.management.base import BaseCommand\nclass Command(BaseCommand):\n    def handle(self, *args, **options): pass\n",
    ),
    (
        "alias_command.py",
        "from django.core.management.base import BaseCommand as Parent\nclass Alias(Parent):\n    def handle(self): pass\n",
    ),
    (
        "shadow_command.py",
        "from django.core.management.base import BaseCommand\nBaseCommand = object\nclass Shadow(BaseCommand):\n    def handle(self): pass\n",
    ),
    (
        "wrong_command.py",
        "from unrelated import BaseCommand\nclass Wrong(BaseCommand):\n    def handle(self): pass\n",
    ),
    (
        "rebound_command.py",
        "from django.core.management.base import BaseCommand\ndef replacement(self): pass\nclass Rebound(BaseCommand):\n    def handle(self): pass\n    handle = replacement\n",
    ),
    (
        "inherited_command.py",
        "from django.core.management.base import BaseCommand\nclass Parent(BaseCommand): pass\nclass Child(Parent):\n    def handle(self): pass\n",
    ),
    (
        "package.json",
        "{\"name\":\"fixture\",\"main\":\"src/start.ts\",\"bin\":{\"fixture\":\"src/cli.ts\"},\"devDependencies\":{\"jest\":\"1\",\"vitest\":\"1\"}}\n",
    ),
    ("src/start.ts", "export function main() {}\nmain();\n"),
    ("src/cli.ts", "export function main() {}\nmain();\n"),
    (
        "tests/node.test.ts",
        "import { test, it } from 'node:test';\ntest('node', function node_case() {});\nit('node it', function node_it_case() {});\nfunction shadow(test: Function) { test('no', function node_shadow() {}); }\n",
    ),
    (
        "tests/jest.test.ts",
        "import { test, it } from '@jest/globals';\ntest('jest', function jest_case() {});\nit('jest it', function jest_it_case() {});\n",
    ),
    (
        "tests/vitest.test.ts",
        "import { test, it } from 'vitest';\ntest('vitest', function vitest_case() {});\nit('vitest it', function vitest_it_case() {});\nfunction shadow(test: Function) { test('no', function vitest_shadow() {}); }\n{ const {test} = fake; test('no', function destructured_shadow() {}); }\nfor (const test of runners) { test('no', function loop_shadow() {}); }\ntest(function first_argument() {}, () => {});\n",
    ),
    (
        "tests/type_only.test.ts",
        "import type { test } from 'vitest';\ntest('not runtime', function type_only_case() {});\n",
    ),
    (
        "tests/type_comment.test.ts",
        "import /* type import */ type { test } from 'vitest';\ntest('not runtime', function type_comment_case() {});\n",
    ),
    (
        "service.py",
        "from fastapi import FastAPI\nimport click\nfrom celery import shared_task\nfrom mcp.server.fastmcp import FastMCP\napp = FastAPI()\nmcp = FastMCP('test')\n@app.get('/items')\ndef route_handler(): pass\n@click.command()\ndef command_handler(): pass\n@shared_task\ndef event_handler(): pass\n@mcp.tool()\ndef tool_handler(): pass\n",
    ),
    (
        "typer_app.py",
        "from typer import Typer\napp = Typer()\n@app.command()\ndef typer_command(): pass\n",
    ),
    (
        "shadows.py",
        "import click\nfrom celery import shared_task as task\nclick = object()\ntask = lambda f: f\n@click.command()\ndef shadowed_click(): pass\n@task\ndef shadowed_task(): pass\ndef shared_task(f): return f\n@shared_task\ndef local_task(): pass\n",
    ),
];

#[test]
fn untested_queries_preserve_test_exclusion_and_signatures() {
    let (_workspace, client) = McpTestHarness::with_files(UNTESTED_FILES);
    let mut client = client.expect("MCP initialize");
    let untested = client
        .payload(
            "code_map_query",
            json!({"operation":"untested","target":"service.py","limit":100}),
        )
        .expect("MCP tool should succeed");
    let untested_names = untested["nodes"]["items"].to_string();
    assert!(untested_names.contains("target"), "{untested}");
    assert!(!untested_names.contains("entry"), "{untested}");
    let unused = crate::json_array(&untested["nodes"]["items"])
        .iter()
        .find(|node| node["name"] == "unused")
        .unwrap();
    assert!(
        unused["signature"]
            .as_str()
            .is_some_and(|signature| signature.contains("int")),
        "{unused}"
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn untested_sql_counts_only_calls_and_inheritance_cycles_terminate() {
    let (workspace, _) = McpTestHarness::with_files(UNTESTED_FILES);
    let connection = rusqlite::Connection::open(workspace.db()).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let page = contextunity_forge_mcp::core::response::QueryOptions::resolve(
        &ResponsePolicy::default(),
        Some(100),
        0,
        None,
        None,
    )
    .unwrap();
    let query_options = traversal::GraphQueryOptions {
        depth: 1,
        direction: None,
        coverage: Default::default(),
        page: &page,
    };
    let inserted = connection
        .execute(
            "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) SELECT e.src_hash,?1,target.node_hash,e.path_id,e.line,e.evidence_id,e.confidence_id,e.occurrence_count FROM edges e JOIN nodes caller ON caller.node_hash=e.src_hash JOIN nodes target ON target.name=?2 WHERE caller.is_test=1 AND e.kind='calls' LIMIT 1",
            rusqlite::params!["references", "unused"],
        )
        .unwrap();
    assert_eq!(inserted, 1);
    let reference_query =
        traversal::query_with_options(&connection, "untested", Some("service.py"), &query_options)
            .unwrap();
    assert!(reference_query["nodes"]["items"]
        .to_string()
        .contains("unused"));
    connection
        .execute(
            "UPDATE edges SET kind=?1 WHERE kind=?2 AND dst_hash=(SELECT node_hash FROM nodes WHERE name=?3)",
            rusqlite::params!["possible_calls", "references", "unused"],
        )
        .unwrap();
    let possible_call_query =
        traversal::query_with_options(&connection, "untested", Some("service.py"), &query_options)
            .unwrap();
    assert!(possible_call_query["nodes"]["items"]
        .to_string()
        .contains("unused"));
    connection
        .execute(
            "UPDATE edges SET kind=?1 WHERE kind=?2 AND dst_hash=(SELECT node_hash FROM nodes WHERE name=?3)",
            rusqlite::params!["calls", "possible_calls", "unused"],
        )
        .unwrap();
    let call_query =
        traversal::query_with_options(&connection, "untested", Some("service.py"), &query_options)
            .unwrap();
    assert!(!call_query["nodes"]["items"].to_string().contains("unused"));
    let inserted_cycle = connection
        .execute(
            "INSERT INTO edges(src_hash,kind,dst_hash,path_id,line,evidence_id,confidence_id,occurrence_count) SELECT e.dst_hash,'inherits',e.src_hash,e.path_id,e.line,e.evidence_id,e.confidence_id,e.occurrence_count FROM edges e JOIN nodes child ON child.node_hash=e.src_hash JOIN nodes parent ON parent.node_hash=e.dst_hash WHERE e.kind='inherits' AND child.name=?1 AND parent.name=?2 LIMIT 1",
            rusqlite::params!["Child", "Base"],
        )
        .unwrap();
    assert_eq!(inserted_cycle, 1);
    let cyclic = traversal::query_with_options(
        &connection,
        "implementations",
        Some("service.py:Base"),
        &query_options,
    )
    .unwrap();
    assert_eq!(cyclic["nodes"]["total"], 2, "{cyclic}");
    connection.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn mcp_implementations_include_python_subclasses_and_rust_trait_impls() {
    let mut files = UNTESTED_FILES.to_vec();
    files.push((
        "traits.rs",
        "pub trait Save {}\npub struct Record;\nimpl Save for Record {}\n",
    ));
    let (_workspace, harness) = McpTestHarness::with_files(&files);
    let mut client = harness.expect("MCP initialize");

    let python = client
        .payload(
            "code_map_query",
            json!({"operation":"implementations","selector":"service.py:Base","limit":100}),
        )
        .expect("MCP tool should succeed");
    let python_names = python["nodes"]["items"].to_string();
    for descendant in ["Child", "GrandChild"] {
        assert!(python_names.contains(descendant), "{python}");
    }
    let rust = client
        .payload(
            "code_map_query",
            json!({"operation":"implementations","selector":"traits.rs:Save","limit":100}),
        )
        .expect("MCP tool should succeed");
    assert!(
        rust["nodes"]["items"].to_string().contains("Record"),
        "{rust}"
    );
    client.finish().expect("server must finish successfully");
}

#[test]
fn route_delta_retags_unchanged_handlers_and_matches_cold_index() {
    // A changed registration can retag an unchanged handler in another file.
    let routes = crate::routing_fixture::RoutingFixture::new(
        "views.py",
        "def first(request: Request, value: str):\n    result = value\n    return result\ndef second(request: Request):\n    return request\n",
        "from django.urls import path\nfrom views import first, second\n",
        "urlpatterns = [path('item/', first)]",
    )
    .expect("real route fixture");
    for (registrations, expected_first, expected_second) in [
        (
            "urlpatterns = [path('item/', first), path('other/', first)]",
            Some("http_route"),
            None,
        ),
        (
            "urlpatterns = [path('other/', first)]",
            Some("http_route"),
            None,
        ),
        (
            "urlpatterns = [path('item/', second)]",
            None,
            Some("http_route"),
        ),
        ("urlpatterns = []", None, None),
        (
            "urlpatterns = [path('item/', first)]",
            Some("http_route"),
            None,
        ),
    ] {
        let delta = routes
            .change_routes(registrations)
            .expect("apply real route delta");
        assert_eq!(delta["reparsed_files"], 1, "{delta}");
        assert_eq!(delta["rewritten_files"], 1, "{delta}");
        let cold = routes.build_cold().expect("real cold comparison index");
        assert_eq!(delta["output_root"], cold["output_root"]);
        let snapshot = routes
            .snapshot()
            .expect("open actual incremental and cold indexes");
        contextunity_forge_mcp::core::commitments::verify(&snapshot.incremental)
            .expect("incremental commitments must verify");
        for (name, expected) in [("first", expected_first), ("second", expected_second)] {
            let pair = snapshot.function(name).expect("read real function facts");
            assert_eq!(
                pair.entrypoint.as_deref(),
                expected,
                "{name}: {registrations}"
            );
            assert_eq!(
                pair.incremental_details, pair.cold_details,
                "{name}: {registrations}"
            );
        }
    }
}

#[test]
fn intrinsic_tests_keep_their_role_after_route_registration_changes() {
    // An intrinsic test remains a test when a route targets it and when that
    // registration is removed, even though the target file is unchanged.
    let intrinsic = crate::routing_fixture::RoutingFixture::new(
        "test_views.py",
        "def test_handler(request: str) -> str:\n    return request\n",
        "from django.urls import path\nfrom test_views import test_handler\n",
        "urlpatterns = []",
    )
    .expect("real intrinsic route fixture");
    for registration in ["path('item/', test_handler)", ""] {
        let delta = intrinsic
            .change_routes(&format!("urlpatterns = [{registration}]"))
            .expect("apply actual route delta");
        let cold = intrinsic.build_cold().expect("real cold comparison index");
        assert_eq!(delta["output_root"], cold["output_root"]);
        let snapshot = intrinsic
            .snapshot()
            .expect("open actual incremental and cold indexes");
        let pair = snapshot
            .function("test_handler")
            .expect("read actual intrinsic test facts");
        let serialized = pair.incremental_details;
        assert_eq!(serialized, pair.cold_details);
        let value: Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(value["entrypoint"], "test");
        assert!(value.get("entrypoint_base").is_none());
        assert_eq!(value["return_type"], "str");
    }
}

#[test]
fn project_script_delta_retags_only_the_selected_function() {
    let scripts = Workspace::with_files(&[
        ("app.py", "def first(): pass\ndef second(): pass\n"),
        ("pyproject.toml", "[project]\nname='sample'\nversion='1'\n"),
    ]);
    scripts.build();
    for (target, first, second) in [
        (Some("app:first"), Some("main"), None),
        (Some("app:second"), None, Some("main")),
        (None, None, None),
    ] {
        let entry = target.map_or_else(String::new, |target| {
            format!("\n[project.scripts]\nsample='{target}'\n")
        });
        scripts.write(
            "pyproject.toml",
            &format!("[project]\nname='sample'\nversion='1'\n{entry}"),
        );
        let delta = scripts.delta(&["pyproject.toml"]);
        let cold = scripts.build_to(scripts.path(".forge/cold.sqlite"));
        assert_eq!(delta["output_root"], cold["output_root"], "{delta}");
        let conn = scripts.open();
        contextunity_forge_mcp::core::commitments::verify(&conn).unwrap();
        for (name, expected) in [("first", first), ("second", second)] {
            let actual: Option<String> = scripts.open().query_row(
                "SELECT json_extract(details,'$.entrypoint') FROM nodes WHERE name=?1 AND kind='function'",
                [name],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(actual.as_deref(), expected, "{target:?} / {name}");
        }
    }
}

#[test]
fn command_alias_scope_and_wildcard_shadowing_survive_delta() {
    let valid_command = "from unrelated import BaseCommand as Unrelated\nfrom django.core.management.base import BaseCommand as Parent, BaseCommand as OtherParent\nclass Command(Parent):\n    def handle(self): pass\n";
    let wildcard_command = "from django.core.management.base import BaseCommand as Parent\nfrom unknown import *\nclass Command(Parent):\n    def handle(self): pass\n";
    let command_scope = Workspace::with_files(&[
        ("requirements.txt", "django\n"),
        ("command.py", valid_command),
        ("wildcard_command.py", wildcard_command),
    ]);
    command_scope.build();
    for (source, expected) in [
        (valid_command, 1),
        (wildcard_command, 0),
        (valid_command, 1),
    ] {
        command_scope.write("command.py", source);
        command_scope.delta(&["command.py"]);
        let command_registration_count = |path: &str| -> i64 {
            command_scope.open().query_row(
                "SELECT count(*) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE p.path=?1 AND n.kind='cli_command_registration'",
                [path],
                |row| row.get(0),
            ).unwrap()
        };
        assert_eq!(command_registration_count("command.py"), expected);
        assert_eq!(command_registration_count("wildcard_command.py"), 0);
    }
}

#[test]
fn entrypoint_overview_and_unwired_queries_preserve_provenance_across_interfaces() {
    let (roots, harness) = McpTestHarness::indexed_workspace(ENTRYPOINT_FILES);
    let mut roots_client = harness.expect("MCP initialize");
    let mut call_tool = |tool: &str, arguments: serde_json::Value| {
        roots_client
            .payload(tool, arguments)
            .expect("MCP tool should succeed")
    };
    let mut overview_for =
        |aspects: &[&str]| call_tool("code_map_overview", json!({"aspects":aspects,"limit":100}));
    let overview = overview_for(&["entrypoints"]);
    let entries = overview["entrypoints"].to_string();
    assert!(
        entries.contains("main") && entries.contains("src/main.rs"),
        "{overview}"
    );
    assert!(
        !entries.contains("ordinary_unwired") && !entries.contains("test_root"),
        "{overview}"
    );

    let with_tests = overview_for(&["entrypoints_with_tests"]);
    let combined = overview_for(&["entrypoints", "entrypoints_with_tests"]);
    assert_eq!(combined["entrypoints"], with_tests["entrypoints"]);
    let test_items = crate::json_array(&with_tests["entrypoints"]["items"]);
    assert!(
        test_items
            .iter()
            .any(|item| item["name"] == "test_root" && item["category"] == "test"),
        "{with_tests}"
    );
    assert!(test_items.len() > crate::json_array(&overview["entrypoints"]["items"]).len());
    let items = crate::json_array(&overview["entrypoints"]["items"]);
    let main_entry = items
        .iter()
        .find(|item| item["name"] == "main" && item["path"] == "src/main.rs")
        .unwrap();
    assert_eq!(main_entry["category"], "main");
    assert_eq!(main_entry["path"], "src/main.rs");
    assert_eq!(main_entry["start_line"], 1);
    assert!(main_entry["signature"]
        .as_str()
        .is_some_and(|value| value.contains("fn main")));
    assert_eq!(main_entry["outgoing_calls"], 1);
    for pair in items.windows(2) {
        let left = (
            &pair[0]["category"],
            &pair[0]["outgoing_calls"],
            &pair[0]["id"],
        );
        let right = (
            &pair[1]["category"],
            &pair[1]["outgoing_calls"],
            &pair[1]["id"],
        );
        let left_category = left.0.as_str().unwrap();
        let right_category = right.0.as_str().unwrap();
        assert!(
            left_category < right_category
                || (left_category == right_category
                    && (left.1.as_u64().unwrap() > right.1.as_u64().unwrap()
                        || (left.1 == right.1
                            && left.2.as_str().unwrap() <= right.2.as_str().unwrap())))
        );
    }
    let mut inspect_node = |selector: &str| {
        call_tool(
            "code_map_inspect",
            json!({"selector":selector,"detail":"full"}),
        )
    };
    for (category, selectors) in [
        (
            Some("main"),
            &[
                "src/main.rs:main",
                "package.py:function:script_main",
                "package.py:function:guarded_main",
                "src/start.ts:function:main",
                "src/cli.ts:function:main",
            ][..],
        ),
        (
            Some("test"),
            &[
                "src/main.rs:test_root",
                "tests/node.test.ts:function:node_case",
                "tests/node.test.ts:function:node_it_case",
                "tests/jest.test.ts:function:jest_case",
                "tests/jest.test.ts:function:jest_it_case",
                "tests/vitest.test.ts:function:vitest_case",
                "tests/vitest.test.ts:function:vitest_it_case",
            ][..],
        ),
        (
            Some("cli_command"),
            &[
                "typer_app.py:function:typer_command",
                "command.py:method:handle",
                "alias_command.py:method:handle",
                "inherited_command.py:method:handle",
                "src/clap_tool.rs:function:run_job",
            ][..],
        ),
        (
            None,
            &[
                "package.py:function:not_guarded",
                "shadow_command.py:method:handle",
                "wrong_command.py:method:handle",
                "rebound_command.py:method:handle",
                "src/unrelated.rs:function:unrelated_job",
                "src/clap_rebound.rs:function:rebound_job",
                "src/clap_shadow.rs:function:fake_job",
                "src/clap_local.rs:function:local_job",
                "src/clap_override.rs:function:override_job",
                "tests/node.test.ts:function:node_shadow",
                "tests/vitest.test.ts:function:vitest_shadow",
                "tests/vitest.test.ts:function:destructured_shadow",
                "tests/vitest.test.ts:function:loop_shadow",
                "tests/vitest.test.ts:function:first_argument",
                "tests/type_only.test.ts:function:type_only_case",
                "tests/type_comment.test.ts:function:type_comment_case",
                "shadows.py:shadowed_click",
                "shadows.py:shadowed_task",
                "shadows.py:local_task",
            ][..],
        ),
    ] {
        for selector in selectors {
            let node = inspect_node(selector);
            match category {
                Some(category) => assert_eq!(
                    node["node"]["details"]["entrypoint"], category,
                    "{selector}: {node}"
                ),
                None => assert!(
                    node["node"]["details"].get("entrypoint").is_none(),
                    "{selector}: {node}"
                ),
            }
            if *selector == "src/main.rs:main" {
                assert_eq!(node["node"]["line"], 1);
            }
        }
    }
    for (name, category, line) in [
        ("route_handler", "http_route", 8),
        ("command_handler", "cli_command", 10),
        ("event_handler", "event_handler", 12),
        ("tool_handler", "mcp_tool", 14),
    ] {
        let selector = format!("service.py:function:{name}");
        let node = inspect_node(&selector);
        assert_eq!(node["node"]["details"]["entrypoint"], category, "{node}");
        assert_eq!(node["node"]["line"], line, "{node}");
        assert!(
            entries.contains(name) && entries.contains(category),
            "{overview}"
        );
    }
    let generation = overview["entrypoints"]["generation"].as_str().unwrap();
    let mut read_pages = |aspect: &str, generation: &str| {
        let mut items = Vec::new();
        let mut offset = 0;
        loop {
            let page = call_tool(
                "code_map_overview",
                json!({"aspects":[aspect],"limit":1,"offset":offset,"generation":generation}),
            );
            items.extend(
                crate::json_array(&page["entrypoints"]["items"])
                    .iter()
                    .cloned(),
            );
            match page["entrypoints"]["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        items
    };
    let paged_items = read_pages("entrypoints", generation);
    assert_eq!(&paged_items, items);
    let cli_overview = |aspect: &str| {
        roots
            .query(&["query", "overview", "--aspects", aspect, "--limit", "100"])
            .expect("CLI should succeed")
    };
    let cli = cli_overview("entrypoints");
    assert_eq!(cli["entrypoints"], overview["entrypoints"]);
    let generation = with_tests["entrypoints"]["generation"].as_str().unwrap();
    let paged_tests = read_pages("entrypoints_with_tests", generation);
    assert_eq!(&paged_tests, test_items);
    let cli = cli_overview("entrypoints_with_tests");
    assert_eq!(cli["entrypoints"], with_tests["entrypoints"]);
    let unwired = call_tool("code_map_query", json!({"operation":"unwired","limit":100}));
    let unwired_nodes = unwired["nodes"]["items"].to_string();
    assert!(unwired_nodes.contains("ordinary_unwired"), "{unwired}");
    assert!(
        !unwired_nodes.contains("\"main\"") && !unwired_nodes.contains("test_root"),
        "{unwired}"
    );
    for name in [
        "route_handler",
        "command_handler",
        "event_handler",
        "tool_handler",
    ] {
        assert!(!unwired_nodes.contains(name), "{unwired}");
    }
    assert!(
        unwired["summary"]["excluded_entrypoints_count"]
            .as_u64()
            .unwrap()
            >= 5
    );
    let cli_unwired = roots
        .query(&["query", "run", "unwired", "--limit", "100"])
        .expect("CLI should succeed");
    assert_eq!(cli_unwired["nodes"], unwired["nodes"]);
    assert_eq!(cli_unwired["summary"], unwired["summary"]);
    roots_client
        .finish()
        .expect("server must finish successfully");
}

#[test]
fn inactive_adapter_does_not_claim_decorated_functions() {
    let mut unactivated_client = McpTestHarness::indexed(&[(
        "jobs.py",
        "from celery import shared_task\n@shared_task\ndef job(): pass\n",
    )])
    .expect("MCP initialize");
    let job = unactivated_client
        .inspect("jobs.py:job", json!({"detail":"full"}))
        .expect("MCP tool should succeed");
    assert!(job["node"]["details"].get("entrypoint").is_none(), "{job}");
    unactivated_client
        .finish()
        .expect("server must finish successfully");
}
