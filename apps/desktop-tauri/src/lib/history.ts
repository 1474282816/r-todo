/**
 * 历史查询：按日期范围获取已完成的任务
 *
 * SQLite 存的是 ISO 8601 UTC 字符串，可以直接字典序比较，
 * 但月份边界要按**本地时间**算（用户看到的是本地日历）。
 */
import { listTasks } from "./engine";
import type { Task } from "./types";

/**
 * 查询某个本地日历月里所有已完成的任务。
 * @param year 本地年份
 * @param monthIdx 0-11
 */
export async function listCompletedInMonth(
  year: number,
  monthIdx: number,
): Promise<Task[]> {
  // 本地时间的月初和下月月初，转成 ISO 用于 SQL 比较
  const start = new Date(year, monthIdx, 1, 0, 0, 0, 0).toISOString();
  const end = new Date(year, monthIdx + 1, 1, 0, 0, 0, 0).toISOString();
  return (await listTasks())
    .filter((task) => !task.deleted && task.completed_at !== null)
    .filter((task) => {
      const completed = task.completed_at ?? "";
      return completed >= start && completed < end;
    })
    .sort((a, b) => (a.completed_at ?? "").localeCompare(b.completed_at ?? ""));
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
