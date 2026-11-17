import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { engineCall } from "./engine";

export interface TransferChannel {
  host: string;
  port: number;
  protocol: "http" | "https";
}

export interface TransferDevice {
  alias: string;
  version: string;
  device_model?: string;
  device_type?: string;
  fingerprint: string;
  download: boolean;
  channels: TransferChannel[];
}

export interface TransferFileInfo {
  id: string;
  file_name: string;
  size: number;
  file_type?: string;
  sha256?: string | null;
  preview?: string | null;
}

export interface TransferSettings {
  enabled: boolean;
  alias: string;
  port: number;
  receiveDir: string;
  pin: string | null;
  autoAcceptFavorites: boolean;
}

export interface PendingIncoming {
  session_id: string;
  ip: string;
  device: { alias: string; fingerprint: string };
  files: TransferFileInfo[];
}

export type ChatMessageKind = "text" | "image" | "file" | "trust";
export type ChatMessageStatus = "pending" | "sent" | "failed";

export interface ChatMessage {
  id: string;
  conversation_id: string;
  sender: string;
  sender_alias: string;
  kind: ChatMessageKind;
  body: string;
  file_path?: string | null;
  file_size?: number | null;
  preview?: string | null;
  forwarded_from?: string | null;
  created_at: number;
  status: ChatMessageStatus;
  direction?: "send" | "receive";
  progress?: number;
}

export interface TransferStateView {
  running: boolean;
  port: number;
  alias: string;
  fingerprint: string;
  settings: TransferSettings;
  error: string | null;
}

export interface TransferStartInfo {
  running: boolean;
  port: number;
  alias: string;
  fingerprint: string;
}

export type TransferDirection = "send" | "receive";
export type TransferSessionEndReason = "completed" | "cancelled" | "rejected" | "error";

export type TransferEventPayload =
  | { schema_version: number; type: "devices"; devices: TransferDevice[] }
  | {
      schema_version: number;
      type: "incoming";
      session_id: string;
      ip: string;
      device: { alias: string; fingerprint: string };
      files: TransferFileInfo[];
    }
  | {
      schema_version: number;
      type: "progress";
      session_id: string;
      file_id: string;
      direction: TransferDirection;
      transferred: number;
      total: number;
      percent: number;
    }
  | {
      schema_version: number;
      type: "completed";
      session_id: string;
      file_id: string;
      direction: TransferDirection;
      path?: string;
    }
  | {
      schema_version: number;
      type: "error";
      scope: "send" | "receive" | "discovery" | "server";
      code: string;
      message: string;
    }
  | {
      schema_version: number;
      type: "session_ended";
      session_id: string;
      reason: TransferSessionEndReason;
    }
  | { schema_version: number; type: "message"; message: ChatMessage };

interface RawTransferStatus {
  running: boolean;
  port: number;
  alias: string;
  fingerprint: string;
  receive_dir: string;
  pin_configured: boolean;
  auto_accept_favorites: boolean;
  error: string | null;
}

export const ERROR_TEXT: Record<string, string> = {
  PORT_IN_USE: "端口被占用",
  FIREWALL: "被防火墙拦截",
  MULTICAST_UNAVAILABLE: "组播不可用",
  TLS: "证书/加密错误",
  PIN_REQUIRED: "需要 PIN",
  PIN_INVALID: "PIN 错误",
  REJECTED: "对方拒绝",
  CHECKSUM_MISMATCH: "校验失败",
  CANCELLED: "已取消",
  IO: "文件读写失败",
  UNKNOWN: "未知错误",
};

export function errorLabel(code: string): string {
  return ERROR_TEXT[code] ?? code;
}

function statusView(raw: RawTransferStatus): TransferStateView {
  return {
    running: raw.running,
    port: raw.port,
    alias: raw.alias,
    fingerprint: raw.fingerprint,
    error: raw.error,
    settings: {
      enabled: raw.running,
      alias: raw.alias,
      port: raw.port,
      receiveDir: raw.receive_dir,
      pin: raw.pin_configured ? "" : null,
      autoAcceptFavorites: raw.auto_accept_favorites,
    },
  };
}

