import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import {
  completeTask,
  createTask,
  deleteTask,
  listTasks,
  reorderTasks,
  subscribeEngine,
  updateTask,
} from "./engine";
import { getDefaultOffset } from "./reminder";
import type { Task, TaskDraft } from "./types";

interface TaskState {
  tasks: Task[];
  loading: boolean;
  loaded: boolean;
  error: string | null;

  hydrate: () => Promise<void>;
  add: (draft: TaskDraft) => Promise<Task>;
  update: (id: string, patch: Partial<Omit<Task, "id" | "created_at">>) => Promise<void>;
  remove: (id: string) => Promise<void>;
  toggleComplete: (id: string) => Promise<void>;
  bulkImport: () => Promise<void>;
  reorderActive: (orderedIds: string[]) => Promise<void>;
}

let unsubscribeEngine: (() => void) | null = null;

async function refresh(
  set: (fn: (state: TaskState) => void) => void,
): Promise<void> {
  const tasks = await listTasks();
  set((state) => {
    state.tasks = tasks;
  });
}

export const useTaskStore = create<TaskState>()(
  immer((set, get) => ({
    tasks: [],
    loading: false,
    loaded: false,
    error: null,

    hydrate: async () => {
      if (get().loaded) return;
      set((state) => {
        state.loading = true;
      });
      try {
        await refresh(set);
        if (!unsubscribeEngine) {
          const dispose = await subscribeEngine((event) => {
            if (event.event === "tasks.changed") {
              refresh(set).catch(() => undefined);
            }
          });
          unsubscribeEngine = dispose;
        }
        set((state) => {
          state.loaded = true;
          state.error = null;
        });
      } catch (error) {
        set((state) => {
          state.loaded = true;
          state.error = error instanceof Error ? error.message : String(error);
        });
      } finally {
        set((state) => {
          state.loading = false;
        });
      }
    },

    add: async (draft) => {
      const task = await createTask({
        title: draft.title.trim(),
        priority: draft.priority ?? "medium",
        remind_at: draft.remind_at ?? null,
        remind_offset_min: draft.remind_offset_min ?? getDefaultOffset(),
        repeat_rule: draft.repeat_rule ?? null,
      });
      set((state) => {
        state.tasks.push(task);
      });
      return task;
    },

    update: async (id, patch) => {
      const current = get().tasks.find((task) => task.id === id);
      if (!current) throw new Error("任务不存在");
      const merged: Task = {
        ...current,
        ...patch,
        updated_at: new Date().toISOString(),
      };
      const saved = await updateTask(merged);
      set((state) => {
        const index = state.tasks.findIndex((task) => task.id === id);
        if (index >= 0) state.tasks[index] = saved;
      });
    },

    remove: async (id) => {
      await deleteTask(id);
      set((state) => {
        state.tasks = state.tasks.filter((task) => task.id !== id);
      });
    },

    toggleComplete: async (id) => {
      const current = get().tasks.find((task) => task.id === id);
      if (!current) return;
      const saved = await completeTask(id, !current.completed_at);
      set((state) => {
        const index = state.tasks.findIndex((task) => task.id === id);
        if (index >= 0) state.tasks[index] = saved;
      });
      await refresh(set);
    },

    bulkImport: async () => {
      await refresh(set);
    },

    reorderActive: async (orderedIds) => {
      const before = get().tasks;
      set((state) => {
        for (const [index, id] of orderedIds.entries()) {
          const task = state.tasks.find((item) => item.id === id);
          if (task) task.sort_order = (index + 1) * 1000;
        }
      });
      try {
        await reorderTasks(orderedIds);
      } catch (error) {
        set((state) => {
          state.tasks = before;
        });
        throw error;
      }
    },
  })),
);
