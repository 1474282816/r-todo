import { Download, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { cn } from "../../lib/cn";
import { formatSize } from "../../lib/transfer";
import type { SessionView } from "./useTransfer";

interface Props {
  session: SessionView | null;
  receiveDir: string;
  busy?: boolean;
  onAccept: (acceptedIds: string[]) => void;
  onReject: () => void;
}

export function ReceiveDialog({ session, receiveDir, busy, onAccept, onReject }: Props) {
  const [selected, setSelected] = useState<string[]>([]);

  useEffect(() => {
    setSelected(session ? session.files.map((f) => f.id) : []);
  }, [session]);

  const total = useMemo(
    () =>
      (session?.files ?? [])
        .filter((f) => selected.includes(f.id))
        .reduce((sum, f) => sum + f.size, 0),
    [selected, session],
  );

  if (!session) return null;

  const toggle = (id: string) => {
    setSelected((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/25 p-4 backdrop-blur-[1px]">
      <div className="w-full max-w-[400px] rounded-xl bg-white p-4 shadow-2xl dark:bg-neutral-900">
        <div className="flex items-start justify-between">
          <div>
            <div className="text-[12px] font-medium text-neutral-800 dark:text-neutral-100">
              接收文件请求
            </div>
            <div className="mt-0.5 text-[10px] text-neutral-500">
              {session.peer} · {session.ip}
            </div>
          </div>
          <button
            onClick={onReject}
            className="rounded p-1 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
            aria-label="拒绝"
          >
            <X size={14} />
          </button>
        </div>

        <ul className="mt-2 max-h-52 space-y-1 overflow-y-auto">
          {session.files.map((file) => {
            const checked = selected.includes(file.id);
            return (
              <li key={file.id}>
                <button
                  onClick={() => toggle(file.id)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
                    checked
                      ? "bg-black/5 dark:bg-white/10"
                      : "opacity-60 hover:bg-black/5 dark:hover:bg-white/5",
                  )}
                >
                  <span
                    className={cn(
                      "flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[4px] border",
                      checked
                        ? "border-emerald-500 bg-emerald-500 text-white"
                        : "border-neutral-300 dark:border-neutral-600",
                    )}
                  >
                    {checked && <Download size={9} />}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[11px] text-neutral-700 dark:text-neutral-200">
                    {file.name}
                  </span>
                  <span className="shrink-0 text-[10px] text-neutral-400">
                    {formatSize(file.size)}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>

        <div className="mt-2 text-[10px] text-neutral-400">
          保存到 {receiveDir || "下载目录/RTools"} · 共 {formatSize(total)}
        </div>

        <div className="mt-3 flex justify-end gap-1.5">
          <button
            onClick={onReject}
            disabled={busy}
            className="rounded-md px-2.5 py-1.5 text-[11px] text-neutral-500 hover:bg-black/5 disabled:opacity-50 dark:hover:bg-white/5"
          >
            拒绝
          </button>
          <button
            onClick={() => onAccept(selected)}
            disabled={busy || selected.length === 0}
            className="rounded-md bg-neutral-900 px-3 py-1.5 text-[11px] text-white hover:bg-neutral-700 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
          >
            接收
          </button>
        </div>
      </div>
    </div>
  );
}
