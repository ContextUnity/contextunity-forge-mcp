use super::*;

#[derive(Clone, Copy)]
pub(crate) enum StateTasks<'a> {
    Listed,
    History(&'a Milestone),
}

#[derive(Debug, PartialEq)]
pub(crate) struct OperationState {
    files: Vec<Vec<u8>>,
    tasks: Value,
}

impl MilestoneHarness {
    pub(crate) fn handoff_times(meta: &serde_yaml::Value) -> (i64, i64) {
        let started =
            chrono::DateTime::parse_from_rfc3339(meta["started_at"].as_str().unwrap()).unwrap();
        let ended =
            chrono::DateTime::parse_from_rfc3339(meta["handoff"]["completed_at"].as_str().unwrap())
                .unwrap();
        (started.timestamp(), (ended - started).num_minutes())
    }

    pub(crate) fn operation_state(
        &self,
        store: &TasksStore,
        paths: &[&str],
        tasks: StateTasks<'_>,
    ) -> OperationState {
        let tasks = match tasks {
            StateTasks::Listed => {
                serde_json::to_value(store.list(None, "all", None).unwrap()).unwrap()
            }
            StateTasks::History(milestone) => Value::Array(
                milestone
                    .tasks
                    .iter()
                    .map(|spec| store.inspect_details(&milestone.task_id(spec)).unwrap())
                    .collect(),
            ),
        };
        OperationState {
            files: paths.iter().map(|path| self.file_bytes(path)).collect(),
            tasks,
        }
    }

    pub(crate) fn retarget_document_receipt(
        document: &str,
        task_ref: &str,
        commit: &str,
    ) -> anyhow::Result<(Receipt, String)> {
        let milestone = Milestone::parse(document, "forge-mcp")?;
        let task = milestone
            .tasks
            .iter()
            .find(|task| task.task_ref == task_ref)
            .ok_or_else(|| anyhow::anyhow!("receipt task absent: {task_ref}"))?;
        let mut receipt = task
            .receipt
            .clone()
            .ok_or_else(|| anyhow::anyhow!("receipt absent: {task_ref}"))?;
        receipt.commit = Some(commit.to_owned());
        let document = milestone_engine::render_task_receipt(document, task_ref, &receipt, &[])?;
        Ok((receipt, document))
    }
}
