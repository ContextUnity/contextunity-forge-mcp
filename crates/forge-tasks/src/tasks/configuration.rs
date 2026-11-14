//! Invocation-local configuration selection for the existing synchronous APIs.
use super::profile::GateProfile;
use std::{
    cell::RefCell,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Command,
    sync::Arc,
    task::{Context, Poll},
};

/// Immutable selections shared by loaders during one CLI invocation.
#[derive(Clone)]
pub struct InvocationSettings {
    root: PathBuf,
    pub config: Option<PathBuf>,
    pub profile: Arc<GateProfile>,
    pub profile_argument: Option<PathBuf>,
    pub code_db: PathBuf,
}

thread_local! {
    static INVOCATIONS: RefCell<Vec<Arc<InvocationSettings>>> = const { RefCell::new(Vec::new()) };
}

fn rebase_path(path: &Path, from_root: &Path, to_root: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(from_root) {
        return to_root.join(relative);
    }
    if path.is_absolute() {
        if let Some(canon_path) = forge_core::fs::canonicalize_with_missing_tail(path) {
            if let Ok(relative) = canon_path.strip_prefix(from_root) {
                return to_root.join(relative);
            }
        }
    }
    path.to_owned()
}

impl InvocationSettings {
    pub fn new(
        root: PathBuf,
        config: Option<PathBuf>,
        profile: Arc<GateProfile>,
        profile_argument: Option<PathBuf>,
        code_db: PathBuf,
    ) -> Self {
        let root = root.canonicalize().unwrap_or(root);
        let config = config.map(|path| rebase_path(&path, &root, &root));
        let profile_argument = profile_argument.map(|path| rebase_path(&path, &root, &root));
        let code_db = rebase_path(&code_db, &root, &root);
        Self {
            root,
            config,
            profile,
            profile_argument,
            code_db,
        }
    }

    /// Bind a selection to a synchronous call, restoring the enclosing selection on error or panic.
    pub fn run<T>(&self, operation: impl FnOnce() -> T) -> T {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                INVOCATIONS.with(|settings| {
                    settings.borrow_mut().pop();
                });
            }
        }
        INVOCATIONS.with(|settings| settings.borrow_mut().push(Arc::new(self.clone())));
        let _restore = Restore;
        operation()
    }

    /// Re-enter only while polling this future; never leave configuration installed across an await.
    pub fn scope<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
        struct Scoped<F: Future> {
            settings: InvocationSettings,
            future: Pin<Box<F>>,
        }
        impl<F: Future> Future for Scoped<F> {
            type Output = F::Output;
            fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.get_mut();
                this.settings.run(|| this.future.as_mut().poll(context))
            }
        }
        Scoped {
            settings: self,
            future: Box::pin(future),
        }
    }

    /// Keep root-relative selections when dispatching into an owned worktree.
    pub fn rebase(&self, root: &Path) -> Self {
        let rebased_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        Self {
            root: rebased_root.clone(),
            config: self
                .config
                .as_deref()
                .map(|path| rebase_path(path, &self.root, &rebased_root)),
            profile: self.profile.clone(),
            profile_argument: self
                .profile_argument
                .as_deref()
                .map(|path| rebase_path(path, &self.root, &rebased_root)),
            code_db: rebase_path(&self.code_db, &self.root, &rebased_root),
        }
    }

    /// Pass settings only to the owned child, without changing the parent's environment.
    pub fn configure_child(&self, command: &mut Command) {
        if let Some(config) = &self.config {
            command.env("FORGE_CONFIG", config);
        }
        if let Some(profile) = &self.profile_argument {
            command.env("FORGE_PROFILE", profile);
        }
        command.env("FORGE_DB", &self.code_db);
    }
}

/// Internal Forge orchestration environment variables that must never leak into project commands.
pub const FORGE_ORCHESTRATION_ENVS: &[&str] = &[
    "FORGE_CONFIG",
    "FORGE_PROFILE",
    "FORGE_DB",
    "FORGE_WORKSPACE_ROOT",
    "FORGE_AGENT_SESSION_ID",
];

/// Clear internal Forge orchestration variables from a project command process.
pub fn sanitize_command_process(command: &mut Command) {
    for var in FORGE_ORCHESTRATION_ENVS {
        command.env_remove(var);
    }
}

/// Return the immutable selection for this exact workspace, if explicitly scoped by the caller.
pub fn current(root: &Path) -> Option<Arc<InvocationSettings>> {
    let root = root.canonicalize().ok()?;
    INVOCATIONS.with(|settings| {
        settings
            .borrow()
            .iter()
            .rev()
            .find(|settings| settings.root == root)
            .cloned()
    })
}

pub fn config_path(root: &Path) -> PathBuf {
    current(root)
        .and_then(|settings| settings.config.clone())
        .unwrap_or_else(|| root.join("forge-mcp.yaml"))
}
