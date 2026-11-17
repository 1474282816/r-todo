import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { Device, McpStatus, McpTemplates, SyncStatus, Task } from "./types";

export interface EngineTask {
  id: string;
  title: string;
  priority: "high" | "medium" | "low";
  remind_at: string | null;
  remind_offset_min: number;
  repeat_rule: string | null;
  sort_order: number;
  completed_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
  revision: number;
}

export interface EngineEvent {
  event: string;
  payload: unknown;
}

export class EngineError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "EngineError";
    this.code = code;
  }
}

function normalizeError(error: unknown): EngineError {
  if (error instanceof EngineError) return error;
  if (error && typeof error === "object") {
    const value = error as { code?: string; message?: string; 0?: string; 1?: string };
    return new EngineError(
      value.code ?? value[0] ?? "ENGINE_ERROR",
      value.message ?? value[1] ?? String(error),
    );
  }
  return new EngineError("ENGINE_ERROR", String(error));
}

export async function engineCall<T>(
  method: string,
  payload: object | null = {},
): Promise<T> {
  try {
    return await invoke<T>("engine_call", { method, payload });
  } catch (error) {
    throw normalizeError(error);
  }
}

export async function engineHealth(): Promise<{ ok: boolean; app: string }> {
  return invoke("engine_health");
}

export function subscribeEngine(
  handler: (event: EngineEvent) => void,
): Promise<UnlistenFn> {
  return listen<EngineEvent>("engine-event", (event) => handler(event.payload));
}

export function toTask(value: EngineTask): Task {
  return {
    id: value.id,
    title: value.title,
    priority: value.priority,
    remind_at: value.remind_at,
    remind_offset_min: value.remind_offset_min,
    repeat_rule: value.repeat_rule,
    sort_order: value.sort_order,
    deleted: value.deleted_at !== null,
    completed_at: value.completed_at,
    created_at: value.created_at,
    updated_at: value.updated_at,
    revision: value.revision,
  };
}

export function toEngineTask(value: Task): EngineTask {
  return {
    id: value.id,
    title: value.title,
    priority: value.priority,
    remind_at: value.remind_at,
    remind_offset_min: value.remind_offset_min,
    repeat_rule: value.repeat_rule,
    sort_order: value.sort_order,
    completed_at: value.completed_at,
    deleted_at: value.deleted ? value.updated_at : null,
    created_at: value.created_at,
    updated_at: value.updated_at,
    revision: value.revision ?? 0,
  };
}

export async function listTasks(): Promise<Task[]> {
  const rows = await engineCall<EngineTask[]>("tasks.list");
  return rows.map(toTask);
}

export async function createTask(input: {
  title: string;
  priority: Task["priority"];
  remind_at?: string | null;
  remind_offset_min?: number;
  repeat_rule?: string | null;
}): Promise<Task> {
  const row = await engineCall<EngineTask>("tasks.create", {
    title: input.title,
    priority: input.priority,
    remind_at: input.remind_at ?? null,
    remind_offset_min: input.remind_offset_min ?? 15,
    repeat_rule: input.repeat_rule ?? null,
  });
  return toTask(row);
}

export async function updateTask(task: Task): Promise<Task> {
  const row = await engineCall<EngineTask>("tasks.update", toEngineTask(task));
  return toTask(row);
}

export async function completeTask(id: string, completed: boolean): Promise<Task> {
  const row = await engineCall<EngineTask>("tasks.complete", { id, completed });
  return toTask(row);
}

export async function deleteTask(id: string): Promise<void> {
  await engineCall("tasks.delete", { id });
}

export async function reorderTasks(ids: string[]): Promise<void> {
  await engineCall("tasks.reorder", { ids });
}

export async function importJson(path: string): Promise<number> {
  const result = await engineCall<{ imported: number }>("tasks.import_json", { path });
  return result.imported;
}

export async function exportJson(path: string): Promise<number> {
  const result = await engineCall<{ exported: number }>("tasks.export_json", { path });
  return result.exported;
}

