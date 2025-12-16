# SVG 模板（外部文件化）

日期：2025-12-16  
执行者：Codex

本目录用于存放“图片渲染（BN / Song 等）”的 SVG 外部模板文件。

设计目标：
- 允许通过修改模板文件来调整卡片布局与字段排列（无需改 Rust 代码）。
- 模板与数据通过明确的“上下文契约”对接，避免隐式耦合。
- 模板文件随项目发布，默认位于 `resources/` 下，便于部署与回滚。

入口代码：
- `src/features/image/renderer/svg_templates.rs`

目录约定：
- `resources/templates/image/bn/*.svg.jinja`：BestN（BN）模板
- `resources/templates/image/song/*.svg.jinja`：单曲模板
- （可选）同名 `.json`：布局参数（列数、边距、分页等），用于让“网格/分页”不必改代码

