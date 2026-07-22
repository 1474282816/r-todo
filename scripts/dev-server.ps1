# 起 PocketBase 服务（本地 Docker）
#
# 注意：推荐把 PocketBase 部署到独立的 Linux 机器上（见 server/README.md），
# 而不是在开发用的 Windows 上跑 Docker。此脚本仅在你确实想本机跑时使用。
Write-Host "→ 启动 PocketBase (本地 Docker)" -ForegroundColor Cyan
Write-Host "  提示：更建议部署到 Linux 服务器，见 server/README.md" -ForegroundColor Yellow
docker compose -f "$PSScriptRoot\..\server\docker-compose.yml" up
