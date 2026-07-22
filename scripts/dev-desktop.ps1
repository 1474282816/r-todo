# 起 Tauri 桌面端开发环境
#
# 依赖：Rust 工具链、pnpm、C++ Build Tools、WebView2 Runtime
Write-Host "→ 启动 Tauri 桌面端 (dev)" -ForegroundColor Cyan
pnpm --filter desktop tauri dev
