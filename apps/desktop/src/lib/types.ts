/**
 * Task 域类型（与 packages/schema/task.schema.json 保持一致）
 * 修改字段时必须同步：
 * - packages/schema/task.schema.json
 * - apps/desktop/src-tauri/src/db.rs (migration)
 * - apps/mobile/lib/shared/models/task.dart
 */

export type Priority = "high" | "medium" | "low";

export interface Task {
  id: string;
  title: string;
  priority: Priority;
  /** ISO 8601 UTC, null = 不提醒 */
  remind_at: string | null;
  /** 提前提醒分钟数，默认 15 */
  remind_offset_min: number;
  /** RRULE (RFC 5545) 字符串，null = 不重复 */
  repeat_rule: string | null;
  /** 手动拖拽排序值，越小越靠上 */
  sort_order: number;
  /** 软删除标记，true = 已删除 */
  deleted: boolean;
  /** ISO 8601 UTC, null = 未完成 */
  completed_at: string | null;
  created_at: string;
  updated_at: string;
}

export type TaskDraft = Partial<Omit<Task, "id" | "created_at" | "updated_at">> & {
  title: string;
};

export const PRIORITY_ORDER: Record<Priority, number> = {
  high: 0,
  medium: 1,
  low: 2,
};

export const PRIORITY_LABEL: Record<Priority, string> = {
  high: "高",
  medium: "中",
  low: "低",
};

export const PRIORITY_COLOR: Record<Priority, string> = {
  high: "#ef4444",
  medium: "#f59e0b",
  low: "#9ca3af",
};
