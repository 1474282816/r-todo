/**
 * 会话式互传的本地存储（只新增表，绝不触碰 tasks / pending_ops）。
 *
 * migration v4 由 db.rs 建表；这里再跑一遍幂等 DDL，保证开发期热重载也能用。
 * 消息不进 pending_ops、不接 PocketBase 同步（Q3/Q14/Q44）。
 */
import Database from "@tauri-apps/plugin-sql";
import { nanoid } from "nanoid";

import type { ChatMessage, ChatMessageKind, ChatMessageStatus } from "./transfer";

const DB_URL = "sqlite:r-todo.db";

const DDL = [
  `CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY NOT NULL, alias TEXT NOT NULL,
    last_message_at INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0)`,
  `CREATE INDEX IF NOT EXISTS idx_conversations_recent ON conversations(last_message_at DESC)`,
  `CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL,
    sender TEXT NOT NULL, sender_alias TEXT NOT NULL, kind TEXT NOT NULL,
    body TEXT NOT NULL, file_path TEXT, file_size INTEGER, preview TEXT,
    forwarded_from TEXT, created_at INTEGER NOT NULL, status TEXT NOT NULL,
    direction TEXT NOT NULL DEFAULT 'receive', progress REAL NOT NULL DEFAULT 0)`,
  `CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at)`,
  `CREATE TABLE IF NOT EXISTS outbox (
    id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL, target TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_at INTEGER NOT NULL)`,
  `CREATE TABLE IF NOT EXISTS trusted_peers (
    fingerprint TEXT PRIMARY KEY NOT NULL, trusted_at INTEGER NOT NULL)`,
];

let dbPromise: Promise<Database> | null = null;
function getDb(): Promise<Database> {
  if (!dbPromise) dbPromise = Database.load(DB_URL);
  return dbPromise;
}

export async function initChatDb(): Promise<void> {
  const db = await getDb();
  for (const sql of DDL) await db.execute(sql);
}

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

export async function listConversations(): Promise<Conversation[]> {
  const db = await getDb();
  return db.select<Conversation[]>(
    "SELECT * FROM conversations ORDER BY pinned DESC, last_message_at DESC",
  );
}

export async function upsertConversation(
  id: string,
  alias: string,
  at: number,
  unreadDelta = 0,
): Promise<void> {
  const db = await getDb();
  await db.execute(
    `INSERT INTO conversations (id, alias, last_message_at, unread)
     VALUES (?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET
       alias = excluded.alias,
       last_message_at = MAX(last_message_at, excluded.last_message_at),
       unread = unread + excluded.unread`,
    [id, alias, at, unreadDelta],
  );
}

export async function markRead(id: string): Promise<void> {
  const db = await getDb();
  await db.execute("UPDATE conversations SET unread = 0 WHERE id = ?", [id]);
}

export async function listMessages(
  conversationId: string,
  limit = 300,
): Promise<ChatRow[]> {
  const db = await getDb();
  return db.select<ChatRow[]>(
    "SELECT * FROM messages WHERE conversation_id = ? ORDER BY created_at ASC LIMIT ?",
    [conversationId, limit],
  );
}

export async function insertMessage(row: ChatRow): Promise<void> {
  const db = await getDb();
  await db.execute(
    `INSERT OR REPLACE INTO messages
      (id, conversation_id, sender, sender_alias, kind, body, file_path,
       file_size, preview, forwarded_from, created_at, status, direction, progress)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      row.id,
      row.conversation_id,
      row.sender,
      row.sender_alias,
      row.kind,
      row.body,
      row.file_path,
      row.file_size,
      row.preview,
      row.forwarded_from,
      row.created_at,
      row.status,
      row.direction,
      row.progress,
    ],
  );
}

export async function patchMessage(
  id: string,
  patch: Partial<
    Pick<
      ChatRow,
      | "status"
      | "file_path"
      | "file_size"
      | "preview"
      | "progress"
      | "forwarded_from"
      | "body"
    >
  >,
): Promise<void> {
  const entries = Object.entries(patch).filter(([, v]) => v !== undefined);
  if (entries.length === 0) return;
  const db = await getDb();
  await db.execute(
    `UPDATE messages SET ${entries.map(([k]) => `${k} = ?`).join(", ")} WHERE id = ?`,
    [...entries.map(([, v]) => v), id],
  );
}

export async function deleteMessageRow(id: string): Promise<void> {
  const db = await getDb();
  await db.execute("DELETE FROM messages WHERE id = ?", [id]);
}

export async function clearConversation(id: string): Promise<void> {
  const db = await getDb();
  await db.execute("DELETE FROM messages WHERE conversation_id = ?", [id]);
  await db.execute("DELETE FROM conversations WHERE id = ?", [id]);
}

// ─────────────────── 首次确认（Q29/Q36） ───────────────────

export async function isTrusted(fingerprint: string): Promise<boolean> {
  const db = await getDb();
  const rows = await db.select<{ fingerprint: string }[]>(
    "SELECT fingerprint FROM trusted_peers WHERE fingerprint = ?",
    [fingerprint.toUpperCase()],
  );
  return rows.length > 0;
}

export async function trust(fingerprint: string): Promise<void> {
  const db = await getDb();
  await db.execute(
    "INSERT OR IGNORE INTO trusted_peers (fingerprint, trusted_at) VALUES (?, ?)",
    [fingerprint.toUpperCase(), Date.now()],
  );
}

export async function revokeTrust(fingerprint: string): Promise<void> {
  const db = await getDb();
  await db.execute("DELETE FROM trusted_peers WHERE fingerprint = ?", [
    fingerprint.toUpperCase(),
  ]);
}

// ─────────────────── 待发送队列（Q10） ───────────────────

export async function enqueueOutbox(
  target: string,
  message: ChatMessage,
): Promise<void> {
  const db = await getDb();
  await db.execute(
    "INSERT OR REPLACE INTO outbox (id, payload, target, created_at) VALUES (?, ?, ?, ?)",
    [message.id, JSON.stringify(message), target.toUpperCase(), Date.now()],
  );
}

export async function listOutbox(): Promise<OutboxRow[]> {
  const db = await getDb();
  return db.select<OutboxRow[]>("SELECT * FROM outbox ORDER BY created_at ASC");
}

export async function removeOutbox(id: string): Promise<void> {
  const db = await getDb();
  await db.execute("DELETE FROM outbox WHERE id = ?", [id]);
}

export async function bumpOutbox(id: string, error: string): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE outbox SET attempts = attempts + 1, last_error = ? WHERE id = ?",
    [error, id],
  );
}

// ─────────────────── 辅助 ───────────────────

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
    status: "sent",
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
    progress: 0,
    ...extra,
  };
}

export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
