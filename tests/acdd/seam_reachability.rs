use super::support::{commit_handoff_fixture_inputs, evidence, fixture, Milestone, Workspace};
use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::{reader, tasks_store::TasksStore},
};
use serde_json::json;

#[test]
fn contract_seam_follows_public_reexports_from_test_helpers() {
    let (root, mut store, _): (Workspace, TasksStore, Milestone) =
        fixture("src/seam.rs::test_connected");
    root.write(
        "src/lib.rs",
        "pub mod api;\npub mod caller;\n#[cfg(test)] mod test_helpers;\n#[cfg(test)] mod seam;\n",
    );
    root.write("src/api.rs", "pub fn route() -> usize { 1 }\n");
    root.write(
        "src/caller.rs",
        "pub use crate::api::route as public_route;\n",
    );
    root.write(
        "src/test_helpers.rs",
        "use crate::caller::public_route;\npub fn helper() -> usize { public_route() }\n",
    );
    root.write(
        "src/seam.rs",
        "use crate::test_helpers::helper;\n#[test]\nfn test_connected() { let actual = helper(); assert_eq!(actual, 1); }\n",
    );
    commit_handoff_fixture_inputs(
        &root,
        &[
            "forge-mcp.yaml",
            "docs/010-test.md",
            "src/lib.rs",
            "src/api.rs",
            "src/caller.rs",
            "src/test_helpers.rs",
            "src/seam.rs",
        ],
    );
    root.build_to(root.path(".forge/code-map.sqlite"));

    let page = QueryOptions::resolve(&ResponsePolicy::default(), Some(20), 0, None, None).unwrap();
    let code_map = reader::open(&root.root().join(".forge/code-map.sqlite"), root.root()).unwrap();
    let explanation = reader::explain_with_options(
        &code_map,
        "src/seam.rs::test_connected",
        &reader::ExplainOptions {
            direction: Some("outgoing"),
            show_doc: false,
            page: &page,
        },
    )
    .unwrap();
    assert!(
        explanation.to_string().contains("src/test_helpers.rs"),
        "the indexed fixture must expose the current test-helper edge: {explanation}"
    );
    let task_id = "forge-mcp/m-test:first";
    TasksStore::claim(
        &mut store,
        task_id,
        "contract",
        "contract-author",
        &root.root().to_string_lossy(),
        Some("test-session-contract-author"),
    )
    .unwrap();
    let task = TasksStore::inspect(&store, task_id).unwrap();
    let mut proof = evidence(&task);
    proof.proof = json!({
        "contract_proof": {
            "seam_test_ref": "src/seam.rs::test_connected",
            "red_exit_code": 101
        }
    });
    let submitted = store.submit(
        task_id,
        "contract",
        &proof,
        "pass",
        None,
        Some("test-session-contract-author"),
    );
    assert!(
        submitted.is_ok(),
        "contract gate should follow the current test-helper and public-reexport path: {submitted:?}"
    );
}
