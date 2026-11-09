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

impl InvocationSettings {
    pub fn new(
        root: PathBuf,
        config: Option<PathBuf>,
        profile: Arc<GateProfile>,
        profile_argument: Option<PathBuf>,
        code_db: PathBuf,
    ) -> Self {
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
        let rebase = |path: &Path| {
            path.strip_prefix(&self.root)
                .map(|relative| root.join(relative))
                .unwrap_or_else(|_| path.to_owned())
        };
        Self {
            root: root.to_owned(),
            config: self.config.as_deref().map(rebase),
            profile: self.profile.clone(),
            profile_argument: self.profile_argument.as_deref().map(rebase),
            code_db: rebase(&self.code_db),
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
