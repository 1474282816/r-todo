import { describe, expect, it } from "vitest";

import type { Task } from "../types";
import { sortTasks } from "../sort";

function task(id: string, patch: Partial<Task> = {}): Task {
  return {
    id,
    title: id,
    priority: "medium",
    remind_at: null,
    remind_offset_min: 15,
    repeat_rule: null,
    sort_order: 0,
    deleted: false,
    completed_at: null,
    created_at: "2026-11-17T08:00:00Z",
    updated_at: "2026-11-17T08:00:00Z",
    ...patch,
  };
}

describe("task ordering", () => {
  it("places active tasks before completed tasks", () => {
    const completed = task("done", { completed_at: "2026-11-17T12:00:00Z" });
    const active = task("active", { sort_order: 2000 });
    expect(sortTasks([completed, active]).map((item) => item.id)).toEqual([
      "active",
      "done",
    ]);
  });

  it("sorts active tasks by manual order and completed tasks descending", () => {
    const values = [
      task("a2", { sort_order: 2000 }),
      task("done-old", { completed_at: "2026-11-17T09:00:00Z" }),
      task("a1", { sort_order: 1000 }),
      task("done-new", { completed_at: "2026-11-17T11:00:00Z" }),
    ];
    expect(sortTasks(values).map((item) => item.id)).toEqual([
      "a1",
      "a2",
      "done-new",
      "done-old",
    ]);
  });
});
