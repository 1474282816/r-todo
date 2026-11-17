import { AnimatePresence, motion } from "framer-motion";
import { X } from "lucide-react";
import { useEffect, useState } from "react";

import { humanize, parseRRule, buildRRule, PRESETS } from "../../lib/rrule";
import { useTaskStore } from "../../lib/store";
import type { Priority, Task } from "../../lib/types";
import { PRIORITY_COLOR, PRIORITY_LABEL } from "../../lib/types";
import { cn } from "../../lib/cn";

const OFFSETS = [0, 5, 10, 15, 30, 60, 120];

interface Props {
  task: Task | null;
  onClose: () => void;
}

/** 将 <input type="datetime-local"> 使用的本地时间字符串 转 ISO UTC */
function localToIso(local: string): string | null {
  if (!local) return null;
  const d = new Date(local);
  if (Number.isNaN(d.getTime())) return null;
  return d.toISOString();
}

/** 从 ISO UTC 转 datetime-local 使用的字符串 */
function isoToLocal(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function TaskEditor({ task, onClose }: Props) {
  const update = useTaskStore((s) => s.update);

  const [title, setTitle] = useState("");
  const [priority, setPriority] = useState<Priority>("medium");
  const [remindLocal, setRemindLocal] = useState("");
  const [offset, setOffset] = useState(15);
  const [rrule, setRrule] = useState<string | null>(null);

  useEffect(() => {
    if (task) {
      setTitle(task.title);
      setPriority(task.priority);
      setRemindLocal(isoToLocal(task.remind_at));
      setOffset(task.remind_offset_min);
      setRrule(task.repeat_rule);
    }
  }, [task]);

  if (!task) return null;

  const save = async () => {
    await update(task.id, {
      title: title.trim() || task.title,
      priority,
      remind_at: localToIso(remindLocal),
      remind_offset_min: offset,
      repeat_rule: rrule,
    });
    onClose();
  };

  const setWeeklyToggle = (day: number) => {
    // 简单版：若当前 rrule 是 weekly，切换某天
    const cfg = rrule ? parseRRule(rrule) : null;
    const base = cfg && cfg.freq === "weekly"
      ? cfg
      : { freq: "weekly" as const, interval: 1, byweekday: [] as number[] };
    const set = new Set(base.byweekday ?? []);
    if (set.has(day)) set.delete(day);
    else set.add(day);
    setRrule(
      buildRRule({
        freq: "weekly",
        interval: base.interval,
        byweekday: Array.from(set).sort(),
      }),
    );
  };

  const cfg = rrule ? parseRRule(rrule) : null;
  const weeklyDays = new Set(
    cfg?.freq === "weekly" ? (cfg.byweekday ?? []) : [],
  );
  const interval = cfg?.interval ?? 1;
  const monthlyDay = cfg?.bymonthday ?? 1;

  const setInterval = (n: number) => {
    const freq = cfg?.freq ?? "daily";
    setRrule(
      buildRRule({
        freq,
        interval: Math.max(1, n),
        byweekday: freq === "weekly" ? cfg?.byweekday : undefined,
        bymonthday: freq === "monthly" ? monthlyDay : undefined,
      }),
    );
  };

  const setMonthlyDay = (d: number) => {
    setRrule(
      buildRRule({
        freq: "monthly",
        interval,
        bymonthday: d,
      }),
    );
  };

  return (
    <AnimatePresence>
      {task && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="absolute inset-0 z-30 flex items-end bg-black/20 backdrop-blur-[2px]"
          onClick={onClose}
        >
          <motion.div
            initial={{ y: 40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 40, opacity: 0 }}
            transition={{ type: "spring", stiffness: 320, damping: 30 }}
            onClick={(e) => e.stopPropagation()}
            className="max-h-[92%] w-full overflow-y-auto rounded-t-2xl bg-white/95 p-4 shadow-2xl dark:bg-neutral-900/95"
          >
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-sm font-semibold text-neutral-700 dark:text-neutral-200">
                编辑任务
              </h2>
              <button
                onClick={onClose}
                className="rounded p-1 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
              >
                <X size={16} />
              </button>
            </div>

            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="任务标题"
              className="mb-3 w-full rounded-md bg-black/5 px-3 py-2 text-sm outline-none dark:bg-white/10"
            />

            {/* 优先级 */}
            <Section label="优先级">
              <div className="flex gap-2">
                {(["high", "medium", "low"] as Priority[]).map((p) => (
                  <button
                    key={p}
                    onClick={() => setPriority(p)}
                    className={cn(
                      "flex items-center gap-1.5 rounded-full px-3 py-1 text-xs transition-all",
                      priority === p
                        ? "bg-black/10 dark:bg-white/15"
                        : "text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5",
                    )}
                  >
                    <span
                      className="h-2 w-2 rounded-full"
                      style={{ background: PRIORITY_COLOR[p] }}
                    />
                    {PRIORITY_LABEL[p]}
                  </button>
                ))}
              </div>
            </Section>

            {/* 提醒 */}
            <Section label="提醒时间">
              <input
                type="datetime-local"
                value={remindLocal}
                onChange={(e) => setRemindLocal(e.target.value)}
                className="w-full rounded-md bg-black/5 px-3 py-1.5 text-xs outline-none dark:bg-white/10"
              />
              {remindLocal && (
                <div className="mt-2">
                  <div className="mb-1 text-[11px] text-neutral-500">
                    提前提醒
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {OFFSETS.map((n) => (
                      <button
                        key={n}
                        onClick={() => setOffset(n)}
                        className={cn(
                          "rounded-full px-2.5 py-0.5 text-[11px] transition-colors",
                          offset === n
                            ? "bg-black/10 dark:bg-white/15"
                            : "text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5",
                        )}
                      >
                        {n === 0 ? "准时" : `${n} 分钟`}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </Section>

            {/* 重复 */}
            <Section label="重复">
              <div className="flex flex-wrap gap-1.5">
                <button
                  onClick={() => setRrule(null)}
                  className={cn(
                    "rounded-full px-2.5 py-0.5 text-[11px] transition-colors",
                    !rrule
                      ? "bg-black/10 dark:bg-white/15"
                      : "text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5",
                  )}
                >
                  不重复
                </button>
                {PRESETS.map((p) => (
                  <button
                    key={p.label}
                    onClick={() => setRrule(p.rrule)}
                    className={cn(
                      "rounded-full px-2.5 py-0.5 text-[11px] transition-colors",
                      rrule === p.rrule
                        ? "bg-black/10 dark:bg-white/15"
                        : "text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5",
                    )}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
              {cfg?.freq === "weekly" && (
                <div className="mt-2 flex gap-1">
                  {["日", "一", "二", "三", "四", "五", "六"].map((d, i) => (
                    <button
                      key={d}
                      onClick={() => setWeeklyToggle(i)}
                      className={cn(
                        "h-6 w-6 rounded text-[11px] transition-colors",
                        weeklyDays.has(i)
                          ? "bg-neutral-800 text-white dark:bg-white dark:text-neutral-900"
                          : "bg-black/5 text-neutral-500 hover:bg-black/10 dark:bg-white/10 dark:hover:bg-white/15",
                      )}
                    >
                      {d}
                    </button>
                  ))}
                </div>
              )}
              {cfg && cfg.freq !== "weekly" && (
                <div className="mt-2">
                  <span className="text-[11px] text-neutral-500">每</span>
                  <input
                    type="number"
                    min={1}
                    max={cfg.freq === "daily" ? 365 : 12}
                    value={interval}
                    onChange={(e) => setInterval(Number(e.target.value) || 1)}
                    className="mx-1 w-12 rounded bg-black/5 px-1.5 py-0.5 text-center text-[11px] outline-none dark:bg-white/10"
                  />
                  <span className="text-[11px] text-neutral-500">
                    {cfg.freq === "daily" ? "天" : "月"}
                  </span>
                </div>
              )}
              {cfg?.freq === "weekly" && (
                <div className="mt-2">
                  <span className="text-[11px] text-neutral-500">每</span>
                  <input
                    type="number"
                    min={1}
                    max={52}
                    value={interval}
                    onChange={(e) => setInterval(Number(e.target.value) || 1)}
                    className="mx-1 w-12 rounded bg-black/5 px-1.5 py-0.5 text-center text-[11px] outline-none dark:bg-white/10"
                  />
                  <span className="text-[11px] text-neutral-500">周</span>
                </div>
              )}
              {cfg?.freq === "monthly" && (
                <div className="mt-2 flex flex-wrap gap-1">
                  {Array.from({ length: 31 }, (_, i) => i + 1).map((d) => (
                    <button
                      key={d}
                      onClick={() => setMonthlyDay(d)}
                      className={cn(
                        "h-6 w-6 rounded text-[11px] transition-colors",
                        monthlyDay === d
                          ? "bg-neutral-800 text-white dark:bg-white dark:text-neutral-900"
                          : "bg-black/5 text-neutral-500 hover:bg-black/10 dark:bg-white/10 dark:hover:bg-white/15",
                      )}
                    >
                      {d}
                    </button>
                  ))}
                </div>
              )}
              {rrule && (
                <div className="mt-2 text-[11px] text-neutral-500">
                  规则：{humanize(rrule)}
                </div>
              )}
            </Section>

            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={onClose}
                className="rounded-md px-3 py-1.5 text-xs text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5"
              >
                取消
              </button>
              <button
                onClick={save}
                className="rounded-md bg-neutral-900 px-3 py-1.5 text-xs text-white hover:bg-neutral-700 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                保存
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function Section({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-3">
      <div className="mb-1 text-[11px] font-medium text-neutral-500 dark:text-neutral-400">
        {label}
      </div>
      {children}
    </div>
  );
}
