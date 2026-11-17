# Tauri 图标占位

需要放置以下图标文件（`tauri.conf.json` 中引用）：

- `icon.ico`（Windows）

生成方式：
```bash
pnpm tauri icon path/to/source.png
```
Tauri CLI 会从 1024x1024 的源图自动生成所有平台所需尺寸。
