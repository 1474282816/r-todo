use anyhow::{Context, Result};
use chrono::Utc;
use rtools_domain::{Priority, Task, TaskDraft, TaskPatch};
use rusqlite::Connection;
use uuid::Uuid;

use crate::db;

pub struct TaskService<'a> {
    conn: &'a Connection,
}

impl<'a> TaskService<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn list(&self) -> Result<Vec<Task>> {
        Ok(db::list_tasks(self.conn, false)?)
    }

    pub fn get(&self, id: &str) -> Result<Option<Task>> {
        Ok(db::get_task(self.conn, id)?)
    }

    pub fn create(&self, draft: TaskDraft) -> Result<Task> {
        let now = Utc::now();
        let min_order: Option<f64> = self.conn.query_row(
            "SELECT MIN(sort_order) FROM tasks WHERE deleted_at IS NULL AND completed_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        let task = Task {
            id: Uuid::new_v4().simple().to_string(),
            title: draft.title.trim().to_string(),
            priority: draft.priority,
            remind_at: draft.remind_at,
            remind_offset_min: draft.remind_offset_min,
            repeat_rule: draft.repeat_rule,
            sort_order: min_order.unwrap_or(0.0) - 1000.0,
            completed_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            revision: 0,
        };
        db::upsert_task(self.conn, &task)?;
        Ok(task)
    }

    pub fn update(&self, id: &str, patch: TaskPatch) -> Result<Task> {
        Ok(db::patch_task(self.conn, id, patch)?)
    }

    pub fn complete(&self, id: &str, completed: bool) -> Result<Task> {
        let patch = TaskPatch {
            completed_at: Some(completed.then(Utc::now)),
            ..Default::default()
        };
        self.update(id, patch)
    }

    pub fn delete(&self, id: &str) -> Result<Task> {
        self.update(
            id,
            TaskPatch {
                deleted_at: Some(Some(Utc::now())),
                ..Default::default()
            },
        )
    }

    pub fn reorder(&self, ids: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (index, id) in ids.iter().enumerate() {
            let task = db::get_task(&tx, id)?.with_context(|| format!("task not found: {id}"))?;
            let mut task = task;
            task.sort_order = (index + 1) as f64 * 1000.0;
            task.updated_at = Utc::now();
            db::upsert_task(&tx, &task)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn default_priority(&self) -> Priority {
        Priority::Medium
    }
}
