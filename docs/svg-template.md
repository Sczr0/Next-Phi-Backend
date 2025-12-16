# SVG 外部模板化（BN / Song）

日期：2025-12-16  
执行者：Codex

本文档说明 Phi-Backend 当前的“外部文件化 SVG 模板”方案：模板文件放在 `resources/` 下，运行时加载渲染，从而允许通过改模板实现“卡片布局与字段排列”的调整，而不必修改 Rust 代码。

## 1. 模板文件位置

- BestN（BN）：`resources/templates/image/bn/{id}.svg.jinja`
- 单曲（Song）：`resources/templates/image/song/{id}.svg.jinja`

可选布局参数（JSON，字段见下文）：
- `resources/templates/image/bn/{id}.json`
- `resources/templates/image/song/{id}.json`

代码入口：
- `src/features/image/renderer/svg_templates.rs`

## 2. API 使用方式

所有图片接口（`/image/bn`、`/image/song`、`/image/bn/user`）新增 Query 参数：
- `template`：模板 ID（例如 `default`）

规则：
- 不传 `template`：走“内置手写 SVG”实现（兼容现有行为）
- 传 `template=xxx`：加载外部模板 `resources/templates/image/{kind}/xxx.svg.jinja`

## 3. 上下文契约（核心变量）

模板引擎：MiniJinja（Jinja 风格语法），默认不启用自动转义。

重要约定：
- 上下文字段中以 `_xml` 结尾的字符串，Rust 侧已做 XML 转义，模板应优先使用它们。
- 允许包含 `<tspan>` 的“片段字段”会明确写在 `inner_xml` 里（同样已转义，且仅由后端拼接固定结构）。

### 3.1 BN 模板变量（`bn/*.svg.jinja`）

- `page.width` / `page.height`
- `fonts.main`（默认中文字体名）
- `colors.*`（背景渐变/文字/卡片等）
- `layout.*`（来自 `bn/{id}.json`，或默认值）
- `background.href_xml` / `background.overlay_rgba`
- `header.player_title_xml` / `header.ap_text_xml` / `header.bn_text_xml`
- `header.right_lines[]`：右上角信息行
  - `y` / `class` / `inner_xml`
- `ap.section_y` / `ap.cards[]`
- `cards[]`：主列表卡片

卡片结构（`cards[]`/`ap.cards[]`）主要字段：
- `x,y,w,h,radius`
- `class_extra`（例如 `card-fc`）
- `clip_id`
- `cover.{x,y,w,h,href_xml}`
- `badge.diff.*`（难度徽章）
- `badge.fc_ap`（可选 AP/FC 徽章）
- `text.*`（坐标 + `*_xml` 文本 + rank）

### 3.2 Song 模板变量（`song/*.svg.jinja`）

- `page.width` / `page.height`
- `fonts.main`
- `layout.*`（来自 `song/{id}.json`，或默认值）
- `background.href_xml`
- `illust.{x,y,w,h,r,clip_id,href_xml}`
- `player.{x,y_name,y_rks,name_xml,rks_xml}`
- `song.{cx,y,name_xml}`
- `difficulty_cards[]`：四难度卡片（EZ/HD/IN/AT），每张卡包含坐标、class 与文案（`*_xml`）
- `footer.{y,pad,text_xml}`

## 4. 调参建议

最常用的“无需改 Rust 代码”的调参：
- BN：改 `bn/default.json` 的 `columns`、`card_gap`、`song_name_max_width`、`song_name_max_lines`
- Song：改 `song/default.json` 的 `width/height/padding`

如果模板改动导致内容超出画布：
- BN：优先增大 `bn/{id}.json` 的 `width` 或减少 `columns`
- Song：优先增大 `song/{id}.json` 的 `width/height`

