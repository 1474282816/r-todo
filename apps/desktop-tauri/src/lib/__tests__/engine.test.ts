import { describe, expect, it } from "vitest";

import { toEngineTask, toTask, type EngineTask } from "../engine";

const engineTask: EngineTask = {
  id: "task-1",
  title: "检查同步",
  priority: "high",
  remind_at: "2026-11-18T01:30:00Z",
  remind_offset_min: 15,
  repeat_rule: "FREQ=DAILY",
  sort_order: 1000,
  completed_at: null,
  deleted_at: null,
  created_at: "2026-11-17T08:00:00Z",
  updated_at: "2026-11-17T08:10:00Z",
  revision: 4,
};

describe("Engine task mapping", () => {
  it("converts Engine timestamps and deletion state into UI tasks", () => {
    const task = toTask(engineTask);
    expect(task.id).toBe("task-1");
    expect(task.priority).toBe("high");
    expect(task.deleted).toBe(false);
    expect(task.revision).toBe(4);
  });

  it("keeps task identity and ordering when sending an update", () => {
    const task = toTask({ ...engineTask, deleted_at: "2026-11-17T09:00:00Z" });
    const wire = toEngineTask(task);
    expect(wire.id).toBe(engineTask.id);
    expect(wire.sort_order).toBe(1000);
    expect(wire.deleted_at).toBe(task.updated_at);
  });
});
