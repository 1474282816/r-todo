/**
 * 无线设备电量挂件
 *
 * 独立小窗口，常驻桌面角落。每台设备一行：图标 + 名称 + 电量条 + 百分比。
 * 视觉跟主挂件一套：同圆角、同半透明底、同字号。
 */
import { Battery, BatteryCharging, BatteryFull, BatteryLow, BatteryWarning, Keyboard, Mouse, RefreshCw, X, Gamepad2, Headphones } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

interface DeviceReading {
  id: string;
  name: string;
  kind: string;
  transport: string;
  percent: number | null;
  state: "discharging" | "charging" | "full" | "unknown";
  online: boolean;
  voltageMv: number | null;
  source: string;
  raw: string;
  updatedAt: number | null;
  error: string | null;
}

/** 电量档位 → 颜色。60 以上绿、30-59 黄、30 以下红 */
function levelColor(percent: number | null): string {
  if (percent === null) return "rgb(156 163 175)"; // neutral-400
  if (percent >= 60) return "rgb(16 185 129)"; // emerald-500
  if (percent >= 30) return "rgb(245 158 11)"; // amber-500
  return "rgb(239 68 68)"; // red-500
}

function KindIcon({ kind, size = 14 }: { kind: string; size?: number }) {
  const cls = "shrink-0 text-neutral-500 dark:text-neutral-400";
  switch (kind) {
    case "keyboard":
      return <Keyboard size={size} className={cls} />;
    case "mouse":
      return <Mouse size={size} className={cls} />;
    case "headset":
      return <Headphones size={size} className={cls} />;
    case "gamepad":
      return <Gamepad2 size={size} className={cls} />;
    default:
      return <Battery size={size} className={cls} />;
  }
}

/** 电量图标：按档位和充电状态换 */
function LevelIcon({ percent, charging }: { percent: number | null; charging: boolean }) {
  const size = 13;
  const color = levelColor(percent);
  if (charging) return <BatteryCharging size={size} style={{ color }} />;
  if (percent === null) return <BatteryWarning size={size} style={{ color }} />;
  if (percent >= 85) return <BatteryFull size={size} style={{ color }} />;
  if (percent < 20) return <BatteryLow size={size} style={{ color }} />;
  return <Battery size={size} style={{ color }} />;
}

