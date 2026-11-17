import { ArrowDownLeft, ArrowUpRight, Check, X } from "lucide-react";

import { cn } from "../../lib/cn";
import { formatSize } from "../../lib/transfer";
import type { SessionView } from "./useTransfer";

interface Props {
  sessions: SessionView[];
  onCancel: (session: SessionView) => void;
}

function statusText(session: SessionView): string {
  if (session.ended) {
    switch (session.ended) {
      case "completed":
        return "已完成";
      case "cancelled":
        return "已取消";
      case "rejected":
        return "已拒绝";
      default:
        return "失败";
    }
  }
  return session.direction === "send" ? "发送中" : "接收中";
}

export function TransferProgress({ sessions, onCancel }: Props) {
  if (sessions.length === 0) return null;

  return (
    <section className="rounded-lg bg-white/70 p-3 shadow-sm dark:bg-white/5">
      <div className="mb-2 text-[11px] font-medium uppercase tracking-wide text-neutral-400">
        传输
      </div>

      <ul className="space-y-2">
        {sessions.map((session) => {
          const Icon = session.direction === "send" ? ArrowUpRight : ArrowDownLeft;
          return (
            <li key={session.id} className="rounded-md bg-black/[0.03] p-2 dark:bg-white/[0.04]">
              <div className="flex items-center gap-1.5">
                <Icon
                  size={13}
                  className={cn(
                    session.direction === "send" ? "text-blue-500" : "text-emerald-500",
                  )}
                />
                <span className="min-w-0 flex-1 truncate text-[11px] text-neutral-700 dark:text-neutral-200">
                  {session.peer}
                </span>
                <span
                  className={cn(
                    "text-[10px]",
                    session.ended === "completed"
                      ? "text-emerald-600 dark:text-emerald-400"
                      : session.ended
                        ? "text-red-500"
                        : "text-neutral-400",
                  )}
                >
                  {statusText(session)}
                </span>
                {!session.ended && (
                  <button
                    onClick={() => onCancel(session)}
                    className="rounded p-0.5 text-neutral-400 hover:text-red-500"
                    aria-label="取消"
                  >
                    <X size={12} />
                  </button>
                )}
              </div>

              <ul className="mt-1 space-y-1">
                {session.files.map((file, index) => {
                  const percent =
                    file.size > 0
                      ? Math.min(100, Math.round((file.transferred / file.size) * 100))
                      : file.done
                        ? 100
                        : 0;
                  return (
                    <li key={file.id || `${session.id}-${index}`}>
                      <div className="flex items-center gap-1.5 text-[10px] text-neutral-500 dark:text-neutral-400">
                        <span className="min-w-0 flex-1 truncate">{file.name}</span>
                        {file.done ? (
                          <Check size={11} className="text-emerald-500" />
                        ) : (
                          <span>
                            {percent}% · {formatSize(file.size)}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 h-0.5 w-full overflow-hidden rounded-full bg-black/10 dark:bg-white/10">
                        <div
                          className={cn(
                            "h-full rounded-full transition-[width] duration-200",
                            file.done ? "bg-emerald-500" : "bg-neutral-500 dark:bg-neutral-300",
                          )}
                          style={{ width: `${percent}%` }}
                        />
                      </div>
                    </li>
                  );
                })}
              </ul>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
