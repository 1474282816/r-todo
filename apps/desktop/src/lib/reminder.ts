/**
 * 提醒调度前端桥
 *
 * 调用 Rust 侧的 schedule_reminder / cancel_reminder command，
 * 由 Rust 用 tokio 定时任务在到点时通过 tauri-plugin-notification 弹通知。
 */
import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

import { formatDateTime } from "./datetime";
import type { Task } from "./types";

const DEFAULT_OFFSET_KEY = "r-todo/default-remind-offset";

/** 全局默认提前提醒分钟数（新建任务时使用） */
export function getDefaultOffset(): number {
  const v = localStorage.getItem(DEFAULT_OFFSET_KEY);
  return v ? Number(v) || 15 : 15;
}

export function setDefaultOffset(min: number): void {
  localStorage.setItem(DEFAULT_OFFSET_KEY, String(min));
}

async function ensurePermission(): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    const p = await requestPermission();
    granted = p === "granted";
  }
  return granted;
}

/** 计算实际触发时刻：remind_at - remind_offset_min */
function computeFireAt(task: Task): Date | null {
  if (!task.remind_at) return null;
  const at = new Date(task.remind_at);
  return new Date(at.getTime() - task.remind_offset_min * 60_000);
}

export async function rescheduleReminder(task: Task): Promise<void> {
  const fireAt = computeFireAt(task);
  if (!fireAt) return;

  await ensurePermission();
  await invoke("schedule_reminder", {
    req: {
      task_id: task.id,
      fire_at: fireAt.toISOString(),
      title: task.title,
      body: `将于 ${formatDateTime(task.remind_at)} 到期`,
    },
  });
}

export async function cancelReminder(taskId: string): Promise<void> {
  await invoke("cancel_reminder", { taskId });
}
