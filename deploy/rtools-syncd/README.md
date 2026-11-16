# rtools-syncd 部署

## 构建

```sh
cd deploy/rtools-syncd
docker compose build
docker compose up -d
```

## 首次配对

```sh
docker exec rtools-syncd rtools-syncd --data-dir /data pair
```

## 导入旧待办

```sh
docker exec rtools-syncd rtools-syncd --data-dir /data seed /migration/rtools-migration-20261116.json
```

## 备份

```sh
docker exec rtools-syncd rtools-syncd --data-dir /data backup
```

