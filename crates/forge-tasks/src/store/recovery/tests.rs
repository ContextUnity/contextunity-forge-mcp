use super::*;

use std::path::PathBuf;

struct TestTempDir(PathBuf);

impl TestTempDir {
    fn new() -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "contextunity-forge-task-store-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pending_manifest_publication_replays_for_non_git_task_store() -> Result<()> {
    let temp = TestTempDir::new()?;
    let workspace = temp.path().join("workspace");
    let manifest = workspace.join("docs/milestones/010-scope.md");
    std::fs::create_dir_all(manifest.parent().context("manifest parent")?)?;
    let before = b"# Scope\n";
    let after = b"# Scope\n\nExtended scope.\n";
    std::fs::write(&manifest, before)?;

    let replacement =
        ManifestReplacement::new(&workspace, "docs/milestones/010-scope.md", before, after)?;
    let database = temp.path().join(".forge/tasks.sqlite");
    let mut store = TasksStore::open_project(&database, "repo", "project")?;
    let context = store.checkpoint_context()?;
    assert!(
        context.is_none(),
        "fixture must not have Git checkpoint context"
    );

    // Existing pending publications keep deserializing after this additive field.
    let legacy: Publication = serde_json::from_str(r#"{"active":[],"retired":[]}"#)?;
    assert!(legacy.manifest_replacement.is_none());

    let mut tx = transaction(&mut store.connection, context)?;
    tx.stage_manifest_replacement(replacement)?;
    let error = tx
        .commit_with_publisher(|_| Err(anyhow::anyhow!("injected publication failure")))
        .expect_err("injected failure must leave the committed publication pending");
    assert!(is_pending(&error));
    assert_eq!(std::fs::read(&manifest)?, before);
    assert!(ensure_observable(&store.connection).is_err());

    std::fs::write(&manifest, b"# Third-party edit\n")?;
    let error = store
        .reconcile_checkpoints()
        .expect_err("recovery must fail closed on unexpected manifest content");
    assert!(is_pending(&error));
    assert!(ensure_observable(&store.connection).is_err());

    std::fs::write(&manifest, before)?;
    assert!(store.reconcile_checkpoints()?);
    assert_eq!(std::fs::read(&manifest)?, after);
    assert!(!store.reconcile_checkpoints()?);
    ensure_observable(&store.connection)?;
    Ok(())
}
