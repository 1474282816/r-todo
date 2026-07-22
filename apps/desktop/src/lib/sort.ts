/**
 * Task 排序：与 SQL ORDER BY 保持一致的纯函数版本
 *
 * 排序规则：
 *   1. 未完成 (completed_at == null) 在上
 *   2. 未完成组内：按 sort_order 升序（手动拖拽结果）
 *   3. 已完成组内：按 completed_at 降序（最近完成的靠上）
 *   4. tiebreaker：created_at 升序
 */
import type { Task } from "./types";

export function sortTasks(tasks: Task[]): Task[] {
  return [...tasks].sort((a, b) => {
    const aCompleted = a.completed_at ? 1 : 0;
    const bCompleted = b.completed_at ? 1 : 0;
    if (aCompleted !== bCompleted) return aCompleted - bCompleted;

    if (!a.completed_at) {
      if (a.sort_order !== b.sort_order) return a.sort_order - b.sort_order;
    } else {
      const cmp = (b.completed_at ?? "").localeCompare(a.completed_at ?? "");
      if (cmp !== 0) return cmp;
    }
    return a.created_at.localeCompare(b.created_at);
  });
}
