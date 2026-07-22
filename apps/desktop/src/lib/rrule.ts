/**
 * RRULE (RFC 5545) 工具函数
 *
 * 用 rrule.js 解析和推进。r-todo 只用于"重复任务勾选完成后生成下一个实例"。
 */
import { RRule, Frequency, Weekday } from "rrule";

import type { Task, TaskDraft } from "./types";

export interface RRuleConfig {
  freq: "daily" | "weekly" | "monthly";
  interval: number;
  /** 仅 weekly：0=周日 ... 6=周六，与 JS Date.getDay() 一致 */
  byweekday?: number[];
  /** 仅 monthly：几号 */
  bymonthday?: number;
}

const WEEKDAYS: readonly Weekday[] = [
  RRule.SU,
  RRule.MO,
  RRule.TU,
  RRule.WE,
  RRule.TH,
  RRule.FR,
  RRule.SA,
] as const;

/** 从 UI 配置生成 RRULE 字符串 */
export function buildRRule(cfg: RRuleConfig): string {
  const opts: Parameters<typeof RRule.optionsToString>[0] = {
    freq:
      cfg.freq === "daily"
        ? Frequency.DAILY
        : cfg.freq === "weekly"
          ? Frequency.WEEKLY
          : Frequency.MONTHLY,
    interval: Math.max(1, cfg.interval || 1),
  };
  if (cfg.freq === "weekly" && cfg.byweekday?.length) {
    opts.byweekday = cfg.byweekday.map((d) => WEEKDAYS[d]);
  }
  if (cfg.freq === "monthly" && cfg.bymonthday) {
    opts.bymonthday = [cfg.bymonthday];
  }
  return RRule.optionsToString(opts);
}

/** 解析 RRULE 字符串（用于 UI 回显） */
export function parseRRule(rrule: string): RRuleConfig | null {
  try {
    const rule = RRule.fromString(rrule);
    const o = rule.options;
    const freq: RRuleConfig["freq"] =
      o.freq === Frequency.DAILY
        ? "daily"
        : o.freq === Frequency.WEEKLY
          ? "weekly"
          : o.freq === Frequency.MONTHLY
            ? "monthly"
            : "daily";
    return {
      freq,
      interval: o.interval ?? 1,
      byweekday: o.byweekday?.map((w) => (w as unknown as number) % 7),
      bymonthday: o.bymonthday?.[0],
    };
  } catch {
    return null;
  }
}

/** 常用预设 */
export const PRESETS: { label: string; rrule: string }[] = [
  { label: "每天", rrule: buildRRule({ freq: "daily", interval: 1 }) },
  {
    label: "工作日",
    rrule: buildRRule({
      freq: "weekly",
      interval: 1,
      byweekday: [1, 2, 3, 4, 5],
    }),
  },
  {
    label: "周末",
    rrule: buildRRule({
      freq: "weekly",
      interval: 1,
      byweekday: [0, 6],
    }),
  },
  { label: "每周", rrule: buildRRule({ freq: "weekly", interval: 1 }) },
  { label: "每月", rrule: buildRRule({ freq: "monthly", interval: 1 }) },
];

/** 人类可读描述 */
export function humanize(rrule: string | null): string {
  if (!rrule) return "";
  try {
    const r = RRule.fromString(rrule);
    return r.toText();
  } catch {
    return "自定义";
  }
}

/**
 * 已完成的重复任务 → 下一次实例
 *
 * - 基于 remind_at（或今天）向后推
 * - 返回一个新的 TaskDraft（未完成状态，remind_at 已推进）
 * - 若无下一次（例如 count 已耗尽），返回 null
 */
export function rollNext(task: Task): TaskDraft | null {
  if (!task.repeat_rule) return null;
  try {
    const rule = RRule.fromString(task.repeat_rule);
    const base = task.remind_at ? new Date(task.remind_at) : new Date();
    const next = rule.after(base, false);
    if (!next) return null;
    return {
      title: task.title,
      priority: task.priority,
      remind_at: task.remind_at ? next.toISOString() : null,
      remind_offset_min: task.remind_offset_min,
      repeat_rule: task.repeat_rule,
      completed_at: null,
    };
  } catch {
    return null;
  }
}
