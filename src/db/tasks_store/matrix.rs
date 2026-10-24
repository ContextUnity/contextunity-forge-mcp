use super::TasksStore;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};

/// Create or upgrade the milestone contract matrix schema.
pub(super) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS milestone_contract_matrix (
            milestone_id TEXT NOT NULL,
            source_task TEXT NOT NULL,
            target_task TEXT NOT NULL,
            contract_type TEXT NOT NULL,
            contract_ref TEXT NOT NULL,
            revision INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (milestone_id, source_task, target_task, contract_ref)
        );",
    )?;
    Ok(())
}

impl TasksStore {
    /// Atomically replace all persisted contract rows for one milestone.
    pub fn replace_contract_matrix(
        &mut self,
        matrix: &crate::core::tasks::MilestoneContractMatrix,
    ) -> Result<()> {
        validate_matrix(matrix)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM milestone_contract_matrix WHERE milestone_id=?1",
            [&matrix.milestone_id],
        )?;
        for entry in &matrix.entries {
            let revision = i64::try_from(entry.revision)
                .context("contract matrix revision exceeds SQLite integer range")?;
            transaction.execute(
                "INSERT INTO milestone_contract_matrix (
                    milestone_id, source_task, target_task, contract_type,
                    contract_ref, revision, status
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry.milestone_id,
                    entry.source_task,
                    entry.target_task,
                    enum_text(&entry.contract_type)?,
                    entry.contract_ref,
                    revision,
                    enum_text(&entry.status)?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Read the persisted contract rows for one milestone in stable key order.
    pub fn list_contract_matrix(
        &self,
        milestone_id: &str,
    ) -> Result<crate::core::tasks::MilestoneContractMatrix> {
        let mut statement = self.connection.prepare(
            "SELECT milestone_id, source_task, target_task, contract_type,
                    contract_ref, revision, status
             FROM milestone_contract_matrix
             WHERE milestone_id=?1
             ORDER BY source_task, target_task, contract_ref",
        )?;
        let rows = statement
            .query_map([milestone_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let entries = rows
            .into_iter()
            .map(
                |(
                    milestone_id,
                    source_task,
                    target_task,
                    contract_type,
                    contract_ref,
                    revision,
                    status,
                )| {
                    Ok(crate::core::tasks::MilestoneContractEntry {
                        milestone_id,
                        source_task,
                        target_task,
                        contract_type: parse_enum(contract_type)?,
                        contract_ref,
                        revision: u64::try_from(revision)
                            .context("contract matrix contains a negative revision")?,
                        status: parse_enum(status)?,
                    })
                },
            )
            .collect::<Result<Vec<_>>>()?;
        Ok(crate::core::tasks::MilestoneContractMatrix {
            milestone_id: milestone_id.to_owned(),
            entries,
        })
    }
}

fn validate_matrix(matrix: &crate::core::tasks::MilestoneContractMatrix) -> Result<()> {
    if matrix.milestone_id.trim().is_empty() {
        bail!("TASK_CONTRACT_MATRIX_INVALID: milestone_id must not be empty");
    }
    for entry in &matrix.entries {
        if entry.milestone_id != matrix.milestone_id {
            bail!("TASK_CONTRACT_MATRIX_INVALID: entry milestone_id must match its matrix");
        }
        if entry.source_task.trim().is_empty()
            || entry.target_task.trim().is_empty()
            || entry.contract_ref.trim().is_empty()
        {
            bail!("TASK_CONTRACT_MATRIX_INVALID: task ids and contract_ref must not be empty");
        }
        if entry.revision == 0 {
            bail!("TASK_CONTRACT_MATRIX_INVALID: revision must be greater than zero");
        }
    }
    Ok(())
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .context("contract matrix enum should serialize as a string")
}

fn parse_enum<T: DeserializeOwned>(value: String) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(value))?)
}
