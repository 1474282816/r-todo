/**
 * r-todo 原生 JSON 格式：无损备份 & 迁移
 *
 * 结构见 docs/decisions/003-sync-strategy.md 附录（TODO）
 */
import type { Task } from "../types";

export interface RTodoJsonFile {
  format: "r-todo/todo";
  version: 1;
  exported_at: string;
  todos: Task[];
}

export function toJson(tasks: Task[]): string {
  const file: RTodoJsonFile = {
    format: "r-todo/todo",
    version: 1,
    exported_at: new Date().toISOString(),
    todos: tasks,
  };
  return JSON.stringify(file, null, 2);
}

export function parseJson(content: string): Task[] {
  const parsed = JSON.parse(content);
  if (!parsed || typeof parsed !== "object") {
    throw new Error("非 r-todo JSON 格式（非对象）");
  }
  if (parsed.format !== "r-todo/todo") {
    throw new Error(`不认识的 format: ${parsed.format ?? "未标注"}`);
  }
  if (!Array.isArray(parsed.todos)) {
    throw new Error("todos 字段缺失或非数组");
  }
  return parsed.todos as Task[];
}
