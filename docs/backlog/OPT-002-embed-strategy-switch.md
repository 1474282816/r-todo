# OPT-002 · Win+D 抵抗（已重开 → 已实现，待实机验证）

**状态**: **已重开并实现**（2026-08-07）
**发现于**: M2 用户验收
**放弃于**: 2026-07-18
**重开/解决于**: 2026-08-07

## 目标

让 r-todo 窗口像 Rainmeter/Fences/滴答清单桌面组件那样：Win+D 时保持在桌面上不被隐藏。

## 之前尝试过的方案（全部失败）

用户环境：Windows 10 20H2。

| 方案 | 现象 |
|---|---|
| `HWND_BOTTOM` + `WS_EX_TOOLWINDOW` + 500ms 轮询 watcher | 闪烁、race condition、事件风暴 |
| Subclass 拦截 `WM_WINDOWPOSCHANGING(SWP_HIDEWINDOW)` + `SC_MINIMIZE` | shell 根本没发这些消息给 WndProc |
| 完全替换 `WndProc`（tauri-plugin-wallpaper pinner.rs 同款）+ 拦 sentinel 位置 | 按 Win+D 时 WndProc 完全没日志变化，shell 绕过消息循环 |
| `SetParent(hwnd, Progman)` + `WS_CHILD`（Rainmeter/滴答清单方案）| 启动后窗口完全看不见 |

**失败根因**：当时以为 Rainmeter 是"挂 Progman"，实际正确的是挂 **0x052C 之后创建的"空 WorkerW"（壁纸层）**，且 **`SetParent` 不会也不应该改窗口的 `WS_CHILD`/`WS_POPUP` 样式**（MSDN 兼容性保证）。`WS_CHILD` + 隐藏的 Progman 双因素叠加 = 窗口不可见。Win+D 之所以收不走普通窗口，是因为它只最小化**顶层窗口**——把窗口变成 WorkerW 的子窗口后，Win+D 根本碰不到它。

## 新方案（2026-08-07 已实现，编译通过，待实机验证）

`apps/desktop/src-tauri/src/desktop_embed.rs` 重写为：

1. `FindWindowW("Progman")` → `SendMessageTimeoutW(0x052C, SMTO_NORMAL, 1000)` 触发 explorer 创建/显示 WorkerW（用 SendMessageTimeout 而非 SendMessage，Win11 兼容）；
2. `EnumWindows` 找类名 `WorkerW`：**优先挂"含 `SHELLDLL_DefView` 子窗口的 WorkerW"（桌面图标层）**——窗口显示在图标之上、应用之下，鼠标可正常命中；
   - ⚠️ **不要挂"空 WorkerW（壁纸层）"**：它在图标层之下，全屏图标层 WorkerW 会先接住所有鼠标消息，窗口看得见但**点不了**（electron-as-wallpaper 专门写低层钩子转发输入就是这个原因）；空 WorkerW 只作为 fallback（保底 Win+D 不收起，但不可交互）；
   - ⚠️ **坑**：`EnumWindows` 回调里**不能**调 `FindWindowExW`/`FindWindow`（内部会发消息，重入死锁报 `0x80070102` 等待超时）——回调只收集 HWND，类名判断和子窗口检查放到回调返回后的普通循环里；
3. `SetParent(hwnd, workerw)` —— **保持 `WS_POPUP`，绝不设 `WS_CHILD`**；
4. `SetWindowPos(SWP_FRAMECHANGED | SWP_SHOWWINDOW)` 刷新，触发 WebView2 重排重绘；
5. 跨进程 SetParent 会 forced reset 本进程 DPI awareness，随后 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 尽力恢复；
6. 卸载：`SetParent(hwnd, NULL)` + 恢复 `WS_EX_APPWINDOW`（回任务栏/Alt+Tab）。

依据（外部证据）：
- electron-as-wallpaper（Rust/Electron 壁纸库）`src/attach.rs`：Progman → 0x052C → DefView 之后的空 WorkerW → SetParent，全程不改样式
- tauri issue #10345：Tauri 2 用户 `SetParent` 挂桌面层成功
- tauri issue #4261：官方确认不内置该能力，需 Win32 自实现
- MSDN `SetParent`：For compatibility reasons, SetParent does not modify the WS_CHILD or WS_POPUP window styles
- SO 50778491 / 78169263：C# 同款算法 + "壁纸切换时 explorer 会新建 WorkerW 盖住"的坑

## 实机验证清单（用户机器）

- [ ] Win10 20H2：开启"嵌入桌面"后按 Win+D，窗口不被收起
- [ ] 窗口在图标之下、应用窗口之下（天然置底）
- [ ] 嵌入后输入（键盘/点击）正常、无白屏
- [ ] 切换"嵌入桌面"开关可逆（恢复浮空 + 回任务栏）
- [ ] Win11 机器（0x052C 行为不同）同样验证

## 已知限制（后续项）

- explorer 崩溃/重启、壁纸切换时 WorkerW 重建，挂载丢失 —— 后续加低频（3~5s）`GetParent` 检查自动重挂
- 跨进程 SetParent 的 DPI reset 在部分机器可能残留模糊 —— 若出现，需在窗口创建前用更高优先级手段规避
