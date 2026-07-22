# 起 Flutter 移动端开发环境
#
# 依赖：Flutter SDK、已连接设备或已启动模拟器
Write-Host "→ 启动 Flutter 移动端 (dev)" -ForegroundColor Cyan
Set-Location -LiteralPath "$PSScriptRoot\..\apps\mobile"
flutter run
