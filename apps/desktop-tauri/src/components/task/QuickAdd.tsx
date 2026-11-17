import { motion } from "framer-motion";
import { Plus } from "lucide-react";
import { forwardRef, useImperativeHandle, useRef, useState, type KeyboardEvent } from "react";

import { PRIORITY_COLOR } from "../../lib/types";
import type { Priority } from "../../lib/types";
import { useTaskStore } from "../../lib/store";
import { cn } from "../../lib/cn";

const PRIORITIES: Priority[] = ["high", "medium", "low"];

export interface QuickAddHandle {
  focus: () => void;
}

export const QuickAdd = forwardRef<QuickAddHandle>(function QuickAdd(_, ref) {
  const [title, setTitle] = useState("");
  const [priority, setPriority] = useState<Priority>("low");
  const inputRef = useRef<HTMLInputElement>(null);
  const add = useTaskStore((s) => s.add);

  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
  }));

  const submit = async () => {
    const t = title.trim();
    if (!t) return;
    await add({ title: t, priority });
    setTitle("");
    setPriority("low");
    inputRef.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="flex items-center gap-2 rounded-lg border border-black/10 bg-white/50 px-3 py-2 shadow-sm dark:border-white/10 dark:bg-white/5">
      <Plus size={16} className="shrink-0 text-neutral-400" />
      <input
        ref={inputRef}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder="添加任务..."
        className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-neutral-400 dark:placeholder:text-neutral-500"
      />
      <div className="flex items-center gap-1">
        {PRIORITIES.map((p) => (
          <motion.button
            key={p}
            whileTap={{ scale: 0.9 }}
            onClick={() => setPriority(p)}
            className={cn(
              "h-4 w-4 rounded-full border transition-all",
              priority === p ? "scale-110 border-transparent" : "border-transparent",
            )}
            style={{
              backgroundColor:
                priority === p ? PRIORITY_COLOR[p] : "transparent",
              boxShadow: priority !== p ? `inset 0 0 0 1.5px ${PRIORITY_COLOR[p]}` : undefined,
              opacity: priority === p ? 1 : 0.7,
            }}
            aria-label={`优先级 ${p}`}
            title={`优先级：${p === "high" ? "高" : p === "medium" ? "中" : "低"}`}
          />
        ))}
      </div>
    </div>
  );
});
