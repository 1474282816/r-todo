import { nanoid } from "nanoid";

import { engineCall } from "./engine";
import type { ChatMessage, ChatMessageKind, ChatMessageStatus } from "./transfer";

export interface Conversation {
  id: string;
  alias: string;
  last_message_at: number;
  unread: number;
  pinned: number;
}

export interface ChatRow {
  id: string;
  conversation_id: string;
  sender: string;
  sender_alias: string;
  kind: ChatMessageKind;
  body: string;
  file_path: string | null;
  file_size: number | null;
  preview: string | null;
  forwarded_from: string | null;
  created_at: number;
  status: ChatMessageStatus;
  direction: "send" | "receive";
  progress: number;
}

export interface OutboxRow {
  id: string;
  payload: string;
  target: string;
  attempts: number;
  last_error: string | null;
  created_at: number;
}

export async function initChatDb(): Promise<void> {}

export async function listConversations(): Promise<Conversation[]> {
  return engineCall<Conversation[]>("chat.list_conversations");
}

export async function upsertConversation(
  id: string,
  alias: string,
  at: number,
  unreadDelta = 0,
): Promise<void> {
  void id;
  void alias;
  void at;
  void unreadDelta;
}

export async function markRead(id: string): Promise<void> {
  await engineCall("chat.mark_read", { conversation_id: id });
}

export async function listMessages(
  conversationId: string,
  limit = 300,
): Promise<ChatRow[]> {
  void limit;
  return engineCall<ChatRow[]>("chat.list_messages", {
    conversation_id: conversationId,
  });
}

export async function insertMessage(row: ChatRow): Promise<void> {
  void row;
}

export async function patchMessage(
  id: string,
  patch: Partial<Pick<ChatRow, "status" | "file_path" | "file_size" | "preview" | "progress" | "forwarded_from" | "body">>,
): Promise<void> {
  void id;
  void patch;
}

export async function deleteMessageRow(id: string): Promise<void> {
  void id;
}

export async function clearConversation(id: string): Promise<void> {
  await engineCall("chat.clear", { conversation_id: id });
}

export async function isTrusted(fingerprint: string): Promise<boolean> {
  const result = await engineCall<{ trusted: boolean }>("chat.trusted", {
    fingerprint,
  });
  return result.trusted;
}

export async function trust(fingerprint: string): Promise<void> {
  await engineCall("chat.trust", { fingerprint });
}

export async function revokeTrust(fingerprint: string): Promise<void> {
  await engineCall("chat.revoke_trust", { fingerprint });
}

export async function enqueueOutbox(
  target: string,
  message: ChatMessage,
): Promise<void> {
  void target;
  void message;
}

export async function listOutbox(): Promise<OutboxRow[]> {
  return [];
}

export async function removeOutbox(id: string): Promise<void> {
  void id;
}

export async function bumpOutbox(id: string, error: string): Promise<void> {
  void id;
  void error;
}

export function newMessage(
  conversationId: string,
  sender: string,
  senderAlias: string,
  kind: ChatMessageKind,
  body: string,
  extra: Partial<ChatMessage> = {},
): ChatMessage {
  return {
    id: nanoid(),
    conversation_id: conversationId,
    sender,
    sender_alias: senderAlias,
    kind,
    body,
    file_path: null,
    file_size: null,
    preview: null,
    forwarded_from: null,
    created_at: Date.now(),
    status: "pending",
    ...extra,
  };
}

export function rowFromMessage(
  message: ChatMessage,
  direction: "send" | "receive",
  extra: Partial<ChatRow> = {},
): ChatRow {
  return {
    id: message.id,
    conversation_id: message.conversation_id,
    sender: message.sender,
    sender_alias: message.sender_alias,
    kind: message.kind,
    body: message.body,
    file_path: message.file_path ?? null,
    file_size: message.file_size ?? null,
    preview: message.preview ?? null,
    forwarded_from: message.forwarded_from ?? null,
    created_at: message.created_at,
    status: message.status,
    direction,
    progress: message.progress ?? 0,
    ...extra,
  };
}

export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
