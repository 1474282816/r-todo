use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use rand::RngCore;
use rtools_sync_proto::{
    ApiError, ChangeNotice, DeviceInfo, OperationBatch, OperationResponse, PairRequest,
    PairResponse, PairingCodeResponse,
};
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{broadcast, Mutex};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{db, service};

#[derive(Clone)]
pub struct AppState {
    pub db_path: std::path::PathBuf,
    pub lock: Arc<Mutex<()>>,
    pub events: broadcast::Sender<ChangeNotice>,
}

pub async fn serve(state: AppState, addr: SocketAddr) -> Result<()> {
    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/pairing-codes", post(create_pairing))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}", delete(revoke_device))
        .route("/api/v1/operations", post(operations))
        .route("/api/v1/changes", get(changes))
        .route("/api/v1/snapshot", get(snapshot))
        .route("/api/v1/events", get(events))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "rtools-syncd listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(json!({"ok":true,"app":"rtools-syncd","api_version":1}))
}

async fn pair(
    State(state): State<AppState>,
    Json(req): Json<PairRequest>,
) -> Result<Json<PairResponse>, ApiErr> {
    let _guard = state.lock.lock().await;
    let conn = db::open(&state.db_path)?;
    if !db::consume_pairing_code(&conn, &req.code)? {
        return Err(ApiErr::new(
            StatusCode::UNAUTHORIZED,
            "PAIRING_CODE_INVALID",
            "接入码无效或已过期",
        ));
    }
    let device_id = Uuid::new_v4().to_string();
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let name = if req.device_name.trim().is_empty() {
        "未命名设备"
    } else {
        req.device_name.trim()
    };
    db::add_device(&conn, &device_id, name, &token)?;
    Ok(Json(PairResponse {
        token,
        device_id,
        server_name: db::server_name(&conn)?,
    }))
}

async fn create_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PairingCodeResponse>, ApiErr> {
    let (device_id, _) = authenticate(&state, &headers)?;
    let code = generate_code();
    let _guard = state.lock.lock().await;
    let conn = db::open(&state.db_path)?;
    db::create_pairing_code(&conn, &code, 10)?;
    tracing::info!(device = %device_id, "pairing code created");
    Ok(Json(PairingCodeResponse {
        code,
        expires_at: Utc::now() + Duration::minutes(10),
    }))
}

async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeviceInfo>>, ApiErr> {
    authenticate(&state, &headers)?;
    let conn = db::open(&state.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id,name,created_at,last_seen_at,revoked_at FROM devices ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DeviceInfo {
            id: r.get(0)?,
            name: r.get(1)?,
            created_at: chrono::DateTime::parse_from_rfc3339(&r.get::<_, String>(2)?)
                .map(|v| v.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?,
            last_seen_at: r
                .get::<_, Option<String>>(3)?
                .map(|v| {
                    chrono::DateTime::parse_from_rfc3339(&v)
                        .map(|x| x.with_timezone(&Utc))
                        .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))
                })
                .transpose()?,
            revoked_at: r
                .get::<_, Option<String>>(4)?
                .map(|v| {
                    chrono::DateTime::parse_from_rfc3339(&v)
                        .map(|x| x.with_timezone(&Utc))
                        .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))
                })
                .transpose()?,
        })
    })?;
    Ok(Json(rows.collect::<rusqlite::Result<Vec<_>>>()?))
}

async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    authenticate(&state, &headers)?;
    let _guard = state.lock.lock().await;
    let conn = db::open(&state.db_path)?;
    let ok = db::revoke_device(&conn, &id)?;
    Ok(Json(json!({"revoked":ok})))
}

async fn operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<OperationBatch>,
) -> Result<Json<OperationResponse>, ApiErr> {
    let (device_id, _) = authenticate(&state, &headers)?;
    if batch.device_id != device_id {
        return Err(ApiErr::new(
            StatusCode::FORBIDDEN,
            "DEVICE_MISMATCH",
            "设备标识不匹配",
        ));
    }
    let _guard = state.lock.lock().await;
    let mut conn = db::open(&state.db_path)?;
    let response = service::apply_batch(&mut conn, batch)?;
    let _ = state.events.send(ChangeNotice {
        kind: "changes.available".into(),
        cursor: response.cursor,
    });
    Ok(Json(response))
}

#[derive(Deserialize)]
struct ChangesQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    500
}

async fn changes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ChangesQuery>,
) -> Result<Json<rtools_sync_proto::ChangePage>, ApiErr> {
    authenticate(&state, &headers)?;
    let conn = db::open(&state.db_path)?;
    Ok(Json(service::changes(
        &conn,
        q.after,
        q.limit.clamp(1, 1000),
    )?))
}

async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<rtools_sync_proto::Snapshot>, ApiErr> {
    authenticate(&state, &headers)?;
    let conn = db::open(&state.db_path)?;
    Ok(Json(service::snapshot(&conn)?))
}

async fn events(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiErr> {
    authenticate(&state, &headers)?;
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let payload = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok::<_, std::convert::Infallible>(axum::response::sse::Event::default().data(payload));
                }
                Err(_) => break,
            }
        }
    };
    Ok(axum::response::Sse::new(stream).into_response())
}

fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<(String, String), ApiErr> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ApiErr::new(StatusCode::UNAUTHORIZED, "AUTH_REQUIRED", "需要设备令牌"))?;
    let device_id = headers
        .get("x-rtools-device-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiErr::new(StatusCode::UNAUTHORIZED, "DEVICE_REQUIRED", "需要设备标识"))?;
    let conn = db::open(&state.db_path)?;
    if !db::authenticate(&conn, device_id, token)? {
        return Err(ApiErr::new(
            StatusCode::UNAUTHORIZED,
            "TOKEN_INVALID",
            "设备令牌无效或已撤销",
        ));
    }
    Ok((device_id.to_string(), token.to_string()))
}

fn generate_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    let code: String = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    format!("{}-{}", &code[..4], &code[4..])
}

pub struct ApiErr {
    status: StatusCode,
    error: ApiError,
}

impl ApiErr {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            error: ApiError::new(code, message),
        }
    }
}

impl From<anyhow::Error> for ApiErr {
    fn from(value: anyhow::Error) -> Self {
        tracing::error!(error=?value, "request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: ApiError {
                code: "INTERNAL_ERROR".into(),
                message: "服务器内部错误".into(),
                retryable: true,
            },
        }
    }
}

impl From<rusqlite::Error> for ApiErr {
    fn from(value: rusqlite::Error) -> Self {
        Self::from(anyhow::Error::from(value))
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}
