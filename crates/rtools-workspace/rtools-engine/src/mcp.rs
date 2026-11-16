use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::RngCore;
use rtools_domain::{Priority, Task, TaskDraft, TaskPatch};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    pub token: String,
    pub port: u16,
    pub error: Option<String>,
}

pub struct McpRuntime {
    running: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    _listener: TcpListener,
}

impl McpRuntime {
    pub fn start(
        db_path: std::path::PathBuf,
        port: u16,
        events: tokio::sync::broadcast::Sender<Value>,
    ) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .with_context(|| format!("MCP 端口 {port} 启动失败"))?;
        listener.set_nonblocking(true)?;
        let running = Arc::new(AtomicBool::new(true));
        let error = Arc::new(Mutex::new(None));
        let thread_listener = listener.try_clone()?;
        let thread_running = running.clone();
        let thread_error = error.clone();
        thread::spawn(move || {
            while thread_running.load(Ordering::Relaxed) {
                match thread_listener.accept() {
                    Ok((stream, _)) => {
                        if let Err(err) = handle(stream, &db_path, &events) {
                            *thread_error.lock().unwrap() = Some(err.to_string());
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(err) => {
                        *thread_error.lock().unwrap() = Some(err.to_string());
                        break;
                    }
                }
            }
        });
        Ok(Self {
            running,
            error,
            _listener: listener,
        })
    }

    pub fn status(&self, enabled: bool, token: &str, port: u16) -> McpStatus {
        McpStatus {
            enabled,
            running: self.running.load(Ordering::Relaxed),
            token: token.to_owned(),
            port,
            error: self.error.lock().unwrap().clone(),
        }
    }
}

impl Drop for McpRuntime {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

fn handle(
    mut stream: TcpStream,
    db_path: &std::path::Path,
    events: &tokio::sync::broadcast::Sender<Value>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let first = request_line.trim_end();
    let mut content_length = 0usize;
    let mut authorization = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = value.trim().to_string();
            }
        }
    }
    if first.starts_with("GET /health ") {
        return write_http(
            &mut stream,
            200,
            "application/json",
            r#"{"ok":true,"app":"rtools-mcp"}"#,
        );
    }
    if !first.starts_with("POST /mcp ") {
        return write_http(&mut stream, 404, "text/plain", "not found");
    }
    let conn = db::open(db_path)?;
    let expected = format!("Bearer {}", token_from_db(&conn)?);
    if authorization != expected {
        return write_http(&mut stream, 401, "text/plain", "unauthorized");
    }
    let mut body = vec![0u8; content_length.min(4 * 1024 * 1024)];
    reader.read_exact(&mut body)?;
    let request: Value = serde_json::from_slice(&body)?;
    let response = dispatch_jsonrpc(&conn, request, events)?;
    if response.is_null() {
        stream.write_all(
            b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(());
    }
    let body = serde_json::to_string(&response)?;
    stream.write_all(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    ).as_bytes())?;
    Ok(())
}

fn write_http(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) -> Result<()> {
    stream.write_all(format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).as_bytes())?;
    Ok(())
}

fn dispatch_jsonrpc(
    conn: &Connection,
    request: Value,
    events: &tokio::sync::broadcast::Sender<Value>,
) -> Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if id.is_null() {
        return Ok(Value::Null);
    }
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion")
                .and_then(Value::as_str).unwrap_or("2025-03-26"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "rtools", "version": env!("CARGO_PKG_VERSION")}
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => call_tool(conn, &params, events),
        _ => Err(json!({"code": -32601, "message": format!("未知方法: {method}")})),
    };
    Ok(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

fn call_tool(
    conn: &Connection,
    params: &Value,
    events: &tokio::sync::broadcast::Sender<Value>,
) -> std::result::Result<Value, Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result: std::result::Result<Value, String> = match name {
        "list_tasks" => list_tool(conn, &args),
        "create_task" => create_tool(conn, &args),
        "update_task" => update_tool(conn, &args),
        "complete_task" | "uncomplete_task" => complete_tool(conn, &args, name == "complete_task"),
        "delete_task" => delete_tool(conn, &args),
        _ => Err(format!("未知工具: {name}")),
    };
    match result {
        Ok(value) => {
            let _ = events.send(json!({
                "event": "tasks.changed",
                "payload": {"source": "mcp"}
            }));
            Ok(json!({
                "content": [{"type": "text", "text": serde_json::to_string(&value).unwrap_or_default()}],
                "structuredContent": value,
                "isError": false
            }))
        }
        Err(message) => Ok(json!({
            "content": [{"type": "text", "text": message}],
            "structuredContent": {"error": message},
            "isError": true
        })),
    }
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"list_tasks",
            "description":"列出待办，可按 active/completed/all、关键字和优先级过滤",
            "inputSchema":{"type":"object","properties":{
                "filter":{"type":"string","enum":["active","completed","all"]},
                "keyword":{"type":"string"},
                "priority":{"type":"string","enum":["high","medium","low"]}
            }}
        }),
        json!({
            "name":"create_task",
            "description":"新建待办",
            "inputSchema":{"type":"object","required":["title"],"properties":{
                "title":{"type":"string"},
                "priority":{"type":"string","enum":["high","medium","low"]},
                "remind_at":{"type":["string","null"]},
                "remind_offset_min":{"type":"integer"},
                "repeat_rule":{"type":["string","null"]}
            }}
        }),
        json!({
            "name":"update_task",
            "description":"编辑待办；未提供字段保持不变，显式 null 清除",
            "inputSchema":{"type":"object","required":["id"],"properties":{
                "id":{"type":"string"},
                "title":{"type":"string"},
                "priority":{"type":"string","enum":["high","medium","low"]},
                "remind_at":{"type":["string","null"]},
                "remind_offset_min":{"type":"integer"},
                "repeat_rule":{"type":["string","null"]}
            }}
        }),
        json!({"name":"complete_task","description":"完成任务","inputSchema":{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}}),
        json!({"name":"uncomplete_task","description":"取消完成任务","inputSchema":{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}}),
        json!({"name":"delete_task","description":"软删除任务","inputSchema":{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}}),
    ]
}

