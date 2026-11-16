use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rtools_domain::{Priority, TaskDraft, TaskPatch};
use rtools_engine::{
    db, mcp,
    rpc::{read_frame, write_frame, Request, Response, RpcError},
    sync_client::SyncClient,
    sync_manager,
    tasks::TaskService,
    transfer::{default_transfer_dirs, TransferRuntime},
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tokio::{net::windows::named_pipe::ServerOptions, sync::Mutex};

#[derive(Parser, Debug)]
#[command(name = "rtools-engine", version)]
struct Cli {
    #[arg(long, global = true, env = "RTOOLS_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pipe: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Serve,
    Health,
    Import { json: PathBuf },
    Backup { output: PathBuf },
}

struct EngineState {
    db_path: PathBuf,
    data_dir: PathBuf,
    mcp: Mutex<Option<mcp::McpRuntime>>,
    transfer: Mutex<TransferRuntime>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rtools_engine=info".into()),
        )
        .init();
    let cli = Cli::parse();
    let data_dir = cli
        .data_dir
        .or_else(default_data_dir)
        .context("cannot resolve data directory")?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("rtools.db");
    match cli.command.unwrap_or(Command::Serve) {
        Command::Health => {
            let conn = db::open(&db_path)?;
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "ok": true,
                    "app": "rtools-engine",
                    "db": db_path,
                    "integrity": db::integrity_check(&conn)?,
                    "pending": db::pending_count(&conn)?,
                    "sync": sync_manager::summary(&conn)?
                }))?
            );
            Ok(())
        }
        Command::Import { json } => {
            let mut conn = db::open(&db_path)?;
            let content = std::fs::read_to_string(&json)?;
            let count = db::import_legacy(&mut conn, &content)?;
            println!("imported={count}");
            Ok(())
        }
        Command::Backup { output } => {
            let conn = db::open(&db_path)?;
            db::backup(&conn, &output)?;
            println!("backup={}", output.display());
            Ok(())
        }
        Command::Serve => {
            let pipe_name = cli.pipe.unwrap_or_else(default_pipe_name);
            serve(data_dir, db_path, pipe_name).await
        }
    }
}

async fn serve(data_dir: PathBuf, db_path: PathBuf, pipe_name: String) -> Result<()> {
    let state = Arc::new(EngineState {
        db_path: db_path.clone(),
        data_dir,
        mcp: Mutex::new(None),
        transfer: Mutex::new(TransferRuntime::new()),
    });
    {
        let conn = db::open(&db_path)?;
        if mcp::is_enabled(&conn)? {
            match mcp::McpRuntime::start(db_path.clone(), 18765) {
                Ok(runtime) => *state.mcp.lock().await = Some(runtime),
                Err(err) => tracing::warn!(%err, "MCP failed to start"),
            }
        }
    }
    tracing::info!(pipe=%pipe_name, db=%db_path.display(), "engine listening");
    let pipe_path = if pipe_name.starts_with(r"\\.\pipe\") {
        pipe_name
    } else {
        format!(r"\\.\pipe\{pipe_name}")
    };
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&pipe_path)?;
        server.connect().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(server, state).await {
                tracing::warn!(%err, "rpc client disconnected");
            }
        });
    }
}

async fn handle_client(
    mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    state: Arc<EngineState>,
) -> Result<()> {
    loop {
        let frame = read_frame(&mut pipe).await?;
        let request: Request = serde_json::from_slice(&frame)?;
        let response = if request.protocol_version != rtools_engine::PROTOCOL_VERSION {
            Response {
                protocol_version: rtools_engine::PROTOCOL_VERSION,
                message_type: "response".into(),
                request_id: request.request_id,
                result: None,
                error: Some(RpcError {
                    code: "PROTOCOL_MISMATCH".into(),
                    message: "Engine 与 UI 协议版本不兼容".into(),
                }),
            }
        } else {
            match dispatch(&state, &request.method, request.payload).await {
                Ok(result) => Response {
                    protocol_version: rtools_engine::PROTOCOL_VERSION,
                    message_type: "response".into(),
                    request_id: request.request_id,
                    result: Some(result),
                    error: None,
                },
                Err(error) => Response {
                    protocol_version: rtools_engine::PROTOCOL_VERSION,
                    message_type: "response".into(),
                    request_id: request.request_id,
                    result: None,
                    error: Some(error),
                },
            }
        };
        write_frame(&mut pipe, &serde_json::to_vec(&response)?).await?;
    }
}

