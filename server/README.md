# r-todo 服务端（PocketBase）

## 部署

服务端设计为**部署在独立的一台机器上**（Linux 服务器 / 家里 NAS / VPS 均可），客户端通过网络访问。**不推荐在开发用的 Windows 上跑 Docker**。

### 部署到 Linux 机器

```bash
# 在 Linux 机器上
git clone <this-repo>
cd r-todo/server
docker compose up -d           # 后台常驻，机器重启自动拉起
docker compose logs -f         # 首次启动看日志
```

首次启动后：
1. 打开 `http://<linux-ip>:8090/_/` 进入管理后台
2. 创建 admin 账号
3. `tasks` collection 已由 migration 自动创建

### 反向代理（1Panel + OpenResty）

**如果你的 Linux 机器装了 1Panel**：不要自己再装 nginx/caddy，用 1Panel 内置的 OpenResty。

1. `docker-compose.yml` 已配置 PocketBase 只监听 `127.0.0.1:8090`（不对外暴露）
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

初次使用需要 admin 在 PocketBase 后台：
1. 创建普通用户账号（Users collection）
2. 给该用户生成一个 impersonation token 或使用其登录 token
3. 把 URL + token 交给客户端

> 注：PocketBase 原生没有"永久 API Token"概念，登录 token 有过期时间（默认 7 天）。
> 想要类 Bitwarden 那种"URL + 密钥"永久登录，有两条路：
>   1. 客户端保存邮箱密码，本地生命周期内自动刷新 token（简单）
>   2. 写一个 JS Hook 增加 "长期 API Key" collection（灵活但工作量大）
>
> **决策**：骨架阶段先走路径 1；如果体验不理想再迭代到路径 2。见 `docs/decisions/003-sync-strategy.md`。

## 数据备份

`pb_data/` 目录就是全部数据（SQLite + 附件）。定期 `tar czf backup.tgz pb_data/` 即可。
