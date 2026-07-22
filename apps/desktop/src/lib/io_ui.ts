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
      ? [{ name: "r-todo JSON", extensions: ["json"] }]
      : [{ name: "iCalendar", extensions: ["ics"] }];
  const defaultName =
    format === "json"
      ? `r-todo-${new Date().toISOString().slice(0, 10)}.json`
      : `r-todo-${new Date().toISOString().slice(0, 10)}.ics`;

  const path = await save({ defaultPath: defaultName, filters });
  if (!path) return null;

  const content = format === "json" ? toJson(tasks) : toIcal(tasks);
  await writeTextFile(path, content);
  return path;
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
