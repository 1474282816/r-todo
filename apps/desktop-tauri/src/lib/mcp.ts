import {
  fillMcpTemplates,
  getMcpStatus as engineMcpStatus,
  getMcpTemplates,
  rotateMcpToken as engineRotate,
  setMcpEnabled as engineSetEnabled,
} from "./engine";
import type { McpStatus, McpTemplates } from "./types";

export interface McpRunning {
  running: boolean;
  error: string | null;
}

export const MCP_URL = "http://127.0.0.1:18765/mcp";

export async function getMcpStatus(): Promise<McpStatus> {
  return engineMcpStatus();
}

export async function setMcpEnabled(enabled: boolean): Promise<McpStatus> {
  return engineSetEnabled(enabled);
}

export async function rotateMcpToken(): Promise<McpStatus> {
  return engineRotate();
}

export async function getMcpTemplatesForToken(token: string): Promise<McpTemplates> {
  return fillMcpTemplates(getMcpTemplates(), token);
}

export async function getMcpTemplatesLegacy(): Promise<McpTemplates> {
  const status = await engineMcpStatus();
  return fillMcpTemplates(getMcpTemplates(), status.token);
}

export async function getMcpRunning(): Promise<McpRunning> {
  const status = await engineMcpStatus();
  return { running: status.running, error: status.error };
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
