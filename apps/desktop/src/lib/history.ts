/**
 * 历史查询：按日期范围获取已完成的任务
 *
 * SQLite 存的是 ISO 8601 UTC 字符串，可以直接字典序比较，
 * 但月份边界要按**本地时间**算（用户看到的是本地日历）。
 */
import Database from "@tauri-apps/plugin-sql";

import type { Priority, Task } from "./types";

const DB_URL = "sqlite:r-todo.db";

let dbPromise: Promise<Database> | null = null;
function getDb(): Promise<Database> {
  if (!dbPromise) {
    dbPromise = Database.load(DB_URL);
  }
  return dbPromise;
}

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

/**
 * 查询某个本地日历月里所有已完成的任务。
 * @param year 本地年份
 * @param monthIdx 0-11
 */
export async function listCompletedInMonth(
  year: number,
  monthIdx: number,
): Promise<Task[]> {
  const db = await getDb();
  // 本地时间的月初和下月月初，转成 ISO 用于 SQL 比较
  const start = new Date(year, monthIdx, 1, 0, 0, 0, 0).toISOString();
  const end = new Date(year, monthIdx + 1, 1, 0, 0, 0, 0).toISOString();
  const rows = await db.select<TaskRow[]>(
    `SELECT * FROM tasks
     WHERE completed_at IS NOT NULL
       AND completed_at >= $1
       AND completed_at < $2
     ORDER BY completed_at ASC`,
    [start, end],
  );
  return rows.map((r) => ({
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
  }));
}

/** 把当月任务按"本地时间的完成日期"分组 → { "2026-07-18": Task[] } */
export function groupByLocalDate(tasks: Task[]): Map<string, Task[]> {
  const map = new Map<string, Task[]>();
  for (const t of tasks) {
    if (!t.completed_at) continue;
    const d = new Date(t.completed_at);
    const key = `${d.getFullYear()}-${(d.getMonth() + 1)
      .toString()
      .padStart(2, "0")}-${d.getDate().toString().padStart(2, "0")}`;
    const bucket = map.get(key);
    if (bucket) bucket.push(t);
    else map.set(key, [t]);
  }
  return map;
}
