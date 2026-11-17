/**
 * 互传窗口（独立窗口，?view=transfer）
 *
 * 视觉沿用主窗口 / 设置面板：Tailwind neutral 色系、11~12px 字号、
 * rounded-lg、bg-black/5 dark:bg-white/5、lucide 图标 12~16px。
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowLeftRight, FolderOpen, Loader2, Settings2, Wifi, X } from "lucide-react";
import { useEffect, useState } from "react";

import { cn } from "../../lib/cn";
import { ChatApp } from "../chat/ChatApp";
import { DeviceList } from "./DeviceList";
import { ReceiveDialog } from "./ReceiveDialog";
import { SendPanel } from "./SendPanel";
import { TransferProgress } from "./TransferProgress";
import { useTransfer } from "./useTransfer";

function LegacyTransferView() {
  const {
    state,
    devices,
    sessions,
    pendingIncoming,
    notice,
    error,
    busy,
    discovering,
    start,
    stop,
    discover,
    addIp,
    sendTo,
    confirmReceive,
    rejectReceive,
    cancelSession,
    saveSettings,
  } = useTransfer();

  const [files, setFiles] = useState<string[]>([]);
  const [pin, setPin] = useState("");
  const [dragging, setDragging] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const running = state?.running ?? false;
  const receiveDir = state?.settings.receiveDir ?? "";

  // 窗口级拖拽：仅 transfer 窗口生效
  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;

    // 拖拽监听依赖 Tauri 的 webview API；在纯浏览器里调试时拿不到，忽略即可
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "enter" || payload.type === "over") {
            setDragging(true);
          } else if (payload.type === "leave") {
            setDragging(false);
          } else if (payload.type === "drop") {
            setDragging(false);
            setFiles((prev) => {
              const merged = [...prev];
              for (const path of payload.paths) {
                if (!merged.includes(path)) merged.push(path);
              }
              return merged;
            });
          }
        })
        .then((fn) => {
          if (cancelled) fn();
          else dispose = fn;
        })
        .catch(() => undefined);
    } catch {
      // 非 Tauri 环境（例如浏览器里看样式），没有拖拽能力
    }

    return () => {
      cancelled = true;
      dispose?.();
    };
  }, []);

  const handleSend = async (device: (typeof devices)[number]) => {
    if (files.length === 0) return;
    const ok = await sendTo(device, files, pin || undefined);
    if (ok) {
      setFiles([]);
      setPin("");
    }
  };

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-[#f5f5f7]/40 text-neutral-800 select-none dark:bg-neutral-900/40 dark:text-neutral-100">
      {/* 顶部 */}
      <div className="flex h-11 shrink-0 items-center justify-between border-b border-black/5 px-3 dark:border-white/5">
        <div className="flex min-w-0 items-center gap-2">
          <ArrowLeftRight size={15} className="shrink-0 text-neutral-500" />
          <span className="text-[13px] font-medium">互传</span>
          <span
            className={cn(
              "h-1.5 w-1.5 shrink-0 rounded-full",
              running ? "bg-emerald-500" : "bg-neutral-400 dark:bg-neutral-600",
            )}
          />
          <span className="truncate text-[10px] text-neutral-400">
            {running
              ? `${state?.alias ?? ""} · 端口 ${state?.port ?? ""}`
              : "服务未启动"}
          </span>
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={discover}
            disabled={!running || discovering}
            title="刷新设备"
            className="rounded-md p-1.5 text-neutral-500 transition-colors hover:bg-black/5 disabled:opacity-40 dark:hover:bg-white/10"
          >
            <Wifi size={14} className={cn(discovering && "animate-pulse")} />
          </button>
          <button
            onClick={() => setSettingsOpen((v) => !v)}
            title="设置"
            className={cn(
              "rounded-md p-1.5 transition-colors hover:bg-black/5 dark:hover:bg-white/10",
              settingsOpen ? "text-neutral-800 dark:text-neutral-100" : "text-neutral-500",
            )}
          >
            <Settings2 size={14} />
          </button>
        </div>
      </div>

      <div className="flex-1 space-y-3 overflow-y-auto p-3">
        {!running ? (
          <div className="rounded-lg bg-white/70 p-6 text-center shadow-sm dark:bg-white/5">
            <ArrowLeftRight size={22} className="mx-auto mb-2 text-neutral-300 dark:text-neutral-600" />
            <div className="text-[12px] text-neutral-700 dark:text-neutral-200">
              局域网互传未启动
            </div>
            <div className="mx-auto mt-1 max-w-[320px] text-[10px] text-neutral-400">
              启动后会在 54417 端口监听，并与官方 LocalSend 及其他 RTools 设备互通；
              端口仅在本机监听，接收文件前一定需要你确认。
            </div>
            <button
              onClick={start}
              disabled={busy}
              className="mt-3 inline-flex items-center gap-1.5 rounded-md bg-neutral-900 px-3 py-1.5 text-[11px] text-white hover:bg-neutral-700 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
            >
              {busy && <Loader2 size={11} className="animate-spin" />}
              启动互传服务
            </button>
            {error && <div className="mt-2 text-[10px] text-red-500">{error}</div>}
          </div>
        ) : (
          <>
            <SendPanel
              files={files}
              pin={pin}
              onPinChange={setPin}
              dragging={dragging}
              disabled={busy}
              onPick={(picked) =>
                setFiles((prev) => {
                  const merged = [...prev];
                  for (const path of picked) if (!merged.includes(path)) merged.push(path);
                  return merged;
                })
              }
              onRemove={(path) => setFiles((prev) => prev.filter((p) => p !== path))}
            />

            <DeviceList
              devices={devices}
              disabled={busy}
              discovering={discovering}
              canSend={files.length > 0}
              onRefresh={discover}
              onSend={handleSend}
              onAddIp={addIp}
            />

            <TransferProgress sessions={sessions} onCancel={cancelSession} />
          </>
        )}

        {settingsOpen && state && (
          <SettingsCard
            state={state}
            busy={busy}
            running={running}
            onStart={start}
            onStop={stop}
            onSave={saveSettings}
            onClose={() => setSettingsOpen(false)}
          />
        )}

        {error && running && (
          <div className="rounded-md bg-red-500/10 px-3 py-2 text-[10px] text-red-500">
            {error}
          </div>
        )}
      </div>

      {notice && (
        <div
          className={cn(
            "pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2 rounded-md px-3 py-1.5 text-[11px] shadow-lg",
            notice.kind === "ok"
              ? "bg-emerald-500/95 text-white"
              : "bg-red-500/95 text-white",
          )}
        >
          {notice.text}
        </div>
      )}

      <ReceiveDialog
        session={pendingIncoming}
        receiveDir={receiveDir}
        busy={busy}
        onAccept={(ids) => pendingIncoming && confirmReceive(pendingIncoming, ids)}
        onReject={() => pendingIncoming && rejectReceive(pendingIncoming)}
      />
    </div>
  );
}

