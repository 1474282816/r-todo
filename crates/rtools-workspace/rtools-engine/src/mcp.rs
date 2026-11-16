use anyhow::{Context, Result};
use rand::RngCore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
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
    pub fn start(db_path: std::path::PathBuf, port: u16) -> Result<Self> {
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
                        if let Err(err) = handle(stream, &db_path) {
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

fn handle(mut stream: TcpStream, db_path: &std::path::Path) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let first = request.lines().next().unwrap_or_default();
    if first.starts_with("GET /health ") {
        let body = r#"{"ok":true,"app":"rtools-mcp"}"#;
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ).as_bytes())?;
        return Ok(());
    }
    let conn = db::open(db_path)?;
    if first.starts_with("GET /api/tasks ") {
        let tasks = db::list_tasks(&conn, false)?;
        let body = serde_json::to_string(&tasks)?;
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ).as_bytes())?;
        return Ok(());
    }
    stream.write_all(
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .as_bytes(),
    )?;
    Ok(())
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

