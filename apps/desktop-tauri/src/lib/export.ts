import { save } from "@tauri-apps/plugin-dialog";

import { engineCall } from "./engine";
import type { Task } from "./types";

export type ExportFormat = "json" | "ics" | "csv";

export async function chooseExportPath(format: ExportFormat): Promise<string | null> {
  return save({
    defaultPath: `rtools-${new Date().toISOString().slice(0, 10)}.${format}`,
    filters: [
      {
        name:
          format === "json"
            ? "RTools JSON"
            : format === "ics"
              ? "iCalendar"
              : "TickTick CSV",
        extensions: [format],
      },
    ],
  });
}

export async function exportJsonFile(): Promise<string | null> {
  const path = await chooseExportPath("json");
  if (!path) return null;
  await engineCall("tasks.export_json", { path });
  return path;
}

export function toIcal(tasks: Task[]): string {
  const lines = [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//RTools//RTools//EN",
    "CALSCALE:GREGORIAN",
    "METHOD:PUBLISH",
  ];
  for (const task of tasks) {
    lines.push("BEGIN:VTODO");
    lines.push(`UID:${task.id}@rtools`);
    lines.push(`DTSTAMP:${toIcalDate(task.updated_at)}`);
    lines.push(`CREATED:${toIcalDate(task.created_at)}`);
    lines.push(`LAST-MODIFIED:${toIcalDate(task.updated_at)}`);
    lines.push(`SUMMARY:${escapeIcal(task.title)}`);
    lines.push(`PRIORITY:${task.priority === "high" ? 1 : task.priority === "low" ? 9 : 5}`);
    if (task.remind_at) lines.push(`DUE:${toIcalDate(task.remind_at)}`);
    if (task.repeat_rule) lines.push(`RRULE:${task.repeat_rule}`);
    if (task.completed_at) {
      lines.push(`COMPLETED:${toIcalDate(task.completed_at)}`);
      lines.push("STATUS:COMPLETED");
    } else {
      lines.push("STATUS:NEEDS-ACTION");
    }
    lines.push("END:VTODO");
  }
  lines.push("END:VCALENDAR");
  return `${lines.join("\r\n")}\r\n`;
}

export function toTickTickCsv(tasks: Task[]): string {
  const lines = [
    "Folder Name,Title,Priority,Due Date,Reminder,Repeat,Status,Created Time,Completed Time,taskId",
  ];
  for (const task of tasks) {
    const priority = task.priority === "high" ? 5 : task.priority === "medium" ? 3 : 1;
    lines.push(
      [
        csv(""),
        csv(task.title),
        csv(String(priority)),
        csv(task.remind_at ?? ""),
        csv(task.remind_at ? `-PT${task.remind_offset_min}M` : ""),
        csv(task.repeat_rule ?? ""),
        csv(task.completed_at ? "1" : "0"),
        csv(task.created_at),
        csv(task.completed_at ?? ""),
        csv(task.id),
      ].join(","),
    );
  }
  return `${lines.join("\r\n")}\r\n`;
}

function csv(value: string): string {
  return `"${value.replace(/"/g, '""')}"`;
}

function toIcalDate(value: string): string {
  const date = new Date(value);
  const pad = (part: number) => part.toString().padStart(2, "0");
  return `${date.getUTCFullYear()}${pad(date.getUTCMonth() + 1)}${pad(date.getUTCDate())}T${pad(date.getUTCHours())}${pad(date.getUTCMinutes())}${pad(date.getUTCSeconds())}Z`;
}

function escapeIcal(value: string): string {
  return value
    .replace(/\\/g, "\\\\")
    .replace(/;/g, "\\;")
    .replace(/,/g, "\\,")
    .replace(/\n/g, "\\n");
}
