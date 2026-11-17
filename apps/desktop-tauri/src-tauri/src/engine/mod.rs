mod credentials;
mod process;

pub use process::{EngineBridge, EngineCallError};

#[tauri::command]
pub async fn engine_call(
    state: tauri::State<'_, EngineBridge>,
    method: String,
    payload: Option<serde_json::Value>,
) -> std::result::Result<serde_json::Value, EngineCallError> {
    state
        .call(&method, payload.unwrap_or(serde_json::Value::Null))
        .await
}

#[tauri::command]
pub async fn engine_health(
    state: tauri::State<'_, EngineBridge>,
) -> std::result::Result<serde_json::Value, EngineCallError> {
    state
        .call("system.health", serde_json::json!({}))
        .await
}

#[tauri::command]
pub fn clear_sync_credentials() {
    #[cfg(windows)]
    {
        credentials::delete("sync-token");
        credentials::delete("sync-device-id");
        credentials::delete("sync-device-name");
        credentials::delete("sync-server-url");
    }
}
