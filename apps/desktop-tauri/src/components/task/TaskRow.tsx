import { motion, Reorder } from "framer-motion";
import { Bell, Check, Repeat, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { niceDatetime } from "../../lib/datetime";
import { humanize } from "../../lib/rrule";
import { useTaskStore } from "../../lib/store";
import type { Task } from "../../lib/types";
import { cn } from "../../lib/cn";
import { Checkbox } from "../ui/Checkbox";
import { PriorityBar } from "../ui/PriorityBar";

interface Props {
  task: Task;
  onEdit: (task: Task) => void;
  /** 是否可拖拽（仅未完成任务开启） */
  draggable?: boolean;
}

/**
 * 阻止 pointerdown 冒泡到 Reorder.Item —— 让交互元素（勾选框、编辑按钮、
 * 删除按钮）先响应点击，而不是启动拖拽。
 */
const stopDragProps = {
  onPointerDown: (e: React.PointerEvent) => e.stopPropagation(),
};

export function TaskRow({ task, onEdit, draggable = false }: Props) {
  const [hovered, setHovered] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const confirmTimerRef = useRef<number | null>(null);
  const toggle = useTaskStore((s) => s.toggleComplete);
  const remove = useTaskStore((s) => s.remove);
  const completed = !!task.completed_at;

  useEffect(() => {
    if (!confirmingDelete) return;
    confirmTimerRef.current = window.setTimeout(() => {
      setConfirmingDelete(false);
    }, 3000);
    return () => {
      if (confirmTimerRef.current !== null) {
        window.clearTimeout(confirmTimerRef.current);
      }
    };
  }, [confirmingDelete]);

  const handleDeleteClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setConfirmingDelete(true);
  };
  const handleConfirmDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    remove(task.id);
  };
  const handleCancelDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    setConfirmingDelete(false);
  };

  const rowInner = (
    <>
      <PriorityBar priority={task.priority} />

      <div className="flex items-center pt-0.5" {...stopDragProps}>
        <Checkbox checked={completed} onChange={() => toggle(task.id)} />
      </div>

      <button
        type="button"
        onClick={() => onEdit(task)}
        className="min-w-0 flex-1 text-left"
      >
        <div
          className={cn(
            "truncate text-sm leading-6 transition-colors",
            completed
              ? "text-neutral-400 line-through dark:text-neutral-500"
              : "text-neutral-800 dark:text-neutral-100",
          )}
        >
          {task.title}
        </div>
        {(task.remind_at || task.repeat_rule) && (
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-neutral-500 dark:text-neutral-400">
            {task.remind_at && (
              <span className="inline-flex items-center gap-1">
                <Bell size={10} />
                {niceDatetime(task.remind_at)}
              </span>
            )}
            {task.repeat_rule && (
              <span className="inline-flex items-center gap-1">
                <Repeat size={10} />
                {humanize(task.repeat_rule)}
              </span>
            )}
          </div>
        )}
      </button>

      <div
        className={cn(
          "flex shrink-0 items-center gap-0.5 transition-opacity duration-[180ms] ease-out",
          hovered || confirmingDelete
            ? "pointer-events-auto opacity-100"
            : "pointer-events-none opacity-0",
        )}
        {...stopDragProps}
      >
        {confirmingDelete ? (
          <>
            <button
              onClick={handleConfirmDelete}
              className="rounded p-1 text-red-500 transition-colors hover:bg-red-500/10"
              aria-label="确认删除"
              title="确认删除"
            >
              <Check size={14} strokeWidth={2.5} />
            </button>
            <button
              onClick={handleCancelDelete}
              className="rounded p-1 text-neutral-400 transition-colors hover:bg-black/5 hover:text-neutral-600 dark:hover:bg-white/10 dark:hover:text-neutral-200"
              aria-label="取消删除"
              title="取消"
            >
              <X size={14} strokeWidth={2.5} />
            </button>
          </>
        ) : (
          <button
            onClick={handleDeleteClick}
            className="shrink-0 rounded p-1 text-neutral-400 transition-colors duration-[350ms] hover:text-red-500"
            aria-label="删除"
            title="删除"
          >
            <Trash2 size={14} />
          </button>
        )}
      </div>
    </>
  );

  const rowClass = cn(
    "group flex items-start gap-2 rounded-lg px-2 py-1.5 transition-colors",
    "hover:bg-black/5 dark:hover:bg-white/5",
    draggable && "cursor-grab active:cursor-grabbing",
  );

  const commonProps = {
    onMouseEnter: () => setHovered(true),
    onMouseLeave: () => {
      setHovered(false);
      setConfirmingDelete(false);
    },
    className: rowClass,
  };

  if (draggable) {
    return (
      <Reorder.Item
        value={task}
        layout
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.15 } }}
        transition={{
          duration: 0.18,
          ease: "easeOut",
          layout: { duration: 0.22, ease: [0.22, 1, 0.36, 1] },
        }}
        {...commonProps}
      >
        {rowInner}
      </Reorder.Item>
    );
  }

  return (
    <motion.div
      layout
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.15 } }}
      transition={{
        duration: 0.18,
        ease: "easeOut",
        layout: { duration: 0.22, ease: [0.22, 1, 0.36, 1] },
      }}
      {...commonProps}
    >
      {rowInner}
    </motion.div>
  );
}
