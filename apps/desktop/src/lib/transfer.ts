/**
 * 局域网互传前端桥（Tauri commands + events）
 *
 * 事件契约 v2：`SCHEMA_VERSION = 2`，事件名 transfer-*（新增 transfer-message）。
 * 三端形状由 crates/transfer-core/tests/contract.rs 冻结。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const TRANSFER_EVENTS = {
  devices: "transfer-devices",
  incoming: "transfer-incoming",
  progress: "transfer-progress",
  completed: "transfer-completed",
  error: "transfer-error",
  sessionEnded: "transfer-session-ended",
  message: "transfer-message",
} as const;

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
  /** 图片缩略图 base64（FileDto.preview） */
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

/** 仍在等待确认的接收请求（窗口重开后恢复弹窗用） */
export interface PendingIncoming {
  session_id: string;
  ip: string;
  device: { alias: string; fingerprint: string };
  files: TransferFileInfo[];
}

export type ChatMessageKind = "text" | "image" | "file" | "trust";
export type ChatMessageStatus = "pending" | "sent" | "failed";

/** 与 Rust `ChatMessage` 一一对应；开发期无旧版兼容负担（契约 v2）。 */
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

export type TransferSessionEndReason =
  | "completed"
  | "cancelled"
  | "rejected"
  | "error";

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
  | {
      schema_version: number;
      type: "message";
      message: ChatMessage;
    };

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

export async function getTransferState(): Promise<TransferStateView> {
  return invoke("transfer_get_state");
}

export async function startTransfer(): Promise<TransferStartInfo> {
  return invoke("transfer_start");
}

export async function stopTransfer(): Promise<void> {
  return invoke("transfer_stop");
}

export async function updateTransferSettings(patch: {
  alias?: string;
  port?: number;
  receiveDir?: string;
  pin?: string | null;
  autoAcceptFavorites?: boolean;
}): Promise<TransferSettings> {
  return invoke("transfer_set_settings", patch);
}

export async function discoverDevices(timeoutMs = 1200): Promise<void> {
  return invoke("transfer_discover", { timeoutMs });
}

export async function addManualIp(
  host: string,
  port?: number,
): Promise<TransferDevice | null> {
  return invoke("transfer_add_manual_ip", { host, port });
}

export async function sendFiles(
  files: string[],
  fingerprint: string,
  pin?: string,
): Promise<string> {
  return invoke("transfer_send", { files, fingerprint, pin });
}

export async function respondPrepare(
  sessionId: string,
  acceptedFileIds: string[],
): Promise<void> {
  return invoke("transfer_respond_prepare", { sessionId, acceptedFileIds });
}

export async function respondFile(
  sessionId: string,
  fileId: string,
  savePath?: string,
): Promise<void> {
  return invoke("transfer_respond_file", { sessionId, fileId, savePath });
}

export async function getPendingIncoming(): Promise<PendingIncoming[]> {
  return invoke("transfer_pending");
}

export async function rejectTransfer(sessionId: string): Promise<void> {
  return invoke("transfer_reject", { sessionId });
}

export async function cancelTransfer(sessionId: string): Promise<void> {
  return invoke("transfer_cancel", { sessionId });
}

export async function openTransferWindow(): Promise<void> {
  return invoke("open_transfer_window");
}

/** 订阅全部互传事件，返回取消订阅函数 */
export async function subscribeTransfer(
  handler: (payload: TransferEventPayload) => void,
): Promise<UnlistenFn> {
  const unlisteners = await Promise.all(
    Object.values(TRANSFER_EVENTS).map((name) =>
      listen<TransferEventPayload>(name, (e) => handler(e.payload)),
    ),
  );
  return () => unlisteners.forEach((fn) => fn());
}

// ───────────────────── 会话式互传命令 ─────────────────────

export async function sendChatText(
  fingerprint: string,
  message: ChatMessage,
): Promise<void> {
  return invoke("chat_send_text", { fingerprint, message });
}

export async function sendChatFiles(
  files: string[],
  fingerprint: string,
  previews: Record<string, string>,
  pin?: string,
): Promise<string> {
  return invoke("chat_send_files", { files, fingerprint, previews, pin });
}

/** 只删接收副本；Rust 侧还有接收目录白名单兜底。 */
export async function deleteChatFile(path: string): Promise<void> {
  return invoke("chat_delete_file", { path });
}

export async function readFileBytes(path: string): Promise<number[]> {
  return invoke("chat_read_file_bytes", { path });
}

export async function flashTransferWindow(): Promise<void> {
  return invoke("chat_flash_window");
}

export async function copyChatFile(src: string, dest: string): Promise<void> {
  return invoke("chat_copy_file", { src, dest });
}

export async function chatFileExists(path: string): Promise<boolean> {
  return invoke("chat_file_exists", { path });
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
