use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tauri::Manager;

pub const MCP_PORT: u16 = 18765;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub token: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { enabled: false, token: String::new() }
    }
}

pub fn config_path(app: &tauri::AppHandle) -> PathBuf {
    app.path().app_config_dir()
        .map(|d| d.join("mcp-config.json"))
        .unwrap_or_else(|_| std::env::temp_dir().join("mcp-config.json"))
}

pub fn load_from(path: &std::path::Path) -> McpConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &std::path::Path, cfg: &McpConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!("rtok_{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct McpTemplates {
    pub claude_code: String,
    pub claude_desktop: String,
    pub opencode: String,
    pub codex: String,
}

pub fn build_templates(token: &str) -> McpTemplates {
    let url = format!("http://127.0.0.1:{MCP_PORT}/mcp");
    let bearer = format!("Bearer {token}");
    McpTemplates {
        claude_code: format!(
            "claude mcp add r-todo --transport http {url} --header \"Authorization: {bearer}\""
        ),
        claude_desktop: serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": { "r-todo": { "url": url, "headers": { "Authorization": bearer } } }
        })).expect("json"),
        opencode: serde_json::to_string_pretty(&serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "mcp": { "r-todo": { "type": "http", "url": url, "headers": { "Authorization": bearer } } }
        })).expect("json"),
        codex: format!(
            "[mcp_servers.r-todo]\ntype = \"http\"\nurl = \"{url}\"\nheaders = {{ Authorization = \"{bearer}\" }}"
        ),
    }
}

#[tauri::command]
pub fn get_mcp_status(app: tauri::AppHandle) -> McpConfig {
    load_from(&config_path(&app))
}

#[tauri::command]
pub fn get_mcp_client_templates(app: tauri::AppHandle) -> Result<McpTemplates, String> {
    let cfg = load_from(&config_path(&app));
    // 不变量由 enabled 承担（token 在关闭时保留，不能以 token 是否为空判断"未启用"）
    if !cfg.enabled {
        return Err("MCP 未启用，请先打开「MCP 服务器」开关".into());
    }
    if cfg.token.is_empty() {
        return Err("MCP token 未初始化，请重新打开开关".into());
    }
    Ok(build_templates(&cfg.token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_default() {
        let path = std::env::temp_dir().join(format!("rtodo_mcp_test_missing_{}", std::process::id()));
        let cfg = load_from(&path);
        assert!(!cfg.enabled);
        assert!(cfg.token.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupted_json_returns_default() {
        let path = std::env::temp_dir().join(format!("rtodo_mcp_test_corrupt_{}", std::process::id()));
        std::fs::write(&path, "{not valid json").expect("write");
        let cfg = load_from(&path);
        assert!(!cfg.enabled);
        assert!(cfg.token.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = std::env::temp_dir().join(format!("rtodo_mcp_test_round_{}", std::process::id()));
        let cfg = McpConfig { enabled: true, token: "rtok_abc123".into() };
        save_to(&path, &cfg).expect("save");
        let loaded = load_from(&path);
        assert_eq!(loaded, cfg);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn token_has_prefix_and_64_hex_chars() {
        let t = generate_token();
        assert!(t.starts_with("rtok_"));
        assert_eq!(t.len(), 5 + 64); // "rtok_" + 32 bytes hex
    }

    #[test]
    fn templates_embed_token_and_url() {
        let tpl = build_templates("rtok_testtoken");
        assert!(tpl.claude_code.contains("http://127.0.0.1:18765/mcp"));
        assert!(tpl.claude_code.contains("Bearer rtok_testtoken"));
        assert!(tpl.claude_desktop.contains("Bearer rtok_testtoken"));
        assert!(tpl.opencode.contains("Bearer rtok_testtoken"));
        assert!(tpl.codex.contains("Bearer rtok_testtoken"));
    }
}
