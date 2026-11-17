/**
 * TickTick / 滴答清单 CSV 导入（导入-only）
 *
 * TickTick 导出的 CSV：
 * - 前若干行是元数据（Date/Version/Status 说明）
 * - 真正的表头以 "Folder Name" 开头
 * - Priority: 0=none 1=low 3=medium 5=high
 * - Status: 0=Active 1=Completed 2=Archived
 * - Repeat: 直接就是 RRULE 字符串
 * - Reminder: ISO 8601 duration，如 `-PT30M`
 */
import Papa from "papaparse";
import { nanoid } from "nanoid";

import type { Priority, Task } from "../types";

const PRIORITY_MAP: Record<string, Priority> = {
  "5": "high",
  "3": "medium",
  "1": "low",
  "0": "low",
};

interface TickTickRow {
  Title?: string;
  Priority?: string;
  "Due Date"?: string;
  Reminder?: string;
  Repeat?: string;
  Status?: string;
  "Created Time"?: string;
  "Completed Time"?: string;
  taskId?: string;
}

function parseTickTickTime(raw: string | undefined): string | null {
  if (!raw) return null;
  const s = raw.trim();
  if (!s) return null;
  // TickTick 用 "2022-03-28T18:30:00+0000"，JS Date 能解析
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return null;
  return d.toISOString();
}

function parseIsoDurationToMinutes(raw: string | undefined): number {
  // 例："-PT30M", "-PT2H", "-P1D", 空
  if (!raw) return 15;
  const m = raw.match(/-?P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?)?/);
  if (!m) return 15;
  const days = parseInt(m[1] ?? "0", 10);
  const hours = parseInt(m[2] ?? "0", 10);
  const mins = parseInt(m[3] ?? "0", 10);
  const total = days * 1440 + hours * 60 + mins;
  return total > 0 ? total : 15;
}

export function parseTickTickCsv(content: string): Task[] {
  // 找到真正的表头行
  const lines = content.split(/\r?\n/);
  let headerIdx = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('"Folder Name"') || lines[i].startsWith("Folder Name")) {
      headerIdx = i;
      break;
    }
  }
  if (headerIdx < 0) {
    throw new Error("看起来不是 TickTick CSV（未找到 'Folder Name' 表头）");
  }

  const csvBody = lines.slice(headerIdx).join("\n");
  const result = Papa.parse<TickTickRow>(csvBody, {
    header: true,
    skipEmptyLines: true,
  });

  if (result.errors.length > 0) {
    console.warn("TickTick CSV parse warnings:", result.errors);
  }

  const nowIso = new Date().toISOString();
  const tasks: Task[] = [];
  for (const row of result.data) {
    const title = row.Title?.trim();
    if (!title) continue;

    const status = (row.Status ?? "0").trim();
    const isCompleted = status !== "0";
    const completedIso = isCompleted
      ? parseTickTickTime(row["Completed Time"]) ?? nowIso
      : null;

    tasks.push({
      id: row.taskId?.trim() || nanoid(15),
      title,
      priority: PRIORITY_MAP[(row.Priority ?? "0").trim()] ?? "low",
      remind_at: parseTickTickTime(row["Due Date"]),
      remind_offset_min: parseIsoDurationToMinutes(row.Reminder),
      repeat_rule: row.Repeat?.trim() || null,
      sort_order: 0,
      deleted: false,
      completed_at: completedIso,
      created_at: parseTickTickTime(row["Created Time"]) ?? nowIso,
      updated_at: nowIso,
    });
  }
  return tasks;
}
