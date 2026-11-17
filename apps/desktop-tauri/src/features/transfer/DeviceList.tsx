import { Laptop, Loader2, RefreshCw, Send, Smartphone, Wifi } from "lucide-react";
import { useState } from "react";

import { cn } from "../../lib/cn";
import type { TransferDevice } from "../../lib/transfer";

interface Props {
  devices: TransferDevice[];
  disabled?: boolean;
  discovering?: boolean;
  canSend: boolean;
  onRefresh: () => void;
  onSend: (device: TransferDevice) => void;
  onAddIp: (host: string, port?: number) => void;
}

export function DeviceList({
  devices,
  disabled,
  discovering,
  canSend,
  onRefresh,
  onSend,
  onAddIp,
}: Props) {
  const [manual, setManual] = useState("");

  const submitManual = () => {
    const value = manual.trim();
    if (!value) return;
    const [host, portText] = value.split(":");
    const port = portText ? Number(portText) : undefined;
    onAddIp(host, Number.isFinite(port) ? port : undefined);
    setManual("");
  };

  return (
    <section className="rounded-lg bg-white/70 p-3 shadow-sm dark:bg-white/5">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-neutral-400">
          <Wifi size={13} />
          附近设备
          <span className="text-neutral-400/70">({devices.length})</span>
        </div>
        <button
          onClick={onRefresh}
          disabled={disabled || discovering}
          className={cn(
            "inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] transition-colors",
            "bg-black/5 hover:bg-black/10 dark:bg-white/10 dark:hover:bg-white/15",
            (disabled || discovering) && "opacity-50",
          )}
        >
          {discovering ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            <RefreshCw size={12} />
          )}
          刷新
        </button>
      </div>

      {devices.length === 0 ? (
        <div className="rounded-md border border-dashed border-black/10 py-5 text-center text-[11px] text-neutral-400 dark:border-white/10">
          没有发现设备
          <div className="mt-0.5 text-[10px]">
            确认对方打开了 LocalSend / RTools 互传，且在同一局域网
          </div>
        </div>
      ) : (
        <ul className="space-y-1">
          {devices.map((device) => {
            const channel = device.channels[0];
            const Icon = device.device_type === "mobile" ? Smartphone : Laptop;
            return (
              <li
                key={device.fingerprint}
                className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-black/5 dark:hover:bg-white/5"
              >
                <Icon size={15} className="shrink-0 text-neutral-400" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[12px] text-neutral-800 dark:text-neutral-100">
                    {device.alias}
                  </div>
                  <div className="truncate text-[10px] text-neutral-400">
                    {device.device_model ?? device.device_type ?? "未知设备"}
                    {channel ? ` · ${channel.host}` : ""}
                  </div>
                </div>
                <button
                  onClick={() => onSend(device)}
                  disabled={!canSend}
                  title={canSend ? "发送已选文件" : "先选择要发送的文件"}
                  className={cn(
                    "inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] transition-colors",
                    canSend
                      ? "bg-neutral-800 text-white hover:bg-neutral-700 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
                      : "bg-black/5 text-neutral-400 dark:bg-white/5",
                  )}
                >
                  <Send size={11} />
                  发送
                </button>
              </li>
            );
          })}
        </ul>
      )}

      <div className="mt-2 flex items-center gap-1.5">
        <input
          value={manual}
          onChange={(e) => setManual(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submitManual();
          }}
          placeholder="手动 IP，如 192.168.1.9:54417"
          className="min-w-0 flex-1 rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/10"
        />
        <button
          onClick={submitManual}
          disabled={disabled || !manual.trim()}
          className="rounded-md bg-black/5 px-2.5 py-1.5 text-[11px] transition-colors hover:bg-black/10 disabled:opacity-50 dark:bg-white/10 dark:hover:bg-white/15"
        >
          连接
        </button>
      </div>
    </section>
  );
}
