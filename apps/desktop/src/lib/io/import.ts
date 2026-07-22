/**
 * 导入统一入口：格式检测 + 合并策略
 */
import type { Task } from "../types";
import { parseIcal } from "./ical";
import { parseJson } from "./json";
import { parseTickTickCsv } from "./ticktick";

export type ImportFormat = "r-todo-json" | "ical" | "ticktick-csv";

export function detectFormat(fileName: string, content: string): ImportFormat {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".json")) return "r-todo-json";
  if (lower.endsWith(".ics") || lower.endsWith(".ical")) return "ical";
  if (lower.endsWith(".csv")) return "ticktick-csv";

  // 内容嗅探
  const trimmed = content.trimStart();
  if (trimmed.startsWith("BEGIN:VCALENDAR")) return "ical";
  if (trimmed.startsWith("{")) return "r-todo-json";
  return "ticktick-csv";
}

export function parseByFormat(format: ImportFormat, content: string): Task[] {
  switch (format) {
    case "r-todo-json":
      return parseJson(content);
    case "ical":
      return parseIcal(content);
    case "ticktick-csv":
      return parseTickTickCsv(content);
  }
}

export interface MergePlan {
  incoming: Task[];
  toInsert: Task[]; // 本地没有
  toUpdate: Task[]; // 本地有但导入的更新
  toSkip: Task[]; // 本地更新（跳过覆盖）
}

/**
 * 生成合并计划（不落库，仅供预览）
 * 策略：id 匹配 → updated_at 较新的赢
 */
export function planMerge(incoming: Task[], existing: Task[]): MergePlan {
  const existingMap = new Map(existing.map((t) => [t.id, t]));
  const toInsert: Task[] = [];
  const toUpdate: Task[] = [];
  const toSkip: Task[] = [];

  for (const t of incoming) {
    const local = existingMap.get(t.id);
    if (!local) {
      toInsert.push(t);
      continue;
    }
    if (new Date(t.updated_at) > new Date(local.updated_at)) {
      toUpdate.push(t);
    } else {
      toSkip.push(t);
    }
  }
  return { incoming, toInsert, toUpdate, toSkip };
}