async fn dispatch(state: &EngineState, method: &str, payload: Value) -> Result<Value, RpcError> {
    macro_rules! rpc_try {
        ($expr:expr) => {
            $expr.map_err(|error| RpcError {
                code: "INTERNAL_ERROR".into(),
                message: error.to_string(),
            })?
        };
    }
    match method {
        "system.health" => Ok(json!({
            "ok": true,
            "app": "rtools-engine",
            "protocol_version": rtools_engine::PROTOCOL_VERSION
        })),
        "tasks.list" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(serde_json::to_value(rpc_try!(db::list_tasks(&conn, false))).unwrap_or_default())
        }
        "tasks.get" => {
            let id = payload["id"].as_str().unwrap_or_default();
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(serde_json::to_value(rpc_try!(db::get_task(&conn, id))).unwrap_or(Value::Null))
        }
        "tasks.create" => {
            let draft = TaskDraft {
                title: payload["title"].as_str().unwrap_or_default().into(),
                priority: parse_priority(payload["priority"].as_str()),
                remind_at: parse_dt(payload.get("remind_at")),
                remind_offset_min: payload["remind_offset_min"].as_i64().unwrap_or(15),
                repeat_rule: payload["repeat_rule"].as_str().map(str::to_owned),
            };
            let conn = rpc_try!(db::open(&state.db_path));
            let task = rpc_try!(TaskService::new(&conn).create(draft));
            rpc_try!(sync_manager::enqueue_task_update(
                &conn,
                &task,
                &uuid::Uuid::new_v4().to_string()
            ));
            Ok(serde_json::to_value(task).unwrap_or_default())
        }
        "tasks.update" => {
            let task: rtools_domain::Task = rpc_try!(serde_json::from_value(payload));
            let patch = TaskPatch {
                title: Some(task.title.clone()),
                priority: Some(task.priority),
                remind_at: Some(task.remind_at),
                remind_offset_min: Some(task.remind_offset_min),
                repeat_rule: Some(task.repeat_rule.clone()),
                sort_order: Some(task.sort_order),
                completed_at: Some(task.completed_at),
                deleted_at: Some(task.deleted_at),
                updated_at: Some(chrono::Utc::now()),
            };
            let conn = rpc_try!(db::open(&state.db_path));
            let updated = rpc_try!(TaskService::new(&conn).update(&task.id, patch));
            rpc_try!(sync_manager::enqueue_task_update(
                &conn,
                &updated,
                &uuid::Uuid::new_v4().to_string()
            ));
            Ok(serde_json::to_value(updated).unwrap_or_default())
        }
        "tasks.complete" => {
            let id = payload["id"].as_str().unwrap_or_default();
            let completed = payload["completed"].as_bool().unwrap_or(false);
            let conn = rpc_try!(db::open(&state.db_path));
            let task = rpc_try!(TaskService::new(&conn).complete(id, completed));
            rpc_try!(sync_manager::enqueue_task_update(
                &conn,
                &task,
                &uuid::Uuid::new_v4().to_string()
            ));
            Ok(serde_json::to_value(task).unwrap_or_default())
        }
        "tasks.delete" => {
            let id = payload["id"].as_str().unwrap_or_default();
            let conn = rpc_try!(db::open(&state.db_path));
            let task = rpc_try!(TaskService::new(&conn).delete(id));
            rpc_try!(sync_manager::enqueue_task_update(
                &conn,
                &task,
                &uuid::Uuid::new_v4().to_string()
            ));
            Ok(serde_json::to_value(task).unwrap_or_default())
        }
        "tasks.reorder" => {
            let ids: Vec<String> = rpc_try!(serde_json::from_value(payload["ids"].clone()));
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(TaskService::new(&conn).reorder(&ids));
            Ok(json!({"ok":true}))
        }
        "tasks.import_json" => {
            let path = payload["path"].as_str().unwrap_or_default();
            let mut conn = rpc_try!(db::open(&state.db_path));
            let content = rpc_try!(std::fs::read_to_string(path));
            Ok(json!({"imported":rpc_try!(db::import_legacy(&mut conn, &content))}))
        }
        "tasks.export_json" => {
            let path = payload["path"].as_str().unwrap_or_default();
            let conn = rpc_try!(db::open(&state.db_path));
            let tasks = rpc_try!(db::list_tasks(&conn, true));
            let content = rpc_try!(serde_json::to_string_pretty(&json!({
                "format":"rtools/todo","version":1,"exported_at":chrono::Utc::now(),"todos":tasks
            })));
            rpc_try!(std::fs::write(path, content));
            Ok(json!({"exported":tasks.len()}))
        }
        "sync.status" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(serde_json::to_value(rpc_try!(sync_manager::summary(&conn))).unwrap_or_default())
        }
        "sync.pair" => Ok(serde_json::to_value(rpc_try!(
            sync_manager::pair(
                &state.db_path,
                payload["server_url"].as_str().unwrap_or_default(),
                payload["pairing_code"].as_str().unwrap_or_default(),
                payload["device_name"].as_str().unwrap_or_default(),
            )
            .await
        ))
        .unwrap_or_default()),
        "sync.sync_now" => Ok(serde_json::to_value(rpc_try!(
            sync_manager::sync_now(&state.db_path).await
        ))
        .unwrap_or_default()),
        "sync.logout" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(serde_json::to_value(rpc_try!(sync_manager::logout(&conn))).unwrap_or_default())
        }
        "sync.restore_credentials" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(
                serde_json::to_value(rpc_try!(sync_manager::restore_credentials(
                    &conn,
                    payload["server_url"].as_str().unwrap_or_default(),
                    payload["device_id"].as_str().unwrap_or_default(),
                    payload["device_name"].as_str().unwrap_or_default(),
                    payload["token"].as_str().unwrap_or_default(),
                )))
                .unwrap_or_default(),
            )
        }
        "devices.list" => {
            let conn = rpc_try!(db::open(&state.db_path));
            let server_url = rpc_try!(db::get_sync_value(&conn, "server_url"));
            let token = rpc_try!(db::get_sync_value(&conn, "token"));
            let device_id = rpc_try!(db::get_sync_value(&conn, "device_id"));
            match (server_url, token, device_id) {
                (Some(url), Some(token), Some(id)) => {
                    let client = rpc_try!(SyncClient::new(&url).map(|c| c.with_token(token)));
                    Ok(serde_json::to_value(rpc_try!(client.devices(&id).await))
                        .unwrap_or_default())
                }
                _ => Ok(json!([])),
            }
        }
        "devices.pairing_code" => {
            let conn = rpc_try!(db::open(&state.db_path));
            let server_url = rpc_try!(db::get_sync_value(&conn, "server_url")).unwrap_or_default();
            let token = rpc_try!(db::get_sync_value(&conn, "token")).unwrap_or_default();
            let device_id = rpc_try!(db::get_sync_value(&conn, "device_id")).unwrap_or_default();
            let client = rpc_try!(SyncClient::new(&server_url).map(|c| c.with_token(token)));
            let response = rpc_try!(client.create_pairing_code(&device_id).await);
            Ok(json!({"code":response.code,"expires_at":response.expires_at}))
        }
        "devices.revoke" => {
            let conn = rpc_try!(db::open(&state.db_path));
            let server_url = rpc_try!(db::get_sync_value(&conn, "server_url")).unwrap_or_default();
            let token = rpc_try!(db::get_sync_value(&conn, "token")).unwrap_or_default();
            let device_id = rpc_try!(db::get_sync_value(&conn, "device_id")).unwrap_or_default();
            let target = payload["device_id"].as_str().unwrap_or_default();
            let client = rpc_try!(SyncClient::new(&server_url).map(|c| c.with_token(token)));
            rpc_try!(client.revoke_device(&device_id, target).await);
            Ok(json!({"revoked":true}))
        }
        "settings.get" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(json!({"value":rpc_try!(db::get_setting(
                &conn,
                payload["key"].as_str().unwrap_or_default()
            ))}))
        }
        "settings.set" => {
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::set_setting(
                &conn,
                payload["key"].as_str().unwrap_or_default(),
                payload["value"].as_str().unwrap_or_default()
            ));
            Ok(json!({"ok":true}))
        }
        "autostart.get" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(
                json!({"enabled":rpc_try!(db::get_setting(&conn,"autostart")).as_deref()==Some("true")}),
            )
        }
        "autostart.set" => {
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::set_setting(
                &conn,
                "autostart",
                if payload["enabled"].as_bool().unwrap_or(false) {
                    "true"
                } else {
                    "false"
                }
            ));
            Ok(json!({"ok":true}))
        }
        "mcp.status" => {
            let conn = rpc_try!(db::open(&state.db_path));
            let enabled = rpc_try!(mcp::is_enabled(&conn));
            let token = rpc_try!(mcp::token_from_db(&conn));
            let guard = state.mcp.lock().await;
            Ok(match guard.as_ref() {
                Some(runtime) => {
                    serde_json::to_value(runtime.status(enabled, &token, 18765)).unwrap_or_default()
                }
                None => {
                    json!({"enabled":enabled,"running":false,"token":token,"port":18765,"error":null})
                }
            })
        }
        "mcp.set_enabled" => {
            let enabled = payload["enabled"].as_bool().unwrap_or(false);
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::set_setting(
                &conn,
                "mcp_enabled",
                if enabled { "true" } else { "false" }
            ));
            let token = rpc_try!(mcp::token_from_db(&conn));
            let mut guard = state.mcp.lock().await;
            if enabled && guard.is_none() {
                match mcp::McpRuntime::start(state.db_path.clone(), 18765) {
                    Ok(runtime) => *guard = Some(runtime),
                    Err(err) => {
                        return Ok(
                            json!({"enabled":enabled,"running":false,"token":token,"port":18765,"error":err.to_string()}),
                        )
                    }
                }
            } else if !enabled {
                *guard = None;
            }
            Ok(
                json!({"enabled":enabled,"running":guard.is_some(),"token":token,"port":18765,"error":null}),
            )
        }
        "mcp.rotate_token" => {
            let conn = rpc_try!(db::open(&state.db_path));
            let mut bytes = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut bytes);
            let token = hex::encode(bytes);
            rpc_try!(db::set_setting(&conn, "mcp_token", &token));
            Ok(
                json!({"enabled":rpc_try!(mcp::is_enabled(&conn)),"running":state.mcp.lock().await.is_some(),"token":token,"port":18765,"error":null}),
            )
        }
        "transfer.status" => {
            Ok(serde_json::to_value(state.transfer.lock().await.status()).unwrap_or_default())
        }
        "transfer.start" => {
            let (receive, identity) = default_transfer_dirs(&state.data_dir);
            Ok(serde_json::to_value(rpc_try!(
                state.transfer.lock().await.start(receive, identity).await
            ))
            .unwrap_or_default())
        }
        "transfer.stop" => Ok(serde_json::to_value(rpc_try!(
            state.transfer.lock().await.stop().await
        ))
        .unwrap_or_default()),
        "transfer.devices" => Ok(
            serde_json::to_value(state.transfer.lock().await.devices().await).unwrap_or_default(),
        ),
        "transfer.refresh" => {
            rpc_try!(state.transfer.lock().await.refresh().await);
            Ok(json!({"ok":true}))
        }
        "transfer.add_manual_ip" => {
            let result = rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .add_manual_ip(
                        payload["host"].as_str().unwrap_or_default(),
                        payload["port"].as_u64().unwrap_or(53317) as u16,
                    )
                    .await
            );
            Ok(serde_json::to_value(result).unwrap_or_default())
        }
        "transfer.send" => {
            let paths: Vec<PathBuf> = rpc_try!(serde_json::from_value(payload["paths"].clone()));
            let session = rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .send(
                        paths,
                        payload["fingerprint"].as_str().unwrap_or_default(),
                        payload["pin"].as_str().map(str::to_owned),
                    )
                    .await
            );
            Ok(json!({"session_id":session}))
        }
        "transfer.pending" => Ok(serde_json::to_value(
            state.transfer.lock().await.pending_incoming().await,
        )
        .unwrap_or_default()),
        "transfer.respond" => {
            let ids: Vec<String> = rpc_try!(serde_json::from_value(payload["file_ids"].clone()));
            rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .respond_prepare(payload["session_id"].as_str().unwrap_or_default(), ids,)
                    .await
            );
            Ok(json!({"ok":true}))
        }
        "transfer.reject" => {
            rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .reject(payload["session_id"].as_str().unwrap_or_default())
                    .await
            );
            Ok(json!({"ok":true}))
        }
        "transfer.cancel" => {
            rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .cancel(payload["session_id"].as_str().unwrap_or_default())
                    .await
            );
            Ok(json!({"ok":true}))
        }
        _ => Err(RpcError {
            code: "METHOD_NOT_FOUND".into(),
            message: format!("unknown method: {method}"),
        }),
    }
}

fn parse_priority(value: Option<&str>) -> Priority {
    match value {
        Some("high") => Priority::High,
        Some("low") => Priority::Low,
        _ => Priority::Medium,
    }
}

fn parse_dt(value: Option<&Value>) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc))
}

fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("rtools").join("engine"))
}

fn default_pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
    format!(r"\\.\pipe\rtools-engine-{user}")
}
