# SVG 模板设计与使用指南（MiniJinja）

日期：2025-12-16  
执行者：Codex

本文面向“需要通过模板调整 BN / Song 图片布局与字段排列”的开发者/设计者，说明如何在本项目中编写、调试并上线 SVG 模板。

> 背景说明：项目支持把 SVG “外壳与布局”外置到 `resources/templates/image/`，后端运行时用 MiniJinja 渲染模板，再按 `format` 选择直接返回 SVG 或栅格化为 PNG/JPEG/WebP。

相关实现与契约文档：
- 机制说明：`docs/svg-template.md`
- 渲染入口：`src/features/image/renderer/svg_templates.rs`
- 默认模板：`resources/templates/image/bn/default.svg.jinja`、`resources/templates/image/song/default.svg.jinja`

---

## 1. 快速上手：从默认模板复制一个新模板

以 BN（BestN）为例：

1) 复制模板文件：
- 从 `resources/templates/image/bn/default.svg.jinja`
- 复制为 `resources/templates/image/bn/my_v2.svg.jinja`

2) （可选）复制布局参数 JSON：
- 从 `resources/templates/image/bn/default.json`
- 复制为 `resources/templates/image/bn/my_v2.json`

3) 调用接口验证：
- `POST /api/v1/image/bn?template=my_v2&format=svg`

Song 同理：
- 模板：`resources/templates/image/song/my_v2.svg.jinja`
- JSON：`resources/templates/image/song/my_v2.json`
- 接口：`POST /api/v1/image/song?template=my_v2&format=svg`

> 注意：`template` 仅影响 SVG 生成层；`format=png/jpeg/webp` 时会先按模板生成 SVG，再进行栅格化编码。

---

## 2. 模板命名与安全限制（必须遵守）

Query 参数 `template` 会被归一化并写入缓存键：
- 允许字符：`[a-zA-Z0-9._-]`
- 最大长度：64
- 不传 `template`：走 `legacy`（内置手写 SVG）
- 非法值：归一为 `default`

建议命名：
- `bn/v2_grid_4col`、`bn/v2_minimal`（实际文件路径为 `bn/v2_grid_4col.svg.jinja`）
- `song/v2_wide`、`song/v2_mobile`

---

## 3. MiniJinja 语法速查（模板里常用）

注释：
```jinja
{# 这是注释，不会出现在输出里 #}
```

变量输出：
```jinja
<text>{{ header.player_title_xml }}</text>
```

条件：
```jinja
{% if background.href_xml %}
  <image href="{{ background.href_xml }}" ... />
{% else %}
  <rect ... />
{% endif %}
```

循环：
```jinja
{% for c in cards %}
  <g transform="translate({{ c.x }}, {{ c.y }})"> ... </g>
{% endfor %}
```

长度判断：
```jinja
{% if ap.cards | length > 0 %} ... {% endif %}
```

> 重要：本项目默认未启用模板引擎的“自动转义”。请优先使用 `_xml` 结尾字段（后端已做 XML 转义）。

---

## 4. “安全输出”规则：优先用 `*_xml`

模板上下文中的字符串分两类：

1) **已 XML 转义的安全字段**：以 `_xml` 结尾  
示例：`header.player_title_xml`、`c.text.song_name_xml`、`footer.generated_text_xml`

2) **允许包含 SVG 片段的字段**：以 `inner_xml` 命名（目前仅用于 header 的 `<tspan>` 片段）  
示例：`header.right_lines[].inner_xml`

规则：
- 用户输入（昵称/曲名/自定义 footer）必须使用 `*_xml` 字段输出
- 不要把用户输入拼成 `inner_xml` 再输出（除非你同时做了严格白名单）

---

## 5. 布局设计建议（让模板“可大改布局”但仍可维护）

### 5.1 把“可调常量”放到同名 JSON

例如 BN：`resources/templates/image/bn/my_v2.json`
- `columns`：列数
- `card_gap`：卡片间距
- `song_name_max_width`：歌名最大显示宽（按“显示宽度”估算）
- `song_name_max_lines`：歌名最大行数

这样做到：
- 轻微的网格与排版调整不用改 Rust
- 模板与参数可以一起版本化与回滚

### 5.2 保持 ID 唯一：clipPath/gradient/filter

SVG 中的 `<clipPath id="...">`、`<filter id="...">`、`<linearGradient id="...">` 是全局命名空间：
- 卡片级 clipPath 必须使用“每张卡唯一 id”（默认上下文里 `clip_id` 已唯一）
- 如果你新增新 filter/gradient，建议加模板前缀，例如 `myv2-card-shadow`

### 5.3 图片引用策略（特别重要）

当 `format=svg` 时，handler 会强制：
- `embed_images=false`（避免 data URI 体积爆炸）
- 曲绘 href 优先改为外部可访问基址（例如 `/_ill` 或配置的外部域名）

因此：
- 模板中 `<image href="{{ ... }}">` 一定要来自上下文里的 `href_xml`
- 不要在模板中自行拼本地绝对路径

---

## 6. BN 模板：你可以改哪些布局？

默认 BN 模板提供了两块可自由改动的布局区域：

1) Header：左侧玩家信息 + 右侧若干行信息
- 可调整字体、y 坐标、是否展示某些行、展示顺序等

2) Cards：`ap.cards[]` 与 `cards[]`
- 你可以改卡片内部字段排列：把 Score 放到右上/把 Acc 放到下面/把 Rank 挪到左侧等
- 你也可以在 `<g>` 内新增装饰元素（阴影、分割线、徽章等）

注意：
- 卡片的位置（`x/y`）与尺寸（`w/h`）由后端计算后传入，你可以在模板里决定如何使用它们

---

## 7. Song 模板：你可以改哪些布局？

Song 默认模板提供：
- `illust`：曲绘位置与裁剪
- `player`：玩家信息位置
- `song`：曲名位置（中心点 + y）
- `difficulty_cards[]`：四难度卡片的矩形与字段（score/acc/rks/常数/无谱面占位）

你可以：
- 把 4 张卡改成 2×2 网格
- 把曲绘缩小，给文字更多空间
- 把“无谱面”的提示改成更明显的占位

---

## 8. 调试与排错清单

1) 模板加载失败：
- 确认文件存在：`resources/templates/image/{kind}/{id}.svg.jinja`
- 确认 `template` 参数只含安全字符，且长度 ≤ 64

2) 输出 SVG 打不开：
- 优先用 `format=svg` 直接拿到 SVG 内容，检查是否缺失闭合标签/错误引用
- 检查是否出现了未转义的 `& < > " '`

3) 图片不显示（cover/背景/曲绘）：
- 确认 `href` 来自 `*_xml` 字段
- `format=svg` 时不内嵌图片：浏览器需要能访问 `/_ill/...` 或外部域名

4) 字体不对/换行不理想：
- 模板可以调整 `font-family`，但实际渲染字体受系统字体与 `resources/fonts` 影响
- 歌名多行/省略号策略由后端提供的 `song_name_max_lines` 等参数影响

---

## 9. 发布建议（避免线上串版与缓存污染）

- 新模板尽量使用新 `template` id（例如从 `default` 切到 `v2`），避免历史缓存影响观察。
- 如果只是在同一个模板 id 上改文件内容：缓存键仍是同一个 id，线上可能需要等待缓存 TTL 过期或做一次实例重启（视部署策略）。

