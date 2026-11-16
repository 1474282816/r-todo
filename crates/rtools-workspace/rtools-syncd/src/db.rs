use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS server_meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );
        INSERT OR IGNORE INTO server_meta(key,value) VALUES('schema_version','1');
        INSERT OR IGNORE INTO server_meta(key,value) VALUES('server_name','rtools');

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
        CREATE TABLE IF NOT EXISTS changes (
            cursor INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            changed_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_changes_task ON changes(task_id, cursor);

        CREATE TABLE IF NOT EXISTS operations (
            operation_id TEXT PRIMARY KEY NOT NULL,
            device_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            response_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS devices (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            token_hash TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            last_seen_at TEXT,
            revoked_at TEXT
        );

        CREATE TABLE IF NOT EXISTS pairing_codes (
            code_hash TEXT PRIMARY KEY NOT NULL,
            expires_at TEXT NOT NULL,
            used_at TEXT
        );
        "#,
    )?;
    Ok(())
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(code.trim().to_ascii_uppercase().as_bytes()))
}

pub fn create_pairing_code(conn: &Connection, code: &str, minutes: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO pairing_codes(code_hash,expires_at) VALUES(?1,?2)",
        params![
            hash_code(code),
            (Utc::now() + chrono::Duration::minutes(minutes)).to_rfc3339()
        ],
    )?;
    Ok(())
}

pub fn consume_pairing_code(conn: &Connection, code: &str) -> Result<bool> {
    let hash = hash_code(code);
    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    let expiry: Option<String> = tx
        .query_row(
            "SELECT expires_at FROM pairing_codes WHERE code_hash=?1 AND used_at IS NULL",
            [&hash],
            |r| r.get(0),
        )
        .optional()?;
    let Some(expiry) = expiry else {
        return Ok(false);
    };
    let expiry = chrono::DateTime::parse_from_rfc3339(&expiry)?.with_timezone(&Utc);
    if expiry < Utc::now() {
        return Ok(false);
    }
    tx.execute(
        "UPDATE pairing_codes SET used_at=?1 WHERE code_hash=?2 AND used_at IS NULL",
        params![now, hash],
    )?;
    tx.commit()?;
    Ok(true)
}

pub fn add_device(conn: &Connection, id: &str, name: &str, token: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO devices(id,name,token_hash,created_at) VALUES(?1,?2,?3,?4)",
        params![id, name, hash_token(token), Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn authenticate(conn: &Connection, device_id: &str, token: &str) -> Result<bool> {
    let hash = hash_token(token);
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE devices SET last_seen_at=?1 WHERE id=?2 AND token_hash=?3 AND revoked_at IS NULL",
        params![now, device_id, hash],
    )?;
    Ok(updated == 1)
}

pub fn revoke_device(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE devices SET revoked_at=?1 WHERE id=?2 AND revoked_at IS NULL",
        params![Utc::now().to_rfc3339(), id],
    )? == 1)
}

pub fn current_cursor(conn: &Connection) -> Result<i64> {
    Ok(
        conn.query_row("SELECT COALESCE(MAX(cursor),0) FROM changes", [], |r| {
            r.get(0)
        })?,
    )
}

pub fn record_change(tx: &rusqlite::Transaction<'_>, task_id: &str, revision: i64) -> Result<i64> {
    tx.execute(
        "INSERT INTO changes(task_id,revision,changed_at) VALUES(?1,?2,?3)",
        params![task_id, revision, Utc::now().to_rfc3339()],
    )?;
    Ok(tx.last_insert_rowid())
}

pub fn backup(conn: &Connection, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    conn.backup(rusqlite::DatabaseName::Main, path, None)?;
    Ok(())
}

pub fn integrity_check(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?)
}

pub fn server_name(conn: &Connection) -> Result<String> {
    Ok(conn.query_row(
        "SELECT value FROM server_meta WHERE key='server_name'",
        [],
        |r| r.get(0),
    )?)
}

pub fn create_task_tx(tx: &rusqlite::Transaction<'_>, task: &rtools_domain::Task) -> Result<i64> {
    task.validate()?;
    let revision = 1;
    tx.execute(
        "INSERT INTO tasks(id,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,completed_at,deleted_at,created_at,updated_at,revision)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            task.id,
            task.title,
            priority_str(task.priority),
            task.remind_at.map(|v| v.to_rfc3339()),
            task.remind_offset_min,
            task.repeat_rule,
            task.sort_order,
            task.completed_at.map(|v| v.to_rfc3339()),
            task.deleted_at.map(|v| v.to_rfc3339()),
            task.created_at.to_rfc3339(),
            task.updated_at.to_rfc3339(),
            revision
        ],
    )?;
    record_change(tx, &task.id, revision)?;
    Ok(revision)
}

