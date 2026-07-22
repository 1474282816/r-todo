/**
 * 同步核心（加固版）：PocketBase 双向同步
 *
 * 三处加固（2026-07-20）：
 * 1. PB_CONNECT 监听 → 断线重连后自动 fullSync
 * 2. 软删除 → 删除标记 deleted=true，不硬删
 * 3. 离线队列 → 写入失败时入队 pending_ops，下次重连自动重试
 */
import PocketBase from "pocketbase";

import * as db from "./db";
import type { Task, Priority } from "./types";

interface RemoteTask {
  id: string;
  user: string;
  title: string;
  priority: Priority;
  remind_at: string;
  remind_offset_min: number;
  repeat_rule: string;
  sort_order: number;
  deleted: boolean;
  completed_at: string;
  created: string;
  updated: string;
}

const STORAGE_URL = "r-todo/sync/url";
const STORAGE_AUTH = "r-todo/sync/auth";

let pb: PocketBase | null = null;
let unsubscribeRealtime: (() => void) | null = null;

export interface SyncStatus {
  connected: boolean;
  syncing: boolean;
  flushing: boolean;
  lastSyncAt: string | null;
  error: string | null;
  userEmail: string | null;
  username: string | null;
  serverUrl: string | null;
}

type Listener = (s: SyncStatus) => void;
const listeners = new Set<Listener>();
let status: SyncStatus = {
  connected: false,
  syncing: false,
  flushing: false,
  lastSyncAt: null,
  error: null,
  userEmail: null,
  username: null,
  serverUrl: null,
};

function updateStatus(patch: Partial<SyncStatus>) {
  status = { ...status, ...patch };
  listeners.forEach((l) => l(status));
}

export function getStatus(): SyncStatus { return status; }

export function subscribeStatus(fn: Listener): () => void {
  listeners.add(fn);
  fn(status);
  return () => { listeners.delete(fn); };
}

export async function restoreSession(): Promise<void> {
  const url = localStorage.getItem(STORAGE_URL);
  const authCookie = localStorage.getItem(STORAGE_AUTH);
  if (!url || !authCookie) return;
  try {
    pb = new PocketBase(url);
    pb.authStore.loadFromCookie(authCookie);
    if (!pb.authStore.isValid) { pb = null; return; }
    try { await pb.collection("users").authRefresh(); } catch { /* 网络失败容忍 */ }
    persistAuth();
    updateStatus({ connected: true, userEmail: pb.authStore.record?.email ?? null, username: pb.authStore.record?.username ?? null, serverUrl: url });
    await fullSync();
    await flushPendingOps();
    subscribeRealtime();
  } catch (e) {
    console.warn("[sync] restore failed:", e);
    pb = null;
  }
}

function persistAuth() {
  if (!pb) return;
  localStorage.setItem(STORAGE_URL, pb.baseUrl);
  localStorage.setItem(STORAGE_AUTH, pb.authStore.exportToCookie({ httpOnly: false, sameSite: "Lax" }));
}

export async function login(url: string, email: string, password: string): Promise<void> {
  updateStatus({ syncing: true, error: null });
  try {
    const normalized = url.replace(/\/$/, "");
    pb = new PocketBase(normalized);
    await pb.collection("users").authWithPassword(email, password);
    persistAuth();
    updateStatus({ connected: true, userEmail: pb.authStore.record?.email ?? null, username: pb.authStore.record?.username ?? null, serverUrl: normalized });
    await fullSync();
    await flushPendingOps();
    subscribeRealtime();
  } catch (e) {
    pb = null;
    updateStatus({ connected: false, syncing: false, error: String(e) });
    throw e;
  } finally {
    updateStatus({ syncing: false });
  }
}

export function logout(): void {
  unsubscribeRealtime?.();
  unsubscribeRealtime = null;
  pb?.authStore.clear();
  pb = null;
  localStorage.removeItem(STORAGE_URL);
  localStorage.removeItem(STORAGE_AUTH);
  updateStatus({ connected: false, userEmail: null, username: null, serverUrl: null, error: null, lastSyncAt: null });
}

function toRemote(t: Task): Record<string, unknown> {
  if (!pb || !pb.authStore.record) throw new Error("not authenticated");
  return {
    id: t.id,
    user: pb.authStore.record.id,
    title: t.title,
    priority: t.priority,
    remind_at: t.remind_at ?? "",
    remind_offset_min: t.remind_offset_min,
    repeat_rule: t.repeat_rule ?? "",
    sort_order: t.sort_order,
    deleted: t.deleted,
    completed_at: t.completed_at ?? "",
  };
}

function fromRemote(r: RemoteTask): Task {
  return {
    id: r.id,
    title: r.title,
    priority: r.priority,
    remind_at: r.remind_at || null,
    remind_offset_min: r.remind_offset_min ?? 15,
    repeat_rule: r.repeat_rule || null,
    sort_order: r.sort_order ?? 0,
    deleted: r.deleted ?? false,
    completed_at: r.completed_at || null,
    created_at: r.created,
    updated_at: r.updated,
  };
}

