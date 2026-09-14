//! MCP 工具数据层：rusqlite 直连应用 SQLite（对齐 src-tauri/src/db.rs migrations 1-3 与前端 db.ts 语义）。
//!
//! 变更路径均写入 pending_ops 离线队列（payload 用 RemoteTask 字段名，不含 user ——
//! 由前端 sync.ts flushPendingOps 在推送时注入当前登录用户）。

use std::path::Path;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskJson {
    pub id: String,
    pub title: String,
    pub priority: String,
    pub remind_at: Option<String>,
    pub remind_offset_min: i64,
    pub repeat_rule: Option<String>,
    pub sort_order: f64,
    pub deleted: bool,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn open_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// 用与应用一致的 schema 建表（对齐 src-tauri/src/db.rs migrations 1-3）。
///
/// SQLite 不支持 `ADD COLUMN IF NOT EXISTS`，且真实应用库已迁移过
/// sort_order/deleted/deleted_at，重复 ADD 会报 duplicate column name。
/// 因此：CREATE TABLE IF NOT EXISTS 直接写全 11 列；随后用 PRAGMA table_info 收集
/// 既有列名，仅对缺失的这三列补 ALTER TABLE —— 两条路径（新库/已迁移库）收敛同一 schema。
///
/// 生产路径的应用库已由 tauri-plugin-sql migrations 建好，此处仅测试与
/// 独立/工具场景需要 bootstrap，故允许非测试构建下的 dead_code。
#[allow(dead_code)]
pub fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK(length(title) > 0),
    priority TEXT NOT NULL CHECK(priority IN ('high', 'medium', 'low')),
    remind_at TEXT,
    remind_offset_min INTEGER NOT NULL DEFAULT 15,
    repeat_rule TEXT,
    sort_order REAL NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    deleted_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_completed ON tasks(completed_at);
CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at);
CREATE INDEX IF NOT EXISTS idx_tasks_deleted ON tasks(deleted);
CREATE TABLE IF NOT EXISTS pending_ops (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    op_type TEXT NOT NULL CHECK(op_type IN ('create', 'update', 'delete')),
    collection TEXT NOT NULL DEFAULT 'tasks',
    record_id TEXT NOT NULL,
    payload TEXT,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX IF NOT EXISTS idx_pending_ops_fifo ON pending_ops(id);
"#,
    )
    .map_err(|e| e.to_string())?;
    // 收集既有列名（PRAGMA table_info 的第 2 列是列名）
    let existing: Vec<String> = conn
        .prepare("PRAGMA table_info(tasks)")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (col, ddl) in [
        (
            "sort_order",
            "ALTER TABLE tasks ADD COLUMN sort_order REAL NOT NULL DEFAULT 0",
        ),
        (
            "deleted",
            "ALTER TABLE tasks ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0",
        ),
        ("deleted_at", "ALTER TABLE tasks ADD COLUMN deleted_at TEXT"),
    ] {
        if !existing.iter().any(|c| c == col) {
            conn.execute_batch(ddl).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 变更入队（payload 用 RemoteTask 字段名，不含 user）
fn push_pending(
    conn: &Connection,
    op_type: &str,
    record_id: &str,
    payload: Option<&serde_json::Value>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO pending_ops (op_type, collection, record_id, payload, created_at)
         VALUES (?1, 'tasks', ?2, ?3, ?4)",
        rusqlite::params![
            op_type,
            record_id,
            payload.map(|p| p.to_string()),
            chrono::Utc::now().timestamp_millis()
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 未完成任务里最小的 sort_order；空表返回 0（对齐前端 db.ts getMinSortOrder）
fn min_sort_order(conn: &Connection) -> Result<f64, String> {
    conn.query_row(
        "SELECT COALESCE(MIN(sort_order), 0) FROM tasks WHERE completed_at IS NULL AND deleted = 0",
        [],
        |row| row.get::<_, f64>(0),
    )
    .map_err(|e| e.to_string())
}

fn row_to_task(row: &rusqlite::Row) -> Result<TaskJson, rusqlite::Error> {
    Ok(TaskJson {
        id: row.get(0)?,
        title: row.get(1)?,
        priority: row.get(2)?,
        remind_at: row.get(3)?,
        remind_offset_min: row.get(4)?,
        repeat_rule: row.get(5)?,
        sort_order: row.get(6)?,
        deleted: row.get::<_, i64>(7)? != 0,
        completed_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

const SELECT_TASK: &str = "SELECT id, title, priority, remind_at, remind_offset_min, repeat_rule,
    sort_order, deleted, completed_at, created_at, updated_at FROM tasks WHERE id = ?1";

pub fn list_tasks(
    conn: &Connection,
    filter: &str,
    keyword: Option<&str>,
    priority: Option<&str>,
) -> Result<Vec<TaskJson>, String> {
    let mut sql = String::from(
        "SELECT id, title, priority, remind_at, remind_offset_min, repeat_rule,
                sort_order, deleted, completed_at, created_at, updated_at FROM tasks WHERE deleted = 0",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    match filter {
        "completed" => sql.push_str(" AND completed_at IS NOT NULL"),
        "all" => {}
        _ => sql.push_str(" AND completed_at IS NULL"),
    }
    if let Some(kw) = keyword {
        // 先转义反斜杠，再转义 % 与 _（顺序不能反）
        let escaped = kw.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        params.push(Box::new(format!("%{escaped}%")));
        sql.push_str(" AND title LIKE ? ESCAPE '\\'");
    }
    if let Some(p) = priority {
        params.push(Box::new(p.to_string()));
        sql.push_str(" AND priority = ?");
    }
    sql.push_str(
        " ORDER BY CASE WHEN completed_at IS NULL THEN 0 ELSE 1 END,
        CASE WHEN completed_at IS NULL THEN sort_order ELSE NULL END ASC,
        CASE WHEN completed_at IS NOT NULL THEN completed_at ELSE NULL END DESC,
        created_at ASC",
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let refs: Vec<&dyn rusqlite::ToSql> = params
        .iter()
        .map(|p| p.as_ref() as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt
        .query_map(refs.as_slice(), row_to_task)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn create_task(
    conn: &mut Connection,
    title: &str,
    priority: Option<&str>,
    remind_at: Option<&str>,
    remind_offset_min: Option<i64>,
    repeat_rule: Option<&str>,
) -> Result<TaskJson, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("title 不能为空".into());
    }
    let priority = priority.unwrap_or("medium").to_string();
    if !["high", "medium", "low"].contains(&priority.as_str()) {
        return Err(format!("priority 非法: {priority}"));
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_iso();
    let sort_order = min_sort_order(&tx)? - 1000.0;
    let remind_offset = remind_offset_min.unwrap_or(15);
    tx.execute(
        "INSERT INTO tasks (id, title, priority, remind_at, remind_offset_min, repeat_rule,
            sort_order, deleted, completed_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
        rusqlite::params![id, title, priority, remind_at, remind_offset, repeat_rule, sort_order, Option::<String>::None, now, now],
    )
    .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "id": id, "title": title, "priority": priority,
        "remind_at": remind_at.unwrap_or(""), "remind_offset_min": remind_offset,
        "repeat_rule": repeat_rule.unwrap_or(""),
        "sort_order": sort_order, "deleted": false,
        "completed_at": ""
    });
    push_pending(&tx, "create", &id, Some(&payload))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_by_id(conn, &id)
}

fn get_by_id(conn: &Connection, id: &str) -> Result<TaskJson, String> {
    conn.query_row(SELECT_TASK, [id], row_to_task).map_err(|e| {
        if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
            format!("任务不存在: {id}")
        } else {
            e.to_string()
        }
    })
}

/// 三态更新：title/priority 传 None=不变；remind_at/repeat_rule 传
/// `Option<Option<&str>>`：None=不变、Some(None)=清除、Some(Some(s))=设置（空串视为清除）。
pub fn update_task(
    conn: &mut Connection,
    id: &str,
    title: Option<&str>,
    priority: Option<&str>,
    remind_at: Option<Option<&str>>,
    repeat_rule: Option<Option<&str>>,
) -> Result<TaskJson, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = now_iso();
    let cur: TaskJson = get_by_id(&tx, id)?;
    if cur.deleted {
        return Err(format!("任务不存在: {id}"));
    }
    let new_title = title.unwrap_or(cur.title.as_str()).trim().to_string();
    if new_title.is_empty() {
        return Err("title 不能为空".into());
    }
    let new_priority = priority.unwrap_or(cur.priority.as_str()).to_string();
    if !["high", "medium", "low"].contains(&new_priority.as_str()) {
        return Err(format!("priority 非法: {new_priority}"));
    }
    // 三态：None=不变；Some(None)=清除；Some(Some(s))=设置，空串视为清除
    let new_remind = match remind_at {
        None => cur.remind_at.clone(),
        Some(None) => None,
        Some(Some(s)) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
    };
    let new_repeat = match repeat_rule {
        None => cur.repeat_rule.clone(),
        Some(None) => None,
        Some(Some(s)) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
    };
    tx.execute(
        "UPDATE tasks SET title = ?1, priority = ?2, remind_at = ?3, repeat_rule = ?4, updated_at = ?5
         WHERE id = ?6 AND deleted = 0",
        rusqlite::params![new_title, new_priority, new_remind, new_repeat, now, id],
    )
    .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "id": id, "title": new_title, "priority": new_priority,
        "remind_at": new_remind.unwrap_or_default(), "remind_offset_min": cur.remind_offset_min,
        "repeat_rule": new_repeat.unwrap_or_default(),
        "sort_order": cur.sort_order, "deleted": false,
        "completed_at": cur.completed_at.unwrap_or_default()
    });
    push_pending(&tx, "update", id, Some(&payload))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_by_id(conn, id)
}

pub fn set_completed(conn: &mut Connection, id: &str, completed: bool) -> Result<TaskJson, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = now_iso();
    let cur: TaskJson = get_by_id(&tx, id)?;
    if cur.deleted {
        return Err(format!("任务不存在: {id}"));
    }
    let completed_at = if completed { Some(now.clone()) } else { None };
    tx.execute(
        "UPDATE tasks SET completed_at = ?1, updated_at = ?2 WHERE id = ?3 AND deleted = 0",
        rusqlite::params![completed_at, now, id],
    )
    .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "id": id, "title": cur.title, "priority": cur.priority,
        "remind_at": cur.remind_at.unwrap_or_default(), "remind_offset_min": cur.remind_offset_min,
        "repeat_rule": cur.repeat_rule.unwrap_or_default(),
        "sort_order": cur.sort_order, "deleted": false,
        "completed_at": completed_at.unwrap_or_default()
    });
    push_pending(&tx, "update", id, Some(&payload))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_by_id(conn, id)
}

