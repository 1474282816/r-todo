# Windows 10 桌面嵌入方案（已废弃，归档备用）

> 状态：**已废弃**。Windows 11（22H2+，含 26H2 / Build 26300）上桌面结构改变，
> 本方案必然踩空，仅作为 Win10 老机器的历史方案存档。
> 现行方案见 `desktop_embed.rs` 的 `find_desktop_host` + `window_effect.rs`。

## 原理

Win10 的桌面窗口层级是：

```
Progman（桌面进程主窗口）
  └ WorkerW          ← 0x052C 消息把图标层"挤"到这个 WorkerW 里
      └ SHELLDLL_DefView   ← 桌面图标层
  WorkerW（壁纸层，可能有多个）
```

嵌入流程：

1. `SendMessageTimeout(FindWindow("Progman"), 0x052C, 0, 0)` —— 让 explorer
   把 `SHELLDLL_DefView`（图标层）从 Progman 直接子级挪进一个新建的 WorkerW；
2. `EnumWindows` 从顶往下找**包含 SHELLDLL_DefView 子窗口**的那个 WorkerW
   （这就是"图标层 WorkerW"）；
3. `SetParent(我们的窗口, 图标层WorkerW)` —— 挂上去后窗口显示在图标之上、
   所有应用窗口之下，鼠标可正常命中（可交互），Win+D 不会收起它。

要点：

- **首选挂含 DefView 的图标层 WorkerW**，挂上去鼠标可交互；
- 挂"壁纸层 WorkerW"（不含 DefView 的）窗口会被图标层挡住所有鼠标消息，
  只能看不能点，只能当壁纸用；
- `SetParent` 不修改 WS_CHILD/WS_POPUP 样式（MSDN 兼容性保证），窗口保持
  WS_POPUP 挂进去即可；
- 挂完 `SetWindowPos(..., SWP_FRAMECHANGED | SWP_SHOWWINDOW)` 刷新。

## 已知坑（全部踩过）

1. **0x052C 在部分 Win10 版本上不生效**（比如关了透明效果、或某些 1904x 构建），
   图标层还在 Progman 直接子级 —— 此时 `FindWindowEx(Progman, "SHELLDLL_DefView")`
   非 NULL，说明本机没有图标层 WorkerW 可挂。
2. **绝不能兜底挂到"新建的隐藏 WorkerW"**：0x052C 后顶层会多出几个隐藏的
   空 WorkerW（有的 135x37），挂进去窗口整条链 `IsWindowVisible=0`，
   `ShowWindow` 救不回来，托盘"显示主窗口"也无效。必须用
   `IsWindowVisible` 过滤。
3. **窗口状态插件会存 visible:false**：托盘"隐藏窗口"后退出，
   `tauri-plugin-window-state` 把不可见状态落盘，下次启动恢复成隐藏。
   解法：StateFlags 排除 `VISIBLE`，启动时强制 `show()+unminimize()`，
   窗口位置在屏幕外时 `center()` 兜底。
4. **cl 编译带中文注释的 C 必须加 `/utf-8`**，否则 MSVC 按 GBK 读源码直接报错。

## Win11 上为什么失效

Win11（22H2+）的桌面结构变为：

```
Progman
  ├ SHELLDLL_DefView        ← 图标层直接挂 Progman，不再包 WorkerW
  ├ WorkerW（壁纸层，可见）
顶层另有若干隐藏 WorkerW 残留（辅助窗口）
```

也就是说 Win11 上**不存在"图标层 WorkerW"**。旧的"找到含 DefView 的
WorkerW 挂进去"逻辑找不到目标，如果代码里再带一个"挂到任意 WorkerW"的
fallback，就会挂进隐藏 WorkerW → 窗口永久不可见（我们实际发生的 bug）。

Win11 现行方案：图标层在 Progman 下时**直接 SetParent 到 Progman**，
再用 `SetWindowPos(HWND_TOP)` 把窗口排到图标层之上。
