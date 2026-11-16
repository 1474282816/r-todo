use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;
use rtools_domain::{Task, TaskPatch};
use rtools_sync_proto::{Operation, OperationBatch, OperationResult};

use crate::{
    db,
    sync_client::{local_to_patch, SyncClient},
};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SyncSummary {
    pub paired: bool,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub server_url: Option<String>,
    pub cursor: i64,
    pub pending: i64,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
}

pub fn summary(conn: &Connection) -> Result<SyncSummary> {
    Ok(SyncSummary {
        paired: db::get_sync_value(conn, "token")?.is_some(),
        device_id: db::get_sync_value(conn, "device_id")?,
        device_name: db::get_sync_value(conn, "device_name")?,
        server_url: db::get_sync_value(conn, "server_url")?,
        cursor: db::get_sync_value(conn, "cursor")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        pending: db::pending_count(conn)?,
        last_sync_at: db::get_sync_value(conn, "last_sync_at")?,
        last_error: db::get_sync_value(conn, "last_error")?,
    })
}

pub async fn pair(
    db_path: &std::path::Path,
    server_url: &str,
    code: &str,
    device_name: &str,
) -> Result<SyncSummary> {
    let client = SyncClient::new(server_url)?;
    let response = client.pair(code, device_name).await?;
    let conn = db::open(db_path)?;
    let summary = persist_pair(&conn, server_url, device_name, &response)?;
    Ok(summary)
}

fn persist_pair(
    conn: &Connection,
    server_url: &str,
    device_name: &str,
    response: &rtools_sync_proto::PairResponse,
) -> Result<SyncSummary> {
    db::set_sync_value(conn, "server_url", server_url.trim_end_matches('/'))?;
    db::set_sync_value(conn, "device_id", &response.device_id)?;
    db::set_sync_value(conn, "device_name", device_name)?;
    db::set_sync_value(conn, "token", &response.token)?;
    db::set_sync_value(conn, "cursor", "0")?;
    db::set_sync_value(conn, "last_error", "")?;
    summary(conn)
}

pub fn logout(conn: &Connection) -> Result<SyncSummary> {
    for key in ["token", "device_id", "device_name", "server_url", "cursor", "last_error"] {
        conn.execute("DELETE FROM sync_state WHERE key=?1", [key])?;
    }
    summary(conn)
}

pub async fn sync_now(db_path: &std::path::Path) -> Result<SyncSummary> {
    let conn = db::open(db_path)?;
    let Some(server_url) = db::get_sync_value(&conn, "server_url")? else {
        anyhow::bail!("尚未配对同步服务");
    };
    let Some(token) = db::get_sync_value(&conn, "token")? else {
        anyhow::bail!("尚未配对同步服务");
    };
    let Some(device_id) = db::get_sync_value(&conn, "device_id")? else {
        anyhow::bail!("缺少设备标识");
    };
    let cursor = db::get_sync_value(&conn, "cursor")?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let client = SyncClient::new(&server_url)?.with_token(token);
    let pending = db::list_operations(&conn)?;
    let mut accepted = Vec::new();
    for item in pending {
        let patch: TaskPatch =
            serde_json::from_str(&item.patch_json).context("invalid outbox patch")?;
        let task = db::get_task(&conn, &item.task_id)?;
        let operation = if let Some(task) = task.as_ref().filter(|_| item.base_revision == 0) {
            Operation {
                operation_id: item.operation_id.clone(),
                task_id: item.task_id.clone(),
                base_revision: item.base_revision,
                client_updated_at: Utc::now(),
                create: Some(task.clone()),
                patch: None,
            }
        } else {
            Operation {
                operation_id: item.operation_id.clone(),
                task_id: item.task_id.clone(),
                base_revision: item.base_revision,
                client_updated_at: Utc::now(),
                create: None,
                patch: Some(patch),
            }
        };
        accepted.push(operation);
    }
    drop(conn);
    if !accepted.is_empty() {
        let batch = OperationBatch {
            device_id: device_id.clone(),
            operations: accepted,
        };
        let response = client.push(&device_id, batch).await?;
        let mut conn = db::open(db_path)?;
        apply_operation_results(&mut conn, response.results)?;
    }

    let conn = db::open(db_path)?;
    let changes = client.pull(&device_id, cursor).await?;
    drop(conn);
    let conn = db::open(db_path)?;
    for task in changes.tasks {
        db::upsert_task(&conn, &task)?;
    }
    db::set_sync_value(&conn, "cursor", &changes.cursor.to_string())?;
    db::set_sync_value(&conn, "last_sync_at", &Utc::now().to_rfc3339())?;
    db::set_sync_value(&conn, "last_error", "")?;
    summary(&conn)
}

fn apply_operation_results(conn: &mut Connection, results: Vec<OperationResult>) -> Result<()> {
    for result in results {
        if let Some(task) = result.task {
            db::upsert_task(conn, &task)?;
        }
        if result.accepted || result.error.as_deref() == Some("TASK_NOT_FOUND") {
            db::remove_operation(conn, &result.operation_id)?;
        } else if let Some(error) = result.error {
            db::mark_operation_error(conn, &result.operation_id, &error)?;
        }
    }
    Ok(())
}

pub fn enqueue_task_update(conn: &Connection, task: &Task, operation_id: &str) -> Result<()> {
    let patch = local_to_patch(task);
    db::enqueue_operation(
        conn,
        operation_id,
        &task.id,
        task.revision,
        &serde_json::to_string(&patch)?,
    )
}

pub fn merge_snapshot(conn: &mut Connection, tasks: Vec<Task>) -> Result<()> {
    for task in tasks {
        let local = db::get_task(conn, &task.id)?;
        if local.as_ref().is_none_or(|local| task.revision >= local.revision) {
            db::upsert_task(conn, &task)?;
        }
    }
    Ok(())
}

