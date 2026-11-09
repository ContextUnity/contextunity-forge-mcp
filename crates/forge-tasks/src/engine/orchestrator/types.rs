use crate::tasks::profile::RunnerConfig;
use std::{collections::BTreeMap, path::PathBuf};

/// Runtime values supplied to a configured role runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerInvocationContext {
    /// Canonical task identifier for this invocation.
    pub task_id: String,
    /// Active lifecycle stage being handled.
    pub stage: String,
    /// Configured role name.
    pub role: String,
    /// Selected model identifier.
    pub model: String,
    /// Selected reasoning effort.
    pub reasoning: String,
    /// Runner session identifier, including a resumed builder session when applicable.
    pub session_id: String,
    /// Prompt rendered from the claimed gate context bundle.
    pub prompt: String,
    /// Worktree in which the runner should execute.
    pub worktree: PathBuf,
}

impl RunnerInvocationContext {
    /// Interpolate execution values into a runner command template in one pass.
    ///
    /// Placeholder-looking text inside a substituted value remains literal. This
    /// renders text only; the process spawner owns safe argument transport.
    pub fn render_runner(&self, config: &RunnerConfig) -> String {
        let worktree = self.worktree.to_string_lossy();
        let mut rendered = String::with_capacity(config.runner.len());
        let mut cursor = 0;

        while let Some(relative_open) = config.runner[cursor..].find('{') {
            let open = cursor + relative_open;
            rendered.push_str(&config.runner[cursor..open]);

            let placeholder_start = open + 1;
            let Some(relative_close) = config.runner[placeholder_start..].find('}') else {
                rendered.push_str(&config.runner[open..]);
                return rendered;
            };
            let close = placeholder_start + relative_close;
            let placeholder = &config.runner[placeholder_start..close];
            let value = match placeholder {
                "model" => Some(self.model.as_str()),
                "reasoning" => Some(self.reasoning.as_str()),
                "session_id" => Some(self.session_id.as_str()),
                "prompt" => Some(self.prompt.as_str()),
                "worktree" => Some(worktree.as_ref()),
                _ => None,
            };

            if let Some(value) = value {
                rendered.push_str(value);
            } else {
                rendered.push_str(&config.runner[open..=close]);
            }
            cursor = close + 1;
        }

        rendered.push_str(&config.runner[cursor..]);
        rendered
    }
}

/// In-memory observations for one orchestrator run.
///
/// Durable task, gate, and checkpoint data remains authoritative. This projection
/// is only used to coordinate runners and report progress during the current run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrchestratorState {
    /// Wave currently being executed, starting at zero.
    pub active_wave_index: usize,
    /// Latest observed worker for each claimed task identifier.
    pub claimed_tasks: BTreeMap<String, String>,
    /// Cumulative runner starts observed for each task during this run.
    pub attempts: BTreeMap<String, u64>,
}

impl OrchestratorState {
    /// Record the active wave and return its start observation.
    pub fn begin_wave(&mut self, wave_index: usize, task_ids: Vec<String>) -> OrchestratorEvent {
        self.active_wave_index = wave_index;
        OrchestratorEvent::WaveStarted {
            wave_index,
            task_ids,
        }
    }

    /// Record the latest worker observed claiming a task and return that observation.
    pub fn record_claim(
        &mut self,
        task_id: &str,
        stage: &str,
        worker_id: &str,
    ) -> OrchestratorEvent {
        self.claimed_tasks
            .insert(task_id.to_owned(), worker_id.to_owned());
        OrchestratorEvent::TaskClaimed {
            task_id: task_id.to_owned(),
            stage: stage.to_owned(),
            worker_id: worker_id.to_owned(),
        }
    }

    /// Count a runner start without resetting prior attempts and return its observation.
    pub fn record_runner_start(
        &mut self,
        task_id: &str,
        stage: &str,
        worker_id: &str,
        session_id: &str,
    ) -> OrchestratorEvent {
        let attempt = self.attempts.entry(task_id.to_owned()).or_default();
        *attempt = attempt.saturating_add(1);
        OrchestratorEvent::RunnerStarted {
            task_id: task_id.to_owned(),
            stage: stage.to_owned(),
            worker_id: worker_id.to_owned(),
            session_id: session_id.to_owned(),
            attempt: *attempt,
        }
    }
}

/// Typed observations emitted by an orchestrator run.
///
/// These events report runner and lifecycle outcomes; they do not advance an
/// authoritative task gate. Gate progression still uses the existing task APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorEvent {
    /// A worker was observed claiming a task gate.
    TaskClaimed {
        /// Canonical task identifier.
        task_id: String,
        /// Claimed lifecycle stage.
        stage: String,
        /// Worker identity returned by the claim.
        worker_id: String,
    },
    /// A runner process was observed starting for a claimed gate.
    RunnerStarted {
        /// Canonical task identifier.
        task_id: String,
        /// Active lifecycle stage.
        stage: String,
        /// Worker identity associated with the runner.
        worker_id: String,
        /// Session used by the runner process.
        session_id: String,
        /// Cumulative runner start count for this task in the current run.
        attempt: u64,
    },
    /// A runner process produced an exit observation.
    RunnerExited {
        /// Canonical task identifier.
        task_id: String,
        /// Active lifecycle stage.
        stage: String,
        /// Worker identity associated with the runner.
        worker_id: String,
        /// Process exit code, or `None` when no exit code was observed.
        exit_code: Option<i32>,
        /// Whether the process was terminated because its timeout elapsed.
        timed_out: bool,
    },
    /// An authoritative gate submission was observed to reject with structured findings.
    GateRejected {
        /// Canonical task identifier.
        task_id: String,
        /// Rejected lifecycle stage.
        stage: String,
        /// Worker identity associated with the rejection.
        worker_id: String,
        /// Structured findings returned by the gate submission.
        findings: serde_json::Value,
    },
    /// An authoritative delivery result was observed for a task.
    TaskDelivered {
        /// Canonical task identifier.
        task_id: String,
        /// Delivery lifecycle stage.
        stage: String,
        /// Worker identity associated with delivery.
        worker_id: String,
        /// Receipt identifier returned by the task store.
        receipt_id: String,
    },
    /// A topological task wave was observed to start.
    WaveStarted {
        /// Active zero-based wave index.
        wave_index: usize,
        /// Tasks scheduled in the wave.
        task_ids: Vec<String>,
    },
    /// A topological task wave was observed to complete.
    WaveCompleted {
        /// Completed zero-based wave index.
        wave_index: usize,
        /// Tasks whose authoritative completion was observed in the wave.
        task_ids: Vec<String>,
    },
}
