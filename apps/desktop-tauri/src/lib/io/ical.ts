/**
 * iCalendar VTODO 导出/导入（RFC 5545）
 *
 * 导出：手写 VTODO 输出（VTODO 支持不好的库不多，反而手写更简单可靠）
 * 导入：用 ical.js（成熟稳定，Mozilla 出品）
 */
import ICAL from "ical.js";
import { nanoid } from "nanoid";

import type { Priority, Task } from "../types";

const PRIORITY_TO_ICAL: Record<Priority, number> = {
  high: 1,
  medium: 5,
  low: 9,
};

function icalPriorityToLevel(p: number | undefined | null): Priority {
  if (p == null) return "medium";
  if (p >= 1 && p <= 4) return "high";
  if (p >= 6 && p <= 9) return "low";
  return "medium";
}

/** RFC 5545 文本转义：`\` `,` `;` `\n` */
function escapeText(s: string): string {
  return s
    .replace(/\\/g, "\\\\")
    .replace(/;/g, "\\;")
    .replace(/,/g, "\\,")
    .replace(/\n/g, "\\n");
}

/** 转 ISO 8601 basic 格式（不含分隔符），UTC：20260719T090000Z */
function toIcalDate(iso: string): string {
  const d = new Date(iso);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return (
    `${d.getUTCFullYear()}${pad(d.getUTCMonth() + 1)}${pad(d.getUTCDate())}` +
    `T${pad(d.getUTCHours())}${pad(d.getUTCMinutes())}${pad(d.getUTCSeconds())}Z`
  );
}

/** 按 75 字节折行（RFC 5545 §3.1），续行以 space 开头 */
function foldLine(line: string): string {
  if (line.length <= 75) return line;
  const chunks: string[] = [];
  let i = 0;
  while (i < line.length) {
    const size = i === 0 ? 75 : 74;
    const chunk = line.slice(i, i + size);
    chunks.push((i === 0 ? "" : " ") + chunk);
    i += size;
  }
  return chunks.join("\r\n");
}

/** 生成 VCALENDAR 字符串 */
export function toIcal(tasks: Task[]): string {
  const lines: string[] = [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//RTools//RTools//EN",
    "CALSCALE:GREGORIAN",
    "METHOD:PUBLISH",
  ];

  for (const t of tasks) {
    const stampNow = toIcalDate(new Date().toISOString());
    lines.push("BEGIN:VTODO");
    lines.push(foldLine(`UID:${t.id}@rtools`));
    lines.push(`DTSTAMP:${stampNow}`);
    lines.push(`CREATED:${toIcalDate(t.created_at)}`);
    lines.push(`LAST-MODIFIED:${toIcalDate(t.updated_at)}`);
    lines.push(foldLine(`SUMMARY:${escapeText(t.title)}`));
    lines.push(`PRIORITY:${PRIORITY_TO_ICAL[t.priority]}`);
    if (t.remind_at) lines.push(`DUE:${toIcalDate(t.remind_at)}`);
    if (t.repeat_rule) lines.push(`RRULE:${t.repeat_rule}`);
    if (t.completed_at) {
      lines.push(`COMPLETED:${toIcalDate(t.completed_at)}`);
      lines.push("STATUS:COMPLETED");
    } else {
      lines.push("STATUS:NEEDS-ACTION");
    }
    if (t.remind_at && t.remind_offset_min > 0) {
      lines.push("BEGIN:VALARM");
      lines.push("ACTION:DISPLAY");
      lines.push(foldLine(`DESCRIPTION:${escapeText(t.title)}`));
      lines.push(`TRIGGER:-PT${t.remind_offset_min}M`);
      lines.push("END:VALARM");
    }
    lines.push("END:VTODO");
  }

  lines.push("END:VCALENDAR");
  return lines.join("\r\n") + "\r\n";
}

/** 从 ical 内容解析 Task 列表 */
export function parseIcal(content: string): Task[] {
  const jcal = ICAL.parse(content);
  const comp = new ICAL.Component(jcal);
  const vtodos = comp.getAllSubcomponents("vtodo");
  const tasks: Task[] = [];

  for (const v of vtodos) {
    const uid = v.getFirstPropertyValue("uid")?.toString() ?? "";
    const summary = v.getFirstPropertyValue("summary")?.toString() ?? "(无标题)";
    const priority = v.getFirstPropertyValue("priority");
    const due = v.getFirstPropertyValue("due");
    const completed = v.getFirstPropertyValue("completed");
    const created = v.getFirstPropertyValue("created");
    const lastMod = v.getFirstPropertyValue("last-modified");
    const status = v.getFirstPropertyValue("status")?.toString() ?? "";

    const rruleProp = v.getFirstProperty("rrule");
    let repeat_rule: string | null = null;
    if (rruleProp) {
      const rec = rruleProp.getFirstValue();
      repeat_rule = (rec as unknown as { toString: () => string }).toString();
    }

    let remind_offset_min = 15;
    const alarm = v.getFirstSubcomponent("valarm");
    if (alarm) {
      const trigger = alarm.getFirstPropertyValue("trigger");
      if (trigger) {
        const dur = trigger as unknown as { toSeconds?: () => number };
        if (typeof dur.toSeconds === "function") {
          const secs = Math.abs(dur.toSeconds());
          remind_offset_min = Math.round(secs / 60);
        }
      }
    }

    const nowIso = new Date().toISOString();
    const toIso = (t: unknown): string | null => {
      if (!t) return null;
      const obj = t as { toJSDate?: () => Date };
      return typeof obj.toJSDate === "function"
        ? obj.toJSDate().toISOString()
        : null;
    };
    const dueIso = toIso(due);
    const completedIso =
      toIso(completed) ?? (status.toUpperCase() === "COMPLETED" ? nowIso : null);

    tasks.push({
      id: uid.replace(/@(?:r-todo|rtools)$/, "") || nanoid(15),
      title: summary,
      priority: icalPriorityToLevel(priority as number | null),
      remind_at: dueIso,
      remind_offset_min,
      repeat_rule,
      sort_order: 0,
      deleted: false,
      completed_at: completedIso,
      created_at: toIso(created) ?? nowIso,
      updated_at: toIso(lastMod) ?? nowIso,
    });
  }
  return tasks;
}
