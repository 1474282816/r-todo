# OPT-002 · Win+D 抵抗（放弃）

**状态**: **放弃**
**发现于**: M2 用户验收
**放弃于**: 2026-07-18

## 目标

让 r-todo 窗口像 Rainmeter/Fences/滴答清单桌面组件那样：Win+D 时保持在桌面上不被隐藏。

## 尝试过的方案（全部在用户机器上失败）

用户环境：Windows 10 20H2。

| 方案 | 现象 |
|---|---|
| `HWND_BOTTOM` + `WS_EX_TOOLWINDOW` + 500ms 轮询 watcher | 闪烁、race condition、事件风暴 |
| Subclass 拦截 `WM_WINDOWPOSCHANGING(SWP_HIDEWINDOW)` + `SC_MINIMIZE` | shell 根本没发这些消息给 WndProc |
| 完全替换 `WndProc`（tauri-plugin-wallpaper pinner.rs 同款）+ 拦 sentinel 位置 | 按 Win+D 时 WndProc 完全没日志变化，shell 绕过消息循环 |
| `SetParent(hwnd, Progman)` + `WS_CHILD`（Rainmeter/滴答清单方案）| 启动后窗口完全看不见 |

## 根本原因（推测）

用户机器上 Win+D 走的不是标准 `SW_MINIMIZE` 消息路径，而是 shell（explorer.exe）跨进程直接改窗口 state，进程内的 WndProc/subclass 全部拦不到。这可能与该机器上安装的软件（安全软件、桌面工具）hook 有关。

## 当前状态

保留基础 `WS_EX_TOOLWINDOW + HWND_BOTTOM` 样式（不进任务栏/Alt+Tab、置底），但**接受 Win+D 会隐藏**。跟微软便笺、Todoist、Microsoft To Do 等主流待办应用行为一致——它们也做不到抵抗 Win+D。

## 未来重开的条件

- 用户换机器测试，验证是不是环境特异
- 或者投入几天时间反汇编 Snipaste/滴答清单 找到它们的真实机制
- 或者切换到全局键盘 hook 吞掉 Win+D 组合键（副作用：全系统 Win+D 失效）
