/**
 * 会话式互传主界面（桌面双栏，Q13）。
 *
 * 每个设备就是一个好友：会话 id = 对端 fingerprint，无账号、无云同步。
 * 文本走自定义消息端点；图片/文件复用 LocalSend v2 文件链路，缩略图随
 * `FileDto.preview` 到达，人工确认只在「每对设备第一次」出现（Q29/Q36）。
 */
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { open, save } from "@tauri-apps/plugin-dialog";
import { open as openPath } from "@tauri-apps/plugin-shell";
import {
  ArrowLeftRight,
  Check,
  Copy,
  Download,
  FolderOpen,
  Forward,
  Image as ImageIcon,
  MessageSquare,
  Paperclip,
  Play,
  Power,
  RefreshCw,
  Send,
  Settings2,
  Star,
  Trash2,
  Wifi,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { cn } from "../../lib/cn";
import {
  baseName,
  bumpOutbox,
  clearConversation,
  deleteMessageRow,
  enqueueOutbox,
  initChatDb,
  insertMessage,
  isTrusted,
  listConversations,
  listMessages,
  listOutbox,
  markRead,
  newMessage,
  patchMessage,
  removeOutbox,
  revokeTrust,
  rowFromMessage,
  trust,
  upsertConversation,
  type ChatRow,
  type Conversation,
} from "../../lib/chat";
import {
  addManualIp,
  chatFileExists,
  copyChatFile,
  deleteChatFile,
  discoverDevices,
  flashTransferWindow,
  formatSize,
  getPendingIncoming,
  getTransferState,
  readFileBytes,
  rejectTransfer,
  respondPrepare,
  sendChatFiles,
  sendChatText,
  startTransfer,
  stopTransfer,
  subscribeTransfer,
  type ChatMessage,
  type TransferDevice,
  type TransferEventPayload,
  type TransferFileInfo,
  type TransferStateView,
} from "../../lib/transfer";

interface TrustRequest {
  sessionId: string;
  alias: string;
  fingerprint: string;
  files: TransferFileInfo[];
}

interface MenuState {
  x: number;
  y: number;
  row: ChatRow;
}

const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp"]);

/** 服务端只有一个 v2 上传会话槽：文件消息串行发送，避免 409（Q9）。 */
let fileSendChain: Promise<unknown> = Promise.resolve();

function ext(path: string): string {
  const name = baseName(path);
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

function isImage(path: string): boolean {
  return IMAGE_EXT.has(ext(path));
}

function mimeOf(path: string): string {
  switch (ext(path)) {
    case "png":
      return "image/png";
    case "webp":
      return "image/webp";
    case "gif":
      return "image/gif";
    default:
      return "image/jpeg";
  }
}

/** 从本地文件生成 ≤320px 的 base64 缩略图（Q24）。 */
async function makeThumbnail(path: string): Promise<string> {
  const bytes = await readFileBytes(path);
  const blob = new Blob([new Uint8Array(bytes)], { type: mimeOf(path) });
  const bitmap = await createImageBitmap(blob);
  const scale = Math.min(1, 320 / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  return canvas.toDataURL("image/jpeg", 0.7).split(",")[1] ?? "";
}

function dataUrl(preview: string): string {
  return `data:image/jpeg;base64,${preview}`;
}

/** 同一会话 5 秒内的多条消息合并成一条通知，显示最新内容（Q45）。 */
const notifyState = new Map<string, { count: number; first: number }>();
async function notifyDetail(fingerprint: string, alias: string, text: string) {
  const now = Date.now();
  const prev = notifyState.get(fingerprint);
  if (prev && now - prev.first < 5000) {
    prev.count += 1;
  } else {
    notifyState.set(fingerprint, { count: 0, first: now });
  }
  const count = notifyState.get(fingerprint)?.count ?? 0;
  const body = count > 0 ? `${text}（另有 ${count} 条）` : text;
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title: `${alias} 发来消息`, body });
  } catch {
    // 通知权限不可用不影响收消息
  }
  void flashTransferWindow().catch(() => undefined);
}

