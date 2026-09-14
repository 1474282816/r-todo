import { invoke } from "@tauri-apps/api/core";

export interface McpStatus {
  enabled: boolean;
  token: string;
}

export interface McpTemplates {
  claude_code: string;
  claude_desktop: string;
  opencode: string;
  codex: string;
}

export interface McpRunning {
  running: boolean;
  error: string | null;
}

export const MCP_URL = "http://127.0.0.1:18765/mcp";

export async function getMcpStatus(): Promise<McpStatus> {
  return invoke("get_mcp_status");
}

export async function setMcpEnabled(enabled: boolean): Promise<McpStatus> {
  return invoke("set_mcp_enabled", { enabled });
}

export async function rotateMcpToken(): Promise<McpStatus> {
  return invoke("rotate_mcp_token");
}

export async function getMcpTemplates(): Promise<McpTemplates> {
  return invoke("get_mcp_client_templates");
}

/** 运行态：running 是否监听中，error 为启动失败原因（端口占用等） */
export async function getMcpRunning(): Promise<McpRunning> {
  return invoke("get_mcp_running");
}

/** 复制到剪贴板（WebView2，用户手势上下文可用） */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
