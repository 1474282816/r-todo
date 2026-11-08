import { FileUp, Paperclip, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";

import { cn } from "../../lib/cn";

interface Props {
  files: string[];
  pin: string;
  onPinChange: (value: string) => void;
  onPick: (files: string[]) => void;
  onRemove: (path: string) => void;
  disabled?: boolean;
  dragging?: boolean;
}

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function SendPanel({
  files,
  pin,
  onPinChange,
  onPick,
  onRemove,
  disabled,
  dragging,
}: Props) {
  const pick = async () => {
    const picked = await open({ multiple: true, title: "选择要发送的文件" });
    if (!picked) return;
    onPick(Array.isArray(picked) ? picked : [picked]);
  };

  return (
    <section className="rounded-lg bg-white/70 p-3 shadow-sm dark:bg-white/5">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-neutral-400">
          <Paperclip size={13} />
          要发送的文件
          {files.length > 0 && <span className="text-neutral-400/70">({files.length})</span>}
        </div>
        <button
          onClick={pick}
          disabled={disabled}
          className={cn(
            "inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] transition-colors",
            "bg-black/5 hover:bg-black/10 dark:bg-white/10 dark:hover:bg-white/15",
            disabled && "opacity-50",
          )}
        >
          <FileUp size={12} />
          选择文件
        </button>
      </div>

      {files.length === 0 ? (
        <button
          onClick={pick}
          disabled={disabled}
          className={cn(
            "flex w-full flex-col items-center justify-center rounded-md border border-dashed py-5 text-[11px] transition-colors",
            dragging
              ? "border-emerald-500/60 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
              : "border-black/10 text-neutral-400 hover:border-black/20 dark:border-white/10 dark:hover:border-white/20",
            disabled && "opacity-50",
          )}
        >
          <FileUp size={18} className="mb-1" />
          拖拽文件到窗口，或点击选择
        </button>
      ) : (
        <ul className="space-y-1">
          {files.map((path) => (
            <li
              key={path}
              className="flex items-center gap-2 rounded-md bg-black/5 px-2 py-1 dark:bg-white/5"
            >
              <span className="min-w-0 flex-1 truncate text-[11px] text-neutral-700 dark:text-neutral-200">
                {baseName(path)}
              </span>
              <button
                onClick={() => onRemove(path)}
                className="rounded p-0.5 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-100"
                aria-label="移除"
              >
                <X size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="mt-2 flex items-center gap-1.5">
        <span className="shrink-0 text-[10px] text-neutral-400">对方 PIN</span>
        <input
          value={pin}
          onChange={(e) => onPinChange(e.target.value.replace(/[^0-9]/g, "").slice(0, 8))}
          placeholder="对方要求时填写，可选"
          className="min-w-0 flex-1 rounded-md bg-black/5 px-2.5 py-1 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/10"
        />
      </div>
    </section>
  );
}
