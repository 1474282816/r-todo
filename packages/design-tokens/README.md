# Design Tokens

三端共享的视觉规范。

## 文件

- `colors.json` — 颜色（优先级、勾选框、背景）
- `spacing.json` — 间距（8px 网格）

## 使用规范

不做自动生成，各端手动同步：

- **Tauri (Tailwind)**: `apps/desktop/tailwind.config.js` → `theme.extend.colors`
- **Flutter**: `apps/mobile/lib/shared/theme/tokens.dart`（尚未创建）

## 风格基准

参考 **Things 3**：
- 极简
- 低信息密度
- 大留白
- 细腻的优先级色标（不刺眼）
