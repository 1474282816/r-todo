#!/usr/bin/env bash
#
# r-todo 服务端一键部署（PocketBase + Docker）
#
# 用法：
#   bash setup.sh                 # 部署 + 首次初始化，打印管理后台账号和客户端 URL/Token
#   bash setup.sh http://NAS-IP:8090   # 指定服务端访问地址（默认 http://127.0.0.1:8090）
#
# 依赖：docker（含 compose 插件）、curl。
# 环境变量可覆盖：ADMIN_EMAIL / ADMIN_PASSWORD / USER_EMAIL / USER_PASSWORD
#   （不设置则自动生成随机密码，并保存到同目录 .credentials 文件，后续复用）
#
# 流程：docker compose up -d（入口自动创建 admin）→ 等就绪 → 创建用户 → 打印连接信息。
# 幂等：重复执行不会重复建用户；Token 过期后重跑本脚本即可拿到新 Token。

set -euo pipefail

cd "$(dirname "$0")"

BASE_URL="${1:-http://127.0.0.1:8090}"
CREDS_FILE="$(pwd)/.credentials"

# ── 凭据：首次生成随机密码并落盘，之后复用 ──
if [ -f "$CREDS_FILE" ]; then
  # shellcheck disable=SC1090
  source "$CREDS_FILE"
else
  ADMIN_EMAIL="${ADMIN_EMAIL:-admin@rtodo.local}"
  ADMIN_PASSWORD="${ADMIN_PASSWORD:-$(head -c 18 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 16)}"
  USER_EMAIL="${USER_EMAIL:-user@rtodo.local}"
  USER_PASSWORD="${USER_PASSWORD:-$(head -c 18 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 16)}"
  umask 177
  cat > "$CREDS_FILE" <<EOF
ADMIN_EMAIL=$ADMIN_EMAIL
ADMIN_PASSWORD=$ADMIN_PASSWORD
USER_EMAIL=$USER_EMAIL
USER_PASSWORD=$USER_PASSWORD
EOF
  umask 022
  echo "==> 已生成随机凭据，保存在 $CREDS_FILE（请妥善保管）"
fi
export ADMIN_EMAIL ADMIN_PASSWORD

# ── 1. 启动容器（compose 入口会自动创建 admin） ──
echo "==> 1/4 启动 PocketBase 容器"
docker compose up -d

# ── 2. 等待服务就绪 ──
echo "==> 2/4 等待服务就绪"
for _ in $(seq 1 60); do
  if curl -sf "$BASE_URL/api/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
if ! curl -sf "$BASE_URL/api/health" >/dev/null 2>&1; then
  echo "✗ 服务未就绪，请检查：docker compose logs pocketbase" >&2
  exit 1
fi

# ── 3. 登录 admin，创建用户（幂等），拿用户 Token ──
echo "==> 3/4 初始化用户"
ADMIN_TOKEN="$(curl -sf -X POST "$BASE_URL/api/admins/auth-with-password" \
  -H 'Content-Type: application/json' \
  -d "{\"identity\":\"$ADMIN_EMAIL\",\"password\":\"$ADMIN_PASSWORD\"}" \
  | sed -n 's/.*"token":"\([^"]*\)".*/\1/p' | head -1)"
if [ -z "$ADMIN_TOKEN" ]; then
  echo "✗ admin 登录失败。若之前部署过且忘了密码，删掉 server/pb_data 重新部署，或用 admin 后台重置。" >&2
  exit 1
fi

# 创建用户；已存在会返回 422，忽略
curl -s -o /dev/null -X POST "$BASE_URL/api/collections/users/records" \
  -H "Authorization: $ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASSWORD\",\"passwordConfirm\":\"$USER_PASSWORD\"}" \
  || true

USER_TOKEN="$(curl -sf -X POST "$BASE_URL/api/collections/users/auth-with-password" \
  -H 'Content-Type: application/json' \
  -d "{\"identity\":\"$USER_EMAIL\",\"password\":\"$USER_PASSWORD\"}" \
  | sed -n 's/.*"token":"\([^"]*\)".*/\1/p' | head -1)"

if [ -z "$USER_TOKEN" ]; then
  echo "✗ 用户登录失败" >&2
  exit 1
fi

# ── 4. 输出连接信息 ──
echo ""
echo "================================================================"
echo " r-todo 服务端就绪 ✅"
echo ""
echo "  管理后台 : $BASE_URL/_/"
echo "  后台账号 : $ADMIN_EMAIL / $ADMIN_PASSWORD"
echo ""
echo "  客户端 URL : $BASE_URL"
echo "  客户端 Token : $USER_TOKEN"
echo ""
echo "  用户账号 : $USER_EMAIL / $USER_PASSWORD（可登录后台/Web 端）"
echo ""
echo "  Token 默认 7 天过期，过期后重跑本脚本即可刷新。"
echo "  数据备份：tar czf backup.tgz server/pb_data/"
echo "================================================================"