pub fn update_task_tx(
    tx: &rusqlite::Transaction<'_>,
    current: &rtools_domain::Task,
    patch: rtools_domain::TaskPatch,
) -> Result<rtools_domain::Task> {
    let mut task = current.clone();
    patch.apply(&mut task, Utc::now());
    task.revision = current.revision + 1;
    task.validate()?;
    tx.execute(
        "UPDATE tasks SET title=?1,priority=?2,remind_at=?3,remind_offset_min=?4,
         repeat_rule=?5,sort_order=?6,completed_at=?7,deleted_at=?8,updated_at=?9,revision=?10 WHERE id=?11",
        params![
            task.title,
            priority_str(task.priority),
            task.remind_at.map(|v| v.to_rfc3339()),
            task.remind_offset_min,
            task.repeat_rule,
            task.sort_order,
            task.completed_at.map(|v| v.to_rfc3339()),
            task.deleted_at.map(|v| v.to_rfc3339()),
            task.updated_at.to_rfc3339(),
            task.revision,
            task.id
        ],
    )?;
    record_change(tx, &task.id, task.revision)?;
    Ok(task)
}

pub fn get_task(conn: &Connection, id: &str) -> Result<Option<rtools_domain::Task>> {
    Ok(conn
        .query_row(
            "SELECT id,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,completed_at,deleted_at,created_at,updated_at,revision FROM tasks WHERE id=?1",
            [id],
            parse_task,
        )
        .optional()?)
}

pub fn list_tasks(conn: &Connection, after: i64, limit: usize) -> Result<Vec<rtools_domain::Task>> {
    let mut stmt = conn.prepare(
        "SELECT t.id,t.title,t.priority,t.remind_at,t.remind_offset_min,t.repeat_rule,
         t.sort_order,t.completed_at,t.deleted_at,t.created_at,t.updated_at,t.revision
         FROM tasks t JOIN (
           SELECT task_id, MAX(cursor) AS cursor FROM changes WHERE cursor>?1 GROUP BY task_id
         ) c ON c.task_id=t.id ORDER BY c.cursor ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![after, limit as i64], parse_task)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn page_cursor(conn: &Connection, after: i64, limit: usize) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT COALESCE(MAX(cursor),?1) FROM (SELECT cursor FROM changes WHERE cursor>?1 ORDER BY cursor ASC LIMIT ?2)",
            params![after, limit as i64],
            |r| r.get(0),
        )?)
}

pub fn snapshot(conn: &Connection) -> Result<Vec<rtools_domain::Task>> {
    let mut stmt = conn.prepare(
        "SELECT id,title,priority,remind_at,remind_offset_min,repeat_rule,sort_order,
         completed_at,deleted_at,created_at,updated_at,revision FROM tasks ORDER BY sort_order ASC",
    )?;
    let rows = stmt.query_map([], parse_task)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn cached_operation(conn: &Connection, operation_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT response_json FROM operations WHERE operation_id=?1",
            [operation_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn store_operation(
    tx: &rusqlite::Transaction<'_>,
    operation_id: &str,
    device_id: &str,
    task_id: &str,
    revision: i64,
    response_json: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO operations(operation_id,device_id,task_id,revision,response_json,created_at)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            operation_id,
            device_id,
            task_id,
            revision,
            response_json,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

fn priority_str(p: rtools_domain::Priority) -> &'static str {
    match p {
        rtools_domain::Priority::High => "high",
        rtools_domain::Priority::Medium => "medium",
        rtools_domain::Priority::Low => "low",
    }
}

fn parse_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<rtools_domain::Task> {
    let priority: String = row.get(2)?;
    let parse = |v: Option<String>| -> rusqlite::Result<Option<chrono::DateTime<Utc>>> {
        v.map(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|x| x.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))
        })
        .transpose()
    };
    Ok(rtools_domain::Task {
        id: row.get(0)?,
        title: row.get(1)?,
        priority: match priority.as_str() {
            "high" => rtools_domain::Priority::High,
            "low" => rtools_domain::Priority::Low,
            _ => rtools_domain::Priority::Medium,
        },
        remind_at: parse(row.get(3)?)?,
        remind_offset_min: row.get(4)?,
        repeat_rule: row.get(5)?,
        sort_order: row.get(6)?,
        completed_at: parse(row.get(7)?)?,
        deleted_at: parse(row.get(8)?)?,
        created_at: parse(row.get(9)?)?
            .context("missing created_at")
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
        updated_at: parse(row.get(10)?)?
            .context("missing updated_at")
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
        revision: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_code_is_single_use() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        create_pairing_code(&c, "ABCD-EFGH", 10).unwrap();
        assert!(consume_pairing_code(&c, "abcd-efgh").unwrap());
        assert!(!consume_pairing_code(&c, "ABCD-EFGH").unwrap());
    }

    #[test]
    fn device_authentication_and_revoke() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        add_device(&c, "d1", "desktop", "secret").unwrap();
        assert!(authenticate(&c, "d1", "secret").unwrap());
        assert!(!authenticate(&c, "d1", "wrong").unwrap());
        revoke_device(&c, "d1").unwrap();
        assert!(!authenticate(&c, "d1", "secret").unwrap());
    }
}
