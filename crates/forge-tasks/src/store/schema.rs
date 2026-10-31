use super::Connection;
use anyhow::{bail, Result};
use rusqlite::TransactionBehavior;

/// Apply additive AC-001 storage migrations after the base task store is opened.
pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    let version: String = connection.query_row(
        "SELECT value FROM task_store_metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version == "7" {
        return Ok(());
    }
    if version != "6" {
        bail!("unsupported tasks schema version '{version}'; use recovery tooling");
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    super::matrix::migrate(&transaction)?;
    super::trace::migrate(&transaction)?;

    let blackboard_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='task_blackboard')",
        [],
        |row| row.get(0),
    )?;
    if !blackboard_exists {
        bail!("TASK_SCHEMA_INVALID: task_blackboard table is missing during version 6 migration");
    }
    if !has_column(&transaction, "recipient_task_id")? {
        transaction
            .execute_batch("ALTER TABLE task_blackboard ADD COLUMN recipient_task_id TEXT;")?;
    }
    if !has_column(&transaction, "status")? {
        transaction.execute_batch(
            "ALTER TABLE task_blackboard ADD COLUMN status TEXT DEFAULT 'active';",
        )?;
    }
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS task_blackboard_recipient_idx
         ON task_blackboard (milestone_ref, recipient_task_id, status);",
    )?;
    if transaction.execute(
        "UPDATE task_store_metadata SET value='7' WHERE key='schema_version' AND value='6'",
        [],
    )? != 1
    {
        bail!("TASK_SCHEMA_MIGRATION_CONFLICT: task store schema version changed during migration");
    }
    transaction.commit()?;
    Ok(())
}

fn has_column(connection: &Connection, column: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_blackboard') WHERE name=?1)",
        [column],
        |row| row.get(0),
    )?)
}
