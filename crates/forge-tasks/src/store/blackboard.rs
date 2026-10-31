use super::*;
use rusqlite::Transaction;

/// Return whether a topic is one of the canonical runtime categories.
pub fn is_canonical_blackboard_topic(topic: &str) -> bool {
    BLACKBOARD_TOPICS
        .iter()
        .any(|(canonical, _)| *canonical == topic)
}

fn validate_blackboard_topic(topic: &str) -> Result<()> {
    if is_canonical_blackboard_topic(topic) {
        Ok(())
    } else {
        let allowed = BLACKBOARD_TOPICS
            .iter()
            .map(|(canonical, _)| *canonical)
            .collect::<Vec<_>>()
            .join(", ");
        bail!("TASK_BLACKBOARD_TOPIC_INVALID: use one of the canonical topics: {allowed}");
    }
}

pub(super) fn validate_milestone_broadcast(
    store: &TasksStore,
    post: BlackboardPostInput<'_>,
) -> Result<()> {
    store.assert_milestone_scope_ref(post.milestone_ref)?;
    validate_milestone_broadcast_fields(post)
}

fn validate_milestone_broadcast_fields(post: BlackboardPostInput<'_>) -> Result<()> {
    if post.task_id.is_some() || post.subtask_ref.is_some() {
        bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
    }
    if post.author.trim().is_empty() {
        bail!("TASK_BLACKBOARD_INVALID: author is required");
    }
    validate_blackboard_topic(post.topic)
}

pub(super) fn insert_milestone_broadcast_in_transaction(
    transaction: &Transaction<'_>,
    post: BlackboardPostInput<'_>,
) -> Result<u64> {
    validate_milestone_broadcast_fields(post)?;
    let inserted = transaction.execute(
        "INSERT INTO task_blackboard(milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,payload,created_at) \
         VALUES(?1,NULL,NULL,NULL,?2,?3,?4,?5,?6)",
        params![
            post.milestone_ref,
            post.gate,
            post.author,
            post.topic,
            post.payload,
            now()
        ],
    )?;
    if inserted != 1 {
        bail!("TASK_BLACKBOARD_STATE_CHANGED");
    }
    Ok(u64::try_from(transaction.last_insert_rowid())?)
}

pub(crate) fn clear_blackboard(connection: &Connection, task_id: &str) -> Result<usize> {
    Ok(connection.execute("DELETE FROM task_blackboard WHERE task_id=?1", [task_id])?)
}

impl TasksStore {
    /// Persist a message for one task and return its SQLite identifier.
    pub fn blackboard_post(
        &self,
        task_id: &str,
        author: &str,
        topic: &str,
        payload: &str,
    ) -> Result<u64> {
        self.assert_id(task_id)?;
        let task = self.inspect(task_id)?;
        let milestone_ref = self.milestone_scope_ref(&task.milestone_ref)?;
        self.blackboard_post_scoped(&milestone_ref, Some(task_id), None, author, topic, payload)
    }

    /// Persist a message at its resolved hierarchy level and return its SQLite identifier.
    pub fn blackboard_post_scoped(
        &self,
        milestone_ref: &str,
        task_id: Option<&str>,
        subtask_ref: Option<&str>,
        author: &str,
        topic: &str,
        payload: &str,
    ) -> Result<u64> {
        self.blackboard_post_scoped_with_gate(BlackboardPostInput {
            milestone_ref,
            task_id,
            subtask_ref,
            gate: None,
            author,
            topic,
            payload,
        })
    }

    /// Persist a message targeted to an optional workflow gate.
    pub fn blackboard_post_scoped_with_gate(&self, post: BlackboardPostInput<'_>) -> Result<u64> {
        self.blackboard_post_scoped_with_recipient(post, None)
    }

