# Phi Backend API 使用指南

## 快速开始
- 启动服务：`cargo run`
- Swagger UI：`http://localhost:3939/docs`
- 健康检查：`GET /health`
- 默认 API 前缀：`/api/v1`（可通过 `APP_API_PREFIX` 覆盖）

## API 端点

### 健康检查
- 端点：`GET /health`
- 用途：探活与版本信息

### 存档 API（Save）
- 端点：`POST {api_prefix}/save`
- 认证：二选一
  - 官方会话：请求体 `{ "sessionToken": "..." }`
  - 外部凭证：请求体 `{ "externalCredentials": { ... } }`
- 可选参数：`calculate_rks=true|false`（查询字符串）
- 成功：返回解析后的存档，或附带玩家 RKS 概览

示例：
```bash
curl -X POST "http://localhost:3939/api/v1/save?calculate_rks=true" \
  -H "Content-Type: application/json" \
  -d '{ "sessionToken": "your-leancloud-session-token" }'
```

### 图片 API（Image）
- BN 图：`POST {api_prefix}/image/bn`（从存档生成 BestN 汇总图）
- 单曲图：`POST {api_prefix}/image/song`（从存档生成单曲图片）
- 用户自报 BN 图：`POST {api_prefix}/image/bn/user`（无需存档，直接提交成绩生成 BestN）
- 响应均为 `image/png`

### 歌曲检索（Song）
- 端点：`GET {api_prefix}/songs/search`
- 参数：
  - `q` 必填：歌曲 ID/名称/别名，支持模糊匹配
  - `unique` 可选：`true|false`，`true` 时要求唯一匹配（未命中 404，多命中 409）
```bash
curl "http://localhost:3939/api/v1/songs/search?q=devil&unique=true"
```

### 统计 API（Stats）

1) 汇总
- 端点：`GET {api_prefix}/stats/summary`
- 返回：首末事件时间、各功能的使用次数与最近时间、唯一用户统计（总量与来源分布）
- 功能次数统计中的“功能名”可能值：
  - `bestn`：生成 BestN 汇总图
  - `bestn_user`：生成用户自报 BestN 图片
  - `single_query`：生成单曲成绩图
  - `save`：获取并解析玩家存档
  - `song_search`：歌曲检索
- 备注：功能统计只包含通过业务打点上报的功能；路由级请求数请使用日聚合接口。

2) 日聚合
- 端点：`GET {api_prefix}/stats/daily?start=YYYY-MM-DD&end=YYYY-MM-DD[&feature=bestn]`
- 返回：`[{ date, feature, route, method, count, err_count }]`

## 配置与环境
- 配置文件：`config.toml`
- 常用覆盖：
  - `APP_API_PREFIX` 调整 API 前缀
  - `APP_STATS_USER_HASH_SALT` 启用去敏化用户哈希
  - `APP_LOGGING_LEVEL` 调整日志级别

## 说明
- 仓库所有字符串与文件均为 UTF-8 编码
- 更多统计细节参考：`STATS.md`

## 性能调优参数

以下参数位于 `config.toml` 的 `[image]` 部分，均可用环境变量 `APP_IMAGE_*` 覆盖（下划线分隔）。

- `optimize_speed: bool`
  - 启用后使用 SVG 栅格的 OptimizeSpeed 策略（更快，画质略降）。
  - 例：`APP_IMAGE_OPTIMIZE_SPEED=true`

- `cache_enabled: bool`
  - 开启 BN/单曲图片缓存（按图片字节大小加权）。
  - 例：`APP_IMAGE_CACHE_ENABLED=true`

- `cache_max_bytes: u64`（字节）
  - 缓存总容量上限（默认 100MB），超过逐出最少使用项。
  - 例：`APP_IMAGE_CACHE_MAX_BYTES=134217728`

- `cache_ttl_secs: u64` / `cache_tti_secs: u64`（秒）
  - TTL：条目自写入起最大存活时间；TTI：条目自最后一次访问起的空闲时间上限。
  - 例：`APP_IMAGE_CACHE_TTL_SECS=60`、`APP_IMAGE_CACHE_TTI_SECS=30`

- `max_parallel: u32`（0 = 自动）
  - 并发渲染许可数（Semaphore）。0 表示自动取 CPU 核心数。
  - 例：`APP_IMAGE_MAX_PARALLEL=8`

说明：
- 缓存键包含用户哈希、存档更新时间、请求参数（BN：n/theme/embed；单曲：song_id/embed），避免跨用户串缓存。
- 统计上报：
  - `image_render`：记录渲染总耗时（ms）、可用许可数、输出 PNG 字节数。
  - `bestn` / `single_query` / `bestn_user`：原有业务事件保持不变。
