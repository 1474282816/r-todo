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
use tokio::sync::{broadcast, mpsc};
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
    Mcp,
}

struct EngineState {
    db_path: PathBuf,
    data_dir: PathBuf,
    mcp: Mutex<Option<mcp::McpRuntime>>,
    transfer: Mutex<TransferRuntime>,
    events: broadcast::Sender<Value>,
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
        Command::Mcp => {
            let (events, _) = broadcast::channel(32);
            let runtime = mcp::McpRuntime::start(db_path.clone(), 18765, events)?;
            let conn = db::open(&db_path)?;
            db::set_setting(&conn, "mcp_enabled", "true")?;
            println!("rtools-mcp listening on http://127.0.0.1:18765/mcp");
            tokio::signal::ctrl_c().await?;
            drop(runtime);
            Ok(())
        }
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
    let (events, _) = broadcast::channel(512);
    let event_sender = events.clone();
    let state = Arc::new(EngineState {
        db_path: db_path.clone(),
        data_dir,
        mcp: Mutex::new(None),
        transfer: Mutex::new(TransferRuntime::new()),
        events,
    });
    {
        let conn = db::open(&db_path)?;
        db::discard_overdue_reminders(&conn, chrono::Utc::now())?;
        if mcp::is_enabled(&conn)? {
            match mcp::McpRuntime::start(db_path.clone(), 18765, event_sender.clone()) {
                Ok(runtime) => *state.mcp.lock().await = Some(runtime),
                Err(err) => tracing::warn!(%err, "MCP failed to start"),
            }
        }
    }
    spawn_reminder_loop(db_path.clone(), event_sender.clone());
    spawn_sync_loop(db_path.clone(), event_sender.clone());
    spawn_transfer_events(state.clone());
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
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    state: Arc<EngineState>,
) -> Result<()> {
    let (mut reader, mut writer) = tokio::io::split(pipe);
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(256);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            write_frame(&mut writer, &frame).await?;
        }
        Ok::<(), anyhow::Error>(())
    });
    let mut events = state.events.subscribe();
    let event_tx = out_tx.clone();
    tokio::spawn(async move {
        while let Ok(value) = events.recv().await {
            let event = rtools_engine::rpc::Event {
                protocol_version: rtools_engine::PROTOCOL_VERSION,
                message_type: "event".into(),
                event: value["event"].as_str().unwrap_or("engine.event").into(),
                payload: value.get("payload").cloned().unwrap_or_default(),
            };
            let Ok(frame) = serde_json::to_vec(&event) else {
                continue;
            };
            if event_tx.send(frame).await.is_err() {
                break;
            }
        }
    });
    let result = loop {
        let frame = match read_frame(&mut reader).await {
            Ok(frame) => frame,
            Err(error) => break Err(error),
        };
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
        if out_tx.send(serde_json::to_vec(&response)?).await.is_err() {
            break Err(anyhow::anyhow!("RPC writer disconnected"));
        }
    };
    // 客户端退出时，事件泵仍可能持有发送端。先丢弃当前连接专属的
    // 事件泵，让 writer 能立即结束；全局广播仍由其他连接继续消费。
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    drop(out_tx);
    let _ = writer_task.await;
    result
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
            let (task, next) = rpc_try!(TaskService::new(&conn).complete_and_roll(id, completed));
            rpc_try!(sync_manager::enqueue_task_update(
                &conn,
                &task,
                &uuid::Uuid::new_v4().to_string()
            ));
            if let Some(next) = next.as_ref() {
                rpc_try!(sync_manager::enqueue_task_update(
                    &conn,
                    next,
                    &uuid::Uuid::new_v4().to_string()
                ));
            }
            let _ = state.events.send(json!({
                "event":"tasks.changed",
                "payload":{"task_ids":[task.id],"created_task_id":next.as_ref().map(|value| value.id.clone())}
            }));
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
            for id in &ids {
                if let Some(task) = rpc_try!(db::get_task(&conn, id)) {
                    rpc_try!(sync_manager::enqueue_task_update(
                        &conn,
                        &task,
                        &uuid::Uuid::new_v4().to_string()
                    ));
                }
            }
            let _ = state.events.send(json!({
                "event":"tasks.changed",
                "payload":{"task_ids":ids}
            }));
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
                match mcp::McpRuntime::start(state.db_path.clone(), 18765, state.events.clone()) {
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
            let (default_receive, identity) = default_transfer_dirs(&state.data_dir);
            let conn = rpc_try!(db::open(&state.db_path));
            let alias = rpc_try!(db::get_setting(&conn, "transfer_alias"))
                .unwrap_or_else(|| "rtools".into());
            let port = rpc_try!(db::get_setting(&conn, "transfer_port"))
                .and_then(|value| value.parse().ok())
                .unwrap_or(53317);
            let receive = rpc_try!(db::get_setting(&conn, "transfer_receive_dir"))
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or(default_receive);
            let pin =
                rpc_try!(db::get_setting(&conn, "transfer_pin")).filter(|value| !value.is_empty());
            let auto_accept_favorites =
                rpc_try!(db::get_setting(&conn, "transfer_auto_accept")).as_deref() == Some("true");
            let config = rtodo_transfer_core::TransferConfig {
                alias,
                port,
                receive_dir: receive.clone(),
                identity_dir: identity.clone(),
                pin,
                auto_accept_favorites,
                verify_checksums: true,
            };
            Ok(serde_json::to_value(rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .start(receive, identity, config)
                    .await
            ))
            .unwrap_or_default())
        }
        "transfer.settings" => {
            let alias = payload["alias"].as_str().unwrap_or("rtools");
            let port = payload["port"].as_u64().unwrap_or(53317).clamp(1, 65535);
            let receive_dir = payload["receive_dir"].as_str().unwrap_or_default();
            let pin = payload["pin"].as_str().unwrap_or_default();
            let auto_accept = payload["auto_accept_favorites"].as_bool().unwrap_or(false);
            let conn = rpc_try!(db::open(&state.db_path));
            for (key, value) in [
                ("transfer_alias", alias.to_string()),
                ("transfer_port", port.to_string()),
                ("transfer_receive_dir", receive_dir.to_string()),
                ("transfer_pin", pin.to_string()),
                (
                    "transfer_auto_accept",
                    if auto_accept { "true" } else { "false" }.to_string(),
                ),
            ] {
                rpc_try!(db::set_setting(&conn, key, &value));
            }
            let mut transfer = state.transfer.lock().await;
            transfer.set_settings(
                alias.to_string(),
                port as u16,
                receive_dir.to_string(),
                !pin.is_empty(),
                auto_accept,
            );
            Ok(serde_json::to_value(transfer.status()).unwrap_or_default())
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
        "transfer.send_files" => {
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
            let session_id = payload["session_id"].as_str().unwrap_or_default();
            let pending = state.transfer.lock().await.pending_incoming().await;
            let selected = pending
                .into_iter()
                .find(|item| item.session_id == session_id);
            let accepted: Vec<_> = selected
                .map(|item| {
                    let fingerprint = item.device.fingerprint.clone();
                    let alias = item.device.alias.clone();
                    item.files
                        .into_iter()
                        .filter(|file| ids.contains(&file.id))
                        .map(|file| (fingerprint.clone(), alias.clone(), file))
                        .collect()
                })
                .unwrap_or_default();
            rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .respond_prepare(session_id, ids)
                    .await
            );
            if !accepted.is_empty() {
                let conn = rpc_try!(db::open(&state.db_path));
                for (fingerprint, alias, file) in accepted {
                    let is_image = file
                        .file_type
                        .as_deref()
                        .is_some_and(|kind| kind.starts_with("image/"))
                        || std::path::Path::new(&file.file_name)
                            .extension()
                            .and_then(|value| value.to_str())
                            .is_some_and(|ext| {
                                matches!(
                                    ext.to_ascii_lowercase().as_str(),
                                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
                                )
                            });
                    let mut message = rtodo_transfer_core::ChatMessage::new(
                        file.id,
                        fingerprint.to_ascii_uppercase(),
                        fingerprint.to_ascii_uppercase(),
                        alias,
                        if is_image {
                            rtodo_transfer_core::MessageKind::Image
                        } else {
                            rtodo_transfer_core::MessageKind::File
                        },
                        file.file_name,
                    );
                    message.file_size = Some(file.size);
                    message.preview = file.preview;
                    message.status = rtodo_transfer_core::MessageStatus::Pending;
                    rpc_try!(db::save_chat_message(&conn, &message, "receive"));
                }
                let _ = state.events.send(json!({
                    "event":"chat.changed",
                    "payload":{"session_id":session_id}
                }));
            }
            Ok(json!({"ok":true}))
        }
        "transfer.respond_file" => {
            rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .respond_file(
                        payload["session_id"].as_str().unwrap_or_default(),
                        payload["file_id"].as_str().unwrap_or_default(),
                        std::path::Path::new(payload["save_path"].as_str().unwrap_or_default()),
                    )
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
        "chat.list_conversations" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(serde_json::to_value(rpc_try!(db::list_conversations(&conn))).unwrap_or_default())
        }
        "chat.list_messages" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(serde_json::to_value(rpc_try!(db::list_messages(
                &conn,
                payload["conversation_id"].as_str().unwrap_or_default()
            )))
            .unwrap_or_default())
        }
        "chat.mark_read" => {
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::mark_conversation_read(
                &conn,
                payload["conversation_id"].as_str().unwrap_or_default()
            ));
            Ok(json!({"ok":true}))
        }
        "chat.clear" => {
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::clear_conversation_files(
                &conn,
                payload["conversation_id"].as_str().unwrap_or_default()
            ));
            rpc_try!(db::clear_conversation(
                &conn,
                payload["conversation_id"].as_str().unwrap_or_default()
            ));
            Ok(json!({"ok":true}))
        }
        "chat.send_text" => {
            let mut message: rtodo_transfer_core::ChatMessage =
                rpc_try!(serde_json::from_value(payload["message"].clone()));
            let fingerprint = payload["fingerprint"].as_str().unwrap_or_default();
            let conn = rpc_try!(db::open(&state.db_path));
            message.status = rtodo_transfer_core::MessageStatus::Pending;
            rpc_try!(db::save_chat_message(&conn, &message, "send"));
            let send_result = state
                .transfer
                .lock()
                .await
                .send_message(fingerprint, &message)
                .await;
            if let Err(error) = send_result {
                rpc_try!(db::enqueue_chat_message(
                    &conn,
                    &message.conversation_id,
                    &serde_json::to_string(&message).unwrap_or_default(),
                    &message.id,
                ));
                let _ = state.events.send(json!({
                    "event":"chat.changed",
                    "payload":{
                        "conversation_id":message.conversation_id,
                        "queued":true,
                        "error":error
                    }
                }));
                return Ok(json!({"ok":true,"queued":true}));
            }
            rpc_try!(db::patch_chat_message_status(&conn, &message.id, "sent"));
            let _ = state.events.send(json!({
                "event":"chat.changed",
                "payload":{"conversation_id":message.conversation_id}
            }));
            Ok(json!({"ok":true,"queued":false}))
        }
        "chat.flush_outbox" => {
            let conn = rpc_try!(db::open(&state.db_path));
            let pending = rpc_try!(db::list_chat_outbox(&conn));
            let mut sent = 0usize;
            let now = chrono::Utc::now().timestamp_millis();
            for (id, target, payload, attempts, created_at) in pending {
                // 离线设备按指数退避重试，避免每 2 秒发起一轮连接。
                let backoff_ms = (60_000_i64)
                    .saturating_mul(1_i64 << attempts.clamp(0, 6) as u32)
                    .min(3_600_000);
                if attempts > 0 && now.saturating_sub(created_at) < backoff_ms {
                    continue;
                }
                let message: rtodo_transfer_core::ChatMessage =
                    rpc_try!(serde_json::from_str(&payload));
                match state
                    .transfer
                    .lock()
                    .await
                    .send_message(&target, &message)
                    .await
                {
                    Ok(()) => {
                        rpc_try!(db::remove_chat_outbox(&conn, &id));
                        rpc_try!(db::patch_chat_message_status(&conn, &message.id, "sent"));
                        sent += 1;
                    }
                    Err(error) => {
                        rpc_try!(db::mark_chat_outbox_error(&conn, &id, &error.to_string()));
                    }
                }
            }
            if sent > 0 {
                let _ = state.events.send(json!({
                    "event":"chat.changed",
                    "payload":{"flushed":sent}
                }));
            }
            Ok(json!({"ok":true,"sent":sent}))
        }
        "chat.trusted" => {
            let conn = rpc_try!(db::open(&state.db_path));
            Ok(json!({
                "trusted":rpc_try!(db::is_trusted_peer(
                    &conn,
                    payload["fingerprint"].as_str().unwrap_or_default()
                ))
            }))
        }
        "chat.trust" => {
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::trust_peer(
                &conn,
                payload["fingerprint"].as_str().unwrap_or_default()
            ));
            Ok(json!({"ok":true}))
        }
        "chat.revoke_trust" => {
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::revoke_peer_trust(
                &conn,
                payload["fingerprint"].as_str().unwrap_or_default()
            ));
            Ok(json!({"ok":true}))
        }
        "chat.send_files" => {
            let paths: Vec<PathBuf> = rpc_try!(serde_json::from_value(payload["paths"].clone()));
            let fingerprint = payload["fingerprint"].as_str().unwrap_or_default();
            let send_paths = paths.clone();
            let session = rpc_try!(
                state
                    .transfer
                    .lock()
                    .await
                    .send(send_paths, fingerprint, None)
                    .await
            );
            let self_fingerprint = state.transfer.lock().await.status().fingerprint;
            let conn = rpc_try!(db::open(&state.db_path));
            let alias = rpc_try!(db::get_setting(&conn, "transfer_alias"))
                .unwrap_or_else(|| "rtools".into());
            for path in &paths {
                let metadata = std::fs::metadata(path).ok();
                let is_image = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
                        )
                    });
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("文件")
                    .to_string();
                let mut message = rtodo_transfer_core::ChatMessage::new(
                    uuid::Uuid::new_v4().simple().to_string(),
                    fingerprint.to_ascii_uppercase(),
                    self_fingerprint.to_ascii_uppercase(),
                    alias.clone(),
                    if is_image {
                        rtodo_transfer_core::MessageKind::Image
                    } else {
                        rtodo_transfer_core::MessageKind::File
                    },
                    name,
                );
                message.file_path = Some(path.to_string_lossy().into_owned());
                message.file_size = metadata.map(|value| value.len());
                rpc_try!(db::save_chat_message(&conn, &message, "send"));
            }
            let _ = state.events.send(json!({
                "event":"chat.changed",
                "payload":{"conversation_id":fingerprint}
            }));
            Ok(json!({"session_id":session}))
        }
        "chat.delete_file" => {
            let path = PathBuf::from(payload["path"].as_str().unwrap_or_default());
            let receive_dir = state.data_dir.join("transfer").join("receive");
            let default_download = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Downloads")
                .join("rtools");
            if !path.starts_with(&receive_dir) && !path.starts_with(&default_download) {
                return Err(RpcError {
                    code: "INVALID_PATH".into(),
                    message: "只能删除 rtools 接收目录中的文件".into(),
                });
            }
            if path.is_file() {
                rpc_try!(std::fs::remove_file(&path));
            }
            let conn = rpc_try!(db::open(&state.db_path));
            rpc_try!(db::delete_chat_file_record(
                &conn,
                path.to_string_lossy().as_ref()
            ));
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

fn spawn_reminder_loop(db_path: PathBuf, events: broadcast::Sender<Value>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let Ok(conn) = db::open(&db_path) else {
                continue;
            };
            loop {
                let Ok(Some(reminder)) = db::take_due_reminder(&conn, chrono::Utc::now()) else {
                    break;
                };
                let _ = events.send(json!({
                    "event":"reminder.due",
                    "payload":{
                        "task_id":reminder.task_id,
                        "title":reminder.title,
                        "due_at":reminder.due_at
                    }
                }));
            }
        }
    });
}