function SettingsCard({
  state,
  busy,
  running,
  onStart,
  onStop,
  onSave,
  onClose,
}: {
  state: NonNullable<ReturnType<typeof useTransfer>["state"]>;
  busy: boolean;
  running: boolean;
  onStart: () => void;
  onStop: () => void;
  onSave: (patch: {
    alias?: string;
    port?: number;
    receiveDir?: string;
    pin?: string | null;
    autoAcceptFavorites?: boolean;
  }) => void;
  onClose: () => void;
}) {
  const [alias, setAlias] = useState(state.settings.alias);
  const [port, setPort] = useState(String(state.settings.port));
  const [receiveDir, setReceiveDir] = useState(state.settings.receiveDir);
  const [pin, setPin] = useState(state.settings.pin ?? "");

  const pickDir = async () => {
    const dir = await open({ directory: true, title: "选择接收目录" });
    if (typeof dir === "string") setReceiveDir(dir);
  };

  return (
    <section className="rounded-lg bg-white/70 p-3 shadow-sm dark:bg-white/5">
      <div className="mb-2 flex items-center justify-between">
        <div className="text-[11px] font-medium uppercase tracking-wide text-neutral-400">
          互传设置
        </div>
        <button
          onClick={onClose}
          className="rounded p-0.5 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
          aria-label="关闭设置"
        >
          <X size={13} />
        </button>
      </div>

      <div className="space-y-2">
        <Field label="设备名称">
          <input
            value={alias}
            onChange={(e) => setAlias(e.target.value)}
            className="w-full rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] outline-none dark:bg-white/10"
          />
        </Field>

        <Field label="端口">
          <input
            value={port}
            onChange={(e) => setPort(e.target.value.replace(/[^0-9]/g, ""))}
            className="w-full rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] outline-none dark:bg-white/10"
          />
        </Field>

        <Field label="接收目录">
          <div className="flex items-center gap-1.5">
            <input
              value={receiveDir}
              onChange={(e) => setReceiveDir(e.target.value)}
              placeholder="默认为 下载目录 / RTools"
              className="min-w-0 flex-1 rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/10"
            />
            <button
              onClick={pickDir}
              className="rounded-md bg-black/5 p-1.5 hover:bg-black/10 dark:bg-white/10 dark:hover:bg-white/15"
              aria-label="选择目录"
            >
              <FolderOpen size={12} />
            </button>
          </div>
        </Field>

        <Field label="PIN（可选）">
          <input
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/[^0-9]/g, "").slice(0, 8))}
            placeholder="留空表示不校验"
            className="w-full rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/10"
          />
        </Field>

        <div className="text-[10px] text-neutral-400">
          fingerprint <span className="font-mono">{state.fingerprint.slice(0, 16)}…</span>
          {" · "}
          接收前必须人工确认；收藏设备自动接受暂未开放
        </div>

        <div className="flex items-center gap-1.5 pt-1">
          <button
            onClick={() => {
              const parsedPort = Number(port);
              onSave({
                alias,
                port: Number.isFinite(parsedPort) && parsedPort > 0 ? parsedPort : undefined,
                receiveDir,
                pin,
              });
            }}
            disabled={busy}
            className="rounded-md bg-neutral-900 px-3 py-1.5 text-[11px] text-white hover:bg-neutral-700 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
          >
            保存
          </button>
          <button
            onClick={running ? onStop : onStart}
            disabled={busy}
            className="rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] hover:bg-black/10 disabled:opacity-50 dark:bg-white/10 dark:hover:bg-white/15"
          >
            {running ? "停止服务" : "启动服务"}
          </button>
        </div>
      </div>
    </section>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[10px] text-neutral-500">{label}</span>
      {children}
    </label>
  );
}

/**
 * `?view=transfer` 的入口：默认会话式双栏界面；
 * 「高级设置」可切回已验收的旧「选设备 + 发文件」链路（Q13 不删除旧入口）。
 */
export function TransferWindow() {
  const [legacy, setLegacy] = useState(false);
  if (legacy) {
    return (
      <div className="relative h-full">
        <button
          onClick={() => setLegacy(false)}
          className="absolute right-2 top-2 z-50 rounded bg-blue-500 px-2 py-1 text-[10px] text-white shadow"
        >
          返回消息
        </button>
        <LegacyTransferView />
      </div>
    );
  }
  return <ChatApp onLegacy={() => setLegacy(true)} />;
}