fn list_tool(conn: &Connection, args: &Value) -> std::result::Result<Value, String> {
    let filter = args
        .get("filter")
        .and_then(Value::as_str)
        .unwrap_or("active");
    let keyword = args
        .get("keyword")
        .and_then(Value::as_str)
        .map(|value| value.to_lowercase());
    let priority = args.get("priority").and_then(Value::as_str);
    let tasks = db::list_tasks(conn, false).map_err(|error| error.to_string())?;
    let filtered: Vec<_> = tasks
        .into_iter()
        .filter(|task| match filter {
            "completed" => task.completed_at.is_some(),
            "all" => true,
            _ => task.completed_at.is_none(),
        })
        .filter(|task| {
            keyword
                .as_ref()
                .is_none_or(|value| task.title.to_lowercase().contains(value))
        })
        .filter(|task| {
            priority.is_none_or(|value| {
                matches!(
                    (value, task.priority),
                    ("high", Priority::High)
                        | ("medium", Priority::Medium)
                        | ("low", Priority::Low)
                )
            })
        })
        .collect();
    Ok(json!({"tasks": filtered}))
}

fn create_tool(conn: &Connection, args: &Value) -> std::result::Result<Value, String> {
    let draft = TaskDraft {
        title: args
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        priority: parse_priority(args.get("priority").and_then(Value::as_str)),
        remind_at: parse_datetime(args.get("remind_at"))?,
        remind_offset_min: args
            .get("remind_offset_min")
            .and_then(Value::as_i64)
            .unwrap_or(15),
        repeat_rule: args
            .get("repeat_rule")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    let task = crate::tasks::TaskService::new(conn)
        .create(draft)
        .map_err(|error| error.to_string())?;
    enqueue(conn, &task)?;
    serde_json::to_value(task).map_err(|error| error.to_string())
}

fn update_tool(conn: &Connection, args: &Value) -> std::result::Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 id".to_string())?;
    let patch = TaskPatch {
        title: args
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        priority: args
            .get("priority")
            .and_then(Value::as_str)
            .map(|value| parse_priority(Some(value))),
        remind_at: optional_datetime(args, "remind_at")?,
        remind_offset_min: args.get("remind_offset_min").and_then(Value::as_i64),
        repeat_rule: optional_string(args, "repeat_rule"),
        updated_at: Some(Utc::now()),
        ..Default::default()
    };
    let task = crate::tasks::TaskService::new(conn)
        .update(id, patch)
        .map_err(|error| error.to_string())?;
    enqueue(conn, &task)?;
    serde_json::to_value(task).map_err(|error| error.to_string())
}

fn complete_tool(
    conn: &Connection,
    args: &Value,
    completed: bool,
) -> std::result::Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 id".to_string())?;
    let (task, next) = crate::tasks::TaskService::new(conn)
        .complete_and_roll(id, completed)
        .map_err(|error| error.to_string())?;
    enqueue(conn, &task)?;
    if let Some(next) = next.as_ref() {
        enqueue(conn, next)?;
    }
    Ok(json!({"task": task, "next": next}))
}

fn delete_tool(conn: &Connection, args: &Value) -> std::result::Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 id".to_string())?;
    let task = crate::tasks::TaskService::new(conn)
        .delete(id)
        .map_err(|error| error.to_string())?;
    enqueue(conn, &task)?;
    Ok(json!({"deleted": true, "id": task.id}))
}

fn enqueue(conn: &Connection, task: &Task) -> std::result::Result<(), String> {
    let patch = crate::sync_client::local_to_patch(task);
    crate::db::enqueue_operation(
        conn,
        &uuid::Uuid::new_v4().to_string(),
        &task.id,
        task.revision,
        &serde_json::to_string(&patch).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn parse_priority(value: Option<&str>) -> Priority {
    match value {
        Some("high") => Priority::High,
        Some("low") => Priority::Low,
        _ => Priority::Medium,
    }
}

fn parse_datetime(value: Option<&Value>) -> std::result::Result<Option<DateTime<Utc>>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => DateTime::parse_from_rfc3339(text)
            .map(|value| Some(value.with_timezone(&Utc)))
            .map_err(|_| format!("时间必须是 ISO8601: {text}")),
        _ => Err("时间必须是 ISO8601 字符串或 null".into()),
    }
}

fn optional_datetime(
    args: &Value,
    key: &str,
) -> std::result::Result<Option<Option<DateTime<Utc>>>, String> {
    if !args
        .as_object()
        .is_some_and(|object| object.contains_key(key))
    {
        return Ok(None);
    }
    Ok(Some(parse_datetime(args.get(key))?))
}

fn optional_string(args: &Value, key: &str) -> Option<Option<String>> {
    let object = args.as_object()?;
    if !object.contains_key(key) {
        return None;
    }
    Some(
        object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string),
    )
}

pub fn token_from_db(conn: &Connection) -> Result<String> {
    if let Some(token) = db::get_setting(conn, "mcp_token")? {
        return Ok(token);
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    db::set_setting(conn, "mcp_token", &token)?;
    Ok(token)
}

pub fn is_enabled(conn: &Connection) -> Result<bool> {
    Ok(db::get_setting(conn, "mcp_enabled")?.as_deref() == Some("true"))
}
