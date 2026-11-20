import { AnimatePresence, Reorder } from "framer-motion";
import { ArrowLeftRight, BatteryFull, CalendarDays, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { invoke } from "@tauri-apps/api/core";

import { SettingsPanel } from "./components/settings/SettingsPanel";
import { QuickAdd, type QuickAddHandle } from "./components/task/QuickAdd";
import { TaskEditor } from "./components/task/TaskEditor";
import { TaskRow } from "./components/task/TaskRow";
import { IconButton } from "./components/ui/IconButton";
import { isCompletedBeforeToday } from "./lib/datetime";
import { useDragEnd } from "./lib/hooks/useDragEnd";
import { useHotkeys } from "./lib/hooks/useHotkeys";
import {
  animateToPosition,
  getCollapseTarget,
  getSnapTarget,
  isCollapseOnSnapEnabled,
  type SnapEdge,
} from "./lib/snap";
import { sortTasks } from "./lib/sort";
import { useTaskStore } from "./lib/store";
import { openTransferWindow } from "./lib/transfer";
import { restoreSession } from "./lib/sync";
import {
  embedToDesktop,
  isEmbeddedPreference,
  restoreBackdrop,
  watchWallpaperChanges,
} from "./lib/system";
import type { Task } from "./lib/types";
import { silentCheckUpdate, subscribeUpdateStatus } from "./lib/updater";

/**
 * r-todo 桌面端主界面
 *
 * 结构：
 *   [顶部拖动条 + 设置按钮]
 *   [快速添加]
 *   [任务列表 - 未完成在上，今日已完成在下]
 *   [编辑面板（浮层）]
 *   [设置面板（浮层）]
 */
export default function App() {
  const tasks = useTaskStore((s) => s.tasks);
  const hydrate = useTaskStore((s) => s.hydrate);
  const loaded = useTaskStore((s) => s.loaded);
  const reorderActive = useTaskStore((s) => s.reorderActive);

  const [editing, setEditing] = useState<Task | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hasUpdate, setHasUpdate] = useState(false);
  const quickAddRef = useRef<QuickAddHandle>(null);

  const openBattery = useCallback(() => {
    invoke("open_battery_widget").catch((e) =>
      console.error("open_battery_widget failed:", e),
    );
  }, []);

  const openMonth = useCallback(() => {
    invoke("open_month_window").catch((e) =>
      console.error("open_month_window failed:", e),
    );
  }, []);

  // 吸附/收起状态
  const snapStateRef = useRef<{
    collapsed: boolean;
    edge: SnapEdge | null;
    expandedPos: { x: number; y: number } | null;
  }>({ collapsed: false, edge: null, expandedPos: null });
  const dragEndSuppressUntilRef = useRef(0);

  useEffect(() => {
    hydrate();
  }, [hydrate]);

  // 启动时应用嵌入偏好（默认 true）
  useEffect(() => {
    if (isEmbeddedPreference()) {
      embedToDesktop().catch((e) => {
        console.warn("failed to auto-embed on startup:", e);
      });
    }
  }, []);

  // 启动时恢复窗口材质偏好（默认 acrylic 毛玻璃）+ 监听壁纸变化实时刷新
  useEffect(() => {
    restoreBackdrop().catch((e) => console.warn("restore backdrop failed:", e));
    watchWallpaperChanges().catch(() => {});
  }, []);

  // 启动时恢复登录 + 后台同步
  useEffect(() => {
    restoreSession().catch((e) => console.warn("restore session failed:", e));
  }, []);

  // 启动时静默检查更新
  useEffect(() => {
    silentCheckUpdate().catch(() => {});
    return subscribeUpdateStatus((status) => {
      setHasUpdate(status.state === "available");
    });
  }, []);

  useHotkeys({
    onNew: () => quickAddRef.current?.focus(),
    onEscape: () => {
      if (editing) setEditing(null);
      else if (settingsOpen) setSettingsOpen(false);
    },
  });

  // 拖动结束 → 检测 snap → 可选收起
  const handleDragEnd = useCallback(async () => {
    // 收起/展开动作本身会移动窗口，避免递归触发
    if (performance.now() < dragEndSuppressUntilRef.current) return;
    if (snapStateRef.current.collapsed) return;
    try {
      const target = await getSnapTarget();
      if (!target) {
        snapStateRef.current.edge = null;
        snapStateRef.current.expandedPos = null;
        return;
      }
      dragEndSuppressUntilRef.current = performance.now() + 400;
      await animateToPosition(target.x, target.y, 180);
      snapStateRef.current.edge = target.edge;
      snapStateRef.current.expandedPos = { x: target.x, y: target.y };

      // 若开了"收起为边条"
      if (isCollapseOnSnapEnabled()) {
        dragEndSuppressUntilRef.current = performance.now() + 800;
        const c = await getCollapseTarget(target.edge);
        await animateToPosition(c.x, c.y, 220);
        snapStateRef.current.collapsed = true;
      }
    } catch (e) {
      console.warn("snap failed:", e);
    }
  }, []);
  useDragEnd(handleDragEnd);

  // 收起状态时：鼠标进 window 展开，离 window 收回
  useEffect(() => {
    const onEnter = async () => {
      if (
        snapStateRef.current.collapsed &&
        snapStateRef.current.expandedPos
      ) {
        dragEndSuppressUntilRef.current = performance.now() + 800;
        const { x, y } = snapStateRef.current.expandedPos;
        await animateToPosition(x, y, 200);
        snapStateRef.current.collapsed = false;
      }
    };
    const onLeave = async () => {
      if (
        !snapStateRef.current.collapsed &&
        snapStateRef.current.edge &&
        isCollapseOnSnapEnabled()
      ) {
        dragEndSuppressUntilRef.current = performance.now() + 800;
        const c = await getCollapseTarget(snapStateRef.current.edge);
        await animateToPosition(c.x, c.y, 220);
        snapStateRef.current.collapsed = true;
      }
    };
    document.body.addEventListener("mouseenter", onEnter);
    document.body.addEventListener("mouseleave", onLeave);
    return () => {
      document.body.removeEventListener("mouseenter", onEnter);
      document.body.removeEventListener("mouseleave", onLeave);
    };
  }, []);

  // 前端过滤 + 排序：
  //   1. 隐藏昨天及以前完成的任务（次日 0:00 自动隐藏）
  //   2. 按优先级/提醒时间/创建时间排序（未完成上，已完成下）
  const visible = useMemo(() => {
    const filtered = tasks.filter(
      (t) => !t.completed_at || !isCompletedBeforeToday(t.completed_at),
    );
    return sortTasks(filtered);
  }, [tasks]);

  const active = visible.filter((t) => !t.completed_at);
  const doneToday = visible.filter((t) => !!t.completed_at);

  return (
    <div className="relative flex h-full flex-col overflow-hidden select-none">
      {/* 顶部拖动条 */}
      <div
        data-tauri-drag-region
        className="flex h-9 shrink-0 items-center justify-between px-2"
      >
        <span
          data-tauri-drag-region
          className="pl-2 text-sm font-medium tracking-wide text-neutral-600 dark:text-neutral-300"
        >
          待办事项
          <span
            data-tauri-drag-region
            className="ml-1.5 text-xs text-neutral-400 dark:text-neutral-500"
          >
            ({active.length}/{visible.length})
          </span>
        </span>
        <div className="flex items-center gap-0.5">
          <IconButton
            aria-label="月视图"
            title="月视图"
            onClick={openMonth}
          >
            <CalendarDays size={14} />
          </IconButton>
          <IconButton
            aria-label="设备电量"
            title="无线设备电量"
            onClick={openBattery}
          >
            <BatteryFull size={14} />
          </IconButton>
          <IconButton
            aria-label="互传"
            title="局域网互传"
            onClick={() =>
              openTransferWindow().catch((e) =>
                console.error("open_transfer_window failed:", e),
              )
            }
          >
            <ArrowLeftRight size={14} />
          </IconButton>
          <IconButton
            aria-label="设置"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings2 size={14} />
            {hasUpdate && (
              <span className="absolute right-0.5 top-0.5 h-1.5 w-1.5 rounded-full bg-blue-500" />
            )}
          </IconButton>
        </div>
      </div>

      {/* 快速添加 */}
      <div className="shrink-0 px-3 pb-2">
        <QuickAdd ref={quickAddRef} />
      </div>

      {/* 列表 */}
      <div
        data-tauri-drag-region
        className="flex-1 overflow-y-auto px-2 pb-4"
      >
        {!loaded && (
          <div className="p-8 text-center text-xs text-neutral-400">
            加载中...
          </div>
        )}
        {loaded && visible.length === 0 && (
          <div
            data-tauri-drag-region
            className="flex h-full flex-col items-center justify-center gap-2 pb-8 text-neutral-400"
          >
            <div data-tauri-drag-region className="text-sm">
              今天没有任务
            </div>
            <div data-tauri-drag-region className="text-[11px]">
              试试上方添加一条
            </div>
          </div>
        )}
        <Reorder.Group
          axis="y"
          values={active}
          onReorder={(newOrder) => {
            reorderActive(newOrder.map((t) => t.id));
          }}
          className="flex flex-col"
        >
          <AnimatePresence initial={false} mode="popLayout">
            {active.map((t) => (
              <TaskRow key={t.id} task={t} onEdit={setEditing} draggable />
            ))}
          </AnimatePresence>
        </Reorder.Group>
        {doneToday.length > 0 && active.length > 0 && (
          <div
            data-tauri-drag-region
            className="my-2 mx-2 border-t border-black/5 dark:border-white/5"
          />
        )}
        <AnimatePresence initial={false} mode="popLayout">
          {doneToday.map((t) => (
            <TaskRow key={t.id} task={t} onEdit={setEditing} />
          ))}
        </AnimatePresence>
      </div>

      <TaskEditor task={editing} onClose={() => setEditing(null)} />
      <SettingsPanel open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
