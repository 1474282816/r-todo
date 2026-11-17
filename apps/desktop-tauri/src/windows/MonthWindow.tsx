/**
 * 月视图窗口 —— 独立 Tauri 窗口
 *
 * 视觉：Google Calendar 风格网格。6 行 × 7 列，每格里直接列当日完成的任务标题。
 * 每格最多显示 4 条，剩余显示 "还有 X 项"。
 * 点击任意日期格 → 弹出当日完整任务列表。
 */
import { AnimatePresence, motion } from "framer-motion";
import { CalendarDays, ChevronLeft, ChevronRight, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { groupByLocalDate, listCompletedInMonth } from "../lib/history";
import { PRIORITY_COLOR, PRIORITY_LABEL } from "../lib/types";
import type { Task } from "../lib/types";
import { cn } from "../lib/cn";

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];
const MAX_ITEMS_PER_CELL = 4;

/** 6x7 网格，本地日历，周一起始 */
function buildMonthGrid(year: number, monthIdx: number): Date[] {
  const first = new Date(year, monthIdx, 1);
  const dow = (first.getDay() + 6) % 7;
  const startDate = new Date(year, monthIdx, 1 - dow);
  const cells: Date[] = [];
  for (let i = 0; i < 42; i++) {
    const d = new Date(startDate);
    d.setDate(startDate.getDate() + i);
    cells.push(d);
  }
  return cells;
}

function localKey(d: Date): string {
  return `${d.getFullYear()}-${(d.getMonth() + 1)
    .toString()
    .padStart(2, "0")}-${d.getDate().toString().padStart(2, "0")}`;
}

