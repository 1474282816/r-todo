/**
 * 导入/导出 UI 层封装（使用 Tauri dialog + fs 插件）
 */
import { open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";

import { detectFormat, parseByFormat, planMerge, type MergePlan } from "./io/import";
import { toIcal } from "./io/ical";
import { toJson } from "./io/json";
import type { Task } from "./types";

export type ExportFormat = "json" | "ical";

export async function exportTasks(
  format: ExportFormat,
  tasks: Task[],
): Promise<string | null> {
  const filters =
    format === "json"
      ? [{ name: "RTools JSON", extensions: ["json"] }]
      : [{ name: "iCalendar", extensions: ["ics"] }];
  const defaultName =
    format === "json"
      ? `rtools-${new Date().toISOString().slice(0, 10)}.json`
      : `rtools-${new Date().toISOString().slice(0, 10)}.ics`;

  const path = await save({ defaultPath: defaultName, filters });
  if (!path) return null;

  const content = format === "json" ? toJson(tasks) : toIcal(tasks);
  await writeTextFile(path, content);
  return path;
}

export async function exportTickTickCsv(tasks: Task[]): Promise<string | null> {
  const path = await save({
    defaultPath: `rtools-${new Date().toISOString().slice(0, 10)}.csv`,
    filters: [{ name: "TickTick CSV", extensions: ["csv"] }],
  });
  if (!path) return null;
  const lines = [
    "Folder Name,Title,Priority,Due Date,Reminder,Repeat,Status,Created Time,Completed Time,taskId",
  ];
  for (const task of tasks) {
    const priority = task.priority === "high" ? 5 : task.priority === "medium" ? 3 : 1;
    lines.push(
      [
        csv(""),
        csv(task.title),
        csv(String(priority)),
        csv(task.remind_at ?? ""),
        csv(task.remind_at ? `-PT${task.remind_offset_min}M` : ""),
        csv(task.repeat_rule ?? ""),
        csv(task.completed_at ? "1" : "0"),
        csv(task.created_at),
        csv(task.completed_at ?? ""),
        csv(task.id),
      ].join(","),
    );
  }
  await writeTextFile(path, `${lines.join("\r\n")}\r\n`);
  return path;
}

function csv(value: string): string {
  return `"${value.replace(/"/g, '""')}"`;
}

export interface ImportPreview {
  path: string;
  plan: MergePlan;
}

/**
 * 弹文件选择器 → 读文件 → 检测格式 → 解析 → 生成合并计划
 * 返回 null 表示用户取消
 */
export async function pickAndPreviewImport(
  existing: Task[],
): Promise<ImportPreview | null> {
  const path = await open({
    multiple: false,
    filters: [
      {
        name: "支持的格式",
        extensions: ["json", "ics", "ical", "csv"],
      },
    ],
  });
  if (!path || Array.isArray(path)) return null;

  const content = await readTextFile(path);
  const fileName = path.split(/[\\/]/).pop() ?? "";
  const format = detectFormat(fileName, content);
  const parsed = parseByFormat(format, content);
  const plan = planMerge(parsed, existing);
  return { path, plan };
}