export function BatteryWidget() {
  const [devices, setDevices] = useState<DeviceReading[]>([]);
  const [loading, setLoading] = useState(true);
  const [hovered, setHovered] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(async (rescan = false) => {
    try {
      const res = await invoke<{ devices: DeviceReading[] }>("battery_list", { rescan });
      setDevices(res.devices ?? []);
    } catch (e) {
      console.error("battery_list failed:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const res = await invoke<{ devices: DeviceReading[] }>("battery_refresh");
      setDevices(res.devices ?? []);
    } catch (e) {
      console.error("battery_refresh failed:", e);
    } finally {
      setRefreshing(false);
    }
  }, []);

  // 窗口高度跟着内容实际高度走。
  //
  // 不用"设备数 × 固定行高"来算：高 DPI 下实际行高和 CSS 里写的 px 不是 1:1，
  // 硬编码的估算值总会差一截（要么底部留白，要么内容被切）。
  // 这里直接量 DOM，量多少设多少。
  const rootRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;

    const sync = () => {
      const rect = el.getBoundingClientRect();
      if (rect.height < 10) return;
      getCurrentWindow()
        .setSize(new LogicalSize(rect.width, Math.ceil(rect.height)))
        .catch(() => {});
    };

    sync();
    const ro = new ResizeObserver(sync);
    ro.observe(el);
    return () => ro.disconnect();
  }, [devices.length]);

  useEffect(() => {
    // 首次进来先读缓存（快），再立刻刷一次（准）
    load().then(() => refresh());

    // 后端轮询和手动刷新都会发这个事件
    const unlisten = listen<{ devices: DeviceReading[] }>("battery-changed", (e) => {
      if (e.payload?.devices) setDevices(e.payload.devices);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load, refresh]);

  const hide = async () => {
    try {
      await invoke("close_battery_widget");
    } catch (e) {
      console.error("close failed:", e);
    }
  };

  // 拖动：交给 Tauri 的 data-tauri-drag-region（走原生 HTCAPTION，
  // 由系统做窗口拖动，比 JS 里调 startDragging 稳 —— 后者要求
  // pointer capture，在无边框透明窗口上偶发失效）。
  const onPointerDown = (e: React.PointerEvent) => {
    // 按钮区域不触发拖动
    if ((e.target as HTMLElement).closest("button")) {
      e.stopPropagation();
    }
  };
  return (
    <div
      ref={rootRef}
      className="flex h-full flex-col overflow-hidden rounded-xl border border-black/5 bg-white/80 shadow-sm backdrop-blur-xl dark:border-white/10 dark:bg-neutral-900/80"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* 标题栏：整条可拖 */}
      <div
        data-tauri-drag-region
        onPointerDown={onPointerDown}
        className="flex shrink-0 cursor-grab items-center gap-1.5 px-3 py-1.5 active:cursor-grabbing"
      >
        <Battery size={12} data-tauri-drag-region className="shrink-0 text-neutral-400" />
        <span
          data-tauri-drag-region
          className="text-[11px] font-medium text-neutral-500 dark:text-neutral-400"
        >
          设备电量
        </span>
        <div data-tauri-drag-region className="flex-1 self-stretch" />
        <button
          type="button"
          onClick={refresh}
          disabled={refreshing}
          className="rounded p-0.5 text-neutral-400 transition-colors hover:text-neutral-600 disabled:opacity-40 dark:hover:text-neutral-200"
          title="刷新"
          aria-label="刷新"
        >
          <RefreshCw size={11} className={refreshing ? "animate-spin" : ""} />
        </button>
        <button
          type="button"
          onClick={hide}
          className="rounded p-0.5 text-neutral-400 opacity-0 transition-opacity hover:text-red-500 group-hover:opacity-100"
          style={{ opacity: hovered ? 1 : 0 }}
          title="关闭"
          aria-label="关闭"
        >
          <X size={11} />
        </button>
      </div>

      {/* 设备列表 */}
      <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-1.5">
        {loading ? (
          <div className="px-2 py-3 text-center text-[11px] text-neutral-400">读取中…</div>
        ) : devices.length === 0 ? (
          <div className="px-2 py-3 text-center text-[11px] text-neutral-400">
            没找到无线设备
          </div>
        ) : (
          devices.map((d) => (
            <DeviceRow key={d.id} device={d} />
          ))
        )}
      </div>
    </div>
  );
}

function DeviceRow({ device }: { device: DeviceReading }) {
  const percent = device.percent;
  const isCharging = device.state === "charging";
  const color = levelColor(percent);

  return (
    <div className="flex items-center gap-2 rounded-lg px-2 py-1.5 transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.06]">
      <KindIcon kind={device.kind} />

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-[12px] leading-4 text-neutral-800 dark:text-neutral-100">
            {device.name}
          </span>
          <span className="ml-auto shrink-0 text-[11px] font-medium tabular-nums" style={{ color }}>
            {percent === null ? (device.online ? "--" : "休眠") : `${percent}%`}
          </span>
        </div>
        {/* 电量条：圆角细条，跟百分比同色 */}
        <div className="mt-1 h-[3px] w-full overflow-hidden rounded-full bg-black/[0.07] dark:bg-white/[0.12]">
          <div
            className="h-full rounded-full transition-all duration-300"
            style={{
              width: `${percent ?? 0}%`,
              backgroundColor: color,
            }}
          />
        </div>
      </div>

      <LevelIcon percent={percent} charging={isCharging} />
    </div>
  );
}
