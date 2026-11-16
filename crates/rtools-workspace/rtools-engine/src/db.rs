use anyhow::{Context, Result};
use rtools_domain::{Task, TaskPatch};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );
        INSERT OR IGNORE INTO schema_meta(key, value) VALUES('schema_version', '1');

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY NOT NULL,
            title TEXT NOT NULL,
            priority TEXT NOT NULL,
            remind_at TEXT,
            remind_offset_min INTEGER NOT NULL DEFAULT 15,
            repeat_rule TEXT,
            sort_order REAL NOT NULL DEFAULT 0,
            completed_at TEXT,
            deleted_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            revision INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_visible ON tasks(deleted_at, completed_at);
        CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at);

        CREATE TABLE IF NOT EXISTS outbox (
            operation_id TEXT PRIMARY KEY NOT NULL,
            task_id TEXT NOT NULL,
            base_revision INTEGER NOT NULL DEFAULT 0,
            patch_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_outbox_created ON outbox(created_at);

        CREATE TABLE IF NOT EXISTS sync_state (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS reminders (
            task_id TEXT PRIMARY KEY NOT NULL,
            due_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_reminders_due ON reminders(due_at);
        "#,
    )?;
    let version: String = conn.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version != "1" {
        anyhow::bail!("unsupported schema version: {version}");
    }
    Ok(())
}

pub fn integrity_check(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?)
}

pub fn backup(conn: &Connection, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    conn.backup(rusqlite::DatabaseName::Main, path, None)?;
    Ok(())
}

fn sql_datetime(
    value: Option<String>,
    column: usize,
) -> rusqlite::Result<Option<chrono::DateTime<chrono::Utc>>> {
    value
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|v| v.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        column,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
        })
        .transpose()
}

fn parse_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let priority: String = row.get(2)?;
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        priority: match priority.as_str() {
            "high" => rtools_domain::Priority::High,
            "low" => rtools_domain::Priority::Low,
            _ => rtools_domain::Priority::Medium,
        },
        remind_at: sql_datetime(row.get(3)?, 3)?,
        remind_offset_min: row.get(4)?,
        repeat_rule: row.get(5)?,
        sort_order: row.get(6)?,
        completed_at: sql_datetime(row.get(7)?, 7)?,
        deleted_at: sql_datetime(row.get(8)?, 8)?,
        created_at: sql_datetime(row.get(9)?, 9)?.unwrap_or_default(),
        updated_at: sql_datetime(row.get(10)?, 10)?.unwrap_or_default(),
        revision: row.get(11)?,
    })
}

const TASK_COLUMNS: &str =
    "id,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,completed_at,deleted_at,created_at,updated_at,revision";

pub fn list_tasks(conn: &Connection, include_deleted: bool) -> Result<Vec<Task>> {
    let sql = if include_deleted {
        format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY sort_order ASC")
    } else {
        format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE deleted_at IS NULL ORDER BY CASE WHEN completed_at IS NULL THEN 0 ELSE 1 END, sort_order ASC, completed_at DESC"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], parse_task_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_task(conn: &Connection, id: &str) -> Result<Option<Task>> {
    Ok(conn
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
            [id],
            parse_task_row,
        )
        .optional()?)
}

pub fn upsert_task(conn: &Connection, task: &Task) -> Result<()> {
    task.validate()?;
    conn.execute(
        "INSERT INTO tasks(id,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,completed_at,deleted_at,created_at,updated_at,revision)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(id) DO UPDATE SET title=excluded.title,priority=excluded.priority,
         remind_at=excluded.remind_at,remind_offset_min=excluded.remind_offset_min,
         repeat_rule=excluded.repeat_rule,sort_order=excluded.sort_order,
         completed_at=excluded.completed_at,deleted_at=excluded.deleted_at,
         created_at=excluded.created_at,updated_at=excluded.updated_at,revision=excluded.revision",
        params![
            task.id,
            task.title,
            match task.priority {
                rtools_domain::Priority::High => "high",
                rtools_domain::Priority::Medium => "medium",
                rtools_domain::Priority::Low => "low",
            },
            task.remind_at.map(|v| v.to_rfc3339()),
            task.remind_offset_min,
            task.repeat_rule,
            task.sort_order,
            task.completed_at.map(|v| v.to_rfc3339()),
            task.deleted_at.map(|v| v.to_rfc3339()),
            task.created_at.to_rfc3339(),
            task.updated_at.to_rfc3339(),
            task.revision,
        ],
    )?;
    refresh_reminder(conn, task)?;
    Ok(())
}