export async function getSetting(key: string): Promise<string | null> {
  const result = await engineCall<{ value: string | null }>("settings.get", { key });
  return result.value;
}

export async function setSetting(key: string, value: string): Promise<void> {
  await engineCall("settings.set", { key, value });
}

interface RawSyncStatus {
  paired: boolean;
  device_id: string | null;
  device_name: string | null;
  server_url: string | null;
  pending: number;
  last_sync_at: string | null;
  last_error: string | null;
}

function toSyncStatus(value: RawSyncStatus): SyncStatus {
  return {
    paired: value.paired,
    connected: value.paired,
    server_url: value.server_url,
    device_id: value.device_id,
    device_name: value.device_name,
    last_sync_at: value.last_sync_at,
    error: value.last_error,
    pending_count: value.pending,
  };
}

export async function getSyncStatus(): Promise<SyncStatus> {
  return toSyncStatus(await engineCall<RawSyncStatus>("sync.status"));
}

export async function pairSync(
  serverUrl: string,
  pairingCode: string,
  deviceName: string,
): Promise<SyncStatus> {
  return toSyncStatus(
    await engineCall<RawSyncStatus>("sync.pair", {
      server_url: serverUrl,
      pairing_code: pairingCode,
      device_name: deviceName,
    }),
  );
}

export async function syncNow(): Promise<SyncStatus> {
  return toSyncStatus(await engineCall<RawSyncStatus>("sync.sync_now"));
}

export async function logoutSync(): Promise<SyncStatus> {
  return toSyncStatus(await engineCall<RawSyncStatus>("sync.logout"));
}

export async function clearSyncCredentials(): Promise<void> {
  await invoke("clear_sync_credentials");
}

export async function listDevices(): Promise<Device[]> {
  const values = await engineCall<
    Array<{
      id: string;
      name: string;
      created_at: string;
      last_seen_at: string | null;
      revoked_at: string | null;
    }>
  >("devices.list");
  return values.map((value) => ({
    id: value.id,
    name: value.name,
    created_at: value.created_at,
    last_used_at: value.last_seen_at,
    revoked_at: value.revoked_at,
  }));
}

export async function revokeDevice(deviceId: string): Promise<void> {
  await engineCall("devices.revoke", { device_id: deviceId });
}

export async function createPairingCode(): Promise<{ code: string; expires_at: string }> {
  return engineCall("devices.pairing_code");
}

export async function getMcpStatus(): Promise<McpStatus> {
  return engineCall("mcp.status");
}

export async function setMcpEnabled(enabled: boolean): Promise<McpStatus> {
  return engineCall("mcp.set_enabled", { enabled });
}

export async function rotateMcpToken(): Promise<McpStatus> {
  return engineCall("mcp.rotate_token");
}

export function getMcpTemplates(): McpTemplates {
  const token = "<从 Engine 状态获取的 token>";
  const url = "http://127.0.0.1:18765/mcp";
  return {
    claude_code: `claude mcp add rtools --transport http ${url} --header "Authorization: Bearer ${token}"`,
    claude_desktop: JSON.stringify({
      mcpServers: {
        rtools: { url, headers: { Authorization: `Bearer ${token}` } },
      },
    }, null, 2),
    opencode: JSON.stringify({
      mcp: {
        rtools: { type: "http", url, headers: { Authorization: `Bearer ${token}` } },
      },
    }, null, 2),
    codex: `[mcp_servers.rtools]\ntype = "http"\nurl = "${url}"\nheaders = { Authorization = "Bearer ${token}" }`,
  };
}

export function fillMcpTemplates(templates: McpTemplates, token: string): McpTemplates {
  return Object.fromEntries(
    Object.entries(templates).map(([key, value]) => [
      key,
      value.split("<从 Engine 状态获取的 token>").join(token),
    ]),
  ) as unknown as McpTemplates;
}

export async function openMonthWindow(): Promise<void> {
  await invoke("open_month_window");
}

export async function openTransferWindow(): Promise<void> {
  await invoke("open_transfer_window");
}