export async function getTransferState(): Promise<TransferStateView> {
  return statusView(await engineCall<RawTransferStatus>("transfer.status"));
}

export async function startTransfer(): Promise<TransferStartInfo> {
  const state = statusView(await engineCall<RawTransferStatus>("transfer.start"));
  return state;
}

export async function stopTransfer(): Promise<void> {
  await engineCall<RawTransferStatus>("transfer.stop");
}

export async function updateTransferSettings(patch: {
  alias?: string;
  port?: number;
  receiveDir?: string;
  pin?: string | null;
  autoAcceptFavorites?: boolean;
}): Promise<TransferSettings> {
  const state = statusView(
    await engineCall<RawTransferStatus>("transfer.settings", {
      alias: patch.alias ?? "rtools",
      port: patch.port ?? 54417,
      receive_dir: patch.receiveDir ?? "",
      pin: patch.pin ?? "",
      auto_accept_favorites: patch.autoAcceptFavorites ?? false,
    }),
  );
  return state.settings;
}

export async function discoverDevices(): Promise<TransferDevice[]> {
  await engineCall("transfer.refresh");
  return engineCall<TransferDevice[]>("transfer.devices");
}

export async function addManualIp(host: string, port?: number): Promise<TransferDevice | null> {
  return engineCall<TransferDevice | null>("transfer.add_manual_ip", {
    host,
    port: port ?? 54417,
  });
}

export async function sendFiles(
  files: string[],
  fingerprint: string,
  pin?: string,
): Promise<string> {
  const result = await engineCall<{ session_id: string }>("transfer.send", {
    paths: files,
    fingerprint,
    pin: pin || null,
  });
  return result.session_id;
}

export async function respondPrepare(
  sessionId: string,
  acceptedFileIds: string[],
): Promise<void> {
  await engineCall("transfer.respond", {
    session_id: sessionId,
    file_ids: acceptedFileIds,
  });
}

export async function getPendingIncoming(): Promise<PendingIncoming[]> {
  return engineCall<PendingIncoming[]>("transfer.pending");
}

export async function rejectTransfer(sessionId: string): Promise<void> {
  await engineCall("transfer.reject", { session_id: sessionId });
}

export async function cancelTransfer(sessionId: string): Promise<void> {
  await engineCall("transfer.cancel", { session_id: sessionId });
}

export async function openTransferWindow(): Promise<void> {
  await invoke("open_transfer_window");
}

export async function subscribeTransfer(
  handler: (payload: TransferEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<{ event: string; payload: TransferEventPayload }>("engine-event", (event) => {
    if (event.payload.event === "transfer.event") {
      handler(event.payload.payload);
    }
  });
}

export async function sendChatText(
  fingerprint: string,
  message: ChatMessage,
): Promise<void> {
  await engineCall("chat.send_text", { fingerprint, message });
}

export async function sendChatFiles(
  files: string[],
  fingerprint: string,
  previews: Record<string, string>,
  pin?: string,
): Promise<string> {
  void previews;
  void pin;
  const result = await engineCall<{ session_id: string }>("chat.send_files", {
    fingerprint,
    paths: files,
  });
  return result.session_id;
}

export async function deleteChatFile(path: string): Promise<void> {
  await engineCall("chat.delete_file", { path });
}

export async function readFileBytes(path: string): Promise<number[]> {
  return invoke("read_file_bytes", { path });
}

export async function flashTransferWindow(): Promise<void> {
  await invoke("flash_transfer_window");
}

export async function copyChatFile(src: string, dest: string): Promise<void> {
  await invoke("copy_file", { src, dest });
}

export async function chatFileExists(path: string): Promise<boolean> {
  return invoke("file_exists", { path });
}

export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}
