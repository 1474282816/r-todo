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

import { followVisualChanges } from "../lib/system";

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

  // 窗口高度跟着内容走，**只在设备数量变化时**调一次。
  //
  // 千万不能用 ResizeObserver 监听 #root：它是 h-full，高度永远等于窗口高度，
  // 量出来的值再喂回 setSize 就是死循环 —— 表现是拖动边缘时窗口疯狂闪烁。
  //
  // 所以这里量的不是 #root，而是内部那个 `h-auto` 的内容容器
  // （它的高度只由内容决定，和窗口尺寸无关）。宽度也不动，
  // 免得和用户手动横向拉伸打架。
  const contentRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;

    // 等内容排完版再量，否则拿到的还是上一轮的高度
    const timer = window.setTimeout(() => {
      const height = el.offsetHeight;
      if (height < 20) return;
      getCurrentWindow()
        .setSize(new LogicalSize(260, Math.ceil(height) + 2))
        .catch(() => {});
    }, 30);

    return () => window.clearTimeout(timer);
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

  // 材质：跟随主挂件。
  //
  // 不能在这里自己读 localStorage 决定材质 —— 每个 Tauri 窗口是独立
  // webview，localStorage 不共享，读到的永远是默认值 translucent，
  // 结果就是"待办窗口是壁纸模糊、电量窗口是一块死灰色"。
  // 由主窗口广播实际生效的材质，这里只负责应用。
  useEffect(() => {
    followVisualChanges().catch(() => {});
  }, []);

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
      // 背景交给 styles.css 的 `body[data-view="battery"] #root` 统一处理，
      // 这里**不要**再写 bg-* / backdrop-blur：
      // 透明窗口上 backdrop-filter 采样不到桌面，只会糊成一块死色，
      // 而且会盖掉主挂件那套壁纸模糊材质。
      className="flex h-full flex-col overflow-hidden rounded-xl border border-black/5 shadow-sm dark:border-white/10"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* 内容容器：h-auto，高度只由内容决定，用来测量自然高度 */}
      <div ref={contentRef} className="flex flex-col">
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
      {/* max-h 兜底：设备多到放不下时列表自己滚，别让窗口长出屏幕 */}
      <div className="min-h-0 overflow-y-auto px-1.5 pb-1.5" style={{ maxHeight: 320 }}>
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
    </div>
  );
}

function DeviceRow({ device }: { device: DeviceReading }) {
  const percent = device.percent;

  // 读数年龄。updatedAt 是最后一次**成功读到**电量的时间：读失败时后端会保留旧值，
  // 所以光看 "95%" 分不清是刚读的还是几小时前的。超过 2 分钟就标灰并写明多久以前。
  const ageSec = device.updatedAt
    ? Math.max(0, Math.round(Date.now() / 1000 - device.updatedAt))
    : null;
  const stale = ageSec !== null && ageSec >= 120;
  const ageText =
    ageSec === null || ageSec < 120
      ? null
      : ageSec < 3600
        ? `${Math.floor(ageSec / 60)} 分钟前`
        : `${Math.floor(ageSec / 3600)} 小时前`;

  const isCharging = device.state === "charging" && !stale;
  const color = stale ? "rgb(156 163 175)" : levelColor(percent);

  // 右侧状态文字：有百分比就报百分比，否则说明为什么没有。
  // 充电态单独说"充电中" —— 这时电压被顶高，换算出的百分比是假的，
  // 与其显示一个虚高的 100%，不如直说在充电。
  const statusText = percent !== null
    ? `${percent}%`
    : isCharging
      ? "充电中"
      : device.online
        ? "--"
        : "休眠";

  // 充电时没有可信百分比，电量条改成满格流动效果，不要画成 0%。
  const barWidth = percent ?? (isCharging ? 100 : 0);
  const barColor = isCharging ? "rgb(16 185 129)" : color;

  return (
    <div
      className="flex items-center gap-2 rounded-lg px-2 py-1.5 transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.06]"
      title={device.error ?? undefined}
    >
      <KindIcon kind={device.kind} />

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-[12px] leading-4 text-neutral-800 dark:text-neutral-100">
            {device.name}
          </span>
          <span className="ml-auto shrink-0 text-[11px] font-medium tabular-nums" style={{ color }}>
            {statusText}
            {ageText && (
              <span className="ml-1 text-[10px] font-normal opacity-70">{ageText}</span>
            )}
          </span>
        </div>
        {/* 电量条：圆角细条，跟百分比同色 */}
        <div className="mt-1 h-[3px] w-full overflow-hidden rounded-full bg-black/[0.07] dark:bg-white/[0.12]">
          <div
            className={
              "h-full rounded-full transition-all duration-300" +
              (isCharging ? " animate-pulse" : "")
            }
            style={{
              width: `${barWidth}%`,
              backgroundColor: barColor,
              opacity: stale ? 0.45 : 1,
            }}
          />
        </div>
      </div>

      <LevelIcon percent={percent} charging={isCharging} />
    </div>
  );
}