pub fn soft_delete(conn: &mut Connection, id: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let now = now_iso();
    let cur: TaskJson = get_by_id(&tx, id)?;
    if cur.deleted {
        return Err(format!("任务不存在: {id}"));
    }
    tx.execute(
        "UPDATE tasks SET deleted = 1, deleted_at = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![now, now, id],
    )
    .map_err(|e| e.to_string())?;
    push_pending(&tx, "delete", id, None)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试独立 db 文件（cargo 并行跑测试时 pid 相同，不隔离会共享数据致 flaky）
    fn test_conn(name: &str) -> Connection {
        let path = std::env::temp_dir().join(format!(
            "rtodo_mcp_tools_{}_{}.db",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_file(&path);
        let conn = open_db(&path).expect("open");
        init_schema(&conn).expect("schema");
        conn
    }

    #[test]
    fn create_lists_and_sorts_newest_first() {
        let mut conn = test_conn("create_lists_and_sorts_newest_first");
        let a = create_task(&mut conn, "任务A", None, None, None, None).expect("a");
        let b = create_task(&mut conn, "任务B", Some("high"), None, None, None).expect("b");
        let all = list_tasks(&conn, "active", None, None).expect("list");
        assert_eq!(all.len(), 2);
        // 后创建的 sort_order 更小 → 排最前
        assert_eq!(all[0].id, b.id);
        assert_eq!(all[0].priority, "high");
        assert_eq!(all[1].id, a.id);
        // pending_ops 有 create 记录
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_ops", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        let _ = a;
    }

    #[test]
    fn complete_and_uncomplete() {
        let mut conn = test_conn("complete_and_uncomplete");
        let t = create_task(&mut conn, "完成我", None, None, None, None).expect("t");
        let done = set_completed(&mut conn, &t.id, true).expect("done");
        assert!(done.completed_at.is_some());
        let active = list_tasks(&conn, "active", None, None).expect("active");
        assert!(active.is_empty());
        let done_list = list_tasks(&conn, "completed", None, None).expect("done list");
        assert_eq!(done_list.len(), 1);
        let undone = set_completed(&mut conn, &t.id, false).expect("undone");
        assert!(undone.completed_at.is_none());
    }

    #[test]
    fn update_changes_fields_and_filters_by_keyword() {
        let mut conn = test_conn("update_changes_fields_and_filters_by_keyword");
        let t = create_task(&mut conn, "买牛奶", None, None, None, None).expect("t");
        let up = update_task(
            &mut conn,
            &t.id,
            Some("买豆浆"),
            Some("low"),
            None,
            Some(Some("FREQ=DAILY")),
        )
        .expect("up");
        assert_eq!(up.title, "买豆浆");
        assert_eq!(up.priority, "low");
        assert_eq!(up.repeat_rule.as_deref(), Some("FREQ=DAILY"));
        let found = list_tasks(&conn, "active", Some("豆浆"), None).expect("kw");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, t.id);
    }

    #[test]
    fn update_three_state_remind_and_repeat() {
        let mut conn = test_conn("update_three_state_remind_and_repeat");
        let t = create_task(
            &mut conn,
            "三态",
            Some("high"),
            Some("2026-01-01T00:00:00Z"),
            Some(30),
            Some("FREQ=DAILY"),
        )
        .expect("t");
        // 缺省（外层 None）→ 保持不变
        let kept = update_task(&mut conn, &t.id, None, None, None, None).expect("keep");
        assert_eq!(kept.remind_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(kept.repeat_rule.as_deref(), Some("FREQ=DAILY"));
        // 显式 Some(None) → 清除
        let cleared = update_task(&mut conn, &t.id, None, None, Some(None), Some(None)).expect("clear");
        assert!(cleared.remind_at.is_none());
        assert!(cleared.repeat_rule.is_none());
        // 空串视为清除
        let again = update_task(&mut conn, &t.id, None, None, Some(Some("  ")), Some(Some(""))).expect("empty");
        assert!(again.remind_at.is_none());
        assert!(again.repeat_rule.is_none());
        // 有值 → 设置
        let set = update_task(
            &mut conn,
            &t.id,
            None,
            None,
            Some(Some("2026-02-02T00:00:00Z")),
            Some(Some("FREQ=WEEKLY")),
        )
        .expect("set");
        assert_eq!(set.remind_at.as_deref(), Some("2026-02-02T00:00:00Z"));
        assert_eq!(set.repeat_rule.as_deref(), Some("FREQ=WEEKLY"));
    }

    #[test]
    fn soft_delete_removes_from_list_and_queues() {
        let mut conn = test_conn("soft_delete_removes_from_list_and_queues");
        let t = create_task(&mut conn, "删我", None, None, None, None).expect("t");
        soft_delete(&mut conn, &t.id).expect("del");
        assert!(list_tasks(&conn, "all", None, None).expect("list").is_empty());
        let row: Option<String> = conn
            .query_row(
                "SELECT op_type FROM pending_ops WHERE record_id = ?1 AND op_type = 'delete'",
                [&t.id],
                |r| r.get(0),
            )
            .ok();
        assert_eq!(row.as_deref(), Some("delete"));
    }

    #[test]
    fn empty_title_and_unknown_id_are_errors() {
        let mut conn = test_conn("empty_title_and_unknown_id_are_errors");
        assert!(create_task(&mut conn, "   ", None, None, None, None).is_err());
        assert!(update_task(&mut conn, "no-such-id", None, None, None, None).is_err());
        assert!(set_completed(&mut conn, "no-such-id", true).is_err());
        assert!(soft_delete(&mut conn, "no-such-id").is_err());
    }

    #[test]
    fn filter_priority_works() {
        let mut conn = test_conn("filter_priority_works");
        create_task(&mut conn, "高", Some("high"), None, None, None).expect("h");
        create_task(&mut conn, "低", Some("low"), None, None, None).expect("l");
        let highs = list_tasks(&conn, "active", None, Some("high")).expect("high");
        assert_eq!(highs.len(), 1);
        assert_eq!(highs[0].title, "高");
    }
}
