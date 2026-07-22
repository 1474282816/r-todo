/**
 * 全局 Task 状态（Zustand + Immer）
 *
 * 单一数据源。所有 mutation 走 store 里的 action，action 先写 DB 再更新内存。
 */
import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import * as db from "./db";
import { rescheduleReminder, cancelReminder } from "./reminder";
import { rollNext } from "./rrule";
import { deleteRemote, onRemoteChange, pushTask } from "./sync";
import type { Task, TaskDraft } from "./types";

interface TaskState {
  tasks: Task[];
  loading: boolean;
  loaded: boolean;

  hydrate: () => Promise<void>;
  add: (draft: TaskDraft) => Promise<Task>;
  update: (id: string, patch: Partial<Omit<Task, "id" | "created_at">>) => Promise<void>;
  remove: (id: string) => Promise<void>;
  toggleComplete: (id: string) => Promise<void>;
  bulkImport: (toInsert: Task[], toUpdate: Task[]) => Promise<void>;
  /** 重新排序未完成任务：传入未完成任务 id 的新顺序数组 */
  reorderActive: (orderedIds: string[]) => Promise<void>;
}

export const useTaskStore = create<TaskState>()(
  immer((set, get) => ({
    tasks: [],
    loading: false,
    loaded: false,

    hydrate: async () => {
      if (get().loaded) return;
      set((s) => {
        s.loading = true;
      });
      const list = await db.listTasks();
      set((s) => {
        s.tasks = list;
        s.loading = false;
        s.loaded = true;
      });
      // 重新排队所有未完成的提醒
      for (const t of list) {
        if (!t.completed_at && t.remind_at) {
          rescheduleReminder(t).catch(() => {
            /* ignore */
          });
        }
      }

      // 远端有变更时重拉本地
      onRemoteChange(async () => {
        const fresh = await db.listTasks();
        set((s) => {
          s.tasks = fresh;
        });
      });
    },

    add: async (draft) => {
      const task = await db.createTask(draft);
      set((s) => {
        // 追加到末尾；排序由展示层的 sortTasks 处理
        s.tasks.push(task);
      });
      if (!task.completed_at && task.remind_at) {
        rescheduleReminder(task).catch(() => {
          /* ignore */
        });
      }
      pushTask(task).catch(() => {
        /* sync 失败不影响本地 */
      });
      return task;
    },

    update: async (id, patch) => {
      await db.updateTask(id, patch);
      set((s) => {
        const idx = s.tasks.findIndex((t) => t.id === id);
        if (idx >= 0) {
          Object.assign(s.tasks[idx], patch);
          s.tasks[idx].updated_at = new Date().toISOString();
        }
      });
      const t = get().tasks.find((t) => t.id === id);
      if (t) {
        if (!t.completed_at && t.remind_at) {
          rescheduleReminder(t).catch(() => {
            /* ignore */
          });
        } else {
          cancelReminder(id).catch(() => {
            /* ignore */
          });
        }
        pushTask(t).catch(() => {
          /* ignore */
        });
      }
    },

    remove: async (id) => {
      await db.softDeleteTask(id);
      set((s) => {
        const idx = s.tasks.findIndex((t) => t.id === id);
        if (idx >= 0) s.tasks.splice(idx, 1);
      });
      cancelReminder(id).catch(() => {
        /* ignore */
      });
      deleteRemote(id).catch(() => {
        /* ignore */
      });
    },

    bulkImport: async (toInsert, toUpdate) => {
      // 直接走 SQL upsert（tauri-plugin-sql 无 upsert，分两步）
      for (const t of toInsert) {
        await db.createTaskRaw(t);
      }
      for (const t of toUpdate) {
        await db.updateTask(t.id, {
          title: t.title,
          priority: t.priority,
          remind_at: t.remind_at,
          remind_offset_min: t.remind_offset_min,
          repeat_rule: t.repeat_rule,
          completed_at: t.completed_at,
        });
      }
      // 重新拉全表最简单
      const list = await db.listTasks();
      set((s) => {
        s.tasks = list;
      });
      // 未完成 + 有提醒的重新排队
      for (const t of list) {
        if (!t.completed_at && t.remind_at) {
          rescheduleReminder(t).catch(() => {
            /* ignore */
          });
        }
      }
    },

    toggleComplete: async (id) => {
      const t = get().tasks.find((t) => t.id === id);
      if (!t) return;
      const completing = !t.completed_at;
      await db.toggleComplete(id, completing);
      set((s) => {
        const idx = s.tasks.findIndex((t) => t.id === id);
        if (idx >= 0) {
          s.tasks[idx].completed_at = completing ? new Date().toISOString() : null;
          s.tasks[idx].updated_at = new Date().toISOString();
        }
      });
      const updated = get().tasks.find((t) => t.id === id);
      if (updated) {
        pushTask(updated).catch(() => {
          /* ignore */
        });
      }
      if (completing) {
        cancelReminder(id).catch(() => {
          /* ignore */
        });
        // 如果有重复规则，生成下一次实例
        if (t.repeat_rule) {
          const next = rollNext(t);
          if (next) {
            await get().add(next);
          }
        }
      } else {
        // 取消完成，如有提醒重新排队
        const nt = get().tasks.find((t) => t.id === id);
        if (nt && nt.remind_at) {
          rescheduleReminder(nt).catch(() => {
            /* ignore */
          });
        }
      }
    },

    reorderActive: async (orderedIds) => {
      // 简单粗暴：直接给每一个位置分配整数 sort_order (0, 1000, 2000, ...)
      // 未来能优化成只更新变动项，用中值避免全表重写。当前规模无需优化。
      const now = new Date().toISOString();
      const patches: Array<{ id: string; order: number }> = [];
      orderedIds.forEach((id, idx) => {
        patches.push({ id, order: (idx + 1) * 1000 });
      });

      // 乐观更新内存
      set((s) => {
        for (const p of patches) {
          const t = s.tasks.find((x) => x.id === p.id);
          if (t) {
            t.sort_order = p.order;
            t.updated_at = now;
          }
        }
      });

      // 落库 + 同步
      for (const p of patches) {
        await db.updateSortOrder(p.id, p.order);
      }
      // 推送每条变动到服务端
      for (const p of patches) {
        const t = get().tasks.find((x) => x.id === p.id);
        if (t) {
          pushTask(t).catch(() => {
            /* ignore */
          });
        }
      }
    },
  })),
);
