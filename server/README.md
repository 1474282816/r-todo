# r-todo 服务端（PocketBase）

服务端设计为**部署在独立的一台机器上**（Linux 服务器 / 家里 NAS / VPS 均可），客户端通过网络访问。**不推荐在开发用的 Windows 上跑 Docker**。

## 快速开始（推荐，一条命令）

在 Linux 机器（含飞牛 fnOS 等 NAS，装好 Docker 即可）上：

```bash
# 1. 把 server/ 目录放到机器上（二选一）
git clone https://gitee.com/renrunqi/r-todo.git && cd r-todo/server
#   或只下载 server/ 目录（解压后 cd server）

# 2. 一键部署
bash setup.sh
```

`setup.sh` 会自动完成：启动容器 → 自动创建 admin 账号 → 创建普通用户 → 打印连接信息：

```
管理后台  : http://<ip>:8090/_/
后台账号  : admin@rtodo.local / <自动生成的密码>
客户端 URL : http://<ip>:8090
客户端 Token : <自动生成>
```

把 **URL + Token** 填进 r-todo 客户端即可开始同步。凭据保存在 `server/.credentials`（首次生成后复用；**Token 过期后重跑 `bash setup.sh` 刷新**）。

> 手动部署（不跑脚本）见下文"传统部署"。

## 传统部署

```bash
# 在 Linux 机器上
git clone <this-repo>
cd r-todo/server
docker compose up -d           # 后台常驻，机器重启自动拉起（入口会自动创建 admin）
docker compose logs -f         # 首次启动看日志
```

首次启动后（admin 已由容器入口自动创建）：
1. 打开 `http://<linux-ip>:8090/_/` 进入管理后台（账号密码见 `docker compose` 的 `ADMIN_EMAIL`/`ADMIN_PASSWORD` 环境变量，默认 `admin@rtodo.local` / `admin123456`，建议首次登录后修改）
2. `tasks` collection 已由 migration 自动创建

## 反向代理（1Panel + OpenResty）

**如果你的 Linux 机器装了 1Panel**：不要自己再装 nginx/caddy，用 1Panel 内置的 OpenResty。

1. `docker-compose.yml` 已配置 PocketBase 只监听 `8090`（局域网直连）
2. 1Panel 面板 → **网站** → 新建"反向代理"站点
3. 目标 URL：`http://127.0.0.1:8090`
4. **务必**在 location 配置里加上支持 SSE/WebSocket 的选项：

```nginx
location / {
    proxy_pass http://127.0.0.1:8090;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;

    # PocketBase Realtime (SSE) 需要
    proxy_buffering off;
    proxy_cache off;
    proxy_read_timeout 3600s;

    # WebSocket 升级
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
}
```

5. 在 1Panel 里为该站点签发 HTTPS 证书（Let's Encrypt 或自签）
6. 客户端从此使用 `https://your-domain.com` 连接

## 客户端"登录"

客户端不使用邮箱/手机号注册，而是用 **服务端 URL + 用户 API Token** 登录。

- `setup.sh` 已自动完成：创建用户并打印 Token，直接填进客户端即可。
- 手动方式：admin 在 PocketBase 后台创建普通用户，然后通过登录接口获取 Token。

> 注：PocketBase 原生的登录 token 有过期时间（默认 7 天）。token 过期后重跑
> `bash setup.sh`（幂等）即可拿到新 Token；未来可迭代为"长期 API Key"方案
> （见 `docs/decisions/003-sync-strategy.md`）。

## 数据备份

`pb_data/` 目录就是全部数据（SQLite + 附件）。定期 `tar czf backup.tgz pb_data/` 即可。
