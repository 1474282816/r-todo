/**
 * SQLite 数据访问层 (基于 tauri-plugin-sql)
 *
 * 单例 Database，模块级懒加载。
 */
import Database from "@tauri-apps/plugin-sql";
import { nanoid } from "nanoid";

import type { Priority, Task, TaskDraft } from "./types";
import { getDefaultOffset } from "./reminder";

const DB_URL = "sqlite:r-todo.db";

let dbPromise: Promise<Database> | null = null;
function getDb(): Promise<Database> {
  if (!dbPromise) {
    dbPromise = Database.load(DB_URL);
  }
  return dbPromise;
}

/** SQLite 行转 Task */
interface TaskRow {
  id: string;
  title: string;
  priority: Priority;
  remind_at: string | null;
  remind_offset_min: number;
  repeat_rule: string | null;
  sort_order: number;
  deleted: number;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface PendingOp {
  id: number;
  op_type: "create" | "update" | "delete";
  collection: string;
  record_id: string;
  payload: string | null;
  created_at: number;
  attempts: number;
  last_error: string | null;
}

function rowToTask(r: TaskRow): Task {
  return {
    id: r.id,
    title: r.title,
    priority: r.priority,
    remind_at: r.remind_at,
    remind_offset_min: r.remind_offset_min,
    repeat_rule: r.repeat_rule,
    sort_order: r.sort_order,
    deleted: r.deleted === 1,
    completed_at: r.completed_at,
    created_at: r.created_at,
    updated_at: r.updated_at,
  };
}

export async function listTasks(): Promise<Task[]> {
  const db = await getDb();
  const rows = await db.select<TaskRow[]>(
    `SELECT * FROM tasks WHERE deleted = 0 ORDER BY
      CASE WHEN completed_at IS NULL THEN 0 ELSE 1 END,
      CASE WHEN completed_at IS NULL THEN sort_order ELSE NULL END ASC,
      CASE WHEN completed_at IS NOT NULL THEN completed_at ELSE NULL END DESC,
      created_at ASC`,
  );
  return rows.map(rowToTask);
}

/** 当前未完成任务里最小的 sort_order；空表返回 0 */
async function getMinSortOrder(): Promise<number> {
  const db = await getDb();
  const rows = await db.select<{ min: number | null }[]>(
    `SELECT MIN(sort_order) as min FROM tasks WHERE completed_at IS NULL AND deleted = 0`,
  );
  return rows[0]?.min ?? 0;
}

export async function createTask(draft: TaskDraft): Promise<Task> {
  const db = await getDb();
  const now = new Date().toISOString();
  const minOrder = await getMinSortOrder();
  const task: Task = {
    id: nanoid(15),
    title: draft.title.trim(),
    priority: draft.priority ?? "medium",
    remind_at: draft.remind_at ?? null,
    remind_offset_min: draft.remind_offset_min ?? getDefaultOffset(),
    repeat_rule: draft.repeat_rule ?? null,
    sort_order: minOrder - 1000,
    deleted: false,
    completed_at: draft.completed_at ?? null,
    created_at: now,
    updated_at: now,
  };
  await db.execute(
    `INSERT INTO tasks (id, title, priority, remind_at, remind_offset_min, repeat_rule, sort_order, deleted, completed_at, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)`,
    [
      task.id, task.title, task.priority, task.remind_at, task.remind_offset_min,
      task.repeat_rule, task.sort_order, task.deleted ? 1 : 0, task.completed_at, task.created_at, task.updated_at,
    ],
  );
  return task;
}

export async function createTaskRaw(t: Task): Promise<void> {
  const db = await getDb();
  let order = t.sort_order;
  if (order == null || Number.isNaN(order) || order === 0) {
    const minOrder = await getMinSortOrder();
    order = minOrder - 1000;
  }
  await db.execute(
    `INSERT OR REPLACE INTO tasks (id, title, priority, remind_at, remind_offset_min, repeat_rule, sort_order, deleted, completed_at, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)`,
    [
      t.id, t.title, t.priority, t.remind_at, t.remind_offset_min,
      t.repeat_rule, order, t.deleted ? 1 : 0, t.completed_at, t.created_at, t.updated_at,
    ],
  );
}

export async function updateSortOrder(id: string, order: number): Promise<void> {
  const db = await getDb();
  const now = new Date().toISOString();
  await db.execute(
    `UPDATE tasks SET sort_order = $1, updated_at = $2 WHERE id = $3`,
    [order, now, id],
  );
}

export async function updateTask(
  id: string,
  patch: Partial<Omit<Task, "id" | "created_at">>,
): Promise<void> {
  const db = await getDb();
  const fields: string[] = [];
  const values: (string | number | null)[] = [];
  let i = 1;
  for (const [k, v] of Object.entries(patch)) {
    fields.push(`${k} = $${i++}`);
    values.push(k === "deleted" ? (v ? 1 : 0) : (v as string | number | null));
  }
  const now = new Date().toISOString();
  fields.push(`updated_at = $${i++}`);
  values.push(now);
  values.push(id);
  await db.execute(
    `UPDATE tasks SET ${fields.join(", ")} WHERE id = $${i}`,
    values,
  );
}

/** 软删除：标记 deleted=1 并记录 deleted_at */
export async function softDeleteTask(id: string): Promise<void> {
  const now = new Date().toISOString();
  await updateTask(id, { deleted: true, completed_at: null } as Partial<Task>);
  const db = await getDb();
  await db.execute(
    `UPDATE tasks SET deleted_at = $1, updated_at = $2 WHERE id = $3`,
    [now, now, id],
  );
}

export async function toggleComplete(id: string, completed: boolean): Promise<void> {
  const now = completed ? new Date().toISOString() : null;
  await updateTask(id, { completed_at: now });
}

// ── pending_ops 离线队列 ──

export async function enqueueOp(
  opType: "create" | "update" | "delete",
  recordId: string,
  payload?: Record<string, unknown>,
): Promise<void> {
  const db = await getDb();
  await db.execute(
    `INSERT INTO pending_ops (op_type, collection, record_id, payload, created_at)
     VALUES ($1, $2, $3, $4, $5)`,
    ["tasks", opType, recordId, payload ? JSON.stringify(payload) : null, Date.now()],
  );
}

export async function listPendingOps(): Promise<PendingOp[]> {
  const db = await getDb();
  return await db.select<PendingOp[]>(
    `SELECT * FROM pending_ops ORDER BY id ASC`,
  );
}

export async function removePendingOp(id: number): Promise<void> {
  const db = await getDb();
  await db.execute(`DELETE FROM pending_ops WHERE id = $1`, [id]);
}

export async function markPendingOpError(id: number, error: string): Promise<void> {
  const db = await getDb();
  await db.execute(
    `UPDATE pending_ops SET attempts = attempts + 1, last_error = $1 WHERE id = $2`,
    [error, id],
  );
}