export async function fullSync(): Promise<void> {
  if (!pb || !pb.authStore.isValid) return;
  updateStatus({ syncing: true, error: null });
  try {
    const [localTasks, remoteRecords] = await Promise.all([
      db.listTasks(),
      pb.collection("tasks").getFullList<RemoteTask>({ perPage: 500 }),
    ]);
    const remoteMap = new Map(remoteRecords.map((r) => [r.id, r]));
    const localMap = new Map(localTasks.map((t) => [t.id, t]));
    for (const local of localTasks) {
      if (!remoteMap.has(local.id)) await pb.collection("tasks").create(toRemote(local));
    }
    for (const remote of remoteRecords) {
      if (!localMap.has(remote.id)) await db.createTaskRaw(fromRemote(remote));
    }
    for (const local of localTasks) {
      const remote = remoteMap.get(local.id);
      if (!remote) continue;
      const lt = new Date(local.updated_at).getTime();
      const rt = new Date(remote.updated).getTime();
      if (lt > rt) await pb.collection("tasks").update(local.id, toRemote(local));
      else if (rt > lt) await db.updateTask(local.id, fromRemote(remote));
    }
    updateStatus({ lastSyncAt: new Date().toISOString(), error: null });
    // 通知 store 重新读取本地数据库
    onRemoteChangeCallbacks.forEach((fn) => fn());
  } catch (e) {
    console.error("[sync] fullSync failed:", e);
    updateStatus({ error: String(e) });
    throw e;
  } finally {
    updateStatus({ syncing: false });
  }
}

// ── 离线队列 ──
export async function flushPendingOps(): Promise<void> {
  if (!pb || !pb.authStore.isValid) return;
  updateStatus({ flushing: true, error: null });
  try {
    const ops = await db.listPendingOps();
    if (ops.length === 0) return;
    for (const op of ops) {
      try {
        if (op.op_type === "create") {
          await pb.collection("tasks").create(JSON.parse(op.payload ?? "{}"));
        } else if (op.op_type === "update") {
          await pb.collection("tasks").update(op.record_id, JSON.parse(op.payload ?? "{}"));
        } else if (op.op_type === "delete") {
          await pb.collection("tasks").update(op.record_id, { deleted: true });
        }
        await db.removePendingOp(op.id);
      } catch (e) {
        const err = e as { status?: number; isAbort?: boolean };
        if (err.status === 0 || err.isAbort) break;
        if (err.status === 400 || err.status === 404) {
          await db.markPendingOpError(op.id, String(e));
          continue;
        }
        break;
      }
    }
  } catch (e) {
    console.error("[sync] flushPendingOps failed:", e);
  } finally {
    updateStatus({ flushing: false });
  }
}

export async function pushTask(t: Task): Promise<void> {
  if (!pb || !pb.authStore.isValid) return;
  try {
    try { await pb.collection("tasks").update(t.id, toRemote(t)); }
    catch (e) {
      const err = e as { status?: number };
      if (err.status === 404) await pb.collection("tasks").create(toRemote(t));
      else throw e;
    }
    updateStatus({ lastSyncAt: new Date().toISOString() });
  } catch (e) {
    console.warn("[sync] pushTask failed, enqueuing:", e);
    await db.enqueueOp("update", t.id, toRemote(t) as Record<string, unknown>);
    updateStatus({ error: String(e) });
  }
}

export async function deleteRemote(id: string): Promise<void> {
  if (!pb || !pb.authStore.isValid) return;
  try {
    await pb.collection("tasks").update(id, { deleted: true });
    updateStatus({ lastSyncAt: new Date().toISOString() });
  } catch (e) {
    const err = e as { status?: number };
    if (err.status === 404) { await db.enqueueOp("delete", id); return; }
    await db.enqueueOp("delete", id);
    updateStatus({ error: String(e) });
  }
}

// ── Realtime ──
type RealtimeEvent = { action: "create" | "update" | "delete"; record: RemoteTask };

export function subscribeRealtime() {
  if (!pb) return;
  unsubscribeRealtime?.();
  // PB_CONNECT：每次连接/重连触发全量同步
  pb.realtime.subscribe("PB_CONNECT", async () => {
    await fullSync().catch(() => {});
    await flushPendingOps().catch(() => {});
  }).catch(() => {});
  pb.collection("tasks").subscribe<RemoteTask>("*", async (e) => {
    const ev = e as RealtimeEvent;
    try {
      if (ev.action === "delete") {
        await db.softDeleteTask(ev.record.id);
      } else {
        const t = fromRemote(ev.record);
        if (t.deleted) {
          const local = (await db.listTasks()).find((x) => x.id === t.id);
          if (local && new Date(t.updated_at) > new Date(local.updated_at)) {
            await db.softDeleteTask(t.id);
          } else if (!local) {
            await db.createTaskRaw(t);
          }
        } else {
          const local = (await db.listTasks()).find((x) => x.id === t.id);
          if (!local) await db.createTaskRaw(t);
          else if (new Date(t.updated_at) > new Date(local.updated_at)) await db.updateTask(t.id, t);
        }
      }
      onRemoteChangeCallbacks.forEach((fn) => fn());
    } catch (err) { console.error("[sync] realtime handler failed:", err); }
  }).then((unsub) => { unsubscribeRealtime = unsub; })
    .catch((err) => { console.error("[sync] subscribe failed:", err); });
}

const onRemoteChangeCallbacks = new Set<() => void>();
export function onRemoteChange(cb: () => void): () => void {
  onRemoteChangeCallbacks.add(cb);
  return () => { onRemoteChangeCallbacks.delete(cb); };
}