export function ChatApp({ onLegacy }: { onLegacy: () => void }) {
  const [state, setState] = useState<TransferStateView | null>(null);
  const [devices, setDevices] = useState<TransferDevice[]>([]);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatRow[]>([]);
  const [input, setInput] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [trustRequest, setTrustRequest] = useState<TrustRequest | null>(null);
  const [forwardRow, setForwardRow] = useState<ChatRow | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [bigImage, setBigImage] = useState<{ src: string; name: string } | null>(null);
  const [narrow, setNarrow] = useState(false);
  const [manualIp, setManualIp] = useState("");
  const activeRef = useRef<string | null>(null);
  /** 事件回调里读最新状态，避免因 state 变化重订阅而漏掉事件。 */
  const stateRef = useRef<TransferStateView | null>(null);
  const selfRef = useRef<{ fp: string; alias: string }>({ fp: "", alias: "RTools" });

  useEffect(() => {
    activeRef.current = activeId;
  }, [activeId]);

  useEffect(() => {
    selfRef.current = {
      fp: state?.fingerprint ?? "",
      alias: state?.alias ?? "RTools",
    };
  }, [state]);

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  const refreshConversations = useCallback(async () => {
    setConversations(await listConversations());
  }, []);

  const refreshMessages = useCallback(async (id: string | null) => {
    if (!id) {
      setMessages([]);
      return;
    }
    setMessages(await listMessages(id));
  }, []);

  const flash = useCallback((text: string) => {
    setNotice(text);
    window.setTimeout(() => setNotice((prev) => (prev === text ? null : prev)), 2600);
  }, []);

  const handleIncomingMessage = useCallback(
    async (message: ChatMessage) => {
      if (message.kind === "trust") {
        await trust(message.sender);
        flash(`${message.sender_alias} 已信任本设备，后续文件免确认`);
        return;
      }
      if (message.sender && message.sender === selfRef.current.fp) return;

      const peer = message.sender;
      const incoming: ChatMessage = { ...message, conversation_id: peer };
      await insertMessage(rowFromMessage(incoming, "receive"));
      await upsertConversation(
        peer,
        message.sender_alias || peer.slice(0, 8),
        message.created_at,
        activeRef.current === peer ? 0 : 1,
      );
      await refreshConversations();
      if (activeRef.current === peer) await refreshMessages(peer);
      void notifyDetail(
        peer,
        message.sender_alias || "设备",
        message.kind === "text" ? message.body : `📎 ${message.body}`,
      );
    },
    [flash, refreshConversations, refreshMessages],
  );

  const acceptIncoming = useCallback(
    async (request: TrustRequest, markTrusted: boolean) => {
      await respondPrepare(
        request.sessionId,
        request.files.map((f) => f.id),
      );
      for (const file of request.files) {
        const image = isImage(file.file_name);
        const msg = newMessage(
          request.fingerprint,
          request.fingerprint,
          request.alias,
          image ? "image" : "file",
          file.file_name,
          {
            file_size: file.size,
            preview: file.preview ?? null,
            status: "pending",
          },
        );
        await insertMessage(rowFromMessage(msg, "receive"));
      }
      await upsertConversation(request.fingerprint, request.alias, Date.now(), 0);
      if (markTrusted) {
        await trust(request.fingerprint);
        const ack = newMessage(
          request.fingerprint,
          selfRef.current.fp,
          selfRef.current.alias,
          "trust",
          "trust",
        );
        // 回执丢失也无妨：各自方向再确认一次即可（Q36）。
        void sendChatText(request.fingerprint, ack).catch(() => undefined);
      }
      setTrustRequest(null);
      await refreshConversations();
      await refreshMessages(activeRef.current);
    },
    [refreshConversations, refreshMessages],
  );

  /**
   * 恢复仍在服务端等待确认的接收请求。
   * 旧「高级设置」界面靠 getPendingIncoming() 补回弹窗，会话界面缺这一步，
   * 就会表现为「对方发文件过来，电脑端一点反应都没有」。
   */
  const restorePending = useCallback(async () => {
    try {
      const list = await getPendingIncoming();
      for (const item of list) {
        const fp = item.device.fingerprint.toUpperCase();
        const request: TrustRequest = {
          sessionId: item.session_id,
          alias: item.device.alias || fp.slice(0, 8),
          fingerprint: fp,
          files: item.files,
        };
        const trusted = await isTrusted(fp);
        const autoAccept = stateRef.current?.settings.autoAcceptFavorites ?? true;
        if (trusted && autoAccept) {
          await acceptIncoming(request, false);
        } else {
          setTrustRequest(request);
        }
      }
    } catch {
      // 服务未启动时没有待确认请求
    }
  }, [acceptIncoming]);

  const handleIncoming = useCallback(
    async (payload: Extract<TransferEventPayload, { type: "incoming" }>) => {
      const fp = payload.device.fingerprint.toUpperCase();
      const request: TrustRequest = {
        sessionId: payload.session_id,
        alias: payload.device.alias || fp.slice(0, 8),
        fingerprint: fp,
        files: payload.files,
      };
      const trusted = await isTrusted(fp);
      const autoAccept = stateRef.current?.settings.autoAcceptFavorites ?? true;
      if (trusted && autoAccept) {
        await acceptIncoming(request, false);
      } else {
        setTrustRequest(request);
      }
    },
    [acceptIncoming],
  );

  const flushOutbox = useCallback(async () => {
    const rows = await listOutbox();
    for (const row of rows) {
      const delay = Math.min(2 ** Math.min(row.attempts, 6), 60) * 1000;
      if (row.attempts > 0 && Date.now() - row.created_at < delay) continue;
      try {
        const message = JSON.parse(row.payload) as ChatMessage;
        await sendChatText(row.target, message);
        await removeOutbox(row.id);
        await patchMessage(message.id, { status: "sent" });
        await refreshMessages(activeRef.current);
      } catch (e) {
        await bumpOutbox(row.id, String(e));
      }
    }
  }, [refreshMessages]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const boot = async () => {
      // 先订阅，再做任何可能耗时或重启服务的动作：订阅太晚会漏掉 incoming 事件，
      // 而 Rust 侧只在自动开窗后补发一次（900ms），救不回来。
      const off = await subscribeTransfer((payload) => {
        switch (payload.type) {
          case "devices":
            setDevices(payload.devices);
            void flushOutbox();
            break;
          case "message":
            void handleIncomingMessage(payload.message);
            break;
          case "incoming":
            void handleIncoming(payload);
            break;
          case "progress": {
            const name = baseName(payload.file_id || "");
            void refreshMessages(activeRef.current);
            if (name) void name;
            break;
          }
          case "completed": {
            if (payload.path && payload.direction === "receive") {
              void (async () => {
                const rows = await listMessages(activeRef.current ?? "");
                const target = [...rows]
                  .reverse()
                  .find((r) => r.kind !== "text" && !r.file_path);
                if (target) {
                  await patchMessage(target.id, {
                    file_path: payload.path ?? null,
                    status: "sent",
                    progress: 100,
                  });
                  await refreshMessages(activeRef.current);
                }
              })();
            }
            break;
          }
          case "error":
            flash(payload.message);
            break;
          default:
            break;
        }
      });
      if (cancelled) {
        off();
        return;
      }
      unlisten = off;

      await initChatDb();
      await refreshConversations();

      let current = await getTransferState();
      if (!current.running) {
        try {
          const started = await startTransfer();
          current = {
            ...current,
            running: started.running,
            alias: started.alias,
            port: started.port,
            fingerprint: started.fingerprint,
            error: null,
          };
        } catch (e) {
          // 启动失败要让人看见原因（端口占用 / 防火墙 / 组播不可用），别再静默吞掉。
          current = { ...current, running: false, error: String(e) };
        }
      }
      if (cancelled) return;
      setState(current);

      // 把还挂在服务端的待确认请求补回来（与旧「高级设置」界面一致）
      await restorePending();

      await discoverDevices().catch(() => undefined);
    };

    void boot();

    const retry = window.setInterval(() => void flushOutbox(), 5000);
    const discover = window.setInterval(
      () => void discoverDevices().catch(() => undefined),
      15000,
    );
    const onResize = () => setNarrow(window.innerWidth < 720);
    onResize();
    window.addEventListener("resize", onResize);

    return () => {
      cancelled = true;
      unlisten?.();
      window.clearInterval(retry);
      window.clearInterval(discover);
      window.removeEventListener("resize", onResize);
    };
  }, [flash, flushOutbox, handleIncoming, handleIncomingMessage, refreshConversations, refreshMessages, restorePending]);

  const startService = useCallback(async () => {
    try {
      const info = await startTransfer();
      setState((prev) =>
        prev
          ? {
              ...prev,
              running: info.running,
              alias: info.alias,
              port: info.port,
              fingerprint: info.fingerprint,
              error: null,
            }
          : prev,
      );
      await discoverDevices().catch(() => undefined);
    } catch (e) {
      const text = String(e);
      setState((prev) => (prev ? { ...prev, running: false, error: text } : prev));
      flash(text);
    }
  }, [flash]);

  const stopService = useCallback(async () => {
    try {
      await stopTransfer();
      setState((prev) => (prev ? { ...prev, running: false } : prev));
      setDevices([]);
    } catch (e) {
      flash(String(e));
    }
  }, [flash]);

  /** 刷新 = 重新广播一次并扫描；服务没跑就先拉起来。 */
  const refreshDevices = useCallback(async () => {
    try {
      if (!stateRef.current?.running) {
        await startService();
        return;
      }
      await discoverDevices();
    } catch (e) {
      flash(String(e));
    }
  }, [flash, startService]);

  const openConversation = useCallback(
    async (id: string) => {
      setActiveId(id);
      await refreshMessages(id);
      await markRead(id);
      await refreshConversations();
      setMenu(null);
    },
    [refreshConversations, refreshMessages],
  );

  const sendText = useCallback(async () => {
    const body = input.trim();
    if (!body || !activeId) return;
    const msg = newMessage(
      activeId,
      selfRef.current.fp,
      selfRef.current.alias,
      "text",
      body,
      { status: "pending" },
    );
    await insertMessage(rowFromMessage(msg, "send"));
    await upsertConversation(activeId, activeId.slice(0, 8), msg.created_at);
    setInput("");
    await refreshMessages(activeId);
    await refreshConversations();
    try {
      await sendChatText(activeId, msg);
      await patchMessage(msg.id, { status: "sent" });
    } catch (e) {
      await patchMessage(msg.id, { status: "failed" });
      await enqueueOutbox(activeId, msg);
      flash(`对方不在线，消息已排队：${String(e)}`);
    }
    await refreshMessages(activeId);
  }, [activeId, flash, input, refreshConversations, refreshMessages]);

  const pickAndSend = useCallback(
    async (imagesOnly: boolean) => {
      if (!activeId) return;
      const picked = await open({
        multiple: true,
        directory: false,
        filters: imagesOnly
          ? [{ name: "图片", extensions: [...IMAGE_EXT] }]
          : undefined,
      });
      const paths = !picked ? [] : Array.isArray(picked) ? picked : [picked];
      if (paths.length === 0) return;

      const previews: Record<string, string> = {};
      for (const path of paths) {
        if (isImage(path)) {
          try {
            previews[path] = await makeThumbnail(path);
          } catch {
            // 缩略图失败不影响发文件
          }
        }
        const msg = newMessage(
          activeId,
          selfRef.current.fp,
          selfRef.current.alias,
          isImage(path) ? "image" : "file",
          baseName(path),
          {
            // 发送方向的 file_path 是用户原始文件，删除时绝不碰（Q32）。
            file_path: path,
            preview: previews[path] ?? null,
            status: "pending",
          },
        );
        await insertMessage(rowFromMessage(msg, "send"));
      }
      await upsertConversation(activeId, activeId.slice(0, 8), Date.now());
      await refreshMessages(activeId);
      await refreshConversations();
      // 串行排队：一次只允许一个在途文件传输，第二个不会撞服务端的 409。
      fileSendChain = fileSendChain
        .catch(() => undefined)
        .then(() => sendChatFiles(paths, activeId, previews))
        .catch((e: unknown) => flash(String(e)));
      await fileSendChain;
    },
    [activeId, flash, refreshConversations, refreshMessages],
  );

  const removeMessage = useCallback(
    async (row: ChatRow) => {
      if (
        !window.confirm(
          "删除这条消息，并同时删除本地文件？此操作不可恢复（发送方原始文件不会被删）。",
        )
      ) {
        return;
      }
      // 只删接收副本：发送方向的消息即使带 file_path 也不能删磁盘文件。
      if (row.direction === "receive" && row.file_path) {
        try {
          await deleteChatFile(row.file_path);
        } catch (e) {
          flash(String(e));
        }
      }
      await deleteMessageRow(row.id);
      await refreshMessages(activeRef.current);
      setMenu(null);
    },
    [flash, refreshMessages],
  );

  const copyImage = useCallback(
    async (row: ChatRow) => {
      if (!row.file_path && !row.preview) return;
      try {
        const bytes = row.file_path
          ? await readFileBytes(row.file_path)
          : Uint8Array.from(atob(row.preview ?? ""), (c) => c.charCodeAt(0));
        const blob = new Blob([new Uint8Array(bytes)], {
          type: row.file_path ? mimeOf(row.file_path) : "image/jpeg",
        });
        const clipboard = navigator.clipboard as unknown as {
          write: (items: unknown[]) => Promise<void>;
        };
        await clipboard.write([new ClipboardItem({ [blob.type]: blob })]);
        flash("图片已复制到剪贴板");
      } catch (e) {
        flash(`复制失败：${String(e)}`);
      }
      setMenu(null);
    },
    [flash],
  );

  const saveAs = useCallback(
    async (row: ChatRow) => {
      if (!row.file_path) return;
      const dest = await save({ defaultPath: baseName(row.file_path) });
      if (dest) {
        try {
          await copyChatFile(row.file_path, dest);
          flash("已另存");
        } catch (e) {
          flash(String(e));
        }
      }
      setMenu(null);
    },
    [flash],
  );

  const showInFolder = useCallback(
    async (row: ChatRow) => {
      if (!row.file_path) return;
      const dir = row.file_path.replace(/[\\/][^\\/]*$/, "");
      await openPath(dir).catch((e) => flash(String(e)));
      setMenu(null);
    },
    [flash],
  );

  const openFile = useCallback(
    async (row: ChatRow) => {
      if (!row.file_path) return;
      await openPath(row.file_path).catch((e) => flash(String(e)));
      setMenu(null);
    },
    [flash],
  );

  const forwardTo = useCallback(
    async (target: string, alias: string) => {
      if (!forwardRow) return;
      const row = forwardRow;
      if (row.kind !== "text") {
        if (!row.file_path || !(await chatFileExists(row.file_path))) {
          flash("原文件已删除或移动，无法转发");
          setForwardRow(null);
          return;
        }
        await sendChatFiles(
          [row.file_path],
          target,
          row.preview ? { [row.file_path]: row.preview } : {},
        );
      }
      const forwarded = newMessage(
        target,
        selfRef.current.fp,
        selfRef.current.alias,
        row.kind === "text" ? "text" : row.kind,
        row.body,
        {
          file_path: row.file_path,
          file_size: row.file_size,
          preview: row.preview,
          forwarded_from: row.sender_alias,
        },
      );
      await insertMessage(rowFromMessage(forwarded, "send"));
      await upsertConversation(target, alias, forwarded.created_at);
      // 本机零副本：只在原消息上打「已转发给 X」标记（Q39）。
      await patchMessage(row.id, { forwarded_from: alias });
      if (row.kind === "text") {
        await sendChatText(target, forwarded).catch(() => undefined);
      }
      setForwardRow(null);
      await refreshConversations();
      await refreshMessages(activeRef.current);
      flash(`已转发给 ${alias}`);
    },
    [flash, forwardRow, refreshConversations, refreshMessages],
  );

  const targets = [
    ...conversations.map((c) => ({ id: c.id, alias: c.alias })),
    ...devices
      .filter((d) => !conversations.some((c) => c.id === d.fingerprint.toUpperCase()))
      .map((d) => ({ id: d.fingerprint.toUpperCase(), alias: d.alias })),
  ].filter((t) => t.id !== activeId);

  const activeAlias =
    conversations.find((c) => c.id === activeId)?.alias ??
    devices.find((d) => d.fingerprint.toUpperCase() === activeId)?.alias ??
    activeId?.slice(0, 8) ??
    "";

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-[#f5f5f7]/40 text-neutral-800 select-none dark:bg-neutral-900/40 dark:text-neutral-100">
      <div className="flex h-11 shrink-0 items-center justify-between border-b border-black/5 px-3 dark:border-white/5">
        <div className="flex min-w-0 items-center gap-2">
          <ArrowLeftRight size={15} className="text-neutral-500" />
          <span className="text-[13px] font-medium">消息</span>
          <span
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              state?.running ? "bg-emerald-500" : "bg-neutral-400",
            )}
          />
          <span className="truncate text-[10px] text-neutral-400">
            {state?.running
              ? `已连接 · ${devices.length} 台设备`
              : state?.error
                ? "互传启动失败"
                : "互传未启动"}
          </span>
        </div>
        <div className="flex items-center gap-1">
          {state?.running ? (
            <button
              className="rounded p-1.5 hover:bg-black/5 dark:hover:bg-white/10"
              title="停止互传服务"
              onClick={() => void stopService()}
            >
              <Power size={14} />
            </button>
          ) : (
            <button
              className="rounded p-1.5 text-emerald-600 hover:bg-black/5 dark:hover:bg-white/10"
              title="启动互传服务"
              onClick={() => void startService()}
            >
              <Play size={14} />
            </button>
          )}
          <button
            className="rounded p-1.5 hover:bg-black/5 dark:hover:bg-white/10"
            title="刷新设备（重新广播并扫描）"
            onClick={() => void refreshDevices()}
          >
            <RefreshCw size={14} />
          </button>
          <button
            className="rounded p-1.5 hover:bg-black/5 dark:hover:bg-white/10"
            title="高级设置 / 旧版互传界面"
            onClick={onLegacy}
          >
            <Settings2 size={14} />
          </button>
        </div>
      </div>

      {state?.error && (
        <div className="shrink-0 border-b border-black/5 bg-red-500/10 px-3 py-1.5 text-[10px] text-red-500 dark:border-white/5">
          {state.error}
        </div>
      )}

      {state?.running && devices.length === 0 && (
        <div className="shrink-0 border-b border-black/5 px-3 py-1.5 text-[10px] text-amber-600 dark:border-white/5">
          还没发现附近设备：让对方在自己的互传里点一次「刷新」，并确认两台设备在同一个
          Wi-Fi、Windows 防火墙已放行本程序（组播入站被拦时手机端看不到本机）。
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        {(!narrow || !activeId) && (
          <aside className="flex w-64 shrink-0 flex-col border-r border-black/5 dark:border-white/5">
            <div className="flex items-center gap-2 px-3 py-2 text-[11px] text-neutral-400">
              <MessageSquare size={12} /> 会话
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto">
              {conversations.length === 0 && (
                <div className="px-3 py-6 text-center text-[11px] text-neutral-400">
                  还没有会话，从右侧「附近设备」开始
                </div>
              )}
              {conversations.map((c) => (
                <button
                  key={c.id}
                  onClick={() => void openConversation(c.id)}
                  className={cn(
                    "flex w-full items-center gap-2 px-3 py-2 text-left text-[12px]",
                    activeId === c.id
                      ? "bg-black/5 dark:bg-white/10"
                      : "hover:bg-black/[0.03] dark:hover:bg-white/5",
                  )}
                >
                  <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-neutral-200 text-[10px] dark:bg-neutral-700">
                    {c.alias.slice(0, 1).toUpperCase()}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate">{c.alias}</div>
                    <div className="truncate text-[10px] text-neutral-400">
                      {c.id.slice(0, 12)}…
                    </div>
                  </div>
                  {c.unread > 0 && (
                    <span className="rounded-full bg-blue-500 px-1.5 text-[10px] text-white">
                      {c.unread}
                    </span>
                  )}
                  {c.pinned === 1 && <Star size={11} className="text-amber-500" />}
                </button>
              ))}
            </div>
            <div className="border-t border-black/5 px-3 py-2 text-[11px] text-neutral-400 dark:border-white/5">
              <div className="mb-1 flex items-center gap-1">
                <Wifi size={12} /> 附近设备
              </div>
              {devices.length === 0 && (
                <div className="text-[10px] text-neutral-400">暂未发现设备</div>
              )}
              {devices.map((d) => {
                const id = d.fingerprint.toUpperCase();
                return (
                  <button
                    key={id}
                    className="flex w-full items-center justify-between rounded px-1 py-1 text-left hover:bg-black/5 dark:hover:bg-white/10"
                    onClick={() => void openConversation(id)}
                  >
                    <span className="truncate">{d.alias}</span>
                    <span className="text-[9px] text-neutral-400">仅文件?</span>
                  </button>
                );
              })}
              <div className="mt-2 flex gap-1">
                <input
                  value={manualIp}
                  onChange={(e) => setManualIp(e.target.value)}
                  placeholder="手动 IP"
                  className="min-w-0 flex-1 rounded border border-black/10 bg-transparent px-1.5 py-1 text-[10px] dark:border-white/10"
                />
                <button
                  className="rounded bg-black/5 px-2 text-[10px] dark:bg-white/10"
                  onClick={() =>
                    void addManualIp(manualIp.trim()).then(() => setManualIp(""))
                  }
                >
                  添加
                </button>
              </div>
            </div>
          </aside>
        )}

        {(!narrow || activeId) && (
          <main className="flex min-w-0 flex-1 flex-col">
            {!activeId ? (
              <div className="flex h-full flex-col items-center justify-center gap-2 text-neutral-400">
                <MessageSquare size={28} />
                <div className="text-[12px]">选一个设备开始聊天</div>
              </div>
            ) : (
              <>
                <div className="flex h-9 shrink-0 items-center justify-between border-b border-black/5 px-3 text-[12px] dark:border-white/5">
                  <div className="flex items-center gap-2">
                    {narrow && (
                      <button onClick={() => setActiveId(null)} title="返回">
                        <X size={13} />
                      </button>
                    )}
                    <span className="font-medium">{activeAlias}</span>
                    <span className="text-[10px] text-neutral-400">
                      {activeId.slice(0, 12)}…
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      className="text-[10px] text-neutral-400 hover:text-amber-500"
                      title="撤销后对方下次发文件会重新要求首次确认"
                      onClick={() => {
                        void (async () => {
                          await revokeTrust(activeId);
                          flash("已撤销信任，下次发文件会重新确认");
                        })();
                      }}
                    >
                      取消信任
                    </button>
                    <button
                      className="text-[10px] text-neutral-400 hover:text-red-500"
                      onClick={() => {
                        if (window.confirm("清空这个会话的本地历史？")) {
                          void (async () => {
                            await clearConversation(activeId);
                            setActiveId(null);
                            await refreshConversations();
                          })();
                        }
                      }}
                    >
                      清空历史
                    </button>
                  </div>
                </div>

                <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
                  {messages.length === 0 && (
                    <div className="pt-10 text-center text-[11px] text-neutral-400">
                      没有消息，发一条试试
                    </div>
                  )}
                  {messages.map((row) => (
                    <div
                      key={row.id}
                      className={cn(
                        "flex",
                        row.direction === "send" ? "justify-end" : "justify-start",
                      )}
                    >
                      <div
                        onContextMenu={(e) => {
                          e.preventDefault();
                          setMenu({ x: e.clientX, y: e.clientY, row });
                        }}
                        className={cn(
                          "max-w-[70%] rounded-lg px-3 py-2 text-[12px] shadow-sm",
                          row.direction === "send"
                            ? "bg-blue-500 text-white"
                            : "bg-white dark:bg-neutral-800",
                        )}
                      >
                        {row.preview ? (
                          <img
                            src={dataUrl(row.preview)}
                            alt={row.body}
                            className="max-h-48 cursor-zoom-in rounded"
                            onClick={() =>
                              setBigImage({ src: dataUrl(row.preview ?? ""), name: row.body })
                            }
                          />
                        ) : row.kind === "text" ? (
                          <span className="whitespace-pre-wrap break-words">{row.body}</span>
                        ) : (
                          <span className="flex items-center gap-1.5">
                            <Paperclip size={12} />
                            {row.body}
                            {row.file_size ? ` · ${formatSize(row.file_size)}` : ""}
                          </span>
                        )}
                        <div className="mt-1 flex items-center gap-2 text-[9px] opacity-60">
                          <span>{new Date(row.created_at).toLocaleTimeString()}</span>
                          {row.status === "pending" && <span>发送中…</span>}
                          {row.status === "failed" && <span>待重发</span>}
                          {row.forwarded_from && <span>转发自 {row.forwarded_from}</span>}
                        </div>
                      </div>
                    </div>
                  ))}
                </div>

                <div className="shrink-0 border-t border-black/5 p-2 dark:border-white/5">
                  <div className="flex items-end gap-2">
                    <button
                      className="rounded p-2 hover:bg-black/5 dark:hover:bg-white/10"
                      title="发文件"
                      onClick={() => void pickAndSend(false)}
                    >
                      <Paperclip size={15} />
                    </button>
                    <button
                      className="rounded p-2 hover:bg-black/5 dark:hover:bg-white/10"
                      title="发图片"
                      onClick={() => void pickAndSend(true)}
                    >
                      <ImageIcon size={15} />
                    </button>
                    <textarea
                      value={input}
                      onChange={(e) => setInput(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && !e.shiftKey) {
                          e.preventDefault();
                          void sendText();
                        }
                      }}
                      rows={1}
                      placeholder="输入消息，Enter 发送"
                      className="max-h-24 min-h-[34px] flex-1 resize-none rounded border border-black/10 bg-white px-2 py-1.5 text-[12px] outline-none dark:border-white/10 dark:bg-neutral-800"
                    />
                    <button
                      className="rounded bg-blue-500 p-2 text-white disabled:opacity-40"
                      disabled={!input.trim()}
                      onClick={() => void sendText()}
                      title="发送"
                    >
                      <Send size={14} />
                    </button>
                  </div>
                </div>
              </>
            )}
          </main>
        )}
      </div>

      {notice && (
        <div className="absolute bottom-3 left-1/2 -translate-x-1/2 rounded bg-black/80 px-3 py-1.5 text-[11px] text-white">
          {notice}
        </div>
      )}

      {trustRequest && (
        <div className="absolute inset-0 flex items-center justify-center bg-black/40">
          <div className="w-80 rounded-lg bg-white p-4 text-[12px] shadow-xl dark:bg-neutral-800">
            <div className="mb-2 font-medium">首次接收确认</div>
            <div className="mb-3 text-[11px] text-neutral-500">
              {trustRequest.alias} 第一次给你发文件。确认后这一对设备后续互传不再需要确认。
            </div>
            <div className="mb-3 max-h-32 space-y-1 overflow-y-auto">
              {trustRequest.files.map((f) => (
                <div key={f.id} className="flex items-center gap-2">
                  {f.preview ? (
                    <img src={dataUrl(f.preview)} className="h-10 w-10 rounded object-cover" alt="" />
                  ) : (
                    <Paperclip size={12} />
                  )}
                  <span className="truncate">{f.file_name}</span>
                  <span className="text-[10px] text-neutral-400">{formatSize(f.size)}</span>
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2">
              <button
                className="rounded px-3 py-1.5 hover:bg-black/5 dark:hover:bg-white/10"
                onClick={() => {
                  void rejectTransfer(trustRequest.sessionId);
                  setTrustRequest(null);
                }}
              >
                拒绝
              </button>
              <button
                className="flex items-center gap-1 rounded bg-blue-500 px-3 py-1.5 text-white"
                onClick={() => void acceptIncoming(trustRequest, true)}
              >
                <Check size={12} /> 接收并信任
              </button>
            </div>
          </div>
        </div>
      )}

      {forwardRow && (
        <div className="absolute inset-0 flex items-center justify-center bg-black/40">
          <div className="w-72 rounded-lg bg-white p-4 text-[12px] shadow-xl dark:bg-neutral-800">
            <div className="mb-2 flex items-center justify-between font-medium">
              <span>转发给</span>
              <button onClick={() => setForwardRow(null)}>
                <X size={13} />
              </button>
            </div>
            <div className="max-h-52 space-y-1 overflow-y-auto">
              {targets.length === 0 && (
                <div className="text-[11px] text-neutral-400">没有其他设备</div>
              )}
              {targets.map((t) => (
                <button
                  key={t.id}
                  className="flex w-full justify-between rounded px-2 py-1.5 text-left hover:bg-black/5 dark:hover:bg-white/10"
                  onClick={() => void forwardTo(t.id, t.alias)}
                >
                  <span className="truncate">{t.alias}</span>
                  <span className="text-[9px] text-neutral-400">{t.id.slice(0, 8)}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {bigImage && (
        <div
          className="absolute inset-0 flex items-center justify-center bg-black/80"
          onClick={() => setBigImage(null)}
        >
          <img src={bigImage.src} alt={bigImage.name} className="max-h-[90%] max-w-[90%]" />
        </div>
      )}

      {menu && (
        <>
          <div className="absolute inset-0" onClick={() => setMenu(null)} />
          <div
            className="absolute z-10 w-40 rounded-lg border border-black/10 bg-white py-1 text-[11px] shadow-xl dark:border-white/10 dark:bg-neutral-800"
            style={{ left: Math.min(menu.x, window.innerWidth - 170), top: menu.y }}
          >
            {(menu.row.kind === "image" || menu.row.preview) && (
              <MenuItem icon={<Copy size={12} />} label="复制图片" onClick={() => void copyImage(menu.row)} />
            )}
            {menu.row.file_path && (
              <>
                <MenuItem icon={<FolderOpen size={12} />} label="在文件夹中显示" onClick={() => void showInFolder(menu.row)} />
                <MenuItem icon={<Download size={12} />} label="另存为" onClick={() => void saveAs(menu.row)} />
                <MenuItem icon={<Paperclip size={12} />} label="用默认程序打开" onClick={() => void openFile(menu.row)} />
              </>
            )}
            <MenuItem icon={<Forward size={12} />} label="转发" onClick={() => { setForwardRow(menu.row); setMenu(null); }} />
            <MenuItem icon={<Trash2 size={12} />} label="删除（含本地文件）" danger onClick={() => void removeMessage(menu.row)} />
          </div>
        </>
      )}
    </div>
  );
}

function MenuItem({
  icon,
  label,
  onClick,
  danger,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-black/5 dark:hover:bg-white/10",
        danger && "text-red-500",
      )}
    >
      {icon}
      {label}
    </button>
  );
}