fn spawn_sync_loop(db_path: PathBuf, events: broadcast::Sender<Value>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            let Ok(conn) = db::open(&db_path) else {
                continue;
            };
            if db::get_sync_value(&conn, "token").ok().flatten().is_none() {
                continue;
            }
            drop(conn);
            if sync_manager::sync_from_loop(&db_path).await.is_ok() {
                let _ = events.send(json!({
                    "event":"tasks.changed",
                    "payload":{"source":"sync"}
                }));
            }
        }
    });
}

fn spawn_transfer_events(state: Arc<EngineState>) {
    tokio::spawn(async move {
        let mut receiver = state.transfer.lock().await.subscribe();
        loop {
            let Ok(envelope) = receiver.recv().await else {
                continue;
            };
            let payload = serde_json::to_value(&envelope).unwrap_or_default();
            if let rtodo_transfer_core::TransferEvent::Message { message } = &envelope.event {
                if let Ok(conn) = db::open(&state.db_path) {
                    let _ = db::save_chat_message(&conn, message, "receive");
                    if message.kind == rtodo_transfer_core::MessageKind::Trust {
                        let _ = db::trust_peer(&conn, &message.sender);
                    }
                }
                let _ = state.events.send(json!({
                    "event":"chat.changed",
                    "payload":{"conversation_id":message.conversation_id}
                }));
            }
            if let rtodo_transfer_core::TransferEvent::Incoming { .. } = &envelope.event {
                let _ = state.events.send(json!({
                    "event":"transfer.incoming",
                    "payload":payload
                }));
            }
            if let rtodo_transfer_core::TransferEvent::Completed {
                file_id,
                direction: rtodo_transfer_core::Direction::Receive,
                path: Some(path),
                ..
            } = &envelope.event
            {
                if let Ok(conn) = db::open(&state.db_path) {
                    let _ = db::attach_chat_file_path(&conn, file_id, path, "sent");
                }
                let _ = state.events.send(json!({
                    "event":"chat.changed",
                    "payload":{"file_id":file_id}
                }));
            }
            let _ = state.events.send(json!({
                "event":"transfer.event",
                "payload":payload
            }));
        }
    });
}