function fmtHHmm(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = (n: number) => n.toString().padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

export function MonthWindow() {
  const today = useMemo(() => new Date(), []);
  const [cursor, setCursor] = useState(
    () => new Date(today.getFullYear(), today.getMonth(), 1),
  );
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  const year = cursor.getFullYear();
  const monthIdx = cursor.getMonth();

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listCompletedInMonth(year, monthIdx)
      .then((res) => {
        if (!cancelled) setTasks(res);
      })
      .catch((e) => console.error("[month-window] load failed:", e))
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [year, monthIdx]);

  const grid = useMemo(() => buildMonthGrid(year, monthIdx), [year, monthIdx]);
  const grouped = useMemo(() => groupByLocalDate(tasks), [tasks]);
  const todayKey = localKey(today);

  const selectedTasks = selectedKey ? grouped.get(selectedKey) ?? [] : [];
  const selectedDate = selectedKey ? new Date(selectedKey) : null;

  // 翻月时关掉弹层
  useEffect(() => {
    setSelectedKey(null);
  }, [year, monthIdx]);

  // Esc 关闭弹层
  useEffect(() => {
    if (!selectedKey) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSelectedKey(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedKey]);

  return (
    <div className="flex h-screen flex-col bg-white text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      {/* 顶栏 */}
      <header className="flex shrink-0 items-center justify-between border-b border-black/10 px-4 py-2 dark:border-white/10">
        <div className="flex items-center gap-2">
          <CalendarDays size={16} className="text-neutral-500" />
          <span className="text-sm font-semibold">月视图</span>
          <span className="text-xs text-neutral-400">
            · 共 {tasks.length} 项已完成
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => setCursor(new Date(year, monthIdx - 1, 1))}
            className="rounded p-1 text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5"
            aria-label="上个月"
          >
            <ChevronLeft size={16} />
          </button>
          <button
            onClick={() =>
              setCursor(new Date(today.getFullYear(), today.getMonth(), 1))
            }
            className="px-3 py-1 text-sm font-medium hover:text-neutral-600 dark:hover:text-neutral-300"
            title="回到本月"
          >
            {year} 年 {monthIdx + 1} 月
          </button>
          <button
            onClick={() => setCursor(new Date(year, monthIdx + 1, 1))}
            className="rounded p-1 text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5"
            aria-label="下个月"
          >
            <ChevronRight size={16} />
          </button>
        </div>
      </header>

      {/* 周标签 */}
      <div className="grid shrink-0 grid-cols-7 border-b border-black/10 bg-neutral-50 dark:border-white/10 dark:bg-neutral-900">
        {WEEKDAYS.map((w, i) => (
          <div
            key={w}
            className={cn(
              "px-2 py-1.5 text-center text-xs font-medium text-neutral-500",
              (i === 5 || i === 6) && "text-neutral-400",
            )}
          >
            周{w}
          </div>
        ))}
      </div>

      {/* 网格：6 行 7 列，每格独立滚动 */}
      <div className="grid min-h-0 flex-1 grid-cols-7 grid-rows-6 gap-px bg-black/10 dark:bg-white/10">
        {grid.map((d) => {
          const key = localKey(d);
          const inMonth = d.getMonth() === monthIdx;
          const isToday = key === todayKey;
          const items = grouped.get(key) ?? [];
          const overflow = Math.max(0, items.length - MAX_ITEMS_PER_CELL);
          const shown = items.slice(0, MAX_ITEMS_PER_CELL);

          return (
            <button
              type="button"
              key={key}
              onClick={() => {
                if (items.length > 0) setSelectedKey(key);
              }}
              className={cn(
                "flex min-h-0 flex-col overflow-hidden p-1.5 text-left transition-colors",
                inMonth
                  ? "bg-white dark:bg-neutral-950"
                  : "bg-neutral-50 dark:bg-neutral-900",
                items.length > 0 &&
                  "hover:bg-neutral-100 dark:hover:bg-neutral-900",
                items.length === 0 && "cursor-default",
              )}
            >
              {/* 日期数字 */}
              <div className="mb-1 flex shrink-0 items-center justify-between">
                <span
                  className={cn(
                    "inline-flex h-5 min-w-5 items-center justify-center rounded-full px-1 text-[11px] leading-none",
                    isToday
                      ? "bg-emerald-500 font-semibold text-white"
                      : inMonth
                        ? "text-neutral-700 dark:text-neutral-200"
                        : "text-neutral-400 dark:text-neutral-600",
                  )}
                >
                  {d.getDate()}
                </span>
                {items.length > 0 && (
                  <span className="text-[9px] text-neutral-400">
                    {items.length}
                  </span>
                )}
              </div>
              {/* 任务列表 */}
              <ul className="min-h-0 flex-1 space-y-0.5 overflow-hidden">
                {shown.map((t) => (
                  <li
                    key={t.id}
                    className="flex items-center gap-1 rounded px-1 py-0.5 text-[11px] leading-tight"
                    title={`${fmtHHmm(t.completed_at)} · ${t.title}`}
                  >
                    <span
                      className="h-1.5 w-1.5 shrink-0 rounded-full"
                      style={{ background: PRIORITY_COLOR[t.priority] }}
                    />
                    <span className="shrink-0 text-neutral-400 tabular-nums">
                      {fmtHHmm(t.completed_at)}
                    </span>
                    <span className="truncate text-neutral-700 dark:text-neutral-300">
                      {t.title}
                    </span>
                  </li>
                ))}
                {overflow > 0 && (
                  <li className="px-1 text-[10px] text-neutral-400">
                    还有 {overflow} 项…
                  </li>
                )}
              </ul>
            </button>
          );
        })}
      </div>

      {loading && (
        <div className="pointer-events-none absolute right-4 top-2 text-[10px] text-neutral-400">
          加载中…
        </div>
      )}

      {/* 当日完整任务详情弹层 */}
      <AnimatePresence>
        {selectedKey && selectedDate && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="absolute inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
            onClick={() => setSelectedKey(null)}
          >
            <motion.div
              initial={{ scale: 0.96, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.96, opacity: 0 }}
              transition={{ type: "spring", stiffness: 320, damping: 28 }}
              onClick={(e) => e.stopPropagation()}
              className="flex max-h-[80vh] w-[520px] max-w-[90vw] flex-col overflow-hidden rounded-lg bg-white shadow-2xl dark:bg-neutral-900"
            >
              <div className="flex shrink-0 items-center justify-between border-b border-black/10 px-4 py-2.5 dark:border-white/10">
                <div>
                  <div className="text-sm font-semibold">
                    {selectedDate.getFullYear()} 年 {selectedDate.getMonth() + 1} 月{" "}
                    {selectedDate.getDate()} 日
                  </div>
                  <div className="text-[11px] text-neutral-400">
                    共 {selectedTasks.length} 项已完成
                  </div>
                </div>
                <button
                  onClick={() => setSelectedKey(null)}
                  className="rounded p-1 text-neutral-400 hover:bg-black/5 hover:text-neutral-700 dark:hover:bg-white/5 dark:hover:text-neutral-200"
                  aria-label="关闭"
                >
                  <X size={16} />
                </button>
              </div>
              <ul className="min-h-0 flex-1 divide-y divide-black/5 overflow-y-auto dark:divide-white/5">
                {selectedTasks.map((t) => (
                  <li
                    key={t.id}
                    className="flex items-start gap-2 px-4 py-2"
                  >
                    <span
                      className="mt-1.5 h-2 w-2 shrink-0 rounded-full"
                      style={{ background: PRIORITY_COLOR[t.priority] }}
                      title={`优先级：${PRIORITY_LABEL[t.priority]}`}
                    />
                    <span className="shrink-0 tabular-nums text-[11px] text-neutral-400 pt-0.5">
                      {fmtHHmm(t.completed_at)}
                    </span>
                    <span className="min-w-0 flex-1 text-[13px] leading-snug text-neutral-700 dark:text-neutral-200">
                      {t.title}
                    </span>
                  </li>
                ))}
              </ul>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