fn refresh_reminder(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute("DELETE FROM reminders WHERE task_id=?1", [&task.id])?;
    if task.deleted_at.is_none() && task.completed_at.is_none() {
        if let Some(remind_at) = task.remind_at {
            let due = remind_at - chrono::Duration::minutes(task.remind_offset_min);
            conn.execute(
                "INSERT INTO reminders(task_id,due_at) VALUES(?1,?2)",
                params![task.id, due.to_rfc3339()],
            )?;
        }
    }
    Ok(())
}

pub fn patch_task(conn: &Connection, id: &str, patch: TaskPatch) -> Result<Task> {
    let mut task = get_task(conn, id)?.context("task not found")?;
    patch.apply(&mut task, chrono::Utc::now());
    upsert_task(conn, &task)?;
    Ok(task)
}

pub fn import_tasks(conn: &mut Connection, tasks: Vec<Task>) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut count = 0;
    for task in tasks {
        task.validate()?;
        upsert_task(&tx, &task)?;
        count += 1;
    }
    tx.commit()?;
    Ok(count)
}

pub fn enqueue_operation(
    conn: &Connection,
    operation_id: &str,
    task_id: &str,
    base_revision: i64,
    patch_json: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO outbox(operation_id,task_id,base_revision,patch_json,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![operation_id, task_id, base_revision, patch_json, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PendingOperation {
    pub operation_id: String,
    pub task_id: String,
    pub base_revision: i64,
    pub patch_json: String,
}

pub fn list_operations(conn: &Connection) -> Result<Vec<PendingOperation>> {
    let mut stmt = conn.prepare(
        "SELECT operation_id,task_id,base_revision,patch_json FROM outbox ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PendingOperation {
            operation_id: row.get(0)?,
            task_id: row.get(1)?,
            base_revision: row.get(2)?,
            patch_json: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn remove_operation(conn: &Connection, operation_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM outbox WHERE operation_id=?1",
        [operation_id],
    )?;
    Ok(())
}

pub fn mark_operation_error(conn: &Connection, operation_id: &str, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE outbox SET attempts=attempts+1,last_error=?1 WHERE operation_id=?2",
        params![error, operation_id],
    )?;
    Ok(())
}

pub fn get_sync_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM sync_state WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()?)
}

pub fn set_sync_value(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn pending_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn import_legacy(conn: &mut Connection, content: &str) -> Result<usize> {
    let tasks = rtools_domain::parse_rtodo_json(content).map_err(anyhow::Error::msg)?;
    import_tasks(conn, tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn task_roundtrip_and_soft_delete() {
        let c = mem();
        let now = Utc::now();
        let t = Task {
            id: "t".into(),
            title: "测试".into(),
            priority: rtools_domain::Priority::High,
            remind_at: None,
            remind_offset_min: 15,
            repeat_rule: None,
            sort_order: 1.0,
            completed_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            revision: 0,
        };
        upsert_task(&c, &t).unwrap();
        assert_eq!(list_tasks(&c, false).unwrap().len(), 1);
        let mut deleted = t.clone();
        deleted.deleted_at = Some(now);
        upsert_task(&c, &deleted).unwrap();
        assert!(list_tasks(&c, false).unwrap().is_empty());
        assert_eq!(list_tasks(&c, true).unwrap().len(), 1);
    }

    #[test]
    fn legacy_import_is_transactional() {
        let mut c = mem();
        let content = r#"{"format":"r-todo/todo","version":1,"todos":[
          {"id":"x","title":"测试","priority":"medium","remind_offset_min":15,
          "sort_order":0,"deleted":false,"created_at":"2026-01-01T00:00:00Z",
          "updated_at":"2026-01-01T00:00:00Z"}]}"#;
        assert_eq!(import_legacy(&mut c, content).unwrap(), 1);
        assert_eq!(list_tasks(&c, false).unwrap().len(), 1);
    }
}
