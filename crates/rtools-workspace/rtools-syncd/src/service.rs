use anyhow::Result;
use chrono::Utc;
use rtools_domain::Task;
use rtools_sync_proto::{
    ChangePage, Operation, OperationBatch, OperationResponse, OperationResult, Snapshot,
};
use rusqlite::{Connection, OptionalExtension};

use crate::db;

pub fn apply_batch(conn: &mut Connection, batch: OperationBatch) -> Result<OperationResponse> {
    let tx = conn.transaction()?;
    let mut results = Vec::with_capacity(batch.operations.len());
    for operation in batch.operations {
        if let Some(cached) = db::cached_operation(&tx, &operation.operation_id)? {
            results.push(serde_json::from_str(&cached)?);
            continue;
        }
        let result = apply_one(&tx, &batch.device_id, operation)?;
        db::store_operation(
            &tx,
            &result.operation_id,
            &batch.device_id,
            &result.task_id,
            result.revision,
            &serde_json::to_string(&result)?,
        )?;
        results.push(result);
    }
    let cursor = db::current_cursor(&tx)?;
    tx.commit()?;
    Ok(OperationResponse { cursor, results })
}

fn apply_one(
    tx: &rusqlite::Transaction<'_>,
    _device_id: &str,
    operation: Operation,
) -> Result<OperationResult> {
    if let Some(create) = operation.create {
        let existing = db::get_task(tx, &operation.task_id)?;
        if let Some(existing) = existing {
            return Ok(OperationResult {
                operation_id: operation.operation_id,
                task_id: operation.task_id,
                revision: existing.revision,
                accepted: true,
                task: Some(existing),
                error: None,
            });
        }
        let mut task = create;
        task.id = operation.task_id.clone();
        let revision = db::create_task_tx(tx, &task)?;
        let mut stored = db::get_task(tx, &task.id)?.unwrap();
        stored.revision = revision;
        return Ok(OperationResult {
            operation_id: operation.operation_id,
            task_id: task.id,
            revision,
            accepted: true,
            task: Some(stored),
            error: None,
        });
    }
    let Some(current) = db::get_task(tx, &operation.task_id)? else {
        return Ok(OperationResult {
            operation_id: operation.operation_id,
            task_id: operation.task_id,
            revision: 0,
            accepted: false,
            task: None,
            error: Some("TASK_NOT_FOUND".into()),
        });
    };
    let Some(patch) = operation.patch else {
        return Ok(OperationResult {
            operation_id: operation.operation_id,
            task_id: operation.task_id,
            revision: current.revision,
            accepted: false,
            task: Some(current),
            error: Some("EMPTY_OPERATION".into()),
        });
    };
    let updated = db::update_task_tx(tx, &current, patch)?;
    Ok(OperationResult {
        operation_id: operation.operation_id,
        task_id: updated.id.clone(),
        revision: updated.revision,
        accepted: true,
        task: Some(updated),
        error: None,
    })
}

pub fn changes(conn: &Connection, after: i64, limit: usize) -> Result<ChangePage> {
    let tasks = db::list_tasks(conn, after, limit)?;
    let cursor = db::page_cursor(conn, after, limit)?;
    let total = db::current_cursor(conn)?;
    let deleted_ids = tasks
        .iter()
        .filter(|t| t.deleted_at.is_some())
        .map(|t| t.id.clone())
        .collect();
    Ok(ChangePage {
        cursor,
        tasks,
        deleted_ids,
        has_more: cursor < total,
    })
}

pub fn snapshot(conn: &Connection) -> Result<Snapshot> {
    Ok(Snapshot {
        cursor: db::current_cursor(conn)?,
        tasks: db::snapshot(conn)?,
    })
}

pub fn seed_task(conn: &mut Connection, task: Task) -> Result<i64> {
    let tx = conn.transaction()?;
    let rev = db::create_task_tx(&tx, &task)?;
    tx.commit()?;
    Ok(rev)
}

pub fn device_name(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT name FROM devices WHERE id=?1", [id], |r| r.get(0))
        .optional()?)
}

pub fn touch_snapshot_time(_conn: &Connection) -> String {
    Utc::now().to_rfc3339()
}