    /// Persist a scoped message with an optional task recipient distinct from its owner scope.
    pub fn blackboard_post_scoped_with_recipient(
        &self,
        post: BlackboardPostInput<'_>,
        recipient_task_id: Option<&str>,
    ) -> Result<u64> {
        let BlackboardPostInput {
            milestone_ref,
            task_id,
            subtask_ref,
            gate,
            author,
            topic,
            payload,
        } = post;
        self.assert_milestone_scope_ref(milestone_ref)?;
        if author.trim().is_empty() {
            bail!("TASK_BLACKBOARD_INVALID: author is required");
        }
        validate_blackboard_topic(topic)?;
        let task_milestone_ref = if let Some(task_id) = task_id {
            self.assert_id(task_id)?;
            let task = self.inspect(task_id)?;
            if self.milestone_scope_ref(&task.milestone_ref)? != milestone_ref {
                bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
            }
            if task.status == "completed" {
                bail!("TASK_TERMINAL");
            }
            if let Some(subtask_ref) = subtask_ref {
                if subtask_ref.trim().is_empty() {
                    bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
                }
                let exists: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2)",
                    params![task_id, subtask_ref],
                    |row| row.get(0),
                )?;
                if !exists {
                    bail!("TASK_SUBTASK_NOT_FOUND");
                }
            }
            Some(task.milestone_ref)
        } else if subtask_ref.is_some() {
            bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
        } else {
            None
        };
        let recipient_milestone_ref = if let Some(recipient_task_id) = recipient_task_id {
            self.assert_id(recipient_task_id)?;
            let recipient = self.inspect(recipient_task_id)?;
            if self.milestone_scope_ref(&recipient.milestone_ref)? != milestone_ref {
                bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
            }
            if recipient.status == "completed" {
                bail!("TASK_TERMINAL");
            }
            Some(recipient.milestone_ref)
        } else {
            None
        };
        let created_at = now();
        let scope_milestone_ref = task_milestone_ref
            .as_deref()
            .or(recipient_milestone_ref.as_deref());
        let inserted = match (task_id, recipient_task_id, scope_milestone_ref) {
            (Some(task_id), recipient_task_id, Some(task_milestone_ref)) => self.connection.execute(
                "INSERT INTO task_blackboard(milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,payload,created_at) \
                 SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9 \
                 FROM tasks \
                 WHERE task_id=?2 AND milestone_ref=?10 \
                   AND json_extract(descriptor,'$.status')!='completed' \
                   AND (?4 IS NULL OR EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?2 AND subtask_ref=?4)) \
                   AND (?3 IS NULL OR EXISTS(SELECT 1 FROM tasks AS recipient WHERE recipient.task_id=?3 \
                     AND recipient.milestone_ref=?10 \
                     AND json_extract(recipient.descriptor,'$.status')!='completed'))",
                params![milestone_ref, task_id, recipient_task_id, subtask_ref, gate, author, topic, payload, created_at, task_milestone_ref],
            )?,
            (None, Some(recipient_task_id), Some(recipient_milestone_ref)) => self.connection.execute(
                "INSERT INTO task_blackboard(milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,payload,created_at) \
                 SELECT ?1,NULL,?2,NULL,?3,?4,?5,?6,?7 \
                 FROM tasks AS recipient \
                 WHERE recipient.task_id=?2 AND recipient.milestone_ref=?8 \
                   AND json_extract(recipient.descriptor,'$.status')!='completed'",
                params![milestone_ref, recipient_task_id, gate, author, topic, payload, created_at, recipient_milestone_ref],
            )?,
            (None, None, None) => self.connection.execute(
                "INSERT INTO task_blackboard(milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,payload,created_at) \
                 VALUES(?1,NULL,NULL,NULL,?2,?3,?4,?5,?6)",
                params![milestone_ref, gate, author, topic, payload, created_at],
            )?,
            _ => bail!("TASK_BLACKBOARD_SCOPE_MISMATCH"),
        };
        if inserted == 0 {
            if let Some(task_id) = task_id {
                let task = self.inspect(task_id)?;
                if self.milestone_scope_ref(&task.milestone_ref)? != milestone_ref {
                    bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
                }
                if task.status == "completed" {
                    bail!("TASK_TERMINAL");
                }
                if let Some(subtask_ref) = subtask_ref {
                    let exists: bool = self.connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2)",
                        params![task_id, subtask_ref],
                        |row| row.get(0),
                    )?;
                    if !exists {
                        bail!("TASK_SUBTASK_NOT_FOUND");
                    }
                }
            }
            if let Some(recipient_task_id) = recipient_task_id {
                self.assert_id(recipient_task_id)?;
                let recipient = self.inspect(recipient_task_id)?;
                if self.milestone_scope_ref(&recipient.milestone_ref)? != milestone_ref {
                    bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
                }
                if recipient.status == "completed" {
                    bail!("TASK_TERMINAL");
                }
            }
            bail!("TASK_BLACKBOARD_STATE_CHANGED");
        }
        Ok(u64::try_from(self.connection.last_insert_rowid())?)
    }

    /// Return unread task-targeted message summaries in insertion order.
    pub fn blackboard_page_for_recipient(
        &self,
        milestone_ref: &str,
        recipient_task_id: &str,
        topic: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<BlackboardPage> {
        self.assert_milestone_scope_ref(milestone_ref)?;
        if let Some(topic) = topic {
            validate_blackboard_topic(topic)?;
        }
        self.assert_id(recipient_task_id)?;
        let recipient = self.inspect(recipient_task_id)?;
        if self.milestone_scope_ref(&recipient.milestone_ref)? != milestone_ref {
            bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
        }
        if recipient.status == "completed" {
            bail!("TASK_TERMINAL");
        }
        let limit = limit.unwrap_or(10).min(50);
        if limit == 0 {
            bail!("TASK_BLACKBOARD_INVALID: limit must be greater than zero");
        }
        let offset = offset.unwrap_or(0);
        let sql_limit = i64::try_from(limit.checked_add(1).context("blackboard limit overflow")?)?;
        let sql_offset = i64::try_from(offset)?;
        let mut statement = self.connection.prepare(
            "SELECT id,milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,created_at \
             FROM task_blackboard \
             WHERE milestone_ref=?1 AND recipient_task_id=?2 AND status='active' \
               AND (?3 IS NULL OR topic=?3) \
             ORDER BY created_at,id LIMIT ?4 OFFSET ?5",
        )?;
        let mut messages = statement
            .query_map(
                params![
                    milestone_ref,
                    recipient_task_id,
                    topic,
                    sql_limit,
                    sql_offset
                ],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        recipient_task_id: row.get(3)?,
                        subtask_ref: row.get(4)?,
                        gate: row.get(5)?,
                        author: row.get(6)?,
                        topic: row.get(7)?,
                        payload: None,
                        created_at: row.get(8)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = messages.len() > limit;
        if has_more {
            messages.pop();
        }
        Ok(BlackboardPage {
            messages,
            pagination: BlackboardPagination {
                limit,
                offset,
                has_more,
                next_offset: has_more.then(|| offset.saturating_add(limit)),
            },
        })
    }

    /// Mark one active recipient message read. Returns false if it is already read or not owned by that recipient.
    pub fn blackboard_mark_read(&self, message_id: u64, recipient_task_id: &str) -> Result<bool> {
        self.assert_id(recipient_task_id)?;
        let recipient = self.inspect(recipient_task_id)?;
        let milestone_ref = self.milestone_scope_ref(&recipient.milestone_ref)?;
        let message_id = i64::try_from(message_id)?;
        Ok(self.connection.execute(
            "UPDATE task_blackboard SET status='read' \
             WHERE id=?1 AND milestone_ref=?2 AND recipient_task_id=?3 AND status='active'",
            params![message_id, milestone_ref, recipient_task_id],
        )? == 1)
    }

    fn assert_milestone_scope_ref(&self, reference: &str) -> Result<()> {
        let Some(relative) = reference.strip_prefix(&self.namespace) else {
            bail!("TASK_PROJECT_MISMATCH");
        };
        if relative.is_empty() {
            bail!("TASK_BLACKBOARD_INVALID: milestone_ref is required");
        }
        crate::tasks::relative_path(relative)?;
        Ok(())
    }

    /// Read one bounded page of payload-free messages for a resolved hierarchy scope.
    pub fn blackboard_page(
        &self,
        milestone_ref: &str,
        task_id: Option<&str>,
        subtask_ref: Option<&str>,
        topic: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<BlackboardPage> {
        self.blackboard_page_for_gate(BlackboardPageRequest {
            milestone_ref,
            task_id,
            subtask_ref,
            topic,
            gate: None,
            limit,
            offset,
        })
    }

    /// Read one bounded page, optionally selecting messages for one workflow gate.
    /// Task scope includes messages posted by the task and messages addressed to it.
    pub fn blackboard_page_for_gate(
        &self,
        request: BlackboardPageRequest<'_>,
    ) -> Result<BlackboardPage> {
        let BlackboardPageRequest {
            milestone_ref,
            task_id,
            subtask_ref,
            topic,
            gate,
            limit,
            offset,
        } = request;
        self.assert_milestone_scope_ref(milestone_ref)?;
        if let Some(topic) = topic {
            validate_blackboard_topic(topic)?;
        }
        if let Some(task_id) = task_id {
            self.assert_id(task_id)?;
            let task = self.inspect(task_id)?;
            if self.milestone_scope_ref(&task.milestone_ref)? != milestone_ref {
                bail!("TASK_BLACKBOARD_SCOPE_MISMATCH");
            }
            if let Some(subtask_ref) = subtask_ref {
                let exists: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_subtasks WHERE task_id=?1 AND subtask_ref=?2)",
                    params![task_id, subtask_ref],
                    |row| row.get(0),
                )?;
                if !exists {
                    bail!("TASK_SUBTASK_NOT_FOUND");
                }
            }
        } else if subtask_ref.is_some() {
            bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
        }
        if subtask_ref.is_some_and(|reference| reference.trim().is_empty()) {
            bail!("TASK_BLACKBOARD_SUBTASK_REQUIRED");
        }
        let limit = limit.unwrap_or(10);
        if limit == 0 {
            bail!("TASK_BLACKBOARD_INVALID: limit must be greater than zero");
        }
        let limit = limit.min(50);
        let offset = offset.unwrap_or(0);
        let sql_limit = i64::try_from(limit.checked_add(1).context("blackboard limit overflow")?)?;
        let sql_offset = i64::try_from(offset)?;
        let mut statement = self.connection.prepare(
            "SELECT id,milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,created_at \
             FROM task_blackboard \
             WHERE milestone_ref=?1 \
               AND ((?2 IS NULL AND task_id IS NULL AND recipient_task_id IS NULL) OR \
                    (?2 IS NOT NULL AND ( \
                     (task_id=?2 AND \
                      ((?3 IS NULL AND subtask_ref IS NULL) OR (?3 IS NOT NULL AND subtask_ref=?3))) OR \
                     (?3 IS NULL AND recipient_task_id=?2)))) \
               AND (?4 IS NULL OR topic=?4) \
               AND (?5 IS NULL OR gate=?5) \
             ORDER BY created_at DESC,id DESC LIMIT ?6 OFFSET ?7",
        )?;
        let mut messages = statement
            .query_map(
                params![
                    milestone_ref,
                    task_id,
                    subtask_ref,
                    topic,
                    gate,
                    sql_limit,
                    sql_offset
                ],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        recipient_task_id: row.get(3)?,
                        subtask_ref: row.get(4)?,
                        gate: row.get(5)?,
                        author: row.get(6)?,
                        topic: row.get(7)?,
                        payload: None,
                        created_at: row.get(8)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = messages.len() > limit;
        if has_more {
            messages.pop();
        }
        Ok(BlackboardPage {
            messages,
            pagination: BlackboardPagination {
                limit,
                offset,
                has_more,
                next_offset: has_more.then(|| offset.saturating_add(limit)),
            },
        })
    }

    /// Read one message, including its payload, if it belongs to this configured project.
    pub fn blackboard_inspect(&self, id: u64) -> Result<Option<BlackboardMessage>> {
        let message = self
            .connection
            .query_row(
                "SELECT id,milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,payload,created_at \
                 FROM task_blackboard WHERE id=?1",
                [id],
                |row| {
                    Ok(BlackboardMessage {
                        id: row.get(0)?,
                        milestone_ref: row.get(1)?,
                        task_id: row.get(2)?,
                        recipient_task_id: row.get(3)?,
                        subtask_ref: row.get(4)?,
                        gate: row.get(5)?,
                        author: row.get(6)?,
                        topic: row.get(7)?,
                        payload: Some(row.get(8)?),
                        created_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        let Some(message) = message else {
            return Ok(None);
        };
        if !message.milestone_ref.starts_with(&self.namespace)
            || message
                .task_id
                .as_deref()
                .is_some_and(|task_id| !self.task_id_in_project(task_id))
        {
            return Ok(None);
        }
        Ok(Some(message))
    }

    /// Read messages posted by or addressed to a task in insertion order.
    ///
    /// Results can be filtered by topic and bounded by an optional limit.
    pub fn blackboard_read(
        &self,
        task_id: &str,
        topic: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<BlackboardMessage>> {
        self.assert_id(task_id)?;
        if let Some(topic) = topic {
            validate_blackboard_topic(topic)?;
        }
        let limit = limit.map(i64::try_from).transpose()?.unwrap_or(i64::MAX);
        let mut statement = self.connection.prepare(
            "SELECT id,milestone_ref,task_id,recipient_task_id,subtask_ref,gate,author,topic,payload,created_at FROM task_blackboard \
             WHERE (task_id=?1 OR recipient_task_id=?1) \
               AND (?2 IS NULL OR topic=?2) ORDER BY created_at,id LIMIT ?3",
        )?;
        let messages = statement
            .query_map(params![task_id, topic, limit], |row| {
                Ok(BlackboardMessage {
                    id: row.get(0)?,
                    milestone_ref: row.get(1)?,
                    task_id: row.get(2)?,
                    recipient_task_id: row.get(3)?,
                    subtask_ref: row.get(4)?,
                    gate: row.get(5)?,
                    author: row.get(6)?,
                    topic: row.get(7)?,
                    payload: Some(row.get(8)?),
                    created_at: row.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(messages)
    }
    /// Delete all transient messages for one task.
    pub fn blackboard_clear(&self, task_id: &str) -> Result<usize> {
        self.assert_id(task_id)?;
        clear_blackboard(&self.connection, task_id)
    }
